use std::{path::Path, time::Instant};

use anyhow::{Context, Result};
use axum::{extract::Request, middleware::Next, response::Response};
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter,
};
use tracing_subscriber::Layer;
use uuid::Uuid;

pub fn init(log_dir: &Path) -> Result<WorkerGuard> {
    std::fs::create_dir_all(log_dir).with_context(|| {
        format!(
            "failed to create log directory for rotating debug logs: {}",
            log_dir.display()
        )
    })?;

    let file_appender = tracing_appender::rolling::daily(log_dir, "stardive-api.debug.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    let stdout_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // Keep stdout focused for operational visibility.
        EnvFilter::new("stardive_api=info,info")
    });

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_filter(stdout_filter);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(file_writer)
        .with_filter(LevelFilter::DEBUG);

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .init();

    Ok(guard)
}

pub async fn log_request_response(request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let start = Instant::now();

    info!(
        %request_id,
        %method,
        %uri,
        "request received"
    );

    let response = next.run(request).await;
    let status = response.status();
    let elapsed_ms = start.elapsed().as_millis();

    info!(
        %request_id,
        %method,
        %uri,
        status = status.as_u16(),
        elapsed_ms,
        "response sent"
    );

    response
}
