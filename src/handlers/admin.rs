use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::auth_store;
use crate::middleware::auth_header;
use crate::models::{AppState, RoleConfig, User};
use crate::security;
use crate::storage;

fn read_users(base: &PathBuf) -> Vec<User> {
    storage::JsonStore::new(base.join("users.json")).read()
}

fn write_users(base: &PathBuf, users: &Vec<User>) {
    storage::JsonStore::new(base.join("users.json")).write(users);
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

    let mut users = read_users(&base);
    let idx = match users.iter().position(|u| u.username == username) {
        Some(i) => i,
        None => {
            return Ok(Json(serde_json::json!({
                "success": false, "message": "用户不存在"
            })))
        }
    };

    let stored = users[idx].password.clone();
    if !security::verify_password(stored.clone(), password.to_string()).await {
        return Ok(Json(serde_json::json!({
            "success": false,
            "message": if username == "admin" { "管理员密码错误" } else { "密码错误" }
        })));
    }

    if !security::is_hashed(&stored) {
        users[idx].password = security::hash_password(password.to_string()).await;
        write_users(&base, &users);
    }

    let role = users[idx].role.clone();
    let token = auth_store::issue_and_store(&base, username);

    Ok(Json(serde_json::json!({
        "success": true,
        "token": token,
        "username": username,
        "role": if username == "admin" { "admin".to_string() } else { role }
    })))
}

pub async fn get_accounts(
    State(base): State<PathBuf>,
    State(_state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
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
    let auth_user = auth_store::verify_token(&base, &token).unwrap_or_default();

    let users = read_users(&base);
    let roles_config: RoleConfig = storage::JsonStore::new(base.join("roles_config.json")).read();
    let role_id = users
        .iter()
        .find(|u| u.username == auth_user)
        .map(|u| u.role.clone())
        .unwrap_or_else(|| "guest".to_string());
    let can_manage = roles_config
        .roles
        .iter()
        .find(|r| r.id == role_id)
        .map(|r| r.permissions.contains(&"manage_users".to_string()))
        .unwrap_or(false);
    if !can_manage && auth_user != "admin" {
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
        password: security::hash_password(new_password.to_string()).await,
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
        network_location: "public".to_string(),
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
    let auth_user = auth_store::verify_token(&base, &token).unwrap_or_default();

    if target == auth_user {
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
            auth_store::revoke_user(&base, &target);
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
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
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

            // 管理员重置密码，重置后原凭证立即失效
            if let Some(pw) = body.get("password").and_then(|v| v.as_str()) {
                if !pw.is_empty() {
                    users[pos].password = security::hash_password(pw.to_string()).await;
                    auth_store::revoke_user(&base, &target);
                }
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
