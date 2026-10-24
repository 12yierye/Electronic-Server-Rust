use std::collections::HashMap;
use std::path::PathBuf;

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::Json;
use axum_extra::extract::Query;

use crate::middleware::AuthUser;
use crate::models::FileMeta;
use crate::storage;

fn read_user_files(base: &PathBuf, username: &str) -> Vec<FileMeta> {
    match storage::safe_join(&base.join("user_files"), &[&format!("{}.json", username)]) {
        Some(path) => storage::read_json_array(&path),
        None => vec![],
    }
}

fn write_user_files(base: &PathBuf, username: &str, files: &[FileMeta]) {
    if !storage::is_safe_segment(username) {
        return;
    }
    let path = base.join("user_files").join(format!("{}.json", username));
    storage::write_json_array(&path, files);
}

/// 目录名必须是安全片段
fn user_upload_dir(base: &PathBuf, username: &str) -> Option<PathBuf> {
    let dir = storage::safe_join(&base.join("uploaded_files"), &[username])?;
    storage::ensure_dir(&dir);
    Some(dir)
}

fn is_self(auth: &Option<Extension<AuthUser>>, username: &str) -> bool {
    auth.as_ref().map(|Extension(u)| u.0 == username).unwrap_or(false)
}

pub async fn get_user_files(
    State(base): State<PathBuf>,
    auth: Option<Extension<AuthUser>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        }).into());
    }
    if !is_self(&auth, username) {
        return Ok(serde_json::json!({
            "success": false, "message": "无权访问其他用户文件"
        }).into());
    }
    let files = read_user_files(&base, username);
    Ok(serde_json::json!({
        "success": true,
        "files": files
    }).into())
}

pub async fn update_user_files(
    State(base): State<PathBuf>,
    auth: Option<Extension<AuthUser>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let files = body.get("files").and_then(|v| v.as_array());

    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        }).into());
    }
    if !is_self(&auth, username) {
        return Ok(serde_json::json!({
            "success": false, "message": "无权修改其他用户文件"
        }).into());
    }
    if files.is_none() {
        return Ok(serde_json::json!({
            "success": false, "message": "文件列表格式错误"
        }).into());
    }

    let files_list: Vec<FileMeta> = serde_json::from_value(serde_json::Value::Array(files.unwrap().clone()))
        .unwrap_or_default();
    write_user_files(&base, username, &files_list);

    Ok(serde_json::json!({
        "success": true, "message": "文件列表更新成功"
    }).into())
}

pub async fn upload_file(
    State(base): State<PathBuf>,
    auth: Option<Extension<AuthUser>>,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    let filename = params.get("filename").map(|s| s.as_str()).unwrap_or("");

    if username.is_empty() || filename.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名和文件名不能为空"
        }).into());
    }
    if !is_self(&auth, username) {
        return Ok(serde_json::json!({
            "success": false, "message": "无权为其他用户上传文件"
        }).into());
    }
    if !storage::is_safe_segment(filename) {
        return Ok(serde_json::json!({
            "success": false, "message": "文件名非法"
        }).into());
    }

    let user_dir = match user_upload_dir(&base, username) {
        Some(dir) => dir,
        None => {
            return Ok(serde_json::json!({
                "success": false, "message": "用户名非法"
            }).into())
        }
    };
    let file_path = match storage::safe_join(&user_dir, &[filename]) {
        Some(path) => path,
        None => {
            return Ok(serde_json::json!({
                "success": false, "message": "文件名非法"
            }).into())
        }
    };
    if std::fs::write(&file_path, &body).is_err() {
        return Ok(serde_json::json!({
            "success": false, "message": "文件写入失败"
        }).into());
    }

    let mut user_files = read_user_files(&base, username);
    let existing_idx = user_files.iter().position(|f| f.name == filename);
    let meta = FileMeta {
        name: filename.to_string(),
        size: body.len() as u64,
        last_modified: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        uploader: username.to_string(),
        upload_time: chrono::Utc::now().to_rfc3339(),
    };

    if let Some(idx) = existing_idx {
        user_files[idx] = meta;
    } else {
        user_files.push(meta);
    }
    write_user_files(&base, username, &user_files);

    Ok(serde_json::json!({
        "success": true, "message": "文件上传成功"
    }).into())
}

pub async fn download_file(
    Path((username, filename)): Path<(String, String)>,
    State(base): State<PathBuf>,
    auth: Option<Extension<AuthUser>>,
) -> Result<(StatusCode, [(String, String); 2], Vec<u8>), StatusCode> {
    if !is_self(&auth, &username) {
        return Err(StatusCode::FORBIDDEN);
    }
    let user_dir = user_upload_dir(&base, &username).ok_or(StatusCode::BAD_REQUEST)?;
    let file_path = storage::safe_join(&user_dir, &[&filename]).ok_or(StatusCode::BAD_REQUEST)?;
    if !file_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }
    let data = std::fs::read(&file_path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok((
        StatusCode::OK,
        [
            ("Content-Disposition".to_string(), format!("attachment; filename=\"{}\"", urlencoding(&filename))),
            ("Content-Type".to_string(), "application/octet-stream".to_string()),
        ],
        data,
    ))
}

fn urlencoding(s: &str) -> String {
    s.chars().map(|c| {
        if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
            c.to_string()
        } else {
            format!("%{:02X}", c as u32)
        }
    }).collect()
}

pub async fn delete_file(
    Path((username, filename)): Path<(String, String)>,
    State(base): State<PathBuf>,
    auth: Option<Extension<AuthUser>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if username.is_empty() || filename.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名和文件名不能为空"
        }).into());
    }
    if !is_self(&auth, &username) {
        return Ok(serde_json::json!({
            "success": false, "message": "无权删除其他用户文件"
        }).into());
    }

    let user_dir = match user_upload_dir(&base, &username) {
        Some(dir) => dir,
        None => {
            return Ok(serde_json::json!({
                "success": false, "message": "用户名非法"
            }).into())
        }
    };
    if let Some(file_path) = storage::safe_join(&user_dir, &[&filename]) {
        if file_path.exists() {
            let _ = std::fs::remove_file(&file_path);
        }
    }

    let mut user_files = read_user_files(&base, &username);
    user_files.retain(|f| f.name != filename);
    write_user_files(&base, &username, &user_files);

    Ok(serde_json::json!({
        "success": true, "message": "文件删除成功"
    }).into())
}

pub async fn get_all_files(
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let users: Vec<crate::models::User> = storage::JsonStore::new(base.join("users.json")).read();
    let mut all_files: Vec<serde_json::Value> = vec![];

    for user in &users {
        let user_dir = match user_upload_dir(&base, &user.username) {
            Some(dir) => dir,
            None => continue,
        };
        let metadata = read_user_files(&base, &user.username);
        let meta_map: HashMap<String, &FileMeta> = metadata.iter().map(|f| (f.name.clone(), f)).collect();

        if let Ok(entries) = std::fs::read_dir(&user_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if entry.file_type().map(|t| !t.is_file()).unwrap_or(true) { continue; }
                let meta = meta_map.get(&name);
                all_files.push(serde_json::json!({
                    "name": name,
                    "size": meta.map(|m| m.size).unwrap_or(0),
                    "lastModified": meta.map(|m| m.last_modified).unwrap_or(0),
                    "uploader": meta.map(|m| m.uploader.clone()).unwrap_or_else(|| user.username.clone()),
                    "uploadTime": meta.map(|m| m.upload_time.clone()).unwrap_or_default()
                }));
            }
        }
    }

    all_files.sort_by(|a, b| {
        let ta = a.get("uploadTime").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("uploadTime").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });

    Ok(serde_json::json!({
        "success": true,
        "files": all_files
    }).into())
}

pub async fn send_file(
    State(base): State<PathBuf>,
    auth: Option<Extension<AuthUser>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sender = body.get("sender").and_then(|v| v.as_str()).unwrap_or("");
    let receiver = body.get("receiver").and_then(|v| v.as_str()).unwrap_or("");
    let filename = body.get("filename").and_then(|v| v.as_str()).unwrap_or("");

    if !is_self(&auth, sender) {
        return Ok(serde_json::json!({"success": false, "message": "无权以他人身份发送文件"}).into());
    }

    let users: Vec<crate::models::User> = storage::JsonStore::new(base.join("users.json")).read();
    if !users.iter().any(|u| u.username == sender) {
        return Ok(serde_json::json!({"success": false, "message": "发送者不存在"}).into());
    }
    if !users.iter().any(|u| u.username == receiver) {
        return Ok(serde_json::json!({"success": false, "message": "接收者不存在"}).into());
    }

    let sender_files = read_user_files(&base, sender);
    if let Some(file) = sender_files.iter().find(|f| f.name == filename) {
        let mut receiver_files = read_user_files(&base, receiver);
        if !receiver_files.iter().any(|f| f.name == filename) {
            receiver_files.push(file.clone());
            write_user_files(&base, receiver, &receiver_files);
        }
        Ok(serde_json::json!({
            "success": true, "message": "文件发送成功"
        }).into())
    } else {
        Ok(serde_json::json!({
            "success": false, "message": "文件不存在"
        }).into())
    }
}
