use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, header},
    response::IntoResponse,
    routing::post,
};
use stardive_core::types::{ModuleCapability, RenderSnippetRequest};

use crate::{
    app_state::AppState,
    config::ModuleFlags,
    error::{ApiError, ApiResult},
};

use super::ModuleDef;

pub fn module_def() -> ModuleDef {
    ModuleDef {
        name: "render",
        register,
        capability,
        enabled: |flags: &ModuleFlags| flags.render,
    }
}

fn register(router: Router<AppState>) -> Router<AppState> {
    router.route("/render/snippet", post(render_snippet))
}

fn capability(state: &AppState) -> ModuleCapability {
    ModuleCapability {
        name: "render".to_string(),
        enabled: true,
        healthy: state.tools.freeze.available,
        detail: if state.tools.freeze.available {
            Some("freeze available".to_string())
        } else {
            Some("freeze unavailable".to_string())
        },
    }
}

async fn render_snippet(
    State(state): State<AppState>,
    Json(payload): Json<RenderSnippetRequest>,
) -> ApiResult<impl IntoResponse> {
    if !state.tools.freeze.available {
        return Err(
            ApiError::service_unavailable("freeze is not available on this server")
                .with_code("freeze_unavailable"),
        );
    }

    if payload.code.trim().is_empty() {
        return Err(ApiError::bad_request("code is required"));
    }

    if payload.code.chars().count() > state.config.max_snippet_chars {
        return Err(ApiError::bad_request(format!(
            "code exceeds max length of {} characters",
            state.config.max_snippet_chars
        )));
    }

    let tmp = tempfile::tempdir()
        .map_err(|err| ApiError::internal(format!("failed to create temp dir: {err}")))?;

    let ext = payload
        .language
        .as_deref()
        .map(sanitize_language)
        .unwrap_or_else(|| "txt".to_string());
    let source_path = tmp.path().join(format!("snippet.{}", ext));
    let output_path = tmp
        .path()
        .join(format!("snippet.{}", payload.format.extension()));

    tokio::fs::write(&source_path, payload.code.as_bytes())
        .await
        .map_err(|err| ApiError::internal(format!("failed to write snippet file: {err}")))?;

    let mut args = vec![
        source_path.to_string_lossy().to_string(),
        "-o".to_string(),
        output_path.to_string_lossy().to_string(),
    ];

    if let Some(language) = payload.language {
        args.extend(["-l".to_string(), language]);
    }
    if let Some(theme) = payload.theme {
        args.extend(["-t".to_string(), theme]);
    }

    let output = state
        .command_runner
        .run("freeze", &args)
        .await
        .map_err(|err| ApiError::bad_gateway(format!("freeze invocation failed: {err}")))?;

    if output.status != 0 {
        return Err(ApiError::bad_gateway(format!(
            "freeze failed (status {}): {}",
            output.status,
            truncate(&output.stderr)
        )));
    }

    let bytes = tokio::fs::read(&output_path)
        .await
        .map_err(|err| ApiError::internal(format!("failed to read rendered output: {err}")))?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(payload.format.content_type()),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));

    Ok((headers, bytes))
}

fn sanitize_language(lang: &str) -> String {
    let clean = lang
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    if clean.is_empty() {
        "txt".to_string()
    } else {
        clean.to_ascii_lowercase()
    }
}

fn truncate(input: &str) -> String {
    const MAX: usize = 300;
    if input.len() <= MAX {
        input.to_string()
    } else {
        format!("{}...", &input[..MAX])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_language_keeps_ascii_alnum() {
        assert_eq!(sanitize_language("TypeScript!"), "typescript");
        assert_eq!(sanitize_language("@@@"), "txt");
    }
}
