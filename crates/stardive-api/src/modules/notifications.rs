use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use p256::SecretKey;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use stardive_core::types::{
    ModuleCapability, NotificationDeliveryResponse, NotificationRequest,
    NotificationSubscribeResponse, NotificationSubscription, NotificationSubscriptionRequest,
    NotificationVapidResponse, PushSubscriptionKeys,
};
use tokio::sync::Mutex;
use uuid::Uuid;
use web_push::{
    ContentEncoding, IsahcWebPushClient, SubscriptionInfo, VapidSignatureBuilder, WebPushClient,
    WebPushError, WebPushMessageBuilder,
};

use crate::{
    app_state::AppState,
    config::ModuleFlags,
    error::{ApiError, ApiResult},
};

use super::ModuleDef;

const MAX_TITLE_CHARS: usize = 160;
const MAX_BODY_CHARS: usize = 2_400;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSubscription {
    id: String,
    endpoint: String,
    keys: PushSubscriptionKeys,
    device_name: Option<String>,
    created_at: DateTime<Utc>,
}

impl StoredSubscription {
    fn public(&self) -> NotificationSubscription {
        NotificationSubscription {
            id: self.id.clone(),
            endpoint: self.endpoint.clone(),
            device_name: self.device_name.clone(),
            created_at: self.created_at,
        }
    }

    fn web_push_info(&self) -> SubscriptionInfo {
        SubscriptionInfo::new(
            self.endpoint.clone(),
            self.keys.p256dh.clone(),
            self.keys.auth.clone(),
        )
    }
}

#[derive(Debug)]
enum SendError {
    Expired,
    Failed(String),
}

#[async_trait]
trait NotificationSender: Send + Sync {
    async fn send(
        &self,
        subscription: &StoredSubscription,
        payload: &[u8],
    ) -> Result<(), SendError>;
}

struct WebPushSender {
    client: IsahcWebPushClient,
    private_key: String,
    subject: String,
}

#[async_trait]
impl NotificationSender for WebPushSender {
    async fn send(
        &self,
        subscription: &StoredSubscription,
        payload: &[u8],
    ) -> Result<(), SendError> {
        let info = subscription.web_push_info();
        let mut signature = VapidSignatureBuilder::from_base64(&self.private_key, &info)
            .map_err(|err| SendError::Failed(err.to_string()))?;
        signature.add_claim("sub", self.subject.clone());

        let mut message = WebPushMessageBuilder::new(&info);
        message.set_payload(ContentEncoding::Aes128Gcm, payload);
        message.set_vapid_signature(
            signature
                .build()
                .map_err(|err| SendError::Failed(err.to_string()))?,
        );

        let message = message
            .build()
            .map_err(|err| SendError::Failed(err.to_string()))?;
        match self.client.send(message).await {
            Ok(()) => Ok(()),
            Err(WebPushError::EndpointNotFound(_) | WebPushError::EndpointNotValid(_)) => {
                Err(SendError::Expired)
            }
            Err(err) => Err(SendError::Failed(err.to_string())),
        }
    }
}

pub(crate) struct NotificationStore {
    subscriptions_path: PathBuf,
    subscriptions: Mutex<Vec<StoredSubscription>>,
    public_key: String,
    sender: Arc<dyn NotificationSender>,
}

impl NotificationStore {
    async fn open(data_root: PathBuf, subject: String) -> Result<Self> {
        if !(subject.starts_with("mailto:") || subject.starts_with("https://")) {
            anyhow::bail!("STARDIVE_VAPID_SUBJECT must use mailto: or HTTPS");
        }
        let root = data_root.join("notifications");
        tokio::fs::create_dir_all(&root)
            .await
            .with_context(|| format!("failed to create {}", root.display()))?;
        harden_permissions(&root, true).await?;

        let private_key_path = root.join("vapid_private.key");
        let private_key = if private_key_path.exists() {
            tokio::fs::read_to_string(&private_key_path)
                .await
                .with_context(|| format!("failed to read {}", private_key_path.display()))?
                .trim()
                .to_string()
        } else {
            let key = SecretKey::random(&mut OsRng);
            let encoded = URL_SAFE_NO_PAD.encode(key.to_bytes());
            tokio::fs::write(&private_key_path, &encoded)
                .await
                .with_context(|| format!("failed to write {}", private_key_path.display()))?;
            encoded
        };
        harden_permissions(&private_key_path, false).await?;

        let vapid = VapidSignatureBuilder::from_base64_no_sub(&private_key)
            .map_err(|err| anyhow::anyhow!("invalid VAPID private key: {err}"))?;
        let public_key = URL_SAFE_NO_PAD.encode(vapid.get_public_key());
        let subscriptions_path = root.join("subscriptions.json");
        let subscriptions = if subscriptions_path.exists() {
            let raw = tokio::fs::read_to_string(&subscriptions_path)
                .await
                .with_context(|| format!("failed to read {}", subscriptions_path.display()))?;
            serde_json::from_str(&raw).context("invalid notification subscriptions json")?
        } else {
            Vec::new()
        };

        let client = IsahcWebPushClient::new()
            .map_err(|err| anyhow::anyhow!("failed to create web push client: {err}"))?;

        Ok(Self {
            subscriptions_path,
            subscriptions: Mutex::new(subscriptions),
            public_key,
            sender: Arc::new(WebPushSender {
                client,
                private_key,
                subject,
            }),
        })
    }

    async fn subscribe(
        &self,
        request: NotificationSubscriptionRequest,
    ) -> Result<NotificationSubscription> {
        let mut subscriptions = self.subscriptions.lock().await;
        let subscription = if let Some(existing) = subscriptions
            .iter_mut()
            .find(|candidate| candidate.endpoint == request.endpoint)
        {
            existing.keys = request.keys;
            existing.device_name = request.device_name;
            existing.clone()
        } else {
            let subscription = StoredSubscription {
                id: Uuid::new_v4().simple().to_string(),
                endpoint: request.endpoint,
                keys: request.keys,
                device_name: request.device_name,
                created_at: Utc::now(),
            };
            subscriptions.push(subscription.clone());
            subscription
        };
        self.persist(&subscriptions).await?;
        Ok(subscription.public())
    }

    async fn remove(&self, id: &str) -> Result<bool> {
        let mut subscriptions = self.subscriptions.lock().await;
        let before = subscriptions.len();
        subscriptions.retain(|subscription| subscription.id != id);
        let removed = before != subscriptions.len();
        if removed {
            self.persist(&subscriptions).await?;
        }
        Ok(removed)
    }

    async fn broadcast(
        &self,
        notification: &NotificationRequest,
    ) -> Result<NotificationDeliveryResponse> {
        let notification_id = Uuid::new_v4().simple().to_string();
        let payload = Arc::new(serde_json::to_vec(&serde_json::json!({
            "id": notification_id,
            "title": notification.title,
            "body": notification.body,
            "url": notification.url,
            "icon": notification.icon,
            "tag": notification.tag,
            "channel": notification.channel,
            "sent_at": Utc::now(),
        }))?);
        let snapshot = self.subscriptions.lock().await.clone();
        let mut delivered = 0;
        let mut failed = 0;
        let mut expired = Vec::new();

        let outcomes = stream::iter(snapshot.iter().cloned())
            .map(|subscription| {
                let sender = self.sender.clone();
                let payload = payload.clone();
                async move {
                    let result = tokio::time::timeout(
                        std::time::Duration::from_secs(15),
                        sender.send(&subscription, payload.as_slice()),
                    )
                    .await
                    .unwrap_or_else(|_| Err(SendError::Failed("delivery timed out".to_string())));
                    (subscription.id, result)
                }
            })
            .buffer_unordered(16)
            .collect::<Vec<_>>()
            .await;

        for (subscription_id, result) in outcomes {
            match result {
                Ok(()) => delivered += 1,
                Err(SendError::Expired) => expired.push(subscription_id),
                Err(SendError::Failed(message)) => {
                    tracing::warn!(
                        subscription_id = %subscription_id,
                        error = %message,
                        "notification delivery failed"
                    );
                    failed += 1;
                }
            }
        }

        if !expired.is_empty() {
            let mut subscriptions = self.subscriptions.lock().await;
            subscriptions.retain(|subscription| !expired.contains(&subscription.id));
            self.persist(&subscriptions).await?;
        }

        Ok(NotificationDeliveryResponse {
            id: notification_id,
            subscriptions: snapshot.len(),
            delivered,
            failed,
            removed: expired.len(),
        })
    }

    async fn persist(&self, subscriptions: &[StoredSubscription]) -> Result<()> {
        let raw = serde_json::to_string_pretty(subscriptions)?;
        let temporary = self.subscriptions_path.with_extension("json.tmp");
        tokio::fs::write(&temporary, raw).await?;
        harden_permissions(&temporary, false).await?;
        tokio::fs::rename(&temporary, &self.subscriptions_path).await?;
        Ok(())
    }
}

#[cfg(unix)]
async fn harden_permissions(path: &std::path::Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = if directory { 0o700 } else { 0o600 };
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .await
        .with_context(|| format!("failed to secure {}", path.display()))
}

#[cfg(not(unix))]
async fn harden_permissions(_: &std::path::Path, _: bool) -> Result<()> {
    Ok(())
}

pub fn module_def() -> ModuleDef {
    ModuleDef {
        name: "notifications",
        register,
        capability,
        enabled: |flags: &ModuleFlags| flags.notifications,
    }
}

pub(crate) async fn new_store(
    data_root: PathBuf,
    subject: String,
) -> Result<Arc<NotificationStore>> {
    Ok(Arc::new(NotificationStore::open(data_root, subject).await?))
}

fn register(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/notifications", post(publish))
        .route("/notifications/vapid-public-key", get(vapid_public_key))
        .route("/notifications/subscriptions", post(subscribe))
        .route("/notifications/subscriptions/{id}", delete(unsubscribe))
}

fn capability(_: &AppState) -> ModuleCapability {
    ModuleCapability {
        name: "notifications".to_string(),
        enabled: true,
        healthy: true,
        detail: Some("persistent Web Push delivery ready".to_string()),
    }
}

async fn vapid_public_key(State(state): State<AppState>) -> Json<NotificationVapidResponse> {
    Json(NotificationVapidResponse {
        public_key: state.notification_store.public_key.clone(),
    })
}

async fn subscribe(
    State(state): State<AppState>,
    Json(request): Json<NotificationSubscriptionRequest>,
) -> ApiResult<Json<NotificationSubscribeResponse>> {
    validate_subscription(&request)?;
    let subscription = state
        .notification_store
        .subscribe(request)
        .await
        .map_err(|err| ApiError::internal(format!("failed to save subscription: {err}")))?;
    Ok(Json(NotificationSubscribeResponse { subscription }))
}

async fn unsubscribe(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<()> {
    if id.is_empty() {
        return Err(ApiError::bad_request("subscription id is required"));
    }
    let removed = state
        .notification_store
        .remove(&id)
        .await
        .map_err(|err| ApiError::internal(format!("failed to remove subscription: {err}")))?;
    if !removed {
        return Err(ApiError::not_found("notification subscription not found"));
    }
    Ok(())
}

async fn publish(
    State(state): State<AppState>,
    Json(request): Json<NotificationRequest>,
) -> ApiResult<Json<NotificationDeliveryResponse>> {
    validate_notification(&request)?;
    let response = state
        .notification_store
        .broadcast(&request)
        .await
        .map_err(|err| ApiError::bad_gateway(format!("notification delivery failed: {err}")))?;
    Ok(Json(response))
}

fn validate_subscription(request: &NotificationSubscriptionRequest) -> ApiResult<()> {
    let uri = request
        .endpoint
        .parse::<http::Uri>()
        .map_err(|_| ApiError::bad_request("subscription endpoint must be a valid URL"))?;
    if uri.scheme_str() != Some("https") || uri.host().is_none() {
        return Err(ApiError::bad_request(
            "subscription endpoint must use HTTPS",
        ));
    }
    if URL_SAFE_NO_PAD.decode(&request.keys.p256dh).is_err()
        || URL_SAFE_NO_PAD.decode(&request.keys.auth).is_err()
    {
        return Err(ApiError::bad_request(
            "subscription keys must be unpadded base64url",
        ));
    }
    if request.keys.p256dh.is_empty() || request.keys.auth.is_empty() {
        return Err(ApiError::bad_request("subscription keys are required"));
    }
    if request
        .device_name
        .as_ref()
        .is_some_and(|name| name.chars().count() > 120)
    {
        return Err(ApiError::bad_request(
            "device_name must be at most 120 characters",
        ));
    }
    Ok(())
}

fn validate_notification(request: &NotificationRequest) -> ApiResult<()> {
    let title = request.title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(ApiError::bad_request(format!(
            "title must be between 1 and {MAX_TITLE_CHARS} characters"
        )));
    }
    if request.body.chars().count() > MAX_BODY_CHARS {
        return Err(ApiError::bad_request(format!(
            "body must be at most {MAX_BODY_CHARS} characters"
        )));
    }
    validate_resource_url(request.url.as_deref(), "url")?;
    validate_resource_url(request.icon.as_deref(), "icon")?;
    if serde_json::to_vec(request)
        .map_err(|err| ApiError::bad_request(format!("invalid notification payload: {err}")))?
        .len()
        > 2_800
    {
        return Err(ApiError::payload_too_large(
            "notification payload must be at most 2800 encoded bytes",
        ));
    }
    Ok(())
}

fn validate_resource_url(value: Option<&str>, field: &str) -> ApiResult<()> {
    if let Some(value) = value
        && !((value.starts_with('/') && !value.starts_with("//")) || value.starts_with("https://"))
    {
        return Err(ApiError::bad_request(format!(
            "{field} must be relative or use HTTPS"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::{
        app_state::{RuntimeTools, ToolStatus},
        command_runner::SystemCommandRunner,
        config::ServerConfig,
        file_store::FileStore,
        modules::{self, lostandfound, orbit},
    };

    struct FakeSender(AtomicUsize);

    #[async_trait]
    impl NotificationSender for FakeSender {
        async fn send(
            &self,
            _subscription: &StoredSubscription,
            _payload: &[u8],
        ) -> Result<(), SendError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    fn subscription() -> NotificationSubscriptionRequest {
        NotificationSubscriptionRequest {
            endpoint: "https://push.example.test/device".to_string(),
            keys: PushSubscriptionKeys {
                p256dh: URL_SAFE_NO_PAD.encode([1_u8; 65]),
                auth: URL_SAFE_NO_PAD.encode([2_u8; 16]),
            },
            expiration_time: None,
            device_name: Some("Noah's iPhone".to_string()),
        }
    }

    #[test]
    fn validates_subscription_transport_and_keys() {
        assert!(validate_subscription(&subscription()).is_ok());
        let mut insecure = subscription();
        insecure.endpoint = "http://push.example.test/device".to_string();
        assert!(validate_subscription(&insecure).is_err());
        let mut invalid_key = subscription();
        invalid_key.keys.auth = "not base64!".to_string();
        assert!(validate_subscription(&invalid_key).is_err());
    }

    #[test]
    fn validates_notification_links_and_lengths() {
        let valid = NotificationRequest {
            title: "New message".to_string(),
            body: "Hello".to_string(),
            url: Some("https://chat.example.test/channel/general".to_string()),
            icon: None,
            tag: None,
            channel: Some("general".to_string()),
        };
        assert!(validate_notification(&valid).is_ok());
        let mut unsafe_link = valid;
        unsafe_link.url = Some("javascript:alert(1)".to_string());
        assert!(validate_notification(&unsafe_link).is_err());
    }

    #[tokio::test]
    async fn http_routes_subscribe_and_broadcast() {
        let data_dir =
            std::env::temp_dir().join(format!("stardive-notifications-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&data_dir)
            .await
            .expect("data dir");
        let config = Arc::new(ServerConfig {
            bind_addr: "127.0.0.1:0".parse().expect("addr"),
            data_dir: data_dir.clone(),
            log_dir: data_dir.join("logs"),
            installers_dir: data_dir.join("installers"),
            eternal_dir: data_dir.join("eternal"),
            api_key: None,
            max_upload_bytes: 1_024_000,
            max_snippet_chars: 20_000,
            vapid_subject: "mailto:test@example.com".to_string(),
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
            },
        });
        let mut notification_store = NotificationStore::open(
            data_dir.join("notification-data"),
            config.vapid_subject.clone(),
        )
        .await
        .expect("notification store");
        notification_store.sender = Arc::new(FakeSender(AtomicUsize::new(0)));
        let state = AppState::new(
            config.clone(),
            Arc::new(FileStore::new(data_dir.clone()).await.expect("file store")),
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
            Arc::new(modules::registry()),
            lostandfound::new_store(),
            orbit::new_store(data_dir.clone())
                .await
                .expect("orbit store"),
            Arc::new(notification_store),
        );
        let app = register(Router::new()).with_state(state);

        let key_response = app
            .clone()
            .oneshot(
                Request::get("/notifications/vapid-public-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("key response");
        assert_eq!(key_response.status(), StatusCode::OK);

        let subscribe_response = app
            .clone()
            .oneshot(
                Request::post("/notifications/subscriptions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&subscription()).unwrap()))
                    .unwrap(),
            )
            .await
            .expect("subscribe response");
        assert_eq!(subscribe_response.status(), StatusCode::OK);

        let publish_response = app
            .oneshot(
                Request::post("/notifications")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"title":"New message","body":"Hello from chat","channel":"general"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .expect("publish response");
        assert_eq!(publish_response.status(), StatusCode::OK);
        let body = to_bytes(publish_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(response["subscriptions"], 1);
        assert_eq!(response["delivered"], 1);
        assert_eq!(response["failed"], 0);
    }
}
