use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapability {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCapability {
    pub ddgs: ToolCapability,
    pub freeze: ToolCapability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleCapability {
    pub name: String,
    pub enabled: bool,
    pub healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub public_mode: bool,
    pub modules: Vec<ModuleCapability>,
    pub tools: ToolsCapability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safesearch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timelimit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractRequest {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractResponse {
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub id: String,
    pub original_name: String,
    pub size: u64,
    pub mime_type: String,
    pub sha256: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResponse {
    pub file: FileMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileListResponse {
    pub files: Vec<FileMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderFormat {
    Svg,
    Png,
}

impl RenderFormat {
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Svg => "image/svg+xml",
            Self::Png => "image/png",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Png => "png",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderSnippetRequest {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    pub format: RenderFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticFileEntry {
    pub name: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticFileListResponse {
    pub files: Vec<StaticFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LostAndFoundHealthResponse {
    pub ok: bool,
    pub service: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LostAndFoundUser {
    pub id: u64,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LostAndFoundLoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LostAndFoundLoginResponse {
    pub token: String,
    pub user: LostAndFoundUser,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LostAndFoundItemStatus {
    Visible,
    Returned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LostAndFoundClaimStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LostAndFoundItem {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub category: String,
    pub found_location: String,
    pub found_date: String,
    pub found_time: String,
    pub image_url: String,
    pub status: LostAndFoundItemStatus,
    pub created_by_user_id: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LostAndFoundClaim {
    pub id: u64,
    pub item_id: u64,
    pub claimer_user_id: u64,
    pub status: LostAndFoundClaimStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LostAndFoundItemFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<LostAndFoundItemStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LostAndFoundCreateItemRequest {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub category: String,
    pub found_location: String,
    pub found_date: String,
    pub found_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_user_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LostAndFoundUpdateItemStatusRequest {
    pub status: LostAndFoundItemStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LostAndFoundCreateClaimRequest {
    pub item_id: u64,
    pub claimer_user_id: u64,
}
