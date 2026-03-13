use crate::config::InstanceConfig;
use crate::proto::claw_agent_client::ClawAgentClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Channel;
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceHealth {
    Unknown,
    Healthy,
    Unhealthy,
}

impl serde::Serialize for InstanceHealth {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let s = match self {
            InstanceHealth::Unknown => "unknown",
            InstanceHealth::Healthy => "healthy",
            InstanceHealth::Unhealthy => "unhealthy",
        };
        serializer.serialize_str(s)
    }
}

pub struct InstanceRegistry {
    instances: HashMap<String, InstanceConfig>,
    clients: RwLock<HashMap<String, ClawAgentClient<Channel>>>,
    health: RwLock<HashMap<String, InstanceHealth>>,
    grpc_secret: String,
}

impl InstanceRegistry {
    pub fn new(instances: HashMap<String, InstanceConfig>, grpc_secret: String) -> Self {
        let health: HashMap<String, InstanceHealth> = instances
            .keys()
            .map(|k| (k.clone(), InstanceHealth::Unknown))
            .collect();
        Self {
            instances,
            clients: RwLock::new(HashMap::new()),
            health: RwLock::new(health),
            grpc_secret,
        }
    }

    pub fn instances(&self) -> &HashMap<String, InstanceConfig> {
        &self.instances
    }

    pub fn grpc_secret(&self) -> &str {
        &self.grpc_secret
    }

    pub async fn get_health(&self, id: &str) -> InstanceHealth {
        self.health
            .read()
            .await
            .get(id)
            .cloned()
            .unwrap_or(InstanceHealth::Unknown)
    }

    pub async fn all_health(&self) -> HashMap<String, InstanceHealth> {
        self.health.read().await.clone()
    }

    pub async fn get_client(
        &self,
        instance_id: &str,
    ) -> anyhow::Result<ClawAgentClient<Channel>> {
        // Check cache
        {
            let clients = self.clients.read().await;
            if let Some(client) = clients.get(instance_id) {
                return Ok(client.clone());
            }
        }

        // Create new connection
        let config = self
            .instances
            .get(instance_id)
            .ok_or_else(|| anyhow::anyhow!("unknown instance: {}", instance_id))?;

        let addr = if config.grpc_address.starts_with("http") {
            config.grpc_address.clone()
        } else {
            format!("http://{}", config.grpc_address)
        };

        let channel = Channel::from_shared(addr)?.connect().await?;
        let client = ClawAgentClient::new(channel);

        let mut clients = self.clients.write().await;
        clients.insert(instance_id.to_string(), client.clone());

        Ok(client)
    }

    /// Remove cached client so the next call reconnects.
    pub async fn invalidate_client(&self, instance_id: &str) {
        let mut clients = self.clients.write().await;
        clients.remove(instance_id);
    }

    pub async fn check_health_all(&self) {
        for (id, _config) in &self.instances {
            let healthy = match self.get_client(id).await {
                Ok(mut client) => {
                    let req = tonic::Request::new(crate::proto::HealthCheckRequest {});
                    match client.health_check(req).await {
                        Ok(resp) => resp.into_inner().healthy,
                        Err(_) => {
                            self.invalidate_client(id).await;
                            false
                        }
                    }
                }
                Err(_) => false,
            };

            let status = if healthy {
                InstanceHealth::Healthy
            } else {
                InstanceHealth::Unhealthy
            };

            let mut health = self.health.write().await;
            health.insert(id.clone(), status);
        }
    }

    pub fn spawn_health_loop(self: &Arc<Self>) {
        let registry = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                info!("running health check on all instances");
                registry.check_health_all().await;
                let health = registry.all_health().await;
                for (id, status) in &health {
                    match status {
                        InstanceHealth::Healthy => info!(instance = %id, "healthy"),
                        InstanceHealth::Unhealthy => warn!(instance = %id, "unhealthy"),
                        InstanceHealth::Unknown => info!(instance = %id, "unknown"),
                    }
                }
            }
        });
    }
}
