use axum::{
    Router,
    body::Body,
    extract::{Multipart, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use stardive_core::types::{FileListResponse, FileMetadata, ModuleCapability, UploadResponse};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    config::ModuleFlags,
    error::{ApiError, ApiResult},
};

use super::ModuleDef;

pub fn module_def() -> ModuleDef {
    ModuleDef {
        name: "files",
        register,
        capability,
        enabled: |flags: &ModuleFlags| flags.files,
    }
}

fn register(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/files", post(upload_file).get(list_files))
        .route("/files/{id}", get(download_file))
}

fn capability(_: &AppState) -> ModuleCapability {
    ModuleCapability {
        name: "files".to_string(),
        enabled: true,
        healthy: true,
        detail: Some("local file store ready".to_string()),
    }
}

async fn upload_file(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<impl IntoResponse> {
    let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad_request(format!("invalid multipart payload: {err}")))?
    else {
        return Err(ApiError::bad_request("missing file field"));
    };

    let original_name = field
        .file_name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "upload.bin".to_string());
    let mime_type = field
        .content_type()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let id = Uuid::new_v4().simple().to_string();
    let blob_path = state.file_store.blob_path(&id);

    let mut file = tokio::fs::File::create(&blob_path)
        .await
        .map_err(|err| ApiError::internal(format!("failed to create blob file: {err}")))?;

    let mut size = 0_u64;
    let mut hasher = Sha256::new();

    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|err| ApiError::bad_request(format!("failed to read multipart chunk: {err}")))?
    {
        size = size.saturating_add(chunk.len() as u64);
        if size > state.config.max_upload_bytes {
            let _ = tokio::fs::remove_file(&blob_path).await;
            return Err(ApiError::payload_too_large(format!(
                "file exceeds max size of {} bytes",
                state.config.max_upload_bytes
            )));
        }

        file.write_all(&chunk)
            .await
            .map_err(|err| ApiError::internal(format!("failed to write uploaded chunk: {err}")))?;
        hasher.update(&chunk);
    }

    file.flush()
        .await
        .map_err(|err| ApiError::internal(format!("failed to flush uploaded file: {err}")))?;

    let sha256 = format!("{:x}", hasher.finalize());
    let metadata = FileMetadata {
        id,
        original_name,
        size,
        mime_type,
        sha256,
        created_at: Utc::now(),
    };

    state
        .file_store
        .insert(metadata.clone())
        .await
        .map_err(|err| ApiError::internal(format!("failed to persist metadata: {err}")))?;

    Ok((
        StatusCode::CREATED,
        axum::Json(UploadResponse { file: metadata }),
    ))
}

async fn list_files(State(state): State<AppState>) -> ApiResult<axum::Json<FileListResponse>> {
    let files = state.file_store.list().await;
    Ok(axum::Json(FileListResponse { files }))
}

async fn download_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let metadata = state
        .file_store
        .get(&id)
        .await
        .ok_or_else(|| ApiError::not_found("file not found"))?;

    let blob_path = state.file_store.blob_path(&id);
    let file = tokio::fs::File::open(&blob_path)
        .await
        .map_err(|_| ApiError::not_found("file content missing"))?;

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&metadata.mime_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    let safe_name = metadata.original_name.replace('"', "");
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{}\"", safe_name)) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }

    Ok((headers, body).into_response())
}
