use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use zeroclaw_api::tool::{Tool, ToolResult};
use zeroclaw_config::policy::SecurityPolicy;
use zeroclaw_config::schema::FileUploadBundleConfig;

const RESPONSE_BODY_LIMIT_BYTES: usize = 4 * 1024;

pub struct FileUploadBundleTool {
    security: Arc<SecurityPolicy>,
    config: FileUploadBundleConfig,
}

impl FileUploadBundleTool {
    pub fn new(security: Arc<SecurityPolicy>, config: FileUploadBundleConfig) -> Self {
        Self { security, config }
    }

    fn detect_mime(bytes: &[u8], file_name: &str) -> &'static str {
        if let Some(kind) = infer::get(bytes) {
            return kind.mime_type();
        }
        Self::mime_for_filename(file_name)
    }

    fn mime_for_filename(name: &str) -> &'static str {
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            // Images
            "png" | "apng" => "image/png",
            "jpg" | "jpeg" | "jfif" | "pjpeg" | "pjp" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "avif" => "image/avif",
            "bmp" => "image/bmp",
            "tiff" | "tif" => "image/tiff",
            "svg" => "image/svg+xml",
            "ico" => "image/vnd.microsoft.icon",
            "heic" | "heif" => "image/heic",
            "jxl" => "image/jxl",

            // Documents
            "pdf" => "application/pdf",
            "rtf" => "application/rtf",
            "epub" => "application/epub+zip",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xls" => "application/vnd.ms-excel",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "ppt" => "application/vnd.ms-powerpoint",
            "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "odt" => "application/vnd.oasis.opendocument.text",
            "ods" => "application/vnd.oasis.opendocument.spreadsheet",
            "odp" => "application/vnd.oasis.opendocument.presentation",

            // Structured data
            "json" => "application/json",
            "ndjson" | "jsonl" => "application/x-ndjson",
            "xml" => "application/xml",
            "yaml" | "yml" => "application/yaml",
            "toml" => "application/toml",
            "csv" => "text/csv",
            "tsv" => "text/tab-separated-values",
            "sql" => "application/sql",
            "ics" => "text/calendar",
            "vcf" => "text/vcard",

            // Text + markup
            "txt" | "log" | "ini" | "cfg" | "conf" | "env" => "text/plain",
            "md" | "markdown" => "text/markdown",
            "html" | "htm" => "text/html",
            "css" => "text/css",

            // Source code
            "js" | "mjs" | "cjs" => "application/javascript",
            "ts" | "tsx" => "application/typescript",
            "jsx" => "text/jsx",
            "py" => "text/x-python",
            "rb" => "text/x-ruby",
            "go" => "text/x-go",
            "rs" => "text/x-rust",
            "java" => "text/x-java",
            "kt" | "kts" => "text/x-kotlin",
            "swift" => "text/x-swift",
            "c" | "h" => "text/x-c",
            "cc" | "cpp" | "cxx" | "hpp" | "hh" => "text/x-c++",
            "cs" => "text/x-csharp",
            "sh" | "bash" | "zsh" => "application/x-sh",

            // Archives
            "zip" => "application/zip",
            "tar" => "application/x-tar",
            "gz" | "tgz" => "application/gzip",
            "bz2" | "tbz2" => "application/x-bzip2",
            "xz" | "txz" => "application/x-xz",
            "7z" => "application/x-7z-compressed",
            "rar" => "application/vnd.rar",

            // Audio
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "ogg" | "oga" | "opus" => "audio/ogg",
            "flac" => "audio/flac",
            "aac" => "audio/aac",
            "m4a" => "audio/mp4",
            "weba" => "audio/webm",
            "mid" | "midi" => "audio/midi",

            // Video
            "mp4" | "m4v" => "video/mp4",
            "webm" => "video/webm",
            "mov" | "qt" => "video/quicktime",
            "mkv" => "video/x-matroska",
            "avi" => "video/x-msvideo",
            "mpg" | "mpeg" => "video/mpeg",
            "3gp" => "video/3gpp",
            "3g2" => "video/3gpp2",

            // Fonts
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "eot" => "application/vnd.ms-fontobject",

            // Web binary
            "wasm" => "application/wasm",

            _ => "application/octet-stream",
        }
    }
}

struct PreparedFile {
    file_name: String,
    bytes: Vec<u8>,
    mime: &'static str,
}

#[async_trait]
impl Tool for FileUploadBundleTool {
    fn name(&self) -> &str {
        "file_upload_bundle"
    }

    fn description(&self) -> &str {
        "Upload N local files as a single atomic bundle via multipart/form-data. \
         All files land or none do — the receiver records the group as a bundle \
         so the consumer resolves siblings deterministically. Use for multi-file \
         deliverables (HTML + CSS + JS, report + figures). File paths stay on \
         the host; bytes are not loaded into model context. Returns the HTTP \
         status and a truncated response body."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "description": "Paths to the files on the agent's filesystem. Relative paths resolve from the workspace."
                },
                "entry_file_name": {
                    "type": "string",
                    "description": "Optional filename within file_paths to mark as the bundle's entry (e.g. \"index.html\"). Defaults to the first file. Must match exactly one path's basename."
                },
                "project_id": {
                    "type": "string",
                    "description": "Optional project UUID to associate the bundle with on the receiver."
                }
            },
            "required": ["file_paths"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let Some(url) = self
            .config
            .url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "file_upload_bundle is disabled: [file_upload_bundle].url is not configured"
                        .into(),
                ),
            });
        };

        let method = self.config.method.to_ascii_uppercase();
        if method != "POST" && method != "PUT" {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unsupported HTTP method '{method}'. Only POST and PUT are allowed."
                )),
            });
        }

        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        let raw_paths = args
            .get("file_paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'file_paths' array parameter"))?;

        if raw_paths.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("file_paths must not be empty".into()),
            });
        }
        if raw_paths.len() as u64 > self.config.max_files as u64 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Too many files: {} (limit: {})",
                    raw_paths.len(),
                    self.config.max_files
                )),
            });
        }

        let entry_hint = args
            .get("entry_file_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let mut paths: Vec<String> = Vec::with_capacity(raw_paths.len());
        for (i, entry) in raw_paths.iter().enumerate() {
            let p = entry
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("file_paths[{i}] must be a non-empty string"))?;
            if !self.security.is_path_allowed(p) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Path not allowed by security policy: {p}")),
                });
            }
            paths.push(p.to_string());
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let mut prepared: Vec<PreparedFile> = Vec::with_capacity(paths.len());
        let mut seen_names: HashSet<String> = HashSet::with_capacity(paths.len());
        let mut total_bytes: u64 = 0;
        for path in &paths {
            let full_path = self.security.resolve_tool_path(path);

            let resolved_path: PathBuf = match tokio::fs::canonicalize(&full_path).await {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to resolve file path {path}: {e}")),
                    });
                }
            };

            if !self.security.is_resolved_path_allowed(&resolved_path) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        self.security
                            .resolved_path_violation_message(&resolved_path),
                    ),
                });
            }

            let metadata = match tokio::fs::metadata(&resolved_path).await {
                Ok(m) => m,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to read file metadata for {path}: {e}")),
                    });
                }
            };

            if !metadata.is_file() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Not a regular file: {}", resolved_path.display())),
                });
            }

            if metadata.len() > self.config.max_file_size_bytes {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "File too large: {} is {} bytes (per-file limit: {} bytes)",
                        resolved_path.display(),
                        metadata.len(),
                        self.config.max_file_size_bytes
                    )),
                });
            }

            total_bytes = total_bytes.saturating_add(metadata.len());
            if total_bytes > self.config.max_total_size_bytes {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Bundle too large: cumulative {} bytes exceeds limit {} bytes",
                        total_bytes, self.config.max_total_size_bytes
                    )),
                });
            }

            let file_name = resolved_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("upload")
                .to_string();
            if !seen_names.insert(file_name.clone()) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Duplicate file name in bundle: {file_name} (filenames must be unique)"
                    )),
                });
            }

            let bytes = match tokio::fs::read(&resolved_path).await {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to read {}: {e}", resolved_path.display())),
                    });
                }
            };

            let mime = Self::detect_mime(&bytes, &file_name);
            prepared.push(PreparedFile {
                file_name,
                bytes,
                mime,
            });
        }

        if let Some(name) = &entry_hint
            && !prepared.iter().any(|f| &f.file_name == name)
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "entry_file_name '{name}' does not match any file in file_paths"
                )),
            });
        }

        let mut form = reqwest::multipart::Form::new();
        for file in &prepared {
            let part = match reqwest::multipart::Part::bytes(file.bytes.clone())
                .file_name(file.file_name.clone())
                .mime_str(file.mime)
            {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to build multipart part: {e}")),
                    });
                }
            };
            form = form.part(self.config.field_name.clone(), part);
        }
        if let Some(name) = entry_hint {
            form = form.text("entry_file_name", name);
        }
        if let Some(pid) = project_id {
            form = form.text("project_id", pid);
        }

        let client = zeroclaw_config::schema::build_runtime_proxy_client_with_timeouts(
            "tool.file_upload_bundle",
            self.config.timeout_secs,
            10,
        );

        let mut request = if method == "PUT" {
            client.put(url)
        } else {
            client.post(url)
        };

        for (k, v) in &self.config.headers {
            request = request.header(k.as_str(), v.as_str());
        }

        let response = match request.multipart(form).send().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Bundle upload request failed: {e}")),
                });
            }
        };

        let status = response.status();
        let raw_body = response.text().await.unwrap_or_default();
        let truncated = if raw_body.len() > RESPONSE_BODY_LIMIT_BYTES {
            format!(
                "{}... [truncated {} bytes]",
                &raw_body[..RESPONSE_BODY_LIMIT_BYTES],
                raw_body.len() - RESPONSE_BODY_LIMIT_BYTES
            )
        } else {
            raw_body
        };

        let file_count = prepared.len();
        if status.is_success() {
            Ok(ToolResult {
                success: true,
                output: format!(
                    "Uploaded bundle of {file_count} files ({status}). Response: {truncated}"
                ),
                error: None,
            })
        } else {
            Ok(ToolResult {
                success: false,
                output: truncated,
                error: Some(format!(
                    "Upload endpoint returned status {status} for bundle of {file_count} files"
                )),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zeroclaw_config::autonomy::AutonomyLevel;

    fn test_security(workspace: PathBuf, level: AutonomyLevel) -> Arc<SecurityPolicy> {
        Arc::new(SecurityPolicy {
            autonomy: level,
            max_actions_per_hour: 100,
            workspace_dir: workspace,
            ..SecurityPolicy::default()
        })
    }

    fn cfg(url: Option<String>) -> FileUploadBundleConfig {
        FileUploadBundleConfig {
            url,
            ..FileUploadBundleConfig::default()
        }
    }

    #[test]
    fn tool_name_and_description() {
        let tmp = TempDir::new().unwrap();
        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            cfg(Some("https://example.com/upload_bundle".into())),
        );
        assert_eq!(tool.name(), "file_upload_bundle");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn schema_requires_file_paths_array() {
        let tmp = TempDir::new().unwrap();
        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            cfg(Some("https://example.com/upload_bundle".into())),
        );
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::Value::String("file_paths".into())));
        assert_eq!(schema["properties"]["file_paths"]["type"], "array");
    }

    #[tokio::test]
    async fn execute_fails_when_url_unset() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("a.txt");
        fs::write(&file, b"a").unwrap();

        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            cfg(None),
        );

        let result = tool
            .execute(json!({ "file_paths": ["a.txt"] }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("disabled"));
    }

    #[tokio::test]
    async fn execute_blocks_readonly_autonomy() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("a.txt");
        fs::write(&file, b"a").unwrap();

        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::ReadOnly),
            cfg(Some("https://example.com/upload_bundle".into())),
        );

        let result = tool
            .execute(json!({ "file_paths": ["a.txt"] }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("read-only"));
    }

    #[tokio::test]
    async fn execute_rejects_empty_file_paths() {
        let tmp = TempDir::new().unwrap();
        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            cfg(Some("https://example.com/upload_bundle".into())),
        );

        let result = tool.execute(json!({ "file_paths": [] })).await.unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("must not be empty"));
    }

    #[tokio::test]
    async fn execute_rejects_too_many_files() {
        let tmp = TempDir::new().unwrap();
        let mut config = cfg(Some("https://example.com/upload_bundle".into()));
        config.max_files = 2;
        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            config,
        );

        let result = tool
            .execute(json!({ "file_paths": ["a.txt", "b.txt", "c.txt"] }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Too many files"));
    }

    #[tokio::test]
    async fn execute_rejects_per_file_over_size_cap() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("ok.bin"), vec![0u8; 100]).unwrap();
        fs::write(tmp.path().join("big.bin"), vec![0u8; 2048]).unwrap();

        let mut config = cfg(Some("https://example.com/upload_bundle".into()));
        config.max_file_size_bytes = 1024;

        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            config,
        );

        let result = tool
            .execute(json!({ "file_paths": ["ok.bin", "big.bin"] }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("too large"));
    }

    #[tokio::test]
    async fn execute_rejects_cumulative_over_total_cap() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.bin"), vec![0u8; 800]).unwrap();
        fs::write(tmp.path().join("b.bin"), vec![0u8; 800]).unwrap();

        let mut config = cfg(Some("https://example.com/upload_bundle".into()));
        config.max_file_size_bytes = 1024;
        config.max_total_size_bytes = 1024;

        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            config,
        );

        let result = tool
            .execute(json!({ "file_paths": ["a.bin", "b.bin"] }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Bundle too large"));
    }

    #[tokio::test]
    async fn execute_rejects_duplicate_filenames() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(tmp.path().join("index.html"), b"<a/>").unwrap();
        fs::write(sub.join("index.html"), b"<b/>").unwrap();

        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            cfg(Some("https://example.com/upload_bundle".into())),
        );

        let result = tool
            .execute(json!({ "file_paths": ["index.html", "sub/index.html"] }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Duplicate file name"));
    }

    #[tokio::test]
    async fn execute_rejects_entry_not_in_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.html"), b"<a/>").unwrap();

        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            cfg(Some("https://example.com/upload_bundle".into())),
        );

        let result = tool
            .execute(json!({
                "file_paths": ["a.html"],
                "entry_file_name": "missing.html"
            }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("does not match any file"));
    }

    #[tokio::test]
    async fn execute_rejects_path_outside_workspace() {
        let workspace = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let file = outside.path().join("secret.txt");
        fs::write(&file, b"nope").unwrap();

        let tool = FileUploadBundleTool::new(
            test_security(workspace.path().to_path_buf(), AutonomyLevel::Full),
            cfg(Some("https://example.com/upload_bundle".into())),
        );

        let result = tool
            .execute(json!({ "file_paths": [file.to_string_lossy()] }))
            .await
            .unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn execute_uploads_bundle_with_multipart_parts_and_headers() {
        let server = MockServer::start().await;
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("index.html"), b"<html></html>").unwrap();
        fs::write(tmp.path().join("styles.css"), b"body{}").unwrap();
        fs::write(tmp.path().join("app.js"), b"console.log(1)").unwrap();

        Mock::given(method("POST"))
            .and(path("/upload_bundle"))
            .and(header("X-Auth", "Bearer xyz"))
            .respond_with(ResponseTemplate::new(201).set_body_string(
                r#"{"bundle_id":"abc","entry_file_id":"def","files":[{"file_name":"index.html"},{"file_name":"styles.css"},{"file_name":"app.js"}]}"#,
            ))
            .expect(1)
            .mount(&server)
            .await;

        let mut headers = HashMap::new();
        headers.insert("X-Auth".into(), "Bearer xyz".into());
        let config = FileUploadBundleConfig {
            url: Some(format!("{}/upload_bundle", server.uri())),
            headers,
            ..FileUploadBundleConfig::default()
        };

        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            config,
        );

        let result = tool
            .execute(json!({
                "file_paths": ["index.html", "styles.css", "app.js"],
                "entry_file_name": "index.html"
            }))
            .await
            .unwrap();

        assert!(result.success, "expected success, got {result:?}");
        assert!(result.output.contains("3 files"));
        assert!(result.output.contains("abc"));
    }

    #[tokio::test]
    async fn execute_reports_non_2xx_response() {
        let server = MockServer::start().await;
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.txt"), b"a").unwrap();

        Mock::given(method("POST"))
            .and(path("/upload_bundle"))
            .respond_with(ResponseTemplate::new(422).set_body_string("bundle_too_large"))
            .expect(1)
            .mount(&server)
            .await;

        let config = FileUploadBundleConfig {
            url: Some(format!("{}/upload_bundle", server.uri())),
            ..FileUploadBundleConfig::default()
        };

        let tool = FileUploadBundleTool::new(
            test_security(tmp.path().to_path_buf(), AutonomyLevel::Full),
            config,
        );

        let result = tool
            .execute(json!({ "file_paths": ["a.txt"] }))
            .await
            .unwrap();
        assert!(!result.success);
        let err = result.error.unwrap();
        assert!(err.contains("422"), "unexpected error: {err}");
    }

    #[test]
    fn detect_mime_uses_content_sniff_for_binary_with_wrong_extension() {
        let png = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        ];
        assert_eq!(
            FileUploadBundleTool::detect_mime(&png, "screenshot.tmp"),
            "image/png"
        );
    }

    #[test]
    fn detect_mime_falls_back_to_extension_for_text_formats() {
        let md = b"# Title\n\nSome paragraph text.\n";
        assert_eq!(
            FileUploadBundleTool::detect_mime(md, "README.md"),
            "text/markdown"
        );
        let yaml = b"key: value\nother: 1\n";
        assert_eq!(
            FileUploadBundleTool::detect_mime(yaml, "config.yaml"),
            "application/yaml"
        );
    }

    #[test]
    fn detect_mime_falls_back_to_octet_stream_for_unknown() {
        let bytes = b"\x00\x01\x02\x03unknown binary garbage";
        assert_eq!(
            FileUploadBundleTool::detect_mime(bytes, "mystery.dat"),
            "application/octet-stream"
        );
    }

    #[test]
    fn mime_table_covers_common_bundle_extensions() {
        let cases = [
            // images
            ("photo.png", "image/png"),
            ("snap.JPG", "image/jpeg"),
            ("anim.gif", "image/gif"),
            ("hero.webp", "image/webp"),
            ("modern.avif", "image/avif"),
            ("favicon.ico", "image/vnd.microsoft.icon"),
            ("vector.svg", "image/svg+xml"),
            ("phone.heic", "image/heic"),
            // documents
            ("paper.PDF", "application/pdf"),
            (
                "brief.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            (
                "budget.xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            (
                "slides.pptx",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
            ("notes.odt", "application/vnd.oasis.opendocument.text"),
            ("book.epub", "application/epub+zip"),
            // data
            ("data.json", "application/json"),
            ("stream.ndjson", "application/x-ndjson"),
            ("conf.yaml", "application/yaml"),
            ("Cargo.toml", "application/toml"),
            ("rows.tsv", "text/tab-separated-values"),
            ("schema.sql", "application/sql"),
            ("invite.ics", "text/calendar"),
            // text + markup
            ("README.md", "text/markdown"),
            ("index.html", "text/html"),
            ("style.css", "text/css"),
            ("setup.env", "text/plain"),
            // source code
            ("app.js", "application/javascript"),
            ("api.ts", "application/typescript"),
            ("Page.tsx", "application/typescript"),
            ("main.py", "text/x-python"),
            ("lib.rs", "text/x-rust"),
            ("Main.kt", "text/x-kotlin"),
            ("run.sh", "application/x-sh"),
            ("app.cpp", "text/x-c++"),
            // archives
            ("src.zip", "application/zip"),
            ("logs.tar.gz", "application/gzip"),
            ("dump.bz2", "application/x-bzip2"),
            ("pack.7z", "application/x-7z-compressed"),
            // audio
            ("song.mp3", "audio/mpeg"),
            ("voice.flac", "audio/flac"),
            ("voice.m4a", "audio/mp4"),
            // video
            ("clip.mp4", "video/mp4"),
            ("rec.mkv", "video/x-matroska"),
            ("legacy.avi", "video/x-msvideo"),
            // fonts
            ("font.woff2", "font/woff2"),
            ("font.ttf", "font/ttf"),
            // web binary
            ("module.wasm", "application/wasm"),
            // fallback
            ("noext", "application/octet-stream"),
            ("weird.qq", "application/octet-stream"),
        ];
        for (name, expected) in cases {
            assert_eq!(
                FileUploadBundleTool::mime_for_filename(name),
                expected,
                "{name} should map to {expected}"
            );
        }
    }
}
