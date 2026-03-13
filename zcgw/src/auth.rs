use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::app_state::AppState;

/// Extracts a bearer token from the Authorization header or `token` query param.
fn extract_token(req: &Request) -> Option<String> {
    // Try Authorization header first
    if let Some(val) = req.headers().get("authorization") {
        if let Ok(s) = val.to_str() {
            if let Some(token) = s.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }

    // Fall back to query param
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(val) = pair.strip_prefix("token=") {
                return Some(val.to_string());
            }
        }
    }

    None
}

/// Returns true if the path should skip auth.
fn is_public(path: &str) -> bool {
    if path == "/health" {
        return true;
    }
    if path.starts_with("/_app/") {
        return true;
    }
    // SPA fallback for paths that don't start with /api or /ws
    if !path.starts_with("/api/") && !path.starts_with("/ws/") {
        return true;
    }
    false
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();

    if is_public(&path) {
        return Ok(next.run(req).await);
    }

    let expected = &state.auth_token;
    if expected.is_empty() {
        // No token configured — allow all
        return Ok(next.run(req).await);
    }

    match extract_token(&req) {
        Some(token) if token == *expected => Ok(next.run(req).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;

    #[test]
    fn test_extract_token_from_header() {
        let req = HttpRequest::builder()
            .uri("/api/instances")
            .header("authorization", "Bearer my-secret")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token(&req), Some("my-secret".to_string()));
    }

    #[test]
    fn test_extract_token_from_query() {
        let req = HttpRequest::builder()
            .uri("/ws/chat?instance=zc-1&token=abc123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token(&req), Some("abc123".to_string()));
    }

    #[test]
    fn test_extract_token_missing() {
        let req = HttpRequest::builder()
            .uri("/api/instances")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token(&req), None);
    }

    #[test]
    fn test_is_public_health() {
        assert!(is_public("/health"));
    }

    #[test]
    fn test_is_public_static() {
        assert!(is_public("/_app/index.js"));
    }

    #[test]
    fn test_is_not_public_api() {
        assert!(!is_public("/api/instances"));
    }

    #[test]
    fn test_is_not_public_ws() {
        assert!(!is_public("/ws/chat"));
    }
}
