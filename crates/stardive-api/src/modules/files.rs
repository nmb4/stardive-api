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
        .route(
            "/files/{id}",
            get(download_file).put(update_file).delete(delete_file),
        )
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
    multipart: Multipart,
) -> ApiResult<impl IntoResponse> {
    let id = Uuid::new_v4().simple().to_string();
    let blob_path = state.file_store.blob_path(&id);
    let metadata = write_multipart_file(multipart, id, Utc::now(), &blob_path).await?;

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

async fn update_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    multipart: Multipart,
) -> ApiResult<impl IntoResponse> {
    let existing = state
        .file_store
        .get(&id)
        .await
        .ok_or_else(|| ApiError::not_found("file not found"))?;

    let blob_path = state.file_store.blob_path(&id);
    let metadata =
        write_multipart_file(multipart, id.clone(), existing.created_at, &blob_path).await?;

    let updated = state
        .file_store
        .update(&id, metadata)
        .await
        .map_err(|err| ApiError::internal(format!("failed to persist metadata: {err}")))?
        .ok_or_else(|| ApiError::not_found("file not found"))?;

    Ok(axum::Json(UploadResponse { file: updated }))
}

async fn delete_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    state
        .file_store
        .delete(&id)
        .await
        .map_err(|err| ApiError::internal(format!("failed to delete metadata: {err}")))?
        .ok_or_else(|| ApiError::not_found("file not found"))?;

    match tokio::fs::remove_file(state.file_store.blob_path(&id)).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(ApiError::internal(format!(
                "failed to delete file content: {err}"
            )));
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn write_multipart_file(
    mut multipart: Multipart,
    id: String,
    created_at: chrono::DateTime<Utc>,
    blob_path: &std::path::Path,
) -> ApiResult<FileMetadata> {
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

    let tmp_path = blob_path.with_extension(format!("{}.tmp", Uuid::new_v4().simple()));

    let mut file = tokio::fs::File::create(&tmp_path)
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

        file.write_all(&chunk)
            .await
            .map_err(|err| ApiError::internal(format!("failed to write uploaded chunk: {err}")))?;
        hasher.update(&chunk);
    }

    file.flush()
        .await
        .map_err(|err| ApiError::internal(format!("failed to flush uploaded file: {err}")))?;
    drop(file);

    tokio::fs::rename(&tmp_path, blob_path)
        .await
        .map_err(|err| ApiError::internal(format!("failed to store uploaded file: {err}")))?;

    let sha256 = format!("{:x}", hasher.finalize());
    Ok(FileMetadata {
        id,
        original_name,
        size,
        mime_type,
        sha256,
        created_at,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        extract::DefaultBodyLimit,
        http::{Method, Request},
    };
    use tower::ServiceExt;

    use crate::{
        app_state::{RuntimeTools, ToolStatus},
        command_runner::SystemCommandRunner,
        config::{ModuleFlags, ServerConfig},
        modules::{ModuleDef, lostandfound, orbit},
    };

    #[tokio::test]
    async fn routes_upload_list_update_download_and_delete_file() {
        let app = test_router().await;

        let upload = app
            .clone()
            .oneshot(multipart_request(
                Method::POST,
                "/files",
                "hello.txt",
                "text/plain",
                b"hello",
            ))
            .await
            .expect("upload response");
        assert_eq!(upload.status(), StatusCode::CREATED);
        let body = to_bytes(upload.into_body(), usize::MAX)
            .await
            .expect("upload body");
        let created: UploadResponse = serde_json::from_slice(&body).expect("upload json");
        assert_eq!(created.file.original_name, "hello.txt");
        assert_eq!(created.file.size, 5);

        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/files")
                    .body(Body::empty())
                    .expect("list request"),
            )
            .await
            .expect("list response");
        assert_eq!(list.status(), StatusCode::OK);
        let body = to_bytes(list.into_body(), usize::MAX)
            .await
            .expect("list body");
        let list: FileListResponse = serde_json::from_slice(&body).expect("list json");
        assert_eq!(list.files.len(), 1);

        let update_uri = format!("/files/{}", created.file.id);
        let update = app
            .clone()
            .oneshot(multipart_request(
                Method::PUT,
                &update_uri,
                "updated.bin",
                "application/octet-stream",
                b"updated contents",
            ))
            .await
            .expect("update response");
        assert_eq!(update.status(), StatusCode::OK);
        let body = to_bytes(update.into_body(), usize::MAX)
            .await
            .expect("update body");
        let updated: UploadResponse = serde_json::from_slice(&body).expect("update json");
        assert_eq!(updated.file.id, created.file.id);
        assert_eq!(updated.file.original_name, "updated.bin");
        assert_eq!(updated.file.size, 16);

        let download = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(&update_uri)
                    .body(Body::empty())
                    .expect("download request"),
            )
            .await
            .expect("download response");
        assert_eq!(download.status(), StatusCode::OK);
        let body = to_bytes(download.into_body(), usize::MAX)
            .await
            .expect("download body");
        assert_eq!(&body[..], b"updated contents");

        let delete = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(&update_uri)
                    .body(Body::empty())
                    .expect("delete request"),
            )
            .await
            .expect("delete response");
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);

        let missing = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(&update_uri)
                    .body(Body::empty())
                    .expect("missing request"),
            )
            .await
            .expect("missing response");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    fn multipart_request(
        method: Method,
        uri: &str,
        name: &str,
        content_type: &str,
        content: &[u8],
    ) -> Request<Body> {
        let boundary = format!("files-test-{}", Uuid::new_v4().simple());
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("content-disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(format!("content-type: {content_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(content);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        Request::builder()
            .method(method)
            .uri(uri)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .expect("multipart request")
    }

    async fn test_router() -> Router {
        let temp = std::env::temp_dir().join(format!("stardive-files-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.expect("data dir");
        let config = Arc::new(ServerConfig {
            bind_addr: "127.0.0.1:0".parse().expect("addr"),
            data_dir: temp.clone(),
            log_dir: temp.join("logs"),
            installers_dir: temp.join("installers"),
            eternal_dir: temp.join("eternal"),
            api_key: None,
            max_upload_bytes: 1,
            max_snippet_chars: 20_000,
            modules: ModuleFlags {
                health: true,
                search: true,
                files: true,
                render: true,
                lostandfound: true,
                orbit: true,
                installers: true,
                eternal: true,
            },
        });
        let file_store = Arc::new(
            crate::file_store::FileStore::new(config.data_dir.clone())
                .await
                .expect("file store"),
        );
        let orbit_store = orbit::new_store(config.data_dir.clone())
            .await
            .expect("orbit store");
        let state = AppState::new(
            config,
            file_store,
            RuntimeTools {
                ddgs: ToolStatus {
                    available: false,
                    path: None,
                },
                freeze: ToolStatus {
                    available: false,
                    path: None,
                },
                opencode: ToolStatus {
                    available: false,
                    path: None,
                },
            },
            Arc::new(SystemCommandRunner),
            Arc::new(Vec::<ModuleDef>::new()),
            lostandfound::new_store(),
            orbit_store,
        );

        register(Router::new())
            .layer(DefaultBodyLimit::disable())
            .with_state(state)
    }
}
