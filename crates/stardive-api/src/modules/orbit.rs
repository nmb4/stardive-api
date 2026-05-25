use std::{path::Path, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::Body,
    extract::{Multipart, Path as AxumPath, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};
use stardive_core::types::{
    ModuleCapability, OrbitScriptListResponse, OrbitScriptMetadata, OrbitScriptSource,
    OrbitScriptStatus, OrbitUploadResponse, OrbitVibecodeRequest, OrbitVibecodeResponse,
};
use tokio::{io::AsyncWriteExt, sync::Mutex};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    config::ModuleFlags,
    error::{ApiError, ApiResult},
};

use super::ModuleDef;

#[derive(Debug)]
pub(crate) struct OrbitStore {
    scripts_dir: std::path::PathBuf,
    jobs_dir: std::path::PathBuf,
    index_path: std::path::PathBuf,
    scripts: Mutex<Vec<OrbitScriptMetadata>>,
}

impl OrbitStore {
    pub async fn new(data_root: std::path::PathBuf) -> Result<Self> {
        let root_dir = data_root.join("orbit");
        let scripts_dir = root_dir.join("scripts");
        let jobs_dir = root_dir.join("jobs");
        tokio::fs::create_dir_all(&scripts_dir)
            .await
            .with_context(|| format!("failed to create orbit dir {}", scripts_dir.display()))?;
        tokio::fs::create_dir_all(&jobs_dir)
            .await
            .with_context(|| format!("failed to create orbit jobs dir {}", jobs_dir.display()))?;

        let index_path = root_dir.join("index.json");
        let existing = if index_path.exists() {
            let raw = tokio::fs::read_to_string(&index_path)
                .await
                .with_context(|| format!("failed to read index {}", index_path.display()))?;
            serde_json::from_str::<Vec<OrbitScriptMetadata>>(&raw)
                .context("invalid orbit index json")?
        } else {
            Vec::new()
        };

        Ok(Self {
            scripts_dir,
            jobs_dir,
            index_path,
            scripts: Mutex::new(existing),
        })
    }

    fn script_path(&self, id: &str) -> std::path::PathBuf {
        self.scripts_dir.join(format!("{id}.lua"))
    }

    fn job_dir(&self, id: &str) -> std::path::PathBuf {
        self.jobs_dir.join(id)
    }

    async fn insert(&self, script: OrbitScriptMetadata) -> Result<()> {
        let mut guard = self.scripts.lock().await;
        guard.retain(|candidate| candidate.id != script.id);
        guard.push(script);
        guard.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        self.persist(&guard).await
    }

    async fn list(&self) -> Vec<OrbitScriptMetadata> {
        self.scripts.lock().await.clone()
    }

    async fn get(&self, id: &str) -> Option<OrbitScriptMetadata> {
        self.scripts
            .lock()
            .await
            .iter()
            .find(|script| script.id == id)
            .cloned()
    }

    async fn get_by_id_or_name(&self, value: &str) -> Option<OrbitScriptMetadata> {
        let guard = self.scripts.lock().await;
        guard
            .iter()
            .find(|script| script.id == value)
            .or_else(|| guard.iter().find(|script| script.name == value))
            .cloned()
    }

    async fn mark_generating(&self, id: &str) -> Result<()> {
        self.update(id, |script| {
            script.status = OrbitScriptStatus::Generating;
            script.updated_at = Utc::now();
        })
        .await
    }

    async fn mark_ready(&self, id: &str, size: u64, sha256: String) -> Result<()> {
        self.update(id, |script| {
            script.status = OrbitScriptStatus::Ready;
            script.size = size;
            script.sha256 = sha256;
            script.error = None;
            script.updated_at = Utc::now();
        })
        .await
    }

    async fn mark_failed(&self, id: &str, error: String) -> Result<()> {
        self.update(id, |script| {
            script.status = OrbitScriptStatus::Failed;
            script.error = Some(error);
            script.updated_at = Utc::now();
        })
        .await
    }

    async fn update<F>(&self, id: &str, mutate: F) -> Result<()>
    where
        F: FnOnce(&mut OrbitScriptMetadata),
    {
        let mut guard = self.scripts.lock().await;
        let script = guard
            .iter_mut()
            .find(|script| script.id == id)
            .with_context(|| format!("orbit script {id} not found"))?;
        mutate(script);
        guard.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        self.persist(&guard).await
    }

    async fn persist(&self, scripts: &[OrbitScriptMetadata]) -> Result<()> {
        let raw = serde_json::to_string_pretty(scripts).context("failed to encode orbit index")?;
        let tmp_path = self.index_path.with_extension("json.tmp");
        tokio::fs::write(&tmp_path, raw)
            .await
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
        tokio::fs::rename(&tmp_path, &self.index_path)
            .await
            .with_context(|| {
                format!(
                    "failed to move {} to {}",
                    tmp_path.display(),
                    self.index_path.display()
                )
            })?;
        Ok(())
    }
}

pub fn module_def() -> ModuleDef {
    ModuleDef {
        name: "orbit",
        register,
        capability,
        enabled: |flags: &ModuleFlags| flags.orbit,
    }
}

pub(crate) async fn new_store(data_root: std::path::PathBuf) -> Result<Arc<OrbitStore>> {
    Ok(Arc::new(OrbitStore::new(data_root).await?))
}

fn register(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/orbit/scripts", post(upload_script).get(list_scripts))
        .route("/orbit/scripts/{name_or_id}", get(download_script))
        .route("/orbit/vibecode", post(vibecode_script))
}

fn capability(state: &AppState) -> ModuleCapability {
    let opencode_available = state.tools.opencode.available;
    ModuleCapability {
        name: "orbit".to_string(),
        enabled: true,
        healthy: opencode_available,
        detail: if opencode_available {
            Some("Lua script store ready; opencode available".to_string())
        } else {
            Some("Lua script store ready; opencode unavailable for vibecoding".to_string())
        },
    }
}

async fn upload_script(
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
        .map(sanitize_lua_name)
        .unwrap_or_else(|| "main.lua".to_string());
    validate_lua_name(&original_name)?;

    let id = Uuid::new_v4().simple().to_string();
    let script_path = state.orbit_store.script_path(&id);
    let mut file = tokio::fs::File::create(&script_path)
        .await
        .map_err(|err| ApiError::internal(format!("failed to create script file: {err}")))?;

    let mut size = 0_u64;
    let mut hasher = Sha256::new();

    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|err| ApiError::bad_request(format!("failed to read multipart chunk: {err}")))?
    {
        size = size.saturating_add(chunk.len() as u64);
        if size > state.config.max_upload_bytes {
            let _ = tokio::fs::remove_file(&script_path).await;
            return Err(ApiError::payload_too_large(format!(
                "script exceeds max size of {} bytes",
                state.config.max_upload_bytes
            )));
        }

        file.write_all(&chunk)
            .await
            .map_err(|err| ApiError::internal(format!("failed to write uploaded script: {err}")))?;
        hasher.update(&chunk);
    }

    file.flush()
        .await
        .map_err(|err| ApiError::internal(format!("failed to flush uploaded script: {err}")))?;

    let now = Utc::now();
    let script = OrbitScriptMetadata {
        id,
        name: original_name,
        size,
        sha256: format!("{:x}", hasher.finalize()),
        status: OrbitScriptStatus::Uploaded,
        source: OrbitScriptSource::Uploaded,
        parent_id: None,
        prompt: None,
        error: None,
        created_at: now,
        updated_at: now,
    };

    state
        .orbit_store
        .insert(script.clone())
        .await
        .map_err(|err| ApiError::internal(format!("failed to persist orbit metadata: {err}")))?;

    Ok((StatusCode::CREATED, Json(OrbitUploadResponse { script })))
}

async fn list_scripts(State(state): State<AppState>) -> ApiResult<Json<OrbitScriptListResponse>> {
    let scripts = state.orbit_store.list().await;
    Ok(Json(OrbitScriptListResponse { scripts }))
}

async fn download_script(
    State(state): State<AppState>,
    AxumPath(name_or_id): AxumPath<String>,
) -> ApiResult<Response> {
    let script = state
        .orbit_store
        .get_by_id_or_name(&name_or_id)
        .await
        .ok_or_else(|| ApiError::not_found("orbit script not found"))?;

    if !matches!(
        script.status,
        OrbitScriptStatus::Uploaded | OrbitScriptStatus::Ready
    ) {
        return Err(ApiError::conflict("orbit script content is not ready"));
    }

    let script_path = state.orbit_store.script_path(&script.id);
    let file = tokio::fs::File::open(&script_path)
        .await
        .map_err(|_| ApiError::not_found("orbit script content missing"))?;

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/x-lua; charset=utf-8"),
    );
    let safe_name = script.name.replace('"', "");
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{}\"", safe_name)) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }

    Ok((headers, body).into_response())
}

async fn vibecode_script(
    State(state): State<AppState>,
    Json(payload): Json<OrbitVibecodeRequest>,
) -> ApiResult<(StatusCode, Json<OrbitVibecodeResponse>)> {
    validate_vibecode_request(&state, &payload).await?;

    let id = Uuid::new_v4().simple().to_string();
    let parent = match &payload.script_id {
        Some(parent_id) => Some(
            state
                .orbit_store
                .get(parent_id)
                .await
                .ok_or_else(|| ApiError::not_found("source orbit script not found"))?,
        ),
        None => None,
    };
    let name = payload
        .name
        .as_deref()
        .map(sanitize_lua_name)
        .or_else(|| parent.as_ref().map(|script| script.name.clone()))
        .unwrap_or_else(|| format!("orbit-{id}.lua"));
    validate_lua_name(&name)?;

    let now = Utc::now();
    let script = OrbitScriptMetadata {
        id: id.clone(),
        name,
        size: 0,
        sha256: String::new(),
        status: OrbitScriptStatus::Pending,
        source: if parent.is_some() {
            OrbitScriptSource::Refactor
        } else {
            OrbitScriptSource::Generated
        },
        parent_id: parent.as_ref().map(|script| script.id.clone()),
        prompt: Some(payload.prompt.clone()),
        error: None,
        created_at: now,
        updated_at: now,
    };

    state
        .orbit_store
        .insert(script.clone())
        .await
        .map_err(|err| ApiError::internal(format!("failed to persist orbit job: {err}")))?;

    spawn_opencode_job(state.clone(), id, payload.prompt, parent);

    Ok((StatusCode::ACCEPTED, Json(OrbitVibecodeResponse { script })))
}

async fn validate_vibecode_request(
    state: &AppState,
    payload: &OrbitVibecodeRequest,
) -> ApiResult<()> {
    let prompt = payload.prompt.trim();
    if prompt.is_empty() {
        return Err(ApiError::bad_request("prompt is required"));
    }
    if prompt.chars().count() > state.config.max_snippet_chars {
        return Err(ApiError::bad_request(format!(
            "prompt is too long (max {} chars)",
            state.config.max_snippet_chars
        )));
    }
    if !state.tools.opencode.available {
        return Err(
            ApiError::service_unavailable("opencode is not available on this server")
                .with_code("opencode_unavailable"),
        );
    }
    Ok(())
}

fn spawn_opencode_job(
    state: AppState,
    id: String,
    prompt: String,
    parent: Option<OrbitScriptMetadata>,
) {
    tokio::spawn(async move {
        if let Err(err) = run_opencode_job(state.clone(), &id, &prompt, parent).await {
            let _ = state
                .orbit_store
                .mark_failed(&id, truncate(&err.to_string()))
                .await;
        }
    });
}

async fn run_opencode_job(
    state: AppState,
    id: &str,
    prompt: &str,
    parent: Option<OrbitScriptMetadata>,
) -> Result<()> {
    state.orbit_store.mark_generating(id).await?;

    let job_dir = state.orbit_store.job_dir(id);
    tokio::fs::create_dir_all(&job_dir)
        .await
        .with_context(|| format!("failed to create job dir {}", job_dir.display()))?;

    let instructions = orbit_instructions();
    write_opencode_project_config(&job_dir).await?;
    write_orbit_skill(&job_dir, instructions).await?;
    copy_inspiration_scripts(&state, id, &job_dir).await?;
    tokio::fs::write(job_dir.join("prompt.txt"), prompt).await?;

    let mut message = format!(
        "Use the orbit-love2d skill.\n\n{}\n\nUser prompt:\n{}\n\nWrite the final Lua script to output.lua. You may read Lua examples from inspiration/*.lua, but only output.lua may be changed.",
        instructions, prompt
    );

    if let Some(parent_script) = parent {
        let source = tokio::fs::read_to_string(state.orbit_store.script_path(&parent_script.id))
            .await
            .with_context(|| format!("failed to read source script {}", parent_script.id))?;
        tokio::fs::write(job_dir.join("input.lua"), &source).await?;
        message
            .push_str("\n\nExisting script is in input.lua. Refactor it according to the prompt.");
    } else {
        message.push_str("\n\nCreate a new complete Love2D app script.");
    }

    let args = vec![
        "--pure".to_string(),
        "--dir".to_string(),
        job_dir.display().to_string(),
        "--title".to_string(),
        format!("Orbit Lua {}", id),
        message,
    ];

    let output = state.command_runner.run("opencode", &args).await?;
    if output.status != 0 {
        anyhow::bail!(
            "opencode failed (status {}): {}",
            output.status,
            truncate(&output.stderr)
        );
    }

    let output_path = job_dir.join("output.lua");
    let raw = tokio::fs::read(&output_path)
        .await
        .with_context(|| format!("opencode did not create {}", output_path.display()))?;
    if raw.is_empty() {
        anyhow::bail!("opencode created an empty output.lua");
    }

    let final_path = state.orbit_store.script_path(id);
    tokio::fs::write(&final_path, &raw)
        .await
        .with_context(|| format!("failed to write {}", final_path.display()))?;

    let sha256 = format!("{:x}", Sha256::digest(&raw));
    state
        .orbit_store
        .mark_ready(id, raw.len() as u64, sha256)
        .await?;

    Ok(())
}

fn orbit_instructions() -> &'static str {
    "You are creating or refactoring a single-file Lua app for Orbit, a mobile app that runs Love2D Lua scripts like a browser. Produce valid Lua for Love2D. Prefer one self-contained file using love.load, love.update, love.draw, and touch/mouse input where useful. Keep assets procedural or embedded as code. Do not use desktop-only assumptions. Write only the final app script to output.lua."
}

async fn write_opencode_project_config(job_dir: &Path) -> Result<()> {
    let config = json!({
        "$schema": "https://opencode.ai/config.json",
        "permission": {
            "*": "deny",
            "read": {
                "*": "deny",
                "prompt.txt": "allow",
                "input.lua": "allow",
                "output.lua": "allow",
                "inspiration/*.lua": "allow",
                ".opencode/skill/orbit-love2d/SKILL.md": "allow"
            },
            "edit": {
                "*": "deny",
                "output.lua": "allow"
            },
            "list": {
                "*": "deny",
                ".": "allow",
                "inspiration": "allow",
                ".opencode/skill/orbit-love2d": "allow"
            },
            "glob": {
                "*": "deny",
                "*.lua": "allow",
                "inspiration/*.lua": "allow"
            },
            "grep": {
                "*": "allow"
            },
            "bash": {
                "*": "deny",
                "stylua": "allow",
                "stylua *": "allow",
                "selene": "allow",
                "selene *": "allow"
            },
            "skill": {
                "*": "deny",
                "orbit-love2d": "allow"
            },
            "task": "deny",
            "webfetch": "deny",
            "websearch": "deny",
            "external_directory": "deny",
            "lsp": "deny"
        }
    });
    let raw = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(job_dir.join("opencode.json"), raw).await?;
    Ok(())
}

async fn write_orbit_skill(job_dir: &Path, instructions: &str) -> Result<()> {
    let skill_dir = job_dir.join(".opencode").join("skill").join("orbit-love2d");
    tokio::fs::create_dir_all(&skill_dir).await?;
    tokio::fs::write(skill_dir.join("SKILL.md"), instructions).await?;
    Ok(())
}

async fn copy_inspiration_scripts(state: &AppState, active_id: &str, job_dir: &Path) -> Result<()> {
    let inspiration_dir = job_dir.join("inspiration");
    tokio::fs::create_dir_all(&inspiration_dir).await?;

    for script in state.orbit_store.list().await {
        if script.id == active_id
            || !matches!(
                script.status,
                OrbitScriptStatus::Uploaded | OrbitScriptStatus::Ready
            )
        {
            continue;
        }

        let source_path = state.orbit_store.script_path(&script.id);
        let Ok(raw) = tokio::fs::read(&source_path).await else {
            continue;
        };
        let target_name = format!("{}_{}", script.id, sanitize_lua_name(&script.name));
        let target_path = inspiration_dir.join(target_name);
        tokio::fs::write(&target_path, raw).await?;

        let mut permissions = tokio::fs::metadata(&target_path).await?.permissions();
        permissions.set_readonly(true);
        tokio::fs::set_permissions(&target_path, permissions).await?;
    }

    Ok(())
}

fn validate_lua_name(name: &str) -> ApiResult<()> {
    if name.trim().is_empty() {
        return Err(ApiError::bad_request("script name is required"));
    }
    if !name.to_ascii_lowercase().ends_with(".lua") {
        return Err(ApiError::bad_request("script name must end with .lua"));
    }
    Ok(())
}

fn sanitize_lua_name(input: &str) -> String {
    Path::new(input)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("main.lua")
        .replace(['/', '\\', '"'], "")
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

    use std::sync::Arc;

    use anyhow::Result;
    use async_trait::async_trait;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::{
        app_state::{RuntimeTools, ToolStatus},
        command_runner::{CommandOutput, CommandRunner},
        config::{ModuleFlags, ServerConfig},
        file_store::FileStore,
        modules::{ModuleDef, lostandfound},
    };

    #[derive(Debug)]
    struct FakeCommandRunner;

    #[async_trait]
    impl CommandRunner for FakeCommandRunner {
        async fn run(&self, _program: &str, args: &[String]) -> Result<CommandOutput> {
            assert!(args.iter().any(|arg| arg == "--pure"));
            assert!(
                !args
                    .iter()
                    .any(|arg| arg == "--dangerously-skip-permissions")
            );
            let dir = args
                .windows(2)
                .find_map(|pair| (pair[0] == "--dir").then(|| pair[1].clone()))
                .expect("dir arg");
            let config =
                tokio::fs::read_to_string(std::path::Path::new(&dir).join("opencode.json")).await?;
            assert!(config.contains("\"stylua *\": \"allow\""));
            assert!(config.contains("\"selene *\": \"allow\""));
            assert!(config.contains("\"task\": \"deny\""));
            tokio::fs::write(
                std::path::Path::new(&dir).join("output.lua"),
                "function love.draw()\n  love.graphics.print('hi', 20, 20)\nend\n",
            )
            .await?;
            Ok(CommandOutput {
                status: 0,
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn validation_rejects_non_lua_name() {
        assert!(validate_lua_name("main.txt").is_err());
    }

    #[tokio::test]
    async fn route_vibecode_creates_temp_script_and_marks_ready() {
        let app = test_router().await;
        let payload = json!({
            "prompt": "make a tiny drawing app",
            "name": "paint.lua"
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/orbit/vibecode")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let created: OrbitVibecodeResponse = serde_json::from_slice(&body).expect("json");
        assert_eq!(created.script.status, OrbitScriptStatus::Pending);

        let listed = wait_for_ready(app.clone(), &created.script.id).await;
        assert_eq!(listed.status, OrbitScriptStatus::Ready);

        let download = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/orbit/scripts/{}", created.script.id))
                    .body(Body::empty())
                    .expect("download"),
            )
            .await
            .expect("download response");
        assert_eq!(download.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn route_vibecode_uses_error_envelope() {
        let app = test_router().await;
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/orbit/vibecode")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "prompt": "" }).to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            value.get("error").and_then(Value::as_str),
            Some("prompt is required")
        );
    }

    #[tokio::test]
    async fn route_download_resolves_uploaded_script_name() {
        let app = test_router().await;
        let boundary = "orbit-test-boundary";
        let body = format!(
            "--{boundary}\r\ncontent-disposition: form-data; name=\"file\"; filename=\"main.lua\"\r\ncontent-type: text/x-lua\r\n\r\nfunction love.draw() end\r\n--{boundary}--\r\n"
        );

        let upload = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/orbit/scripts")
                    .header(
                        "content-type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .expect("upload request"),
            )
            .await
            .expect("upload response");
        assert_eq!(upload.status(), StatusCode::CREATED);

        let download = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/orbit/scripts/main.lua")
                    .body(Body::empty())
                    .expect("download request"),
            )
            .await
            .expect("download response");

        assert_eq!(download.status(), StatusCode::OK);
        let body = to_bytes(download.into_body(), usize::MAX)
            .await
            .expect("download body");
        assert_eq!(&body[..], b"function love.draw() end");
    }

    async fn wait_for_ready(app: Router, id: &str) -> OrbitScriptMetadata {
        for _ in 0..20 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::GET)
                        .uri("/orbit/scripts")
                        .body(Body::empty())
                        .expect("list"),
                )
                .await
                .expect("list response");
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("list body");
            let list: OrbitScriptListResponse = serde_json::from_slice(&body).expect("list json");
            if let Some(script) = list.scripts.into_iter().find(|script| script.id == id)
                && script.status == OrbitScriptStatus::Ready
            {
                return script;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("script never became ready");
    }

    async fn test_router() -> Router {
        let data_dir = std::env::temp_dir().join(format!("stardive-orbit-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&data_dir)
            .await
            .expect("create data dir");

        let config = Arc::new(ServerConfig {
            bind_addr: "127.0.0.1:0".parse().expect("addr"),
            data_dir: data_dir.clone(),
            log_dir: data_dir.join("logs"),
            installers_dir: data_dir.join("installers"),
            eternal_dir: data_dir.join("eternal"),
            api_key: None,
            max_upload_bytes: 1_024_000,
            max_snippet_chars: 20_000,
            modules: ModuleFlags {
                health: true,
                search: true,
                files: true,
                render: true,
                lostandfound: true,
                installers: true,
                eternal: true,
                orbit: true,
            },
        });

        let file_store = Arc::new(
            FileStore::new(config.data_dir.clone())
                .await
                .expect("file store"),
        );

        let orbit_store = new_store(config.data_dir.clone())
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
                    available: true,
                    path: Some("opencode".to_string()),
                },
            },
            Arc::new(FakeCommandRunner),
            Arc::new(Vec::<ModuleDef>::new()),
            lostandfound::new_store(),
            orbit_store,
        );

        register(Router::new()).with_state(state)
    }
}
