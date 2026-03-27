//! Minimal MCP client for Streamable HTTP transport.
//!
//! Connects to external MCP servers (e.g. Composio-hosted) at startup,
//! discovers their tools via `tools/list`, and wraps each tool into the
//! [`Tool`] trait so they can be registered in the agent's tool registry.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for an external MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct McpServerConfig {
    /// Human-readable name for the server (e.g. "composio-gmail").
    #[serde(default)]
    pub name: String,
    /// Transport type: "streamable-http" or "sse". Only streamable-http is implemented.
    #[serde(default = "default_transport")]
    pub transport: String,
    /// Server URL (the MCP endpoint).
    #[serde(default)]
    pub url: String,
    /// Whether this server is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Optional HTTP headers (e.g. API keys).
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

fn default_transport() -> String {
    "streamable-http".to_string()
}
fn default_enabled() -> bool {
    true
}

// ---------------------------------------------------------------------------
// MCP JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
    id: u64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
    #[allow(dead_code)]
    id: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: Option<i64>,
    message: Option<String>,
}

/// A tool definition as returned by MCP `tools/list`.
#[derive(Debug, Clone, Deserialize)]
struct McpToolDef {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "inputSchema")]
    input_schema: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// MCP Client
// ---------------------------------------------------------------------------

/// Minimal MCP client for Streamable HTTP transport.
struct McpClient {
    url: String,
    headers: HashMap<String, String>,
    client: reqwest::Client,
    next_id: AtomicU64,
}

impl McpClient {
    fn new(config: &McpServerConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            url: config.url.clone(),
            headers: config.headers.clone(),
            client,
            next_id: AtomicU64::new(1),
        }
    }

    async fn call(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
            id,
        };

        let mut builder = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");

        for (key, value) in &self.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }

        let resp = builder.json(&req).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("MCP server returned {status}: {body}");
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body_text = resp.text().await?;

        // Handle both plain JSON and SSE responses
        let json_text = if content_type.contains("text/event-stream") || body_text.contains("event: message") {
            // Parse SSE: extract the last "data:" line
            body_text
                .lines()
                .filter(|l| l.starts_with("data: "))
                .last()
                .map(|l| l.strip_prefix("data: ").unwrap_or(l).to_string())
                .unwrap_or(body_text)
        } else {
            body_text
        };

        let rpc_resp: JsonRpcResponse = serde_json::from_str(&json_text)
            .map_err(|e| anyhow::anyhow!("Failed to parse MCP response: {e} — body: {json_text}"))?;

        if let Some(err) = rpc_resp.error {
            anyhow::bail!(
                "MCP error: {}",
                err.message.unwrap_or_else(|| "unknown".to_string())
            );
        }

        rpc_resp
            .result
            .ok_or_else(|| anyhow::anyhow!("MCP response has no result"))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "zeroclaw",
                "version": "1.0"
            }
        });
        self.call("initialize", Some(params)).await?;
        Ok(())
    }

    async fn list_tools(&self) -> anyhow::Result<Vec<McpToolDef>> {
        let result = self.call("tools/list", None).await?;
        let tools = result
            .get("tools")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut defs = Vec::new();
        for t in tools {
            if let Ok(def) = serde_json::from_value::<McpToolDef>(t) {
                defs.push(def);
            }
        }
        Ok(defs)
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });
        self.call("tools/call", Some(params)).await
    }
}

// ---------------------------------------------------------------------------
// McpTool — wraps a single MCP tool into the Tool trait
// ---------------------------------------------------------------------------

/// A single tool exposed by an external MCP server.
struct McpTool {
    /// Display name (prefixed, e.g. "composio_gmail_GMAIL_SEND_EMAIL")
    display_name: String,
    /// Original MCP tool name used in `tools/call` (e.g. "GMAIL_SEND_EMAIL")
    mcp_name: String,
    tool_description: String,
    schema: serde_json::Value,
    client: Arc<McpClient>,
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.display_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        match self.client.call_tool(&self.mcp_name, args).await {
            Ok(result) => {
                // MCP tools/call returns { content: [{ type, text }], isError }
                let is_error = result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
                let output = result
                    .get("content")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| item.get("text").and_then(|v| v.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_else(|| serde_json::to_string_pretty(&result).unwrap_or_default());

                if is_error {
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(output),
                    })
                } else {
                    Ok(ToolResult {
                        success: true,
                        output,
                        error: None,
                    })
                }
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("MCP call failed: {e}")),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Connect to all configured MCP servers and return their tools.
///
/// Each tool is wrapped in a `Box<dyn Tool>` with a namespaced name derived
/// from the server config name (e.g. `composio_gmail_GMAIL_SEND_EMAIL`).
///
/// Servers that fail to connect are logged and skipped.
pub async fn create_mcp_tools(configs: &[McpServerConfig]) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();

    for config in configs {
        if !config.enabled || config.url.is_empty() {
            continue;
        }

        if config.transport != "streamable-http" {
            tracing::warn!(
                name = %config.name,
                transport = %config.transport,
                "Unsupported MCP transport, skipping (only streamable-http is supported)"
            );
            continue;
        }

        let client = Arc::new(McpClient::new(config));

        // Initialize the connection
        if let Err(e) = client.initialize().await {
            tracing::warn!(
                name = %config.name,
                error = %e,
                "Failed to initialize MCP server, skipping"
            );
            continue;
        }

        // Discover tools
        let tool_defs = match client.list_tools().await {
            Ok(defs) => defs,
            Err(e) => {
                tracing::warn!(
                    name = %config.name,
                    error = %e,
                    "Failed to list tools from MCP server, skipping"
                );
                continue;
            }
        };

        let prefix = config
            .name
            .replace('-', "_")
            .replace(' ', "_");

        tracing::info!(
            name = %config.name,
            tools = tool_defs.len(),
            "Connected to MCP server"
        );

        for def in tool_defs {
            let schema = def.input_schema.unwrap_or_else(|| {
                serde_json::json!({
                    "type": "object",
                    "properties": {}
                })
            });

            let display_name = if prefix.is_empty() {
                def.name.clone()
            } else {
                format!("{}_{}", prefix, def.name)
            };

            let description = def
                .description
                .unwrap_or_else(|| format!("MCP tool: {}", def.name));

            tools.push(Box::new(McpTool {
                display_name,
                mcp_name: def.name,
                tool_description: description,
                schema,
                client: client.clone(),
            }));
        }
    }

    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_server_is_skipped() {
        let config = McpServerConfig {
            name: "test".to_string(),
            transport: "streamable-http".to_string(),
            url: "http://localhost:1234".to_string(),
            enabled: false,
            headers: HashMap::new(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tools = rt.block_on(create_mcp_tools(&[config]));
        assert!(tools.is_empty());
    }

    #[test]
    fn empty_url_is_skipped() {
        let config = McpServerConfig {
            name: "test".to_string(),
            transport: "streamable-http".to_string(),
            url: String::new(),
            enabled: true,
            headers: HashMap::new(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tools = rt.block_on(create_mcp_tools(&[config]));
        assert!(tools.is_empty());
    }

    #[test]
    fn unsupported_transport_is_skipped() {
        let config = McpServerConfig {
            name: "test".to_string(),
            transport: "stdio".to_string(),
            url: "http://localhost:1234".to_string(),
            enabled: true,
            headers: HashMap::new(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tools = rt.block_on(create_mcp_tools(&[config]));
        assert!(tools.is_empty());
    }
}
