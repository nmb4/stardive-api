mod app_state;
mod auth;
mod command_runner;
mod config;
mod error;
mod file_store;
mod logging;
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
    let config = ServerConfig::from_env()?;
    let _file_log_guard = logging::init(&config.log_dir)?;
    let file_store = FileStore::new(config.data_dir.clone()).await?;
    let tools = app_state::RuntimeTools::detect();

    let module_defs = registry();
    let available_modules = module_defs.iter().map(|def| def.name).collect::<Vec<_>>();
    let enabled: Vec<ModuleDef> = module_defs
        .iter()
        .copied()
        .filter(|def| (def.enabled)(&config.modules))
        .collect();
    let enabled_modules = enabled.iter().map(|def| def.name).collect::<Vec<_>>();

    info!(
        bind_addr = %config.bind_addr,
        data_dir = %config.data_dir.display(),
        log_dir = %config.log_dir.display(),
        installers_dir = %config.installers_dir.display(),
        eternal_dir = %config.eternal_dir.display(),
        api_key_set = config.api_key.is_some(),
        public_mode = config.public_mode(),
        max_upload_bytes = config.max_upload_bytes,
        max_snippet_chars = config.max_snippet_chars,
        health_enabled = config.modules.health,
        search_enabled = config.modules.search,
        files_enabled = config.modules.files,
        render_enabled = config.modules.render,
        lostandfound_enabled = config.modules.lostandfound,
        installers_enabled = config.modules.installers,
        eternal_enabled = config.modules.eternal,
        "startup configuration loaded"
    );
    info!(
        available_modules = ?available_modules,
        enabled_modules = ?enabled_modules,
        "module registry initialized"
    );

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
        .nest("/v1", v1.with_state(state.clone()))
        .layer(middleware::from_fn(logging::log_request_response));

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
