use crate::config::GatewayConfig;

#[test]
fn test_config_parsing() {
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
    assert_eq!(config.instances["zc-1"].display_name, "Agent One");
}

#[test]
fn test_config_missing_instances_fails() {
    let toml_str = r#"
listen_addr = "0.0.0.0:8080"
"#;
    let result: Result<GatewayConfig, _> = toml::from_str(toml_str);
    assert!(result.is_err());
}

// Auth tests
mod auth_tests {
    use axum::body::Body;
    use axum::http::Request as HttpRequest;

    fn extract_token(req: &HttpRequest<Body>) -> Option<String> {
        // Test the same logic as in auth.rs
        if let Some(val) = req.headers().get("authorization") {
            if let Ok(s) = val.to_str() {
                if let Some(token) = s.strip_prefix("Bearer ") {
                    return Some(token.to_string());
                }
            }
        }
        if let Some(query) = req.uri().query() {
            for pair in query.split('&') {
                if let Some(val) = pair.strip_prefix("token=") {
                    return Some(val.to_string());
                }
            }
        }
        None
    }

    #[test]
    fn test_auth_valid_bearer() {
        let req = HttpRequest::builder()
            .uri("/api/instances")
            .header("authorization", "Bearer test-token-123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token(&req), Some("test-token-123".to_string()));
    }

    #[test]
    fn test_auth_invalid_scheme() {
        let req = HttpRequest::builder()
            .uri("/api/instances")
            .header("authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token(&req), None);
    }

    #[test]
    fn test_auth_no_token() {
        let req = HttpRequest::builder()
            .uri("/api/instances")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token(&req), None);
    }

    #[test]
    fn test_auth_query_param() {
        let req = HttpRequest::builder()
            .uri("/ws/chat?instance=zc-1&token=my-token")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token(&req), Some("my-token".to_string()));
    }
}
