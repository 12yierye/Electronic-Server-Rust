use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::middleware::sync_secret;
use crate::models::{AppState, User};
use crate::storage;

pub async fn sync_users(
    State(state): State<Arc<AppState>>,
    State(base): State<PathBuf>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let expected = std::env::var("SYNC_SECRET").unwrap_or_default();
    if expected.is_empty() || sync_secret(&headers).unwrap_or_default() != expected {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "未授权"
        })));
    }

    let users: Vec<User> = storage::JsonStore::new(base.join("users.json")).read();
    // 主从同步走可信通道，需下发完整用户（含密码哈希），否则从端解析会失败并清空 users.json
    let online_users: Vec<String> = state.online_users.iter().map(|s| s.key().clone()).collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "users": users,
        "online_users": online_users,
    })))
}
