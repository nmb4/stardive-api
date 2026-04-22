use std::path::Path;

use axum::{Json, Router, extract::State, routing::post};
use serde_json::Value;
use stardive_core::types::{
    ExtractRequest, ExtractResponse, ModuleCapability, SearchRequest, SearchResponse,
};

use crate::{
    app_state::AppState,
    config::ModuleFlags,
    error::{ApiError, ApiResult},
};

use super::ModuleDef;

pub fn module_def() -> ModuleDef {
    ModuleDef {
        name: "search",
        register,
        capability,
        enabled: |flags: &ModuleFlags| flags.search,
    }
}

fn register(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/search/text", post(search_text))
        .route("/search/news", post(search_news))
        .route("/extract", post(extract_url))
}

fn capability(state: &AppState) -> ModuleCapability {
    ModuleCapability {
        name: "search".to_string(),
        enabled: true,
        healthy: state.tools.ddgs.available,
        detail: if state.tools.ddgs.available {
            Some("ddgs available".to_string())
        } else {
            Some("ddgs unavailable".to_string())
        },
    }
}

async fn search_text(
    State(state): State<AppState>,
    Json(payload): Json<SearchRequest>,
) -> ApiResult<Json<SearchResponse>> {
    validate_search_request(&payload)?;
    let mut args = vec!["text".to_string(), "-q".to_string(), payload.query.clone()];
    apply_search_options(&payload, &mut args);
    let results = run_ddgs_json(&state, args).await?;
    Ok(Json(SearchResponse { results }))
}

async fn search_news(
    State(state): State<AppState>,
    Json(payload): Json<SearchRequest>,
) -> ApiResult<Json<SearchResponse>> {
    validate_search_request(&payload)?;
    let mut args = vec!["news".to_string(), "-q".to_string(), payload.query.clone()];
    apply_search_options(&payload, &mut args);
    let results = run_ddgs_json(&state, args).await?;
    Ok(Json(SearchResponse { results }))
}

async fn extract_url(
    State(state): State<AppState>,
    Json(payload): Json<ExtractRequest>,
) -> ApiResult<Json<ExtractResponse>> {
    if payload.url.trim().is_empty() {
        return Err(ApiError::bad_request("url is required"));
    }

    let mut args = vec!["extract".to_string(), "-u".to_string(), payload.url];
    if let Some(format) = payload.format {
        args.extend(["-f".to_string(), format]);
    }

    let result = run_ddgs_json(&state, args).await?;
    Ok(Json(ExtractResponse { result }))
}

fn validate_search_request(request: &SearchRequest) -> ApiResult<()> {
    if request.query.trim().is_empty() {
        return Err(ApiError::bad_request("query is required"));
    }

    if request.query.len() > 512 {
        return Err(ApiError::bad_request("query is too long (max 512 chars)"));
    }

    if let Some(max) = request.max_results
        && max > 50
    {
        return Err(ApiError::bad_request("max_results must be <= 50"));
    }

    Ok(())
}

fn apply_search_options(request: &SearchRequest, args: &mut Vec<String>) {
    if let Some(region) = &request.region {
        args.extend(["-r".to_string(), region.clone()]);
    }
    if let Some(safesearch) = &request.safesearch {
        args.extend(["-s".to_string(), safesearch.clone()]);
    }
    if let Some(timelimit) = &request.timelimit {
        args.extend(["-t".to_string(), timelimit.clone()]);
    }
    if let Some(max_results) = request.max_results {
        args.extend(["-m".to_string(), max_results.to_string()]);
    }
}

async fn run_ddgs_json(state: &AppState, mut args: Vec<String>) -> ApiResult<Value> {
    if !state.tools.ddgs.available {
        return Err(
            ApiError::service_unavailable("ddgs is not available on this server")
                .with_code("ddgs_unavailable"),
        );
    }

    let temp_dir = tempfile::tempdir()
        .map_err(|err| ApiError::internal(format!("failed to create temp dir: {err}")))?;
    let out_path = temp_dir.path().join("ddgs.json");
    args.extend([
        "-o".to_string(),
        out_path.to_str().unwrap_or_default().to_string(),
    ]);

    let output = state
        .command_runner
        .run("ddgs", &args)
        .await
        .map_err(|err| ApiError::bad_gateway(format!("ddgs invocation failed: {err}")))?;

    if output.status != 0 {
        return Err(ApiError::bad_gateway(format!(
            "ddgs failed (status {}): {}",
            output.status,
            truncate(&output.stderr)
        )));
    }

    let raw = tokio::fs::read_to_string(Path::new(&out_path))
        .await
        .map_err(|err| ApiError::internal(format!("failed to read ddgs output: {err}")))?;

    let parsed = serde_json::from_str::<Value>(&raw)
        .map_err(|err| ApiError::internal(format!("failed to parse ddgs json: {err}")))?;

    Ok(parsed)
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
    fn search_validation_rejects_empty_query() {
        let request = SearchRequest {
            query: "".to_string(),
            region: None,
            safesearch: None,
            timelimit: None,
            max_results: None,
        };
        assert!(validate_search_request(&request).is_err());
    }

    #[test]
    fn apply_search_options_sets_flags() {
        let request = SearchRequest {
            query: "rust".to_string(),
            region: Some("us-en".to_string()),
            safesearch: Some("off".to_string()),
            timelimit: Some("w".to_string()),
            max_results: Some(5),
        };
        let mut args = vec![];
        apply_search_options(&request, &mut args);
        assert!(args.contains(&"-r".to_string()));
        assert!(args.contains(&"us-en".to_string()));
        assert!(args.contains(&"-m".to_string()));
    }
}
