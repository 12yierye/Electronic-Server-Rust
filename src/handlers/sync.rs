use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use crate::models::{AppState, User, UserPublic};
use crate::storage;

#[derive(Deserialize)]
pub struct SyncQuery {
    pub secret: String,
    #[allow(dead_code)]
    pub since: Option<String>,
}

pub async fn sync_users(
    State(state): State<Arc<AppState>>,
    State(base): State<PathBuf>,
    Query(params): Query<SyncQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let expected = std::env::var("SYNC_SECRET").unwrap_or_default();
    if expected.is_empty() || params.secret != expected {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "未授权"
        })));
    }

    let users: Vec<User> = storage::JsonStore::new(base.join("users.json")).read();
    let public_users: Vec<UserPublic> = users
        .into_iter()
        .map(|u| {
            let online = state.online_users.contains(&u.username);
            let mut pub_user: UserPublic = u.into();
            pub_user.online = Some(online);
            pub_user
        })
        .collect();

    let online_users: Vec<String> = state.online_users.iter().map(|s| s.key().clone()).collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "users": public_users,
        "online_users": online_users,
    })))
}
