mod api;
mod app_state;
mod auth;
mod config;
mod registry;
mod static_files;
mod ws;

pub mod proto {
    tonic::include_proto!("zeroclaw");
}

#[cfg(test)]
mod tests;

use app_state::AppState;
use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load config
    let config_path =
        std::env::var("ZCGW_CONFIG_PATH").unwrap_or_else(|_| "./zcgw.toml".to_string());
    let config = config::GatewayConfig::load(&config_path)?;
    info!(listen = %config.listen_addr, instances = config.instances.len(), "loaded config");

    let auth_token = std::env::var("ZCGW_AUTH_TOKEN").unwrap_or_default();
    let grpc_secret = std::env::var("ZCGW_GRPC_SECRET").unwrap_or_default();

    if auth_token.is_empty() {
        tracing::warn!("ZCGW_AUTH_TOKEN not set — API endpoints are unprotected");
    }

    let registry = Arc::new(registry::InstanceRegistry::new(
        config.instances.clone(),
        grpc_secret.clone(),
    ));
    registry.spawn_health_loop();

    let state = AppState {
        registry,
        auth_token,
        grpc_secret,
        started_at: chrono::Utc::now(),
    };

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    info!(addr = %config.listen_addr, "zcgw listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(api::health))
        .route("/api/instances", get(api::list_instances))
        .route("/api/instances/{id}/status", get(api::get_status))
        .route("/api/instances/{id}/history", get(api::get_history))
        .route("/api/instances/{id}/history", delete(api::clear_history))
        .route("/api/instances/{id}/config", get(api::get_config))
        .route("/api/instances/{id}/config", put(api::update_config))
        .route("/api/instances/{id}/tools", get(api::list_tools))
        .route("/api/instances/{id}/memory", get(api::list_memory))
        .route(
            "/api/instances/{id}/memory/search",
            get(api::search_memory),
        )
        .route("/api/instances/{id}/memory", post(api::store_memory))
        .route(
            "/api/instances/{id}/memory/{key}",
            delete(api::forget_memory),
        )
        .route("/api/instances/{id}/chat", post(api::chat))
        .route("/ws/chat", get(ws::ws_chat));

    let spa = Router::new().fallback(static_files::static_handler);

    api.merge(spa)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl-c");
    info!("shutdown signal received");
}
