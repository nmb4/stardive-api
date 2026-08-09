use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
};
use stardive_core::types::ModuleCapability;

use crate::{app_state::AppState, config::ModuleFlags, error::ApiError};

use super::ModuleDef;

pub fn module_def() -> ModuleDef {
    ModuleDef {
        name: "obscura",
        register,
        capability,
        enabled: |flags: &ModuleFlags| flags.obscura,
    }
}

fn register(router: Router<AppState>) -> Router<AppState> {
    router.route("/obscura/mcp", any(proxy_mcp))
}

fn capability(state: &AppState) -> ModuleCapability {
    ModuleCapability {
        name: "obscura".to_string(),
        enabled: true,
        healthy: true,
        detail: Some(format!(
            "HTTP MCP proxy configured for {}; backend availability is checked per request",
            state.config.obscura_mcp_url
        )),
    }
}

async fn proxy_mcp(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let mut request = state
        .http_client
        .request(method, &state.config.obscura_mcp_url)
        .body(reqwest::Body::wrap_stream(body.into_data_stream()));

    for (name, value) in &headers {
        if !is_hop_by_hop(name.as_str()) && name.as_str() != "host" {
            request = request.header(name, value);
        }
    }

    let upstream = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return ApiError::service_unavailable(format!(
                "obscura MCP backend unavailable: {error}"
            ))
            .with_code("obscura_unavailable")
            .into_response();
        }
    };

    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let mut response = Response::builder().status(status);
    for (name, value) in &upstream_headers {
        if !is_hop_by_hop(name.as_str()) && name.as_str() != "content-length" {
            response = response.header(name, value);
        }
    }

    match response.body(Body::from_stream(upstream.bytes_stream())) {
        Ok(response) => response,
        Err(error) => ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("invalid response from obscura MCP backend: {error}"),
        )
        .with_code("obscura_invalid_response")
        .into_response(),
    }
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{Bytes, to_bytes},
        http::{Request, header},
        routing::post,
    };
    use tower::ServiceExt;

    use crate::{
        app_state::RuntimeTools,
        command_runner::SystemCommandRunner,
        config::ServerConfig,
        file_store::FileStore,
        modules::{self, lostandfound, notifications, orbit},
    };

    use super::*;

    #[test]
    fn filters_hop_by_hop_headers() {
        assert!(is_hop_by_hop("connection"));
        assert!(is_hop_by_hop("transfer-encoding"));
        assert!(!is_hop_by_hop("mcp-session-id"));
        assert!(!is_hop_by_hop("last-event-id"));
    }

    #[tokio::test]
    async fn route_proxies_mcp_body_and_session_headers() {
        let upstream = Router::new().route(
            "/mcp",
            post(|headers: HeaderMap, body: Bytes| async move {
                assert_eq!(headers.get("mcp-protocol-version").unwrap(), "2025-03-26");
                (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, "application/json"),
                        (
                            header::HeaderName::from_static("mcp-session-id"),
                            "test-session",
                        ),
                    ],
                    body,
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock obscura");
        let address = listener.local_addr().expect("mock address");
        let upstream_task =
            tokio::spawn(
                async move { axum::serve(listener, upstream).await.expect("mock server") },
            );

        let temp = tempfile::tempdir().expect("tempdir");
        let data_dir = temp.path().join("data");
        let config = Arc::new(ServerConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_dir: data_dir.clone(),
            log_dir: data_dir.join("logs"),
            installers_dir: temp.path().join("installers"),
            eternal_dir: temp.path().join("eternal"),
            api_key: None,
            max_upload_bytes: 1024,
            max_snippet_chars: 1024,
            vapid_subject: "mailto:test@example.com".to_string(),
            obscura_mcp_url: format!("http://{address}/mcp"),
            modules: ModuleFlags {
                health: true,
                search: true,
                files: true,
                render: true,
                lostandfound: true,
                orbit: true,
                notifications: true,
                installers: true,
                eternal: true,
                obscura: true,
            },
        });
        let notification_store =
            notifications::new_store(data_dir.clone(), config.vapid_subject.clone())
                .await
                .expect("notification store");
        let state = AppState::new(
            config,
            Arc::new(FileStore::new(data_dir.clone()).await.expect("file store")),
            RuntimeTools::detect(),
            Arc::new(SystemCommandRunner),
            Arc::new(modules::registry()),
            lostandfound::new_store(),
            orbit::new_store(data_dir).await.expect("orbit store"),
            notification_store,
        );
        let app = register(Router::new()).with_state(state);
        let payload = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;

        let response = app
            .oneshot(
                Request::post("/obscura/mcp")
                    .header("content-type", "application/json")
                    .header("accept", "application/json, text/event-stream")
                    .header("mcp-protocol-version", "2025-03-26")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .expect("proxy response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("mcp-session-id").unwrap(),
            "test-session"
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert_eq!(body, payload);

        upstream_task.abort();
    }
}
