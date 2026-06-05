use std::collections::HashMap;
use std::path::PathBuf;

use axum::{extract::State, http::StatusCode, Json};
use axum_extra::extract::Query;
use chrono::Utc;

use crate::models::ChatMessage;
use crate::storage;

fn get_avatar_url(base: &PathBuf, username: &str) -> String {
    let users: Vec<crate::models::User> = storage::JsonStore::new(base.join("users.json")).read();
    users.iter().find(|u| u.username == username).map(|u| u.avatar.clone()).unwrap_or_default()
}

fn enrich_with_avatar(base: &PathBuf, mut msgs: Vec<ChatMessage>) -> Vec<ChatMessage> {
    for msg in &mut msgs {
        let sender = if msg.from.is_empty() { &msg.sender } else { &msg.from };
        msg.sender_avatar = get_avatar_url(base, sender);
    }
    msgs
}

pub async fn send_message(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sender = body.get("sender").and_then(|v| v.as_str()).unwrap_or("");
    let receiver = body.get("receiver").and_then(|v| v.as_str()).unwrap_or("");
    let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("");

    if sender.is_empty() || receiver.is_empty() || message.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "发送者、接收者和消息内容不能为空"
        }).into());
    }

    let users: Vec<crate::models::User> = storage::JsonStore::new(base.join("users.json")).read();
    if !users.iter().any(|u| u.username == sender) {
        return Ok(serde_json::json!({
            "success": false, "message": "发送者不存在"
        }).into());
    }
    if !users.iter().any(|u| u.username == receiver) {
        return Ok(serde_json::json!({
            "success": false, "message": "接收者不存在"
        }).into());
    }

    let now = Utc::now();
    let msg = ChatMessage {
        id: now.timestamp_millis() as u64,
        sender: sender.to_string(),
        receiver: receiver.to_string(),
        message: message.to_string(),
        timestamp: now.to_rfc3339(),
        read_by: vec![sender.to_string()],
        from: String::new(),
        to: String::new(),
        msg_type: String::new(),
        sender_avatar: String::new(),
    };

    storage::ensure_dir(base.join("chat_messages"));
    let chat_path = storage::make_conv_path(&base.join("chat_messages"), sender, receiver);
    let mut messages: Vec<ChatMessage> = if chat_path.exists() {
        storage::read_json_array(&chat_path)
    } else {
        vec![]
    };
    messages.push(msg.clone());
    storage::write_json_array(&chat_path, &messages);

    let mut resp_msg = msg.clone();
    resp_msg.sender_avatar = get_avatar_url(&base, sender);

    let _state = axum::extract::State(std::sync::Arc::new(crate::models::AppState {
        online_users: dashmap::DashSet::new(),
        lan_online_users: dashmap::DashSet::new(),
        ws_clients: dashmap::DashMap::new(),
        request_stats: tokio::sync::Mutex::new(crate::models::RequestStats {
            today_date: String::new(),
            total_requests: 0,
            error_requests: 0,
            peak_concurrency: 0,
            current_concurrency: 0,
            data_transfer_bytes: 0,
        }),
        cached_cpu_usage: tokio::sync::Mutex::new(0.0),
        prev_cpu: tokio::sync::Mutex::new(None),
    }));

    Ok(serde_json::json!({
        "success": true,
        "message": "消息发送成功",
        "data": resp_msg
    }).into())
}

pub async fn get_messages(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sender = params.get("sender").map(|s| s.as_str()).unwrap_or("");
    let receiver = params.get("receiver").map(|s| s.as_str()).unwrap_or("");

    if sender.is_empty() || receiver.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "发送者和接收者不能为空"
        }).into());
    }

    storage::ensure_dir(base.join("chat_messages"));
    let chat_path = storage::make_conv_path(&base.join("chat_messages"), sender, receiver);
    let messages: Vec<ChatMessage> = if chat_path.exists() {
        storage::read_json_array(&chat_path)
    } else {
        vec![]
    };

    let enriched = enrich_with_avatar(&base, messages);

    Ok(serde_json::json!({
        "success": true,
        "messages": enriched
    }).into())
}

pub async fn mark_read(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let target = body.get("target").and_then(|v| v.as_str()).unwrap_or("");

    if username.is_empty() || target.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名和目标用户不能为空"
        }).into());
    }

    let chat_path = storage::make_conv_path(&base.join("chat_messages"), username, target);
    let mut messages: Vec<ChatMessage> = if chat_path.exists() {
        storage::read_json_array(&chat_path)
    } else {
        vec![]
    };

    let mut changed = 0;
    for msg in &mut messages {
        if !msg.read_by.contains(&username.to_string()) {
            msg.read_by.push(username.to_string());
            changed += 1;
        }
    }
    if changed > 0 {
        storage::write_json_array(&chat_path, &messages);
    }

    Ok(serde_json::json!({
        "success": true, "marked": changed
    }).into())
}

pub async fn mark_read_group(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let group_id = body.get("groupId").and_then(|v| v.as_str()).unwrap_or("");

    if username.is_empty() || group_id.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名和群ID不能为空"
        }).into());
    }

    let msg_path = base.join("lan_group_messages").join(format!("{}.json", group_id));
    let mut messages: Vec<ChatMessage> = if msg_path.exists() {
        storage::read_json_array(&msg_path)
    } else {
        vec![]
    };

    let mut changed = 0;
    for msg in &mut messages {
        if !msg.read_by.contains(&username.to_string()) {
            msg.read_by.push(username.to_string());
            changed += 1;
        }
    }
    if changed > 0 {
        storage::write_json_array(&msg_path, &messages);
    }

    Ok(serde_json::json!({
        "success": true, "marked": changed
    }).into())
}

// ── LAN Chat ──

pub async fn lan_login(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");

    if username.is_empty() || password.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名或密码不能为空"
        }).into());
    }

    let users: Vec<crate::models::User> = storage::JsonStore::new(base.join("users.json")).read();
    if let Some(user) = users.iter().find(|u| u.username == username && u.password == password) {
        let p: crate::models::UserPublic = user.clone().into();
        Ok(serde_json::json!({
            "success": true,
            "message": "登录成功",
            "user": p
        }).into())
    } else {
        Ok(serde_json::json!({
            "success": false, "message": "用户名或密码错误"
        }).into())
    }
}

pub async fn lan_logout(
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        }).into());
    }
    Ok(serde_json::json!({
        "success": true, "message": "退出成功"
    }).into())
}

pub async fn lan_friends(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let current = params.get("username").map(|s| s.as_str()).unwrap_or("");
    let users: Vec<crate::models::User> = storage::JsonStore::new(base.join("users.json")).read();

    let list: Vec<serde_json::Value> = users
        .iter()
        .filter(|u| u.username != current)
        .map(|u| {
            let p: crate::models::UserPublic = u.clone().into();
            serde_json::json!({
                "id": p.id,
                "username": p.username,
                "name": p.name,
                "email": p.email,
                "role": p.role,
                "avatar": p.avatar,
                "signature": p.signature,
                "network_location": p.network_location,
                "online": false
            })
        })
        .collect();

    Ok(serde_json::json!({
        "success": true,
        "friends": list
    }).into())
}

pub async fn lan_get_messages(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let from = params.get("from").map(|s| s.as_str()).unwrap_or("");
    let to = params.get("to").map(|s| s.as_str()).unwrap_or("");

    if from.is_empty() || to.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "发送者和接收者不能为空"
        }).into());
    }

    storage::ensure_dir(base.join("lan_chat_messages"));
    let chat_path = storage::make_conv_path(&base.join("lan_chat_messages"), from, to);
    let messages: Vec<ChatMessage> = if chat_path.exists() {
        storage::read_json_array(&chat_path)
    } else {
        vec![]
    };

    let enriched = enrich_with_avatar(&base, messages);

    Ok(serde_json::json!({
        "success": true,
        "messages": enriched
    }).into())
}

pub async fn lan_send_message(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let from = body.get("from").and_then(|v| v.as_str()).unwrap_or("");
    let to = body.get("to").and_then(|v| v.as_str()).unwrap_or("");
    let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let msg_type = body.get("type").and_then(|v| v.as_str()).unwrap_or("text");

    if from.is_empty() || to.is_empty() || message.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "发送者、接收者和消息内容不能为空"
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
        to: to.to_string(),
        msg_type: msg_type.to_string(),
        sender_avatar: get_avatar_url(&base, from),
    };

    storage::ensure_dir(base.join("lan_chat_messages"));
    let chat_path = storage::make_conv_path(&base.join("lan_chat_messages"), from, to);
    let mut messages: Vec<ChatMessage> = if chat_path.exists() {
        storage::read_json_array(&chat_path)
    } else {
        vec![]
    };
    messages.push(msg.clone());
    storage::write_json_array(&chat_path, &messages);

    tracing::info!("[LAN Msg] {} -> {}: {}", from, to, &message[..message.len().min(50)]);

    Ok(serde_json::json!({
        "success": true,
        "message": "消息发送成功",
        "data": msg
    }).into())
}

pub async fn lan_online(
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(serde_json::json!({
        "success": true,
        "onlineUsers": []
    }).into())
}

// ── Unread Counts ──

pub async fn unread_counts_get(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        }).into());
    }

    let mut conversations = serde_json::Map::new();
    let mut groups = serde_json::Map::new();

    let chat_dir = base.join("chat_messages");
    if chat_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&chat_dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".json") { continue; }
                let parts: Vec<&str> = fname.trim_end_matches(".json").split('_').collect();
                if parts.len() != 2 || !parts.contains(&username) { continue; }
                let other = if parts[0] == username { parts[1] } else { parts[0] };
                let msgs: Vec<ChatMessage> = storage::read_json_array(&entry.path());
                let unread = msgs.iter().filter(|m| {
                    m.sender != username && !m.read_by.contains(&username.to_string())
                }).count();
                if unread > 0 {
                    conversations.insert(other.to_string(), serde_json::json!(unread));
                }
            }
        }
    }

    let groups_list: Vec<crate::models::Group> = storage::JsonStore::new(base.join("lan_groups").join("groups.json")).read();
    for g in &groups_list {
        if !g.members.contains(&username.to_string()) { continue; }
        let msg_path = base.join("lan_group_messages").join(format!("{}.json", g.id));
        if !msg_path.exists() { continue; }
        let msgs: Vec<ChatMessage> = storage::read_json_array(&msg_path);
        let unread = msgs.iter().filter(|m| {
            m.from != username && !m.read_by.contains(&username.to_string())
        }).count();
        if unread > 0 {
            groups.insert(g.id.clone(), serde_json::json!(unread));
        }
    }

    Ok(serde_json::json!({
        "success": true,
        "conversations": conversations,
        "groups": groups
    }).into())
}

pub async fn unread_counts_post(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let read_points = body.get("readPoints").and_then(|v| v.as_object()).cloned().unwrap_or_default();

    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        }).into());
    }

    let mut conversations = serde_json::Map::new();
    let mut groups = serde_json::Map::new();

    let chat_dir = base.join("chat_messages");
    if chat_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&chat_dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".json") { continue; }
                let parts: Vec<&str> = fname.trim_end_matches(".json").split('_').collect();
                if parts.len() != 2 || !parts.contains(&username) { continue; }
                let other = if parts[0] == username { parts[1] } else { parts[0] };
                let conv_key = format!("user:{}", other);
                let last_read = read_points.get(&conv_key).and_then(|v| v.as_u64()).unwrap_or(0);
                let msgs: Vec<ChatMessage> = storage::read_json_array(&entry.path());
                let unread = msgs.iter().filter(|m| {
                    m.sender != username && (m.id > last_read)
                }).count();
                conversations.insert(other.to_string(), serde_json::json!(unread));
            }
        }
    }

    let groups_list: Vec<crate::models::Group> = storage::JsonStore::new(base.join("lan_groups").join("groups.json")).read();
    for g in &groups_list {
        if !g.members.contains(&username.to_string()) { continue; }
        let conv_key = format!("group:{}", g.id);
        let last_read = read_points.get(&conv_key).and_then(|v| v.as_u64()).unwrap_or(0);
        let msg_path = base.join("lan_group_messages").join(format!("{}.json", g.id));
        if !msg_path.exists() { continue; }
        let msgs: Vec<ChatMessage> = storage::read_json_array(&msg_path);
        let unread = msgs.iter().filter(|m| {
            m.from != username && (m.id > last_read)
        }).count();
        groups.insert(g.id.clone(), serde_json::json!(unread));
    }

    Ok(serde_json::json!({
        "success": true,
        "conversations": conversations,
        "groups": groups
    }).into())
}
