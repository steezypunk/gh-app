use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct App {
    pub id: u64,
    pub slug: String,
    #[serde(default)]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Owner {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Installation {
    pub id: u64,
    pub app_id: u64,
    pub account: Owner,
    pub repository_selection: String,
    #[serde(default)]
    pub permissions: BTreeMap<String, String>,
    #[serde(default)]
    pub suspended_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub token: String,
    pub expires_at: String,
    #[serde(default)]
    pub permissions: BTreeMap<String, String>,
    pub repository_selection: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepositoryPage {
    pub total_count: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MintRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repositories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<BTreeMap<String, String>>,
}
