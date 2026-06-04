use std::collections::HashMap;
use std::path::PathBuf;

use axum::{body::Bytes, extract::State, http::StatusCode, Json};
use axum_extra::extract::Query;

use crate::models::FileMeta;
use crate::storage;

fn read_user_files(base: &PathBuf, username: &str) -> Vec<FileMeta> {
    let path = base.join("user_files").join(format!("{}.json", username));
    storage::read_json_array(&path)
}

fn write_user_files(base: &PathBuf, username: &str, files: &[FileMeta]) {
    let path = base.join("user_files").join(format!("{}.json", username));
    storage::write_json_array(&path, files);
}

fn get_user_upload_dir(base: &PathBuf, username: &str) -> PathBuf {
    let dir = base.join("uploaded_files").join(username);
    storage::ensure_dir(&dir);
    dir
}

pub async fn get_user_files(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
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
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let files = body.get("files").and_then(|v| v.as_array());

    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
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
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    let filename = params.get("filename").map(|s| s.as_str()).unwrap_or("");

    if username.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名不能为空"
        }).into());
    }
    if filename.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "文件名不能为空"
        }).into());
    }

    let user_dir = get_user_upload_dir(&base, username);
    let file_path = user_dir.join(filename);
    let _ = std::fs::write(&file_path, &body);

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
    axum::extract::Path((username, filename)): axum::extract::Path<(String, String)>,
    State(base): State<PathBuf>,
) -> Result<(StatusCode, [(String, String); 2], Vec<u8>), StatusCode> {
    let file_path = get_user_upload_dir(&base, &username).join(&filename);
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
            format!("%{:02X}", c as u8)
        }
    }).collect()
}

pub async fn delete_file(
    axum::extract::Path((username, filename)): axum::extract::Path<(String, String)>,
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if username.is_empty() || filename.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "用户名和文件名不能为空"
        }).into());
    }

    let user_dir = get_user_upload_dir(&base, &username);
    let file_path = user_dir.join(&filename);
    if file_path.exists() {
        let _ = std::fs::remove_file(&file_path);
    }

    // Also clean downloaded copy
    let download_path = base.join("downloaded_files").join(&username).join(&filename);
    if download_path.exists() {
        let _ = std::fs::remove_file(&download_path);
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
        let user_dir = get_user_upload_dir(&base, &user.username);
        let metadata = read_user_files(&base, &user.username);
        let meta_map: HashMap<String, &FileMeta> = metadata.iter().map(|f| (f.name.clone(), f)).collect();

        if user_dir.exists() {
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
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sender = body.get("sender").and_then(|v| v.as_str()).unwrap_or("");
    let receiver = body.get("receiver").and_then(|v| v.as_str()).unwrap_or("");
    let filename = body.get("filename").and_then(|v| v.as_str()).unwrap_or("");

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
