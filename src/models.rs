use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ── User ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub password: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_role")]
    pub role: String,
    #[serde(default)]
    pub avatar: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub birthday: String,
    #[serde(default = "default_gender")]
    pub gender: String,
    #[serde(default)]
    pub starred_users: Vec<String>,
    #[serde(default)]
    pub friends: Vec<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub managed_nodes: Vec<String>,
    #[serde(default = "default_network_location")]
    pub network_location: String,
}

fn default_network_location() -> String {
    "public".to_string()
}

fn default_role() -> String {
    "teacher".to_string()
}
fn default_gender() -> String {
    "none".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPublic {
    pub id: u64,
    pub username: String,
    pub email: String,
    pub name: String,
    pub role: String,
    pub avatar: String,
    pub signature: String,
    pub birthday: String,
    pub gender: String,
    pub starred_users: Vec<String>,
    pub friends: Vec<String>,
    pub title: String,
    pub subject: String,
    pub managed_nodes: Vec<String>,
    #[serde(default = "default_network_location")]
    pub network_location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online: Option<bool>,
}

impl From<User> for UserPublic {
    fn from(u: User) -> Self {
        UserPublic {
            id: u.id,
            username: u.username,
            email: u.email,
            name: u.name,
            role: u.role,
            avatar: u.avatar,
            signature: u.signature,
            birthday: u.birthday,
            gender: u.gender,
            starred_users: u.starred_users,
            friends: u.friends,
            title: u.title,
            subject: u.subject,
            managed_nodes: u.managed_nodes,
            network_location: u.network_location,
            online: None,
        }
    }
}

// ── Chat Messages ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: u64,
    pub sender: String,
    pub receiver: String,
    pub message: String,
    pub timestamp: String,
    #[serde(default)]
    pub read_by: Vec<String>,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub sender_avatar: String,
}

// ── LAN / Group ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub creator: String,
    pub members: Vec<String>,
    #[serde(default)]
    pub network_type: String,
    pub created_at: String,
}

// ── File Metadata ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub last_modified: u64,
    #[serde(default)]
    pub uploader: String,
    #[serde(default)]
    pub upload_time: String,
}

// ── Pending (Offline) Files ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingFileMeta {
    pub id: String,
    pub sender: String,
    pub receiver: String,
    pub filename: String,
    #[serde(default)]
    pub size: u64,
    pub expire_at: String,
    pub created_at: String,
}

// ── Friend Request ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendRequest {
    pub id: u64,
    pub sender: String,
    pub receiver: String,
    pub status: String,
    pub timestamp: String,
}

// ── Credential ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub username: String,
    pub token: String,
    pub created_at: u64,
}

// ── Broadcast ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Broadcast {
    pub id: String,
    pub sender_name: String,
    pub target_node_ids: Vec<String>,
    pub target_count: usize,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<String>,
    pub timestamp: String,
    #[serde(default)]
    pub read_by: Vec<String>,
}

// ── Org Tree ──

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrgNode {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub children: Vec<OrgNode>,
    #[serde(default)]
    pub members: Vec<String>,
}

// ── Role Config ──

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleConfig {
    pub roles: Vec<Role>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub permissions: Vec<String>,
}

// ── Request Stats ──

#[derive(Debug, Clone)]
pub struct RequestStats {
    pub today_date: String,
    pub total_requests: u64,
    pub error_requests: u64,
    pub peak_concurrency: u64,
    pub current_concurrency: u64,
    pub data_transfer_bytes: u64,
}

// ── CPU Snapshot ──

#[derive(Debug, Clone)]
pub struct CpuSnapshot {
    pub idle: u64,
    pub total: u64,
}

// ── App State ──

#[derive(Debug)]
pub struct AppState {
    pub online_users: dashmap::DashSet<String>,
    pub lan_online_users: dashmap::DashSet<String>,
    pub ws_clients: dashmap::DashMap<String, HashSet<String>>,
    pub request_stats: tokio::sync::Mutex<RequestStats>,
    pub cached_cpu_usage: tokio::sync::Mutex<f64>,
    pub prev_cpu: tokio::sync::Mutex<Option<CpuSnapshot>>,
}
