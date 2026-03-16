use crate::registry::InstanceRegistry;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<InstanceRegistry>,
    pub auth_token: String,
    pub grpc_secret: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub config_path: String,
    pub docker_config: crate::docker::DockerConfig,
}
