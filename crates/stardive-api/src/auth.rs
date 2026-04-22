use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{app_state::AppState, error::ApiError};

pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let expected_key = match &state.config.api_key {
        Some(key) => key,
        None => return next.run(request).await,
    };

    let path = request.uri().path();
    if path == "/up" || path.ends_with("/health") {
        return next.run(request).await;
    }

    let Some(value) = request.headers().get(header::AUTHORIZATION) else {
        return ApiError::new(StatusCode::UNAUTHORIZED, "missing authorization header")
            .into_response();
    };

    let Ok(value_str) = value.to_str() else {
        return ApiError::new(StatusCode::UNAUTHORIZED, "invalid authorization header")
            .into_response();
    };

    let expected = format!("Bearer {expected_key}");
    if value_str != expected {
        return ApiError::new(StatusCode::UNAUTHORIZED, "invalid api key").into_response();
    }

    next.run(request).await
}
