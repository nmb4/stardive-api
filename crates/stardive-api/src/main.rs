mod app_state;
mod auth;
mod command_runner;
mod config;
mod error;
mod file_store;
mod modules;

use std::sync::Arc;

use anyhow::Context;
use app_state::AppState;
use axum::{Router, extract::DefaultBodyLimit, http::StatusCode, middleware, routing::get};
use command_runner::SystemCommandRunner;
use config::ServerConfig;
use file_store::FileStore;
use modules::{ModuleDef, registry};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "stardive_api=info,info".to_string()),
        )
        .init();

    let config = ServerConfig::from_env()?;
    let file_store = FileStore::new(config.data_dir.clone()).await?;
    let tools = app_state::RuntimeTools::detect();

    let module_defs = registry();
    let enabled: Vec<ModuleDef> = module_defs
        .iter()
        .copied()
        .filter(|def| (def.enabled)(&config.modules))
        .collect();

    let state = AppState::new(
        Arc::new(config.clone()),
        Arc::new(file_store),
        tools,
        Arc::new(SystemCommandRunner),
        Arc::new(module_defs),
        modules::lostandfound::new_store(),
    );

    let mut v1: Router<AppState> = Router::new();
    for def in enabled {
        v1 = (def.register)(v1);
    }

    let body_limit = usize::try_from(config.max_upload_bytes).unwrap_or(usize::MAX);
    let v1 = v1
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ));

    let app = Router::new()
        .route("/up", get(up))
        .nest("/v1", v1.with_state(state.clone()));

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("failed to bind to {}", config.bind_addr))?;

    info!("stardive-api listening on {}", config.bind_addr);
    axum::serve(listener, app)
        .await
        .context("api server stopped unexpectedly")?;
    Ok(())
}

async fn up() -> StatusCode {
    StatusCode::OK
}
