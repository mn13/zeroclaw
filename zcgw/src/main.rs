mod admin;
mod api;
mod app_state;
mod auth;
mod config;
mod docker;
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
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file FIRST, before anything else reads env vars.
    // Try ZCGW_ENV_FILE, then docker/.env, then .env
    // Process env vars always take precedence over .env file values.
    let env_file_path = std::env::var("ZCGW_ENV_FILE").ok().or_else(|| {
        for candidate in &["docker/.env", ".env"] {
            if std::path::Path::new(candidate).exists() {
                return Some(candidate.to_string());
            }
        }
        None
    });
    if let Some(ref env_path) = env_file_path {
        if let Ok(content) = std::fs::read_to_string(env_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((key, val)) = trimmed.split_once('=') {
                    let key = key.trim();
                    let val = val.trim();
                    if std::env::var(key).is_err() {
                        std::env::set_var(key, val);
                    }
                }
            }
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    if let Some(ref env_path) = env_file_path {
        info!(path = %env_path, "loaded env file");
    }

    // Load config
    let config_path =
        std::env::var("ZCGW_CONFIG_PATH").unwrap_or_else(|_| "./zcgw.toml".to_string());
    let mut config = config::GatewayConfig::load(&config_path)?;
    info!(listen = %config.listen_addr, instances = config.instances.len(), "loaded config");

    let auth_token = std::env::var("ZCGW_AUTH_TOKEN").unwrap_or_default();
    let grpc_secret = std::env::var("ZCGW_GRPC_SECRET").unwrap_or_default();

    if auth_token.is_empty() {
        tracing::warn!("ZCGW_AUTH_TOKEN not set — API endpoints are unprotected");
    }

    // Load docker config from env vars
    let mut docker_env_vars: HashMap<String, String> = std::env::var("ZCGW_DOCKER_ENV_VARS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?.to_string();
            let val = parts.next()?.to_string();
            Some((key, val))
        })
        .collect();

    // Auto-forward well-known API key env vars to new containers
    for key in &[
        "VENICE_API_KEY",
        "OPENROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
    ] {
        if !docker_env_vars.contains_key(*key) {
            if let Ok(val) = std::env::var(key) {
                if !val.is_empty() {
                    docker_env_vars.insert(key.to_string(), val);
                }
            }
        }
    }

    // host_mode: true when gateway runs outside Docker (no Docker socket container)
    let host_mode = std::env::var("ZCGW_HOST_MODE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(true); // default true for local dev

    let agents_dir = std::env::var("ZCGW_AGENTS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("docker/agents"));

    // Host-side path for bind mounts (needed when gateway runs in Docker).
    // Falls back to agents_dir for host-mode operation.
    let host_agents_dir = std::env::var("ZCGW_HOST_AGENTS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| agents_dir.clone());

    let base_port: u16 = std::env::var("ZCGW_BASE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50051);

    let docker_config = docker::DockerConfig {
        image: std::env::var("ZCGW_DOCKER_IMAGE").unwrap_or_else(|_| "zeroclaw:latest".into()),
        network: std::env::var("ZCGW_DOCKER_NETWORK")
            .unwrap_or_else(|_| "zeroclaw-net".into()),
        grpc_port: std::env::var("ZCGW_DOCKER_GRPC_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(50051),
        memory_limit: std::env::var("ZCGW_DOCKER_MEMORY_LIMIT")
            .unwrap_or_else(|_| "512m".into()),
        env_vars: docker_env_vars,
        config_template_path: std::env::var("ZCGW_DOCKER_CONFIG_TEMPLATE")
            .unwrap_or_default(),
        host_mode,
        agents_dir,
        host_agents_dir,
        base_port,
    };

    // Ensure agents directory exists
    tokio::fs::create_dir_all(&docker_config.agents_dir).await?;

    let registry = Arc::new(registry::InstanceRegistry::new(
        config.instances.clone(),
        grpc_secret.clone(),
    ));

    // Ensure all configured instances have containers matching their desired state.
    // This creates missing containers and starts/stops as needed.
    docker::ensure_agents_from_config(&mut config, &config_path, &docker_config).await;

    registry.spawn_health_loop();

    let state = AppState {
        registry,
        auth_token,
        grpc_secret,
        started_at: chrono::Utc::now(),
        config_path,
        docker_config,
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
        // Identity files
        .route(
            "/api/instances/{id}/identity",
            get(api::list_identity).put(api::batch_update_identity),
        )
        .route(
            "/api/instances/{id}/identity/{filename}",
            get(api::get_identity_file)
                .put(api::update_identity_file)
                .delete(api::delete_identity_file),
        )
        .route("/ws/chat", get(ws::ws_chat))
        // Admin endpoints
        .route("/api/admin/stats", get(admin::stats))
        .route("/api/admin/config", get(admin::get_config))
        .route("/api/admin/config", put(admin::update_config))
        .route("/api/admin/instances", get(admin::list_instances))
        .route("/api/admin/instances", post(admin::create_instance))
        .route(
            "/api/admin/instances/{id}/action",
            post(admin::instance_action),
        )
        .route("/api/admin/template", get(admin::get_template))
        // Connectors
        .route(
            "/api/instances/{id}/connectors",
            get(api::get_connectors).put(api::update_connectors),
        )
        // MCP Servers
        .route(
            "/api/instances/{id}/mcp-servers",
            get(api::get_mcp_servers).put(api::update_mcp_servers),
        )
        // Integrations: Composio
        .route(
            "/api/instances/{id}/integrations/composio",
            get(api::get_composio).put(api::update_composio),
        )
        // Integrations: Google (GOGCLI)
        .route(
            "/api/instances/{id}/integrations/google",
            get(api::get_google).put(api::update_google),
        )
        .route(
            "/api/instances/{id}/integrations/google/auth/init",
            post(api::google_auth_init),
        )
        .route(
            "/api/instances/{id}/integrations/google/auth/complete",
            post(api::google_auth_complete),
        )
        .route(
            "/api/instances/{id}/integrations/google/accounts/{email}",
            delete(api::delete_google_account),
        )
        // Cron Jobs
        .route(
            "/api/instances/{id}/cron",
            get(api::list_cron_jobs).post(api::create_cron_job),
        )
        .route(
            "/api/instances/{id}/cron/{job_id}",
            put(api::update_cron_job).delete(api::delete_cron_job),
        )
        .route(
            "/api/instances/{id}/cron/{job_id}/runs",
            get(api::get_cron_runs),
        )
        // Skills
        .route("/api/instances/{id}/skills", get(api::list_skills))
        .route(
            "/api/instances/{id}/skills/{name}",
            put(api::update_skill).delete(api::delete_skill),
        );

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
