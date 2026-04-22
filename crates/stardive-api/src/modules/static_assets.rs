use std::path::Path;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use stardive_core::types::{ModuleCapability, StaticFileEntry, StaticFileListResponse};

use crate::{
    app_state::AppState,
    config::ModuleFlags,
    error::{ApiError, ApiResult},
    modules::ModuleDef,
};

pub fn installers_module_def() -> ModuleDef {
    ModuleDef {
        name: "installers",
        register: register_installers,
        capability: installers_capability,
        enabled: |flags: &ModuleFlags| flags.installers,
    }
}

pub fn eternal_module_def() -> ModuleDef {
    ModuleDef {
        name: "eternal",
        register: register_eternal,
        capability: eternal_capability,
        enabled: |flags: &ModuleFlags| flags.eternal,
    }
}

fn register_installers(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/installers", get(list_installers))
        .route("/installers/{name}", get(get_installer))
}

fn register_eternal(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/eternal", get(list_eternal))
        .route("/eternal/{name}", get(get_eternal))
}

fn installers_capability(state: &AppState) -> ModuleCapability {
    folder_capability("installers", &state.config.installers_dir)
}

fn eternal_capability(state: &AppState) -> ModuleCapability {
    folder_capability("eternal", &state.config.eternal_dir)
}

fn folder_capability(name: &str, dir: &Path) -> ModuleCapability {
    ModuleCapability {
        name: name.to_string(),
        enabled: true,
        healthy: dir.exists(),
        detail: Some(if dir.exists() {
            format!("serving from {}", dir.display())
        } else {
            format!("missing directory {}", dir.display())
        }),
    }
}

async fn list_installers(State(state): State<AppState>) -> ApiResult<Json<StaticFileListResponse>> {
    list_folder(&state.config.installers_dir).await
}

async fn get_installer(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> ApiResult<Response> {
    get_file_response(&state.config.installers_dir, &name).await
}

async fn list_eternal(State(state): State<AppState>) -> ApiResult<Json<StaticFileListResponse>> {
    list_folder(&state.config.eternal_dir).await
}

async fn get_eternal(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> ApiResult<Response> {
    get_file_response(&state.config.eternal_dir, &name).await
}

async fn list_folder(dir: &Path) -> ApiResult<Json<StaticFileListResponse>> {
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|err| ApiError::internal(format!("failed to read {}: {err}", dir.display())))?;

    let mut files = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| ApiError::internal(format!("failed to read dir entry: {err}")))?
    {
        let path = entry.path();
        let md = entry
            .metadata()
            .await
            .map_err(|err| ApiError::internal(format!("failed to read metadata: {err}")))?;
        if !md.is_file() {
            continue;
        }
        let modified_at = md.modified().ok().map(DateTime::<Utc>::from);

        files.push(StaticFileEntry {
            name: path
                .file_name()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or_default(),
            size: md.len(),
            modified_at,
        });
    }

    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(StaticFileListResponse { files }))
}

async fn get_file_response(dir: &Path, name: &str) -> ApiResult<Response> {
    validate_static_name(name)?;

    let path = dir.join(name);
    let canonical_root = std::fs::canonicalize(dir)
        .map_err(|err| ApiError::internal(format!("failed to resolve root dir: {err}")))?;
    let canonical_path =
        std::fs::canonicalize(&path).map_err(|_| ApiError::not_found("file not found"))?;

    if !canonical_path.starts_with(&canonical_root) {
        return Err(ApiError::bad_request("invalid file path"));
    }

    let bytes = tokio::fs::read(&canonical_path)
        .await
        .map_err(|_| ApiError::not_found("file not found"))?;

    let mime = mime_from_name(name);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));

    Ok((headers, Body::from(bytes)).into_response())
}

fn validate_static_name(name: &str) -> ApiResult<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(ApiError::bad_request("invalid file name"));
    }
    Ok(())
}

fn mime_from_name(name: &str) -> &'static str {
    if name.ends_with(".sh") {
        "text/x-shellscript; charset=utf-8"
    } else if name.ends_with(".md") {
        "text/markdown; charset=utf-8"
    } else if name.ends_with(".txt") {
        "text/plain; charset=utf-8"
    } else if name.ends_with(".json") {
        "application/json"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_path_traversal() {
        assert!(validate_static_name("../bad").is_err());
        assert!(validate_static_name("ok.sh").is_ok());
    }

    #[test]
    fn mime_resolution_works() {
        assert!(mime_from_name("x.sh").starts_with("text/x-shellscript"));
        assert_eq!(mime_from_name("x.bin"), "application/octet-stream");
    }
}
