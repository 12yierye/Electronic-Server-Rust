use std::collections::HashMap;
use std::path::PathBuf;

use axum::{extract::State, http::StatusCode, Json};
use axum_extra::extract::Query;
use chrono::Utc;

use crate::models::{Broadcast, OrgNode, RoleConfig, User};
use crate::storage;

fn resolve_target_users(base: &PathBuf, target_node_ids: &[String]) -> Vec<String> {
    let tree: OrgNode = storage::JsonStore::new(base.join("org_tree.json")).read();
    let mut all_users = Vec::new();

    for node_id in target_node_ids {
        collect_members(&tree, node_id, &mut all_users);
    }

    all_users.sort();
    all_users.dedup();
    all_users
}

fn collect_members(node: &OrgNode, target_id: &str, result: &mut Vec<String>) {
    if node.id == target_id {
        for m in &node.members {
            result.push(m.clone());
        }
    }
    for child in &node.children {
        collect_members(child, target_id, result);
    }
}

pub async fn send_broadcast(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sender_id = body.get("senderId").and_then(|v| v.as_str()).unwrap_or("");
    let title = body.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let target_node_ids: Vec<String> = body.get("targetNodeIds")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let attachments: Vec<String> = body.get("attachments")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    if sender_id.is_empty() || title.is_empty() || content.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "发送者、标题和内容不能为空"
        }).into());
    }

    // Permission check
    let users: Vec<User> = storage::JsonStore::new(base.join("users.json")).read();
    let user = users.iter().find(|u| u.username == sender_id);
    let roles_config: RoleConfig = storage::JsonStore::new(base.join("roles_config.json")).read();
    let role = roles_config.roles.iter().find(|r| {
        r.id == user.map(|u| u.role.as_str()).unwrap_or("guest")
    });

    let has_broadcast_all = role.map(|r| r.permissions.contains(&"broadcast_all".to_string())).unwrap_or(false);
    let has_broadcast_class = role.map(|r| r.permissions.contains(&"broadcast_class".to_string())).unwrap_or(false);

    if !has_broadcast_all {
        if !has_broadcast_class {
            return Ok(serde_json::json!({
                "success": false, "message": "您没有发送广播的权限"
            }).into());
        }
        if let Some(u) = user {
            let managed = &u.managed_nodes;
            let allowed = target_node_ids.iter().all(|nid| managed.contains(nid));
            if !allowed {
                return Ok(serde_json::json!({
                    "success": false, "message": "无权向该范围发送广播"
                }).into());
            }
        }
    }

    let target_users = resolve_target_users(&base, &target_node_ids);
    if target_users.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "目标范围内没有用户"
        }).into());
    }

    let broadcasts: Vec<Broadcast> = storage::JsonStore::new(base.join("broadcasts.json")).read();
    let broadcast_id = format!("bc_{}_{}", Utc::now().timestamp_millis(), &random_str(8));

    let broadcast = Broadcast {
        id: broadcast_id,
        sender_name: sender_id.to_string(),
        target_node_ids,
        target_count: target_users.len(),
        title: title.to_string(),
        content: content.to_string(),
        attachments,
        timestamp: Utc::now().to_rfc3339(),
        read_by: vec![],
    };

    let mut all = broadcasts;
    all.push(broadcast.clone());
    storage::JsonStore::new(base.join("broadcasts.json")).write(&all);

    tracing::info!("[Broadcast] {} sent '{}' to {} users", sender_id, title, target_users.len());

    Ok(serde_json::json!({
        "success": true,
        "broadcast": broadcast,
        "message": "广播发送成功"
    }).into())
}

pub async fn list_broadcasts(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        }).into());
    }

    let broadcasts: Vec<Broadcast> = storage::JsonStore::new(base.join("broadcasts.json")).read();
    let mut user_broadcasts: Vec<serde_json::Value> = vec![];

    for bc in &broadcasts {
        let target_users = resolve_target_users(&base, &bc.target_node_ids);
        if target_users.contains(&username.to_string()) {
            user_broadcasts.push(serde_json::json!({
                "id": bc.id,
                "senderName": bc.sender_name,
                "title": bc.title,
                "content": bc.content,
                "attachments": bc.attachments,
                "timestamp": bc.timestamp,
                "read": bc.read_by.contains(&username.to_string()),
                "readCount": bc.read_by.len(),
                "totalCount": bc.target_count
            }));
        }
    }

    Ok(serde_json::json!({
        "success": true,
        "broadcasts": user_broadcasts
    }).into())
}

pub async fn get_broadcast_receipts(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let broadcasts: Vec<Broadcast> = storage::JsonStore::new(base.join("broadcasts.json")).read();
    let bc = broadcasts.iter().find(|b| b.id == id);

    match bc {
        Some(bc) => {
            let target_users = resolve_target_users(&base, &bc.target_node_ids);
            let read_by: Vec<String> = target_users.iter()
                .filter(|u| bc.read_by.contains(*u))
                .cloned()
                .collect();
            let unread_by: Vec<String> = target_users.iter()
                .filter(|u| !bc.read_by.contains(*u))
                .cloned()
                .collect();

            Ok(serde_json::json!({
                "success": true,
                "total": bc.target_count,
                "read": bc.read_by.len(),
                "unread": unread_by.len(),
                "readBy": read_by,
                "unreadBy": unread_by
            }).into())
        }
        None => Ok(serde_json::json!({
            "success": false, "message": "广播不存在"
        }).into()),
    }
}

pub async fn mark_broadcast_read(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        }).into());
    }

    let mut broadcasts: Vec<Broadcast> = storage::JsonStore::new(base.join("broadcasts.json")).read();
    let bc = broadcasts.iter_mut().find(|b| b.id == id);

    match bc {
        Some(bc) => {
            if !bc.read_by.contains(&username.to_string()) {
                bc.read_by.push(username.to_string());
                storage::JsonStore::new(base.join("broadcasts.json")).write(&broadcasts);
            }
            Ok(serde_json::json!({
                "success": true, "message": "已标记为已读"
            }).into())
        }
        None => Ok(serde_json::json!({
            "success": false, "message": "广播不存在"
        }).into()),
    }
}

fn random_str(len: usize) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();
    (0..len).map(|i| {
        let idx = (seed.wrapping_mul(i as u64 + 1).wrapping_add(seed >> (i % 8))) as usize % chars.len();
        chars[idx]
    }).collect()
}
