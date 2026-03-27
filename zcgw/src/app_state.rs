use crate::registry::InstanceRegistry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// A Google account authenticated at the gateway level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleAccount {
    pub email: String,
    pub assigned_to: Vec<String>,
    pub authenticated_at: String,
}

/// Persistent store for gateway-level Google accounts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoogleAccountsStore {
    pub accounts: Vec<GoogleAccount>,
}

/// Pending state for an in-progress Google OAuth redirect flow.
pub struct OAuthPendingState {
    pub email: String,
    /// The `redirect_uri` extracted from the auth URL (e.g. `http://localhost:8080/oauth2/callback`).
    /// Used to reconstruct the full callback URL for GOG CLI step 2.
    pub redirect_uri: String,
    pub created_at: std::time::Instant,
}

/// A named Signal connection managed at the gateway level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalConnection {
    /// User-chosen label (e.g. "support-line").
    pub name: String,
    /// E.164 phone number (e.g. "+1234567890").
    pub account: String,
    /// ISO 8601 timestamp of when this account was linked.
    pub linked_at: String,
    /// Agent instance IDs this connection is assigned to.
    pub assigned_to: Vec<String>,
}

/// Persistent store for gateway-level Signal connections.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SignalConnectionsStore {
    pub connections: Vec<SignalConnection>,
}

/// Pending state for an in-progress Signal device-link flow.
pub struct SignalLinkPendingState {
    /// The device name used for linking (kept for diagnostics/logging).
    #[allow(dead_code)]
    pub device_name: String,
    /// Handle to the signal-cli link child process.
    pub child: tokio::process::Child,
    pub created_at: std::time::Instant,
}

/// A Composio connected account managed at the gateway level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioConnection {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub toolkit_slug: String,
    pub display_name: String,
    pub user_id: String,
    pub assigned_to: Vec<String>,
    pub status: String,
    pub connected_at: String,
}

/// Persistent store for gateway-level Composio connections.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComposioStore {
    pub connections: Vec<ComposioConnection>,
    #[serde(default)]
    pub mcp_servers: HashMap<String, ComposioMcpServerEntry>,
}

/// An MCP server created via Composio's hosted MCP API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioMcpServerEntry {
    pub server_id: String,
    pub toolkit_slug: String,
    pub created_at: String,
}

/// Pending state for an in-progress Composio OAuth flow.
#[allow(dead_code)]
pub struct ComposioOAuthPendingState {
    pub user_id: String,
    pub toolkit_slug: String,
    pub connected_account_id: Option<String>,
    pub name: String,
    pub instance_id: Option<String>,
    pub created_at: std::time::Instant,
}

/// Configuration for the signal-cli daemon managed by the gateway.
#[derive(Debug, Clone)]
pub struct SignalCliConfig {
    /// Path to the signal-cli binary.
    pub cli_path: String,
    /// Port for the signal-cli HTTP daemon.
    pub http_port: u16,
    /// Data directory for signal-cli account state.
    pub data_dir: std::path::PathBuf,
}

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<InstanceRegistry>,
    pub auth_token: String,
    pub grpc_secret: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub config_path: String,
    pub docker_config: crate::docker::DockerConfig,
    /// Google OAuth client credentials JSON (from ZEROCLAW_GOOGLE_CREDENTIALS_JSON env var).
    pub google_credentials_json: Option<String>,
    /// Public hostname for Google OAuth redirect URI (from ZEROCLAW_GOOGLE_REDIRECT_HOST).
    pub google_redirect_host: Option<String>,
    /// Maps the OAuth `state` parameter to pending auth context. Entries expire after 10 minutes.
    pub oauth_pending: Arc<std::sync::Mutex<HashMap<String, OAuthPendingState>>>,
    /// Directory for gateway's GOG CLI config/keyring (e.g. <agents_dir>/.google/gogcli).
    pub gog_home: std::path::PathBuf,
    /// Gateway-level Google accounts store.
    pub google_accounts: Arc<tokio::sync::RwLock<GoogleAccountsStore>>,
    /// Gateway-level Signal connections store.
    pub signal_connections: Arc<tokio::sync::RwLock<SignalConnectionsStore>>,
    /// signal-cli daemon configuration.
    pub signal_cli_config: SignalCliConfig,
    /// Handle to the supervised signal-cli daemon process (None when not running).
    pub signal_cli_handle: Arc<tokio::sync::Mutex<Option<tokio::process::Child>>>,
    /// Pending Signal link operations (keyed by a random ID).
    pub signal_link_pending: Arc<std::sync::Mutex<HashMap<String, SignalLinkPendingState>>>,
    /// Composio API key (from COMPOSIO_API_KEY env var).
    pub composio_api_key: Option<String>,
    /// Gateway-level Composio connections store.
    pub composio_store: Arc<tokio::sync::RwLock<ComposioStore>>,
    /// Pending Composio OAuth flows (keyed by a random ID).
    pub composio_oauth_pending: Arc<std::sync::Mutex<HashMap<String, ComposioOAuthPendingState>>>,
    /// Public hostname for Composio OAuth redirect URI.
    pub composio_redirect_host: Option<String>,
    /// Tenant ID for scoping Composio user IDs across shared API keys.
    pub tenant_id: Option<String>,
}
