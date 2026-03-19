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
}
