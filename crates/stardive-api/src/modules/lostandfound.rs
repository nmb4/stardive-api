use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use chrono::Utc;
use stardive_core::types::{
    LostAndFoundClaim, LostAndFoundClaimStatus, LostAndFoundCreateClaimRequest,
    LostAndFoundCreateItemRequest, LostAndFoundHealthResponse, LostAndFoundItem,
    LostAndFoundItemFilter, LostAndFoundItemStatus, LostAndFoundLoginRequest,
    LostAndFoundLoginResponse, LostAndFoundUpdateItemStatusRequest, LostAndFoundUser,
    ModuleCapability,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    config::ModuleFlags,
    error::{ApiError, ApiResult},
};

use super::ModuleDef;

#[derive(Debug)]
pub(crate) struct LostAndFoundStore {
    users: Vec<LostAndFoundUser>,
    items: Vec<LostAndFoundItem>,
    claims: Vec<LostAndFoundClaim>,
    next_item_id: u64,
    next_claim_id: u64,
}

impl LostAndFoundStore {
    fn seeded() -> Self {
        let users = vec![
            LostAndFoundUser {
                id: 1,
                name: "Lena Weber".to_string(),
                email: "lena@hdm-stuttgart.de".to_string(),
            },
            LostAndFoundUser {
                id: 2,
                name: "Tom Schneider".to_string(),
                email: "tom@hdm-stuttgart.de".to_string(),
            },
        ];

        let items = vec![
            LostAndFoundItem {
                id: 1,
                title: "Schwarze Trinkflasche".to_string(),
                description: "Matte Flasche mit HdM-Aufkleber am Deckel.".to_string(),
                category: "Accessoires".to_string(),
                found_location: "Gebaeude A / Raum 1.14".to_string(),
                found_date: "2026-04-24".to_string(),
                found_time: "11:30".to_string(),
                image_url:
                    "https://images.unsplash.com/photo-1602143407151-7111542de6e8?w=800&auto=format&fit=crop"
                        .to_string(),
                status: LostAndFoundItemStatus::Visible,
                created_by_user_id: 1,
                created_at: Utc::now(),
            },
            LostAndFoundItem {
                id: 2,
                title: "AirPods Ladecase".to_string(),
                description: "Weisses Apple-Ladecase ohne Kopfhoerer.".to_string(),
                category: "Elektronik".to_string(),
                found_location: "Bibliothek / 2. OG".to_string(),
                found_date: "2026-04-25".to_string(),
                found_time: "09:10".to_string(),
                image_url:
                    "https://images.unsplash.com/photo-1588156979435-379b9d802b0c?w=800&auto=format&fit=crop"
                        .to_string(),
                status: LostAndFoundItemStatus::Visible,
                created_by_user_id: 2,
                created_at: Utc::now(),
            },
            LostAndFoundItem {
                id: 3,
                title: "Blauer Hoodie".to_string(),
                description: "Dunkelblauer Hoodie Groesse M mit weissem Rueckenprint.".to_string(),
                category: "Kleidung".to_string(),
                found_location: "Mensa".to_string(),
                found_date: "2026-04-22".to_string(),
                found_time: "14:40".to_string(),
                image_url:
                    "https://images.unsplash.com/photo-1576566588028-4147f3842f27?w=800&auto=format&fit=crop"
                        .to_string(),
                status: LostAndFoundItemStatus::Returned,
                created_by_user_id: 1,
                created_at: Utc::now(),
            },
            LostAndFoundItem {
                id: 4,
                title: "Studentenausweis".to_string(),
                description: "HdM-Ausweis auf den Namen Mia Koch.".to_string(),
                category: "Dokumente".to_string(),
                found_location: "Gebaeude C / Eingang".to_string(),
                found_date: "2026-04-21".to_string(),
                found_time: "08:05".to_string(),
                image_url:
                    "https://images.unsplash.com/photo-1589330694653-ded6df03f754?w=800&auto=format&fit=crop"
                        .to_string(),
                status: LostAndFoundItemStatus::Visible,
                created_by_user_id: 2,
                created_at: Utc::now(),
            },
            LostAndFoundItem {
                id: 5,
                title: "Silberne Halskette".to_string(),
                description: "Feine Kette mit kleinem Herzanhaenger.".to_string(),
                category: "Schmuck".to_string(),
                found_location: "Sporthalle / Umkleide".to_string(),
                found_date: "2026-04-20".to_string(),
                found_time: "18:20".to_string(),
                image_url:
                    "https://images.unsplash.com/photo-1617038220319-276d3cfab638?w=800&auto=format&fit=crop"
                        .to_string(),
                status: LostAndFoundItemStatus::Visible,
                created_by_user_id: 1,
                created_at: Utc::now(),
            },
            LostAndFoundItem {
                id: 6,
                title: "Grauer Rucksack".to_string(),
                description: "Rucksack mit rotem Schluesselanhaenger.".to_string(),
                category: "Sonstige".to_string(),
                found_location: "Hoersaal 2".to_string(),
                found_date: "2026-04-19".to_string(),
                found_time: "12:55".to_string(),
                image_url:
                    "https://images.unsplash.com/photo-1553062407-98eeb64c6a62?w=800&auto=format&fit=crop"
                        .to_string(),
                status: LostAndFoundItemStatus::Visible,
                created_by_user_id: 2,
                created_at: Utc::now(),
            },
            LostAndFoundItem {
                id: 7,
                title: "Schwarzes Notizbuch".to_string(),
                description: "A5 Notizbuch mit karierten Seiten und Gummiband.".to_string(),
                category: "Dokumente".to_string(),
                found_location: "Seminarraum 4.02".to_string(),
                found_date: "2026-04-18".to_string(),
                found_time: "10:15".to_string(),
                image_url:
                    "https://images.unsplash.com/photo-1531346878377-a5be20888e57?w=800&auto=format&fit=crop"
                        .to_string(),
                status: LostAndFoundItemStatus::Visible,
                created_by_user_id: 1,
                created_at: Utc::now(),
            },
            LostAndFoundItem {
                id: 8,
                title: "Rote Powerbank".to_string(),
                description: "10.000 mAh Powerbank mit USB-C Kabel.".to_string(),
                category: "Elektronik".to_string(),
                found_location: "Campus-Lounge".to_string(),
                found_date: "2026-04-17".to_string(),
                found_time: "16:35".to_string(),
                image_url:
                    "https://images.unsplash.com/photo-1609592806787-3d9cbbf7fd16?w=800&auto=format&fit=crop"
                        .to_string(),
                status: LostAndFoundItemStatus::Returned,
                created_by_user_id: 2,
                created_at: Utc::now(),
            },
        ];

        let claims = vec![LostAndFoundClaim {
            id: 1,
            item_id: 3,
            claimer_user_id: 2,
            status: LostAndFoundClaimStatus::Approved,
            created_at: Utc::now(),
        }];

        Self {
            users,
            items,
            claims,
            next_item_id: 9,
            next_claim_id: 2,
        }
    }
}

pub fn module_def() -> ModuleDef {
    ModuleDef {
        name: "lostandfound",
        register,
        capability,
        enabled: |flags: &ModuleFlags| flags.lostandfound,
    }
}

fn register(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/lostandfound/health", get(health))
        .route("/lostandfound/auth/login", post(login))
        .route("/lostandfound/items", get(list_items).post(create_item))
        .route("/lostandfound/items/{id}", get(get_item))
        .route("/lostandfound/items/{id}/status", patch(update_item_status))
        .route("/lostandfound/claims", get(list_claims).post(create_claim))
        .route("/lostandfound/categories", get(list_categories))
}

fn capability(_: &AppState) -> ModuleCapability {
    ModuleCapability {
        name: "lostandfound".to_string(),
        enabled: true,
        healthy: true,
        detail: Some("in-memory lost-and-found mock API".to_string()),
    }
}

async fn health() -> Json<LostAndFoundHealthResponse> {
    Json(LostAndFoundHealthResponse {
        ok: true,
        service: "lostandfound-mock-api".to_string(),
    })
}

async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LostAndFoundLoginRequest>,
) -> ApiResult<Json<LostAndFoundLoginResponse>> {
    if payload.password.trim().is_empty() {
        return Err(ApiError::bad_request("password must not be empty"));
    }

    let store = state.lostandfound_store.read().await;
    let user = store
        .users
        .iter()
        .find(|candidate| candidate.email.eq_ignore_ascii_case(&payload.email))
        .cloned()
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "invalid email or password"))?;

    Ok(Json(LostAndFoundLoginResponse {
        token: format!("mock-token-{}", Uuid::new_v4()),
        user,
    }))
}

async fn list_items(
    State(state): State<AppState>,
    Query(filter): Query<LostAndFoundItemFilter>,
) -> Json<Vec<LostAndFoundItem>> {
    let store = state.lostandfound_store.read().await;
    let items = store
        .items
        .iter()
        .filter(|item| matches_item_filter(item, &filter))
        .cloned()
        .collect::<Vec<_>>();

    Json(items)
}

async fn get_item(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Json<LostAndFoundItem>> {
    let store = state.lostandfound_store.read().await;
    let item = store
        .items
        .iter()
        .find(|item| item.id == id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("item {id} not found")))?;
    Ok(Json(item))
}

async fn create_item(
    State(state): State<AppState>,
    Json(payload): Json<LostAndFoundCreateItemRequest>,
) -> ApiResult<(StatusCode, Json<LostAndFoundItem>)> {
    validate_create_item_request(&payload)?;

    let mut store = state.lostandfound_store.write().await;

    let item = LostAndFoundItem {
        id: store.next_item_id,
        title: payload.title,
        description: payload
            .description
            .unwrap_or_else(|| "Keine Beschreibung angegeben".to_string()),
        category: payload.category,
        found_location: payload.found_location,
        found_date: payload.found_date,
        found_time: payload.found_time,
        image_url: payload
            .image_url
            .unwrap_or_else(|| {
                "https://images.unsplash.com/photo-1542291026-7eec264c27ff?w=800&auto=format&fit=crop"
                    .to_string()
            }),
        status: LostAndFoundItemStatus::Visible,
        created_by_user_id: payload.created_by_user_id.unwrap_or(1),
        created_at: Utc::now(),
    };

    store.next_item_id = store.next_item_id.saturating_add(1);
    store.items.push(item.clone());

    Ok((StatusCode::CREATED, Json(item)))
}

async fn update_item_status(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(payload): Json<LostAndFoundUpdateItemStatusRequest>,
) -> ApiResult<Json<LostAndFoundItem>> {
    let mut store = state.lostandfound_store.write().await;
    let item = store
        .items
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or_else(|| ApiError::not_found(format!("item {id} not found")))?;
    item.status = payload.status;
    Ok(Json(item.clone()))
}

async fn list_claims(State(state): State<AppState>) -> Json<Vec<LostAndFoundClaim>> {
    Json(state.lostandfound_store.read().await.claims.clone())
}

async fn create_claim(
    State(state): State<AppState>,
    Json(payload): Json<LostAndFoundCreateClaimRequest>,
) -> ApiResult<(StatusCode, Json<LostAndFoundClaim>)> {
    let mut store = state.lostandfound_store.write().await;

    let has_item = store.items.iter().any(|item| item.id == payload.item_id);
    let has_user = store
        .users
        .iter()
        .any(|user| user.id == payload.claimer_user_id);

    if !has_item || !has_user {
        return Err(ApiError::bad_request("unknown item_id or claimer_user_id"));
    }

    let claim = LostAndFoundClaim {
        id: store.next_claim_id,
        item_id: payload.item_id,
        claimer_user_id: payload.claimer_user_id,
        status: LostAndFoundClaimStatus::Pending,
        created_at: Utc::now(),
    };

    store.next_claim_id = store.next_claim_id.saturating_add(1);
    store.claims.push(claim.clone());

    Ok((StatusCode::CREATED, Json(claim)))
}

async fn list_categories() -> Json<Vec<&'static str>> {
    Json(vec![
        "Elektronik",
        "Accessoires",
        "Schmuck",
        "Kleidung",
        "Dokumente",
        "Sonstige",
    ])
}

fn matches_item_filter(item: &LostAndFoundItem, filter: &LostAndFoundItemFilter) -> bool {
    if let Some(status) = &filter.status
        && item.status != *status
    {
        return false;
    }

    if let Some(category) = &filter.category
        && !item.category.eq_ignore_ascii_case(category)
    {
        return false;
    }

    true
}

fn validate_create_item_request(request: &LostAndFoundCreateItemRequest) -> ApiResult<()> {
    if request.title.trim().is_empty() || request.found_location.trim().is_empty() {
        return Err(ApiError::bad_request(
            "title and found_location are required",
        ));
    }

    Ok(())
}

pub(crate) fn new_store() -> Arc<RwLock<LostAndFoundStore>> {
    Arc::new(RwLock::new(LostAndFoundStore::seeded()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use anyhow::Result;
    use async_trait::async_trait;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::{
        app_state::{AppState, RuntimeTools, ToolStatus},
        command_runner::{CommandOutput, CommandRunner},
        config::{ModuleFlags, ServerConfig},
        file_store::FileStore,
        modules::ModuleDef,
    };

    #[derive(Debug)]
    struct NoopCommandRunner;

    #[async_trait]
    impl CommandRunner for NoopCommandRunner {
        async fn run(&self, _program: &str, _args: &[String]) -> Result<CommandOutput> {
            Ok(CommandOutput {
                status: 0,
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn create_item_validation_rejects_missing_fields() {
        let request = LostAndFoundCreateItemRequest {
            title: "  ".to_string(),
            description: None,
            category: "Elektronik".to_string(),
            found_location: String::new(),
            found_date: "2026-04-26".to_string(),
            found_time: "10:00".to_string(),
            image_url: None,
            created_by_user_id: None,
        };

        assert!(validate_create_item_request(&request).is_err());
    }

    #[test]
    fn filter_is_case_insensitive_for_category() {
        let item = LostAndFoundItem {
            id: 1,
            title: "A".to_string(),
            description: "B".to_string(),
            category: "Elektronik".to_string(),
            found_location: "Room".to_string(),
            found_date: "2026-04-26".to_string(),
            found_time: "10:00".to_string(),
            image_url: "https://example.com/x.jpg".to_string(),
            status: LostAndFoundItemStatus::Visible,
            created_by_user_id: 1,
            created_at: Utc::now(),
        };
        let filter = LostAndFoundItemFilter {
            status: None,
            category: Some("elektronik".to_string()),
        };

        assert!(matches_item_filter(&item, &filter));
    }

    #[tokio::test]
    async fn route_create_and_fetch_item() {
        let app = test_router().await;
        let payload = json!({
            "title": "Wallet",
            "description": "Brown leather",
            "category": "Accessoires",
            "found_location": "Building B / Lab 2",
            "found_date": "2026-04-26",
            "found_time": "12:45",
            "created_by_user_id": 1
        });

        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/lostandfound/items")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .expect("create request"),
            )
            .await
            .expect("create response");

        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body = to_bytes(create_response.into_body(), usize::MAX)
            .await
            .expect("create body");
        let created_item: LostAndFoundItem =
            serde_json::from_slice(&create_body).expect("create item json");

        let get_response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/lostandfound/items/{}", created_item.id))
                    .body(Body::empty())
                    .expect("get request"),
            )
            .await
            .expect("get response");

        assert_eq!(get_response.status(), StatusCode::OK);
        let get_body = to_bytes(get_response.into_body(), usize::MAX)
            .await
            .expect("get body");
        let fetched_item: LostAndFoundItem = serde_json::from_slice(&get_body).expect("get json");
        assert_eq!(fetched_item.title, "Wallet");
    }

    #[tokio::test]
    async fn route_login_uses_error_envelope() {
        let app = test_router().await;
        let payload = json!({
            "email": "lena@hdm-stuttgart.de",
            "password": ""
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/lostandfound/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .expect("login request"),
            )
            .await
            .expect("login response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("login body");
        let value: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            value.get("error").and_then(Value::as_str),
            Some("password must not be empty")
        );
    }

    async fn test_router() -> Router {
        let data_dir = std::env::temp_dir().join(format!("stardive-lf-{}", Uuid::new_v4()));
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
            },
        });

        let file_store = Arc::new(
            FileStore::new(config.data_dir.clone())
                .await
                .expect("file store"),
        );

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
            },
            Arc::new(NoopCommandRunner),
            Arc::new(Vec::<ModuleDef>::new()),
            new_store(),
        );

        register(Router::new()).with_state(state)
    }
}
