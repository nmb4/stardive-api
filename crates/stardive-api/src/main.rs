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
use axum::{
    Router,
    body::{Body, Bytes},
    extract::DefaultBodyLimit,
    http::{Method, StatusCode, header},
    middleware,
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use command_runner::SystemCommandRunner;
use config::ServerConfig;
use file_store::FileStore;
use modules::{ModuleDef, registry};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

const FILES_WEBAPP_HTML: &str = include_str!("../../../files-webapp.html");
const NOTIFY_HTML: &str = include_str!("../../../notify/index.html");
const NOTIFY_CSS: &str = include_str!("../../../notify/styles.css");
const NOTIFY_JS: &str = include_str!("../../../notify/app.js");
const NOTIFY_SERVICE_WORKER: &str = include_str!("../../../notify/service-worker.js");
const NOTIFY_MANIFEST: &str = include_str!("../../../notify/manifest.webmanifest");
const NOTIFY_ICON_SVG: &str = include_str!("../../../notify/icons/icon.svg");
const NOTIFY_ICON_192: &[u8] = include_bytes!("../../../notify/icons/icon-192.png");
const NOTIFY_ICON_512: &[u8] = include_bytes!("../../../notify/icons/icon-512.png");
const NOTIFY_APPLE_ICON: &[u8] = include_bytes!("../../../notify/icons/apple-touch-icon.png");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ServerConfig::from_env()?;
    let _file_log_guard = logging::init(&config.log_dir)?;
    let file_store = FileStore::new(config.data_dir.clone()).await?;
    let orbit_store = modules::orbit::new_store(config.data_dir.clone()).await?;
    let notification_store =
        modules::notifications::new_store(config.data_dir.clone(), config.vapid_subject.clone())
            .await?;
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
        orbit_enabled = config.modules.orbit,
        notifications_enabled = config.modules.notifications,
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
        orbit_store,
        notification_store,
    );

    let mut v1: Router<AppState> = Router::new();
    for def in enabled {
        v1 = (def.register)(v1);
    }

    let v1 = v1
        .layer(DefaultBodyLimit::disable())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ));

    let app = Router::new()
        .route("/up", get(up))
        .route("/", get(files_webapp))
        .route("/files-webapp", get(files_webapp))
        .route("/files-webapp.html", get(files_webapp))
        .route("/notify", get(notify_redirect))
        .route("/notify/", get(notify_index))
        .route("/notify/styles.css", get(notify_css))
        .route("/notify/app.js", get(notify_js))
        .route("/notify/service-worker.js", get(notify_service_worker))
        .route("/notify/manifest.webmanifest", get(notify_manifest))
        .route("/notify/icons/icon.svg", get(notify_icon_svg))
        .route("/notify/icons/icon-192.png", get(notify_icon_192))
        .route("/notify/icons/icon-512.png", get(notify_icon_512))
        .route("/notify/icons/apple-touch-icon.png", get(notify_apple_icon))
        .nest("/v1", v1.with_state(state.clone()))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers(Any),
        )
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

async fn files_webapp() -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "public, max-age=0, must-revalidate")],
        Html(FILES_WEBAPP_HTML),
    )
}

async fn notify_redirect() -> Redirect {
    Redirect::permanent("/notify/")
}

async fn notify_index() -> Response {
    notify_asset(
        "text/html; charset=utf-8",
        Bytes::from_static(NOTIFY_HTML.as_bytes()),
    )
}

async fn notify_css() -> Response {
    notify_asset(
        "text/css; charset=utf-8",
        Bytes::from_static(NOTIFY_CSS.as_bytes()),
    )
}

async fn notify_js() -> Response {
    notify_asset(
        "text/javascript; charset=utf-8",
        Bytes::from_static(NOTIFY_JS.as_bytes()),
    )
}

async fn notify_service_worker() -> Response {
    let mut response = notify_asset(
        "text/javascript; charset=utf-8",
        Bytes::from_static(NOTIFY_SERVICE_WORKER.as_bytes()),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("service-worker-allowed"),
        header::HeaderValue::from_static("/notify/"),
    );
    response
}

async fn notify_manifest() -> Response {
    notify_asset(
        "application/manifest+json",
        Bytes::from_static(NOTIFY_MANIFEST.as_bytes()),
    )
}

async fn notify_icon_svg() -> Response {
    notify_asset(
        "image/svg+xml",
        Bytes::from_static(NOTIFY_ICON_SVG.as_bytes()),
    )
}

async fn notify_icon_192() -> Response {
    notify_asset("image/png", Bytes::from_static(NOTIFY_ICON_192))
}

async fn notify_icon_512() -> Response {
    notify_asset("image/png", Bytes::from_static(NOTIFY_ICON_512))
}

async fn notify_apple_icon() -> Response {
    notify_asset("image/png", Bytes::from_static(NOTIFY_APPLE_ICON))
}

fn notify_asset(content_type: &'static str, content: Bytes) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "public, max-age=0, must-revalidate")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'self'; connect-src 'self' https: http://localhost:*; img-src 'self' https: data:; style-src 'self'; script-src 'self'; worker-src 'self'; manifest-src 'self'",
        )
        .body(Body::from(content))
        .expect("valid embedded notification asset response")
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn files_webapp_routes_serve_embedded_html() {
        let app = Router::new()
            .route("/", get(files_webapp))
            .route("/files-webapp", get(files_webapp))
            .route("/files-webapp.html", get(files_webapp));

        for uri in ["/", "/files-webapp", "/files-webapp.html"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers().get(header::CACHE_CONTROL).unwrap(),
                "public, max-age=0, must-revalidate"
            );

            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body");
            let html = std::str::from_utf8(&body).expect("utf8 html");
            assert!(html.contains("STARDIVE | Isometric Files Vault"));
            assert!(html.contains("https://api.stardive.space"));
        }
    }

    #[tokio::test]
    async fn notification_webapp_routes_serve_embedded_pwa() {
        let app = Router::new()
            .route("/notify", get(notify_redirect))
            .route("/notify/", get(notify_index))
            .route("/notify/styles.css", get(notify_css))
            .route("/notify/app.js", get(notify_js))
            .route("/notify/service-worker.js", get(notify_service_worker))
            .route("/notify/manifest.webmanifest", get(notify_manifest))
            .route("/notify/icons/icon-192.png", get(notify_icon_192));

        let redirect = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/notify")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("redirect response");
        assert!(redirect.status().is_redirection());
        assert_eq!(
            redirect.headers().get(header::LOCATION).unwrap(),
            "/notify/"
        );

        for (uri, content_type) in [
            ("/notify/", "text/html; charset=utf-8"),
            ("/notify/styles.css", "text/css; charset=utf-8"),
            ("/notify/app.js", "text/javascript; charset=utf-8"),
            (
                "/notify/service-worker.js",
                "text/javascript; charset=utf-8",
            ),
            ("/notify/manifest.webmanifest", "application/manifest+json"),
            ("/notify/icons/icon-192.png", "image/png"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("asset response");
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE).unwrap(),
                content_type
            );
            assert_eq!(
                response
                    .headers()
                    .get(header::X_CONTENT_TYPE_OPTIONS)
                    .unwrap(),
                "nosniff"
            );
            if uri.ends_with("service-worker.js") {
                assert_eq!(
                    response.headers().get("service-worker-allowed").unwrap(),
                    "/notify/"
                );
            }
        }
    }
}
