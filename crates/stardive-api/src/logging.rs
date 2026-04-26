use std::{fmt, path::Path, time::Instant};

use anyhow::{Context, Result};
use axum::{extract::Request, middleware::Next, response::Response};
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt::time::FormatTime,
    filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter,
};
use tracing_subscriber::Layer;

struct SecondPrecisionTimer;

impl FormatTime for SecondPrecisionTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z"))
    }
}

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
        .with_target(false)
        .with_timer(SecondPrecisionTimer)
        .compact()
        .with_filter(stdout_filter);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .with_timer(SecondPrecisionTimer)
        .compact()
        .with_writer(file_writer)
        .with_filter(LevelFilter::DEBUG);

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .init();

    Ok(guard)
}

pub async fn log_request_response(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let start = Instant::now();
    let should_log = uri.path() != "/up";

    if should_log {
        info!(
            %method,
            %uri,
            "← request received"
        );
    }

    let response = next.run(request).await;
    let status = response.status();
    let elapsed_ms = start.elapsed().as_millis();

    if should_log {
        let outgoing = if status.is_success() || status.is_redirection() {
            "🟢→ response sent"
        } else {
            "🔴→ response sent"
        };

        info!(
            %method,
            %uri,
            status = status.as_u16(),
            elapsed_ms,
            "{outgoing}"
        );
    }

    response
}
