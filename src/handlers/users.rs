use std::collections::HashMap;
use std::path::PathBuf;

use axum::{extract::State, http::StatusCode, Json};
use axum_extra::extract::Query;

use crate::models::{FriendRequest, User, UserPublic};
use crate::storage;

fn get_users_store(base: &PathBuf) -> storage::JsonStore<Vec<User>> {
    storage::JsonStore::new(base.join("users.json"))
}

fn read_users(base: &PathBuf) -> Vec<User> {
    get_users_store(base).read()
}

fn write_users(base: &PathBuf, users: &Vec<User>) {
    get_users_store(base).write(users);
}

fn to_public(user: &User) -> UserPublic {
    let mut p: UserPublic = user.clone().into();
    p.online = None;
    p
}

pub async fn get_users(
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let users = read_users(&base);
    let publics: Vec<UserPublic> = users.iter().map(|u| {
        let p = to_public(u);
        let _ = &p;
        to_public(u)
    }).collect();
    Ok(Json(serde_json::json!({
        "success": true,
        "users": publics
    })))
}

pub async fn search_users(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let query = params.get("query").map(|s| s.as_str()).unwrap_or("").to_lowercase();
    if query.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "查询参数不能为空"
        })));
    }

    let users = read_users(&base);
    let matched: Vec<UserPublic> = users
        .iter()
        .filter(|u| {
            u.username.to_lowercase().contains(&query)
                || u.email.to_lowercase().contains(&query)
        })
        .map(to_public)
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "users": matched
    })))
}

pub async fn add_friend(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let current = body.get("currentUser").and_then(|v| v.as_str()).unwrap_or("");
    let friend = body.get("friendUsername").and_then(|v| v.as_str()).unwrap_or("");

    let mut users = read_users(&base);
    let ui = users.iter().position(|u| u.username == current);
    let fi = users.iter().position(|u| u.username == friend);

    match (ui, fi) {
        (Some(ui), Some(_)) => {
            if users[ui].friends.contains(&friend.to_string()) {
                return Ok(Json(serde_json::json!({
                    "success": false, "message": "该用户已经是您的好友"
                })));
            }
            users[ui].friends.push(friend.to_string());
            write_users(&base, &users);
            Ok(Json(serde_json::json!({
                "success": true, "message": "添加好友成功"
            })))
        }
        (None, _) => Ok(Json(serde_json::json!({
            "success": false, "message": "当前用户不存在"
        }))),
        (_, None) => Ok(Json(serde_json::json!({
            "success": false, "message": "目标用户不存在"
        }))),
    }
}

pub async fn remove_friend(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let current = body.get("currentUser").and_then(|v| v.as_str()).unwrap_or("");
    let friend = body.get("friendUsername").and_then(|v| v.as_str()).unwrap_or("");

    let mut users = read_users(&base);
    let ui = users.iter().position(|u| u.username == current);

    if let Some(ui) = ui {
        if !users[ui].friends.contains(&friend.to_string()) {
            return Ok(Json(serde_json::json!({
                "success": false, "message": "该用户不是您的好友"
            })));
        }
        users[ui].friends.retain(|f| f != friend);
        write_users(&base, &users);
        Ok(Json(serde_json::json!({
            "success": true, "message": "移除好友成功"
        })))
    } else {
        Ok(Json(serde_json::json!({
            "success": false, "message": "当前用户不存在"
        })))
    }
}

pub async fn get_friends(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        })));
    }
    let users = read_users(&base);
    if let Some(user) = users.iter().find(|u| u.username == username) {
        let friends: Vec<UserPublic> = users
            .iter()
            .filter(|u| user.friends.contains(&u.username))
            .map(to_public)
            .collect();
        Ok(Json(serde_json::json!({
            "success": true,
            "friends": friends
        })))
    } else {
        Ok(Json(serde_json::json!({
            "success": false, "message": "用户不存在"
        })))
    }
}

pub async fn star_user(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let current = body.get("currentUser").and_then(|v| v.as_str()).unwrap_or("");
    let target = body.get("starredUsername").and_then(|v| v.as_str()).unwrap_or("");

    let mut users = read_users(&base);
    let ui = users.iter().position(|u| u.username == current);

    if let Some(ui) = ui {
        let is_starred = users[ui].starred_users.contains(&target.to_string());
        if is_starred {
            users[ui].starred_users.retain(|u| u != target);
        } else {
            users[ui].starred_users.push(target.to_string());
        }
        write_users(&base, &users);
        Ok(Json(serde_json::json!({
            "success": true,
            "message": if is_starred { "取消星标成功" } else { "星标成功" },
            "starred": !is_starred
        })))
    } else {
        Ok(Json(serde_json::json!({
            "success": false, "message": "当前用户不存在"
        })))
    }
}

pub async fn get_starred(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        })));
    }
    let users = read_users(&base);
    if let Some(user) = users.iter().find(|u| u.username == username) {
        let starred: Vec<UserPublic> = users
            .iter()
            .filter(|u| user.starred_users.contains(&u.username))
            .map(to_public)
            .collect();
        Ok(Json(serde_json::json!({
            "success": true,
            "starredUsers": starred
        })))
    } else {
        Ok(Json(serde_json::json!({
            "success": false, "message": "用户不存在"
        })))
    }
}

pub async fn update_profile(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        })));
    }

    let signature = body.get("signature").and_then(|v| v.as_str());
    let birthday = body.get("birthday").and_then(|v| v.as_str());
    let gender = body.get("gender").and_then(|v| v.as_str());

    if let Some(sig) = signature {
        if sig.len() > 100 {
            return Ok(Json(serde_json::json!({
                "success": false, "message": "个性签名不能超过100个字符"
            })));
        }
    }
    if let Some(b) = birthday {
        if !b.is_empty() && !regex_match(r"^\d{4}-\d{2}-\d{2}$", b) {
            return Ok(Json(serde_json::json!({
                "success": false, "message": "生日格式无效，应为 YYYY-MM-DD"
            })));
        }
    }
    let valid_genders = ["male", "female", "private", "none"];
    if let Some(g) = gender {
        if !valid_genders.contains(&g) {
            return Ok(Json(serde_json::json!({
                "success": false, "message": "性别只能为 male、female、private 或 none"
            })));
        }
    }

    let mut users = read_users(&base);
    let ui = users.iter().position(|u| u.username == username);
    if let Some(ui) = ui {
        if let Some(s) = signature { users[ui].signature = s.to_string(); }
        if let Some(b) = birthday { users[ui].birthday = b.to_string(); }
        if let Some(g) = gender { users[ui].gender = g.to_string(); }
        write_users(&base, &users);
        Ok(Json(serde_json::json!({
            "success": true, "message": "保存成功"
        })))
    } else {
        Ok(Json(serde_json::json!({
            "success": false, "message": "用户不存在"
        })))
    }
}

fn regex_match(pattern: &str, s: &str) -> bool {
    if let Ok(re) = regex::Regex::new(pattern) {
        re.is_match(s)
    } else {
        false
    }
}

// Avatar
pub async fn upload_avatar(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        })));
    }
    if body.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "头像数据不能为空"
        })));
    }

    let avatar_path = base.join("avatars").join(format!("{}.jpg", username));
    storage::ensure_dir(base.join("avatars"));
    let _ = std::fs::write(&avatar_path, &body);

    let mut users = read_users(&base);
    let ui = users.iter().position(|u| u.username == username);
    if let Some(ui) = ui {
        let avatar_url = format!("/user/avatar/{}", username);
        users[ui].avatar = avatar_url.clone();
        write_users(&base, &users);

        Ok(Json(serde_json::json!({
            "success": true,
            "message": "头像上传成功",
            "avatar": avatar_url
        })))
    } else {
        Ok(Json(serde_json::json!({
            "success": false, "message": "用户不存在"
        })))
    }
}

pub async fn get_avatar(
    axum::extract::Path(username): axum::extract::Path<String>,
    State(base): State<PathBuf>,
) -> Result<(axum::http::StatusCode, [(String, String); 2], Vec<u8>), StatusCode> {
    let avatar_path = base.join("avatars").join(format!("{}.jpg", username));
    if avatar_path.exists() {
        let data = std::fs::read(&avatar_path).map_err(|_| StatusCode::NOT_FOUND)?;
        Ok((
            StatusCode::OK,
            [
                ("Content-Type".to_string(), "image/jpeg".to_string()),
                ("Cache-Control".to_string(), "public, max-age=3600".to_string()),
            ],
            data,
        ))
    } else {
        Err(StatusCode::NO_CONTENT)
    }
}

pub async fn delete_avatar(
    axum::extract::Path(username): axum::extract::Path<String>,
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let avatar_path = base.join("avatars").join(format!("{}.jpg", username));
    if avatar_path.exists() {
        let _ = std::fs::remove_file(&avatar_path);
    }
    let mut users = read_users(&base);
    let ui = users.iter().position(|u| u.username == username);
    if let Some(ui) = ui {
        users[ui].avatar = String::new();
        write_users(&base, &users);
    }
    Ok(Json(serde_json::json!({
        "success": true, "message": "头像删除成功"
    })))
}

// Friend requests
fn get_fr_path(base: &PathBuf, username: &str) -> std::path::PathBuf {
    base.join("friend_requests").join(format!("{}.json", username))
}

pub async fn send_friend_request(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sender = body.get("sender").and_then(|v| v.as_str()).unwrap_or("");
    let receiver = body.get("receiver").and_then(|v| v.as_str()).unwrap_or("");

    let users = read_users(&base);
    let sender_user = users.iter().find(|u| u.username == sender);
    let receiver_user = users.iter().find(|u| u.username == receiver);

    if sender_user.is_none() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "发送者不存在"
        })));
    }
    if receiver_user.is_none() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "接收者不存在"
        })));
    }
    if sender_user.unwrap().friends.contains(&receiver.to_string()) {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "该用户已经是您的好友"
        })));
    }

    storage::ensure_dir(base.join("friend_requests"));
    let mut reqs: Vec<FriendRequest> = storage::read_json_array(get_fr_path(&base, receiver));
    if reqs.iter().any(|r| r.sender == sender && r.status == "pending") {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "您已经向该用户发送过好友申请"
        })));
    }

    let now = chrono::Utc::now();
    reqs.push(FriendRequest {
        id: now.timestamp_millis() as u64,
        sender: sender.to_string(),
        receiver: receiver.to_string(),
        status: "pending".to_string(),
        timestamp: now.to_rfc3339(),
    });
    storage::write_json_array(&get_fr_path(&base, receiver), &reqs);

    Ok(Json(serde_json::json!({
        "success": true, "message": "好友申请发送成功"
    })))
}

pub async fn get_friend_requests(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::value::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    let req_type = params.get("type").map(|s| s.as_str()).unwrap_or("");

    let users = read_users(&base);
    let mut requests: Vec<FriendRequest> = vec![];

    match req_type {
        "sent" => {
            for user in &users {
                let reqs: Vec<FriendRequest> = storage::read_json_array(get_fr_path(&base, &user.username));
                for r in reqs {
                    if r.sender == username && r.status == "pending" {
                        requests.push(r);
                    }
                }
            }
        }
        "received" => {
            requests = storage::read_json_array(get_fr_path(&base, username));
        }
        _ => {
            for user in &users {
                let reqs: Vec<FriendRequest> = storage::read_json_array(get_fr_path(&base, &user.username));
                for r in reqs {
                    if r.sender == username || r.receiver == username {
                        requests.push(r);
                    }
                }
            }
            let received: Vec<FriendRequest> = storage::read_json_array(get_fr_path(&base, username));
            for r in received {
                if !requests.iter().any(|x| x.id == r.id) {
                    requests.push(r);
                }
            }
        }
    }

    Ok(serde_json::json!({
        "success": true,
        "requests": requests
    }).into())
}

pub async fn handle_friend_request(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let request_id = body.get("requestId").and_then(|v| v.as_u64()).unwrap_or(0);
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");

    if request_id == 0 || action.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "申请ID和操作不能为空"
        })));
    }
    if action != "accept" && action != "reject" {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "操作必须是 accept 或 reject"
        })));
    }

    let users = read_users(&base);
    let mut found_req: Option<FriendRequest> = None;
    let _found_owner: Option<String> = None;

    for user in &users {
        let mut reqs: Vec<FriendRequest> = storage::read_json_array(get_fr_path(&base, &user.username));
        if let Some(pos) = reqs.iter().position(|r| r.id == request_id) {
            found_req = Some(reqs.remove(pos));
            let _ = user.username.clone();
            storage::write_json_array(&get_fr_path(&base, &user.username), &reqs);
            break;
        }
    }

    if found_req.is_none() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "好友申请不存在"
        })));
    }

    let req = found_req.unwrap();
    if action == "accept" {
        let mut users = read_users(&base);
        let si = users.iter().position(|u| u.username == req.sender);
        let ri = users.iter().position(|u| u.username == req.receiver);
        if let (Some(si), Some(ri)) = (si, ri) {
            if !users[si].friends.contains(&req.receiver) {
                users[si].friends.push(req.receiver.clone());
            }
            if !users[ri].friends.contains(&req.sender) {
                users[ri].friends.push(req.sender.clone());
            }
            write_users(&base, &users);
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": if action == "accept" { "好友申请已接受" } else { "好友申请已拒绝" }
    })))
}
