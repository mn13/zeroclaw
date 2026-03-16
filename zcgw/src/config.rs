use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayConfig {
    pub listen_addr: String,
    pub instances: HashMap<String, InstanceConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstanceConfig {
    pub grpc_address: String,
    pub display_name: String,
}

impl GatewayConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: GatewayConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let toml_str = r#"
listen_addr = "0.0.0.0:8080"

[instances.zc-1]
grpc_address = "localhost:50051"
display_name = "Agent One"

[instances.zc-2]
grpc_address = "localhost:50052"
display_name = "Agent Two"
"#;
        let config: GatewayConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.listen_addr, "0.0.0.0:8080");
        assert_eq!(config.instances.len(), 2);
        assert_eq!(
            config.instances["zc-1"].grpc_address,
            "localhost:50051"
        );
        assert_eq!(config.instances["zc-2"].display_name, "Agent Two");
    }

    #[test]
    fn test_parse_empty_instances() {
        let toml_str = r#"
listen_addr = "127.0.0.1:9090"

[instances]
"#;
        let config: GatewayConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.listen_addr, "127.0.0.1:9090");
        assert!(config.instances.is_empty());
    }
}
