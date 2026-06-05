use std::path::PathBuf;
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use axum_extra::extract::Query;
use serde::Deserialize;

use crate::models::{AppState, User, UserPublic};
use crate::storage;

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Deserialize)]
pub struct RegisterReq {
    pub username: Option<String>,
    pub password: Option<String>,
    pub email: Option<String>,
    #[allow(dead_code)]
    pub role: Option<String>,
    #[allow(dead_code)]
    pub network_location: Option<String>,
}

fn get_users_store(base: &PathBuf) -> storage::JsonStore<Vec<User>> {
    storage::JsonStore::new(base.join("users.json"))
}

fn read_users(base: &PathBuf) -> Vec<User> {
    get_users_store(base).read()
}

fn write_users(base: &PathBuf, users: &Vec<User>) {
    get_users_store(base).write(users);
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    State(base): State<PathBuf>,
    Json(body): Json<LoginReq>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.username.unwrap_or_default();
    let password = body.password.unwrap_or_default();

    if username.is_empty() || password.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名或密码不能为空"
        })));
    }

    let users = read_users(&base);
    if let Some(user) = users.iter().find(|u| u.username == username && u.password == password) {
        storage::ensure_dir(base.join("user_files"));
        let uf_path = base.join("user_files").join(format!("{}.json", username));
        if !uf_path.exists() {
            storage::write_json_array(&uf_path, &Vec::<crate::models::FileMeta>::new());
        }

        state.online_users.insert(username.clone());
        tracing::info!("User {} logged in", username);

        let pub_user: UserPublic = user.clone().into();
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "登录成功",
            "user": pub_user,
            "pendingFiles": []
        })));
    }

    Ok(Json(serde_json::json!({
        "success": false, "message": "用户名或密码错误"
    })))
}

pub async fn register(
    State(base): State<PathBuf>,
    Json(body): Json<RegisterReq>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.username.unwrap_or_default();
    let password = body.password.unwrap_or_default();
    let email = body.email.unwrap_or_default();

    if username.is_empty() || password.is_empty() || email.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名、密码和邮箱不能为空"
        })));
    }

    if !username.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '|' || c == '~' || c == ':') {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名只允许字母、数字和下划线，特殊符号仅限 | ~ :"
        })));
    }

    let mut users = read_users(&base);
    if users.iter().any(|u| u.username == username) {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名已存在"
        })));
    }

    let same_email_count = users.iter().filter(|u| u.email == email).count();
    if same_email_count >= 2 {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "每个邮箱最多只能注册两个账号"
        })));
    }

    let max_id = users.iter().map(|u| u.id).max().unwrap_or(0);
    let network_location = body.network_location.unwrap_or_else(|| "public".to_string());
    let new_user = User {
        id: max_id + 1,
        username: username.clone(),
        password: password.clone(),
        email: email.clone(),
        name: username.clone(),
        role: "teacher".to_string(),
        avatar: String::new(),
        signature: String::new(),
        birthday: String::new(),
        gender: "none".to_string(),
        starred_users: vec![],
        friends: vec![],
        title: String::new(),
        subject: String::new(),
        managed_nodes: vec![],
        network_location,
    };

    users.push(new_user);
    write_users(&base, &users);

    storage::ensure_dir(base.join("user_files"));
    let uf_path = base.join("user_files").join(format!("{}.json", &username));
    storage::write_json_array(&uf_path, &Vec::<crate::models::FileMeta>::new());

    storage::ensure_dir(base.join("uploaded_files").join(&username));

    tracing::info!("User {} registered", username);

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "注册成功",
        "user": {
            "id": max_id + 1,
            "username": username,
            "email": email,
            "role": "teacher",
            "avatar": ""
        }
    })))
}

pub async fn logout_verify(
    State(base): State<PathBuf>,
    Json(body): Json<LoginReq>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.username.unwrap_or_default();
    let password = body.password.unwrap_or_default();

    if username.is_empty() || password.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名或密码不能为空"
        })));
    }

    let users = read_users(&base);
    if let Some(user) = users.iter().find(|u| u.username == username && u.password == password) {
        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "验证成功",
            "user": {
                "id": user.id,
                "username": user.username,
                "role": user.role
            }
        })));
    }

    Ok(Json(serde_json::json!({
        "success": false, "message": "密码错误"
    })))
}

pub async fn user_logout(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        })));
    }
    state.online_users.remove(username);
    tracing::info!("User {} logged out", username);
    Ok(Json(serde_json::json!({
        "success": true, "message": "注销成功"
    })))
}

pub async fn user_online(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");

    if username.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        })));
    }

    match action {
        "login" => {
            state.online_users.insert(username.to_string());
        }
        "logout" => {
            state.online_users.remove(username);
        }
        _ => {}
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "online": state.online_users.contains(username)
    })))
}

pub async fn check_online(
    State(state): State<Arc<AppState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        })));
    }
    Ok(Json(serde_json::json!({
        "success": true,
        "online": state.online_users.contains(username)
    })))
}
