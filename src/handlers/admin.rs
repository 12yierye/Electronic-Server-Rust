use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::Utc;

use crate::middleware::auth_header;
use crate::models::{AppState, Credential, RoleConfig, User};
use crate::storage;

fn read_users(base: &PathBuf) -> Vec<User> {
    storage::JsonStore::new(base.join("users.json")).read()
}

fn write_users(base: &PathBuf, users: &Vec<User>) {
    storage::JsonStore::new(base.join("users.json")).write(users);
}

fn read_credentials(base: &PathBuf) -> Vec<Credential> {
    storage::JsonStore::new(base.join("credentials.json")).read()
}

fn write_credentials(base: &PathBuf, creds: &Vec<Credential>) {
    storage::JsonStore::new(base.join("credentials.json")).write(creds);
}

pub async fn admin_login(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");

    if username.is_empty() || password.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名或密码不能为空"
        })));
    }

    let users = read_users(&base);

    if username == "admin" {
        if users.iter().any(|u| u.username == "admin" && u.password == password) {
            let token = create_token(username);
            let mut credentials = read_credentials(&base);
            credentials.retain(|c| c.username != username);
            credentials.push(Credential {
                username: username.to_string(),
                token: token.clone(),
                created_at: Utc::now().timestamp_millis() as u64,
            });
            write_credentials(&base, &credentials);
            return Ok(Json(serde_json::json!({
                "success": true, "token": token, "username": username, "role": "admin"
            })));
        }
        return Ok(Json(serde_json::json!({
            "success": false, "message": "管理员密码错误"
        })));
    }

    let user = users.iter().find(|u| u.username == username);
    match user {
        Some(u) if u.password == password => {
            let token = create_token(username);
            let mut credentials = read_credentials(&base);
            credentials.retain(|c| c.username != username);
            credentials.push(Credential {
                username: username.to_string(),
                token: token.clone(),
                created_at: Utc::now().timestamp_millis() as u64,
            });
            write_credentials(&base, &credentials);
            Ok(Json(serde_json::json!({
                "success": true, "token": token, "username": username, "role": u.role
            })))
        }
        Some(_) => Ok(Json(serde_json::json!({
            "success": false, "message": "密码错误"
        }))),
        None => Ok(Json(serde_json::json!({
            "success": false, "message": "用户不存在"
        }))),
    }
}

fn create_token(username: &str) -> String {
    format!("{}:{}:{}", username, Utc::now().timestamp_millis(), &random_str(12))
}

fn verify_token(base: &PathBuf, token: &str) -> Option<String> {
    let credentials = read_credentials(base);
    let cred = credentials.iter().find(|c| c.token == token)?;
    let thirty_days: u64 = 30 * 24 * 60 * 60 * 1000;
    if Utc::now().timestamp_millis() as u64 - cred.created_at > thirty_days {
        let mut all = credentials;
        all.retain(|c| c.token != token);
        write_credentials(base, &all);
        return None;
    }
    Some(cred.username.clone())
}

pub async fn get_accounts(
    State(base): State<PathBuf>,
    State(_state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let token = auth_header(&headers).unwrap_or_default();
    if verify_token(&base, &token).is_none() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "未提供凭证或凭证无效"
        })));
    }

    let users = read_users(&base);
    let publics: Vec<serde_json::Value> = users.iter().map(|u| {
        serde_json::json!({
            "id": u.id,
            "username": u.username,
            "name": u.name,
            "email": u.email,
            "role": u.role,
            "title": u.title,
            "subject": u.subject,
            "managedNodes": u.managed_nodes,
            "avatar": u.avatar,
        })
    }).collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "users": publics,
        "total": publics.len()
    })))
}

pub async fn add_account(
    State(base): State<PathBuf>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let token = auth_header(&headers).unwrap_or_default();
    let auth_user = verify_token(&base, &token);
    if auth_user.is_none() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "未提供凭证或凭证无效"
        })));
    }

    let users = read_users(&base);
    let current_user = users.iter().find(|u| u.username == auth_user.as_deref().unwrap_or(""));
    let roles_config: RoleConfig = storage::JsonStore::new(base.join("roles_config.json")).read();
    let role_obj = roles_config.roles.iter().find(|r| r.id == current_user.map(|u| u.role.as_str()).unwrap_or("guest"));
    let can_manage = role_obj.map(|r| r.permissions.contains(&"manage_users".to_string())).unwrap_or(false);
    if !can_manage && auth_user.as_deref() != Some("admin") {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "权限不足"
        })));
    }

    let new_username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let new_password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");
    let new_role = body.get("role").and_then(|v| v.as_str()).unwrap_or("student");

    if new_username.is_empty() || new_password.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名和密码不能为空"
        })));
    }

    let mut users = read_users(&base);
    if users.iter().any(|u| u.username == new_username) {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名已存在"
        })));
    }

    let max_id = users.iter().map(|u| u.id).max().unwrap_or(0);
    users.push(User {
        id: max_id + 1,
        username: new_username.to_string(),
        password: new_password.to_string(),
        email: body.get("email").and_then(|v| v.as_str()).unwrap_or(&format!("{}@local", new_username)).to_string(),
        name: body.get("name").and_then(|v| v.as_str()).unwrap_or(new_username).to_string(),
        role: new_role.to_string(),
        title: body.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        subject: body.get("subject").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        managed_nodes: body.get("managedNodes").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        avatar: String::new(),
        signature: String::new(),
        birthday: String::new(),
        gender: "none".to_string(),
        starred_users: vec![],
        friends: vec![],
    });
    write_users(&base, &users);

    Ok(Json(serde_json::json!({
        "success": true,
        "user": serde_json::json!({
            "id": max_id + 1,
            "username": new_username,
            "name": new_username,
            "role": new_role
        }),
        "message": "账户创建成功"
    })))
}

pub async fn delete_account(
    axum::extract::Path(target): axum::extract::Path<String>,
    State(base): State<PathBuf>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let token = auth_header(&headers).unwrap_or_default();
    let auth_user = verify_token(&base, &token);
    if auth_user.is_none() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "未提供凭证或凭证无效"
        })));
    }

    if target == auth_user.as_deref().unwrap_or("") {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "不能删除自己的账户"
        })));
    }

    let mut users = read_users(&base);
    let pos = users.iter().position(|u| u.username == target);
    match pos {
        Some(pos) => {
            users.remove(pos);
            write_users(&base, &users);
            let mut credentials = read_credentials(&base);
            credentials.retain(|c| c.username != target);
            write_credentials(&base, &credentials);
            Ok(Json(serde_json::json!({
                "success": true, "message": format!("用户 {} 已删除", target)
            })))
        }
        None => Ok(Json(serde_json::json!({
            "success": false, "message": "用户不存在"
        }))),
    }
}

pub async fn update_account(
    axum::extract::Path(target): axum::extract::Path<String>,
    State(base): State<PathBuf>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let token = auth_header(&headers).unwrap_or_default();
    if verify_token(&base, &token).is_none() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "未提供凭证或凭证无效"
        })));
    }

    let mut users = read_users(&base);
    let pos = users.iter().position(|u| u.username == target);

    match pos {
        Some(pos) => {
            if let Some(v) = body.get("name").and_then(|v| v.as_str()) { users[pos].name = v.to_string(); }
            if let Some(v) = body.get("role").and_then(|v| v.as_str()) { users[pos].role = v.to_string(); }
            if let Some(v) = body.get("title").and_then(|v| v.as_str()) { users[pos].title = v.to_string(); }
            if let Some(v) = body.get("subject").and_then(|v| v.as_str()) { users[pos].subject = v.to_string(); }
            if let Some(v) = body.get("email").and_then(|v| v.as_str()) { users[pos].email = v.to_string(); }
            if let Some(v) = body.get("managedNodes").and_then(|v| v.as_array()) {
                users[pos].managed_nodes = v.iter().filter_map(|x| x.as_str().map(String::from)).collect();
            }
            let resp_user = serde_json::json!({
                "username": users[pos].username,
                "name": users[pos].name,
                "role": users[pos].role,
                "title": users[pos].title,
                "subject": users[pos].subject,
                "managedNodes": users[pos].managed_nodes,
                "email": users[pos].email
            });
            write_users(&base, &users);
            Ok(Json(serde_json::json!({
                "success": true,
                "message": "用户信息已更新",
                "user": resp_user
            })))
        }
        None => Ok(Json(serde_json::json!({
            "success": false, "message": "用户不存在"
        }))),
    }
}

pub async fn get_roles(
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let config: RoleConfig = storage::JsonStore::new(base.join("roles_config.json")).read();
    Ok(Json(serde_json::json!({
        "success": true,
        "roles": config.roles
    })))
}

pub async fn account_stats(
    State(base): State<PathBuf>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let users = read_users(&base);
    Ok(Json(serde_json::json!({
        "success": true,
        "totalUsers": users.len(),
        "onlineUsers": state.online_users.len()
    })))
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
