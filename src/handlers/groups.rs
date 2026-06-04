use std::collections::HashMap;
use std::path::PathBuf;

use axum::{extract::State, http::StatusCode, Json};
use axum_extra::extract::Query;
use chrono::Utc;

use crate::models::{ChatMessage, Group};
use crate::storage;

pub async fn create_group(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let creator = body.get("creator").and_then(|v| v.as_str()).unwrap_or("");
    let members = body.get("members").and_then(|v| v.as_array());
    let network_type = body.get("networkType").and_then(|v| v.as_str()).unwrap_or("lan");

    if name.is_empty() || creator.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "群名称和创建者不能为空"
        }).into());
    }

    storage::ensure_dir(base.join("lan_groups"));
    let mut groups: Vec<Group> = storage::JsonStore::new(base.join("lan_groups").join("groups.json")).read();

    let group_id = format!("group_{}_{}", Utc::now().timestamp_millis(), &random_str(9));

    let mut member_set: Vec<String> = vec![creator.to_string()];
    if let Some(m) = members {
        for v in m {
            let s = v.as_str().unwrap_or("");
            if !s.is_empty() && !member_set.contains(&s.to_string()) {
                member_set.push(s.to_string());
            }
        }
    }

    let new_group = Group {
        id: group_id,
        name: name.to_string(),
        creator: creator.to_string(),
        members: member_set,
        network_type: network_type.to_string(),
        created_at: Utc::now().to_rfc3339(),
    };

    groups.push(new_group.clone());
    storage::JsonStore::new(base.join("lan_groups").join("groups.json")).write(&groups);

    tracing::info!("[Group] {} created '{}' with members: {:?}", creator, name, new_group.members);

    Ok(serde_json::json!({
        "success": true,
        "message": "群聊创建成功",
        "group": new_group
    }).into())
}

pub async fn get_user_groups(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        }).into());
    }

    let groups: Vec<Group> = storage::JsonStore::new(base.join("lan_groups").join("groups.json")).read();
    let user_groups: Vec<&Group> = groups.iter().filter(|g| g.members.contains(&username.to_string())).collect();

    Ok(serde_json::json!({
        "success": true,
        "groups": user_groups
    }).into())
}

pub async fn get_group_messages(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let group_id = params.get("groupId").map(|s| s.as_str()).unwrap_or("");
    if group_id.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "群ID不能为空"
        }).into());
    }

    let msg_path = base.join("lan_group_messages").join(format!("{}.json", group_id));
    let messages: Vec<ChatMessage> = if msg_path.exists() {
        storage::read_json_array(&msg_path)
    } else {
        vec![]
    };

    Ok(serde_json::json!({
        "success": true,
        "messages": messages
    }).into())
}

pub async fn send_group_message(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let group_id = body.get("groupId").and_then(|v| v.as_str()).unwrap_or("");
    let from = body.get("from").and_then(|v| v.as_str()).unwrap_or("");
    let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let msg_type = body.get("type").and_then(|v| v.as_str()).unwrap_or("text");

    if group_id.is_empty() || from.is_empty() || message.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "群ID、发送者和消息内容不能为空"
        }).into());
    }

    let groups: Vec<Group> = storage::JsonStore::new(base.join("lan_groups").join("groups.json")).read();
    let group = groups.iter().find(|g| g.id == group_id);

    match group {
        Some(g) => {
            if !g.members.contains(&from.to_string()) {
                return Ok(serde_json::json!({
                    "success": false, "message": "您不是该群成员"
                }).into());
            }

            let now = Utc::now();
            let msg = ChatMessage {
                id: now.timestamp_millis() as u64,
                sender: String::new(),
                receiver: String::new(),
                message: message.to_string(),
                timestamp: now.to_rfc3339(),
                read_by: vec![from.to_string()],
                from: from.to_string(),
                to: String::new(),
                msg_type: msg_type.to_string(),
                sender_avatar: String::new(),
            };

            storage::ensure_dir(base.join("lan_group_messages"));
            let msg_path = base.join("lan_group_messages").join(format!("{}.json", group_id));
            let mut messages: Vec<ChatMessage> = if msg_path.exists() {
                storage::read_json_array(&msg_path)
            } else {
                vec![]
            };
            messages.push(msg.clone());
            storage::write_json_array(&msg_path, &messages);

            tracing::info!("[Group Msg] {} in {}: {}", from, g.name, &message[..message.len().min(50)]);

            Ok(serde_json::json!({
                "success": true,
                "message": "消息发送成功",
                "data": msg
            }).into())
        }
        None => Ok(serde_json::json!({
            "success": false, "message": "群聊不存在"
        }).into()),
    }
}

pub async fn join_group(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let group_id = body.get("groupId").and_then(|v| v.as_str()).unwrap_or("");
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");

    if group_id.is_empty() || username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "群ID和用户名不能为空"
        }).into());
    }

    let mut groups: Vec<Group> = storage::JsonStore::new(base.join("lan_groups").join("groups.json")).read();
    let gi = groups.iter().position(|g| g.id == group_id);

    match gi {
        Some(gi) => {
            if groups[gi].members.contains(&username.to_string()) {
                return Ok(serde_json::json!({
                    "success": false, "message": "您已经是群成员"
                }).into());
            }
            groups[gi].members.push(username.to_string());
            storage::JsonStore::new(base.join("lan_groups").join("groups.json")).write(&groups);
            tracing::info!("[Group] {} joined '{}'", username, groups[gi].name);
            Ok(serde_json::json!({
                "success": true,
                "message": "加入群聊成功",
                "group": &groups[gi]
            }).into())
        }
        None => Ok(serde_json::json!({
            "success": false, "message": "群聊不存在"
        }).into()),
    }
}

pub async fn leave_group(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let group_id = body.get("groupId").and_then(|v| v.as_str()).unwrap_or("");
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");

    if group_id.is_empty() || username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "群ID和用户名不能为空"
        }).into());
    }

    let mut groups: Vec<Group> = storage::JsonStore::new(base.join("lan_groups").join("groups.json")).read();
    let gi = groups.iter().position(|g| g.id == group_id);

    match gi {
        Some(gi) => {
            if groups[gi].creator == username {
                return Ok(serde_json::json!({
                    "success": false, "message": "群主不能退群，请选择解散群聊"
                }).into());
            }
            groups[gi].members.retain(|m| m != username);
            storage::JsonStore::new(base.join("lan_groups").join("groups.json")).write(&groups);
            tracing::info!("[Group] {} left '{}'", username, groups[gi].name);
            Ok(serde_json::json!({
                "success": true, "message": "退出群聊成功"
            }).into())
        }
        None => Ok(serde_json::json!({
            "success": false, "message": "群聊不存在"
        }).into()),
    }
}

pub async fn delete_group(
    axum::extract::Path(group_id): axum::extract::Path<String>,
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        }).into());
    }

    let mut groups: Vec<Group> = storage::JsonStore::new(base.join("lan_groups").join("groups.json")).read();
    let gi = groups.iter().position(|g| g.id == group_id);

    match gi {
        Some(gi) => {
            if groups[gi].creator != username {
                return Ok(serde_json::json!({
                    "success": false, "message": "只有群主才能解散群聊"
                }).into());
            }
            let name = groups[gi].name.clone();
            groups.remove(gi);
            storage::JsonStore::new(base.join("lan_groups").join("groups.json")).write(&groups);

            let msg_path = base.join("lan_group_messages").join(format!("{}.json", group_id));
            if msg_path.exists() {
                let _ = std::fs::remove_file(&msg_path);
            }
            tracing::info!("[Group] '{}' deleted by {}", name, username);
            Ok(serde_json::json!({
                "success": true, "message": "群聊已解散"
            }).into())
        }
        None => Ok(serde_json::json!({
            "success": false, "message": "群聊不存在"
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
