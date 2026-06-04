use std::collections::HashMap;
use std::path::PathBuf;

use axum::{body::Bytes, extract::State, http::StatusCode, Json};
use axum_extra::extract::Query;
use chrono::Utc;

use crate::models::PendingFileMeta;
use crate::storage;

pub async fn store_pending_file(
    State(base): State<PathBuf>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Parse sender/receiver/filename/expireAt from query params or body
    // For simplicity, we use a JSON-based approach where metadata is embedded
    // In the original code, this was done via express.raw with multipart parsing
    // We'll extract from query params

    let body_str = String::from_utf8_lossy(&body);
    let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap_or(serde_json::Value::Null);

    let sender = parsed.get("sender").and_then(|v| v.as_str()).unwrap_or("");
    let receiver = parsed.get("receiver").and_then(|v| v.as_str()).unwrap_or("");
    let filename = parsed.get("filename").and_then(|v| v.as_str()).unwrap_or("");

    if sender.is_empty() || receiver.is_empty() || filename.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "发送者、接收者和文件名不能为空"
        }).into());
    }

    let file_id = format!("file_{}_{}", Utc::now().timestamp_millis(), &random_str(9));

    storage::ensure_dir(base.join("pending_files"));

    // Save file content
    let file_path = base.join("pending_files").join(&file_id);
    let _ = std::fs::write(&file_path, &body);

    // Read metadata
    let meta_path = base.join("pending_files_meta.json");
    let mut pending: Vec<PendingFileMeta> = if meta_path.exists() {
        storage::read_json_array(&meta_path)
    } else {
        vec![]
    };

    let now = Utc::now();
    let expire = now + chrono::Duration::days(7);

    let file_id_clone = file_id.clone();
    pending.push(PendingFileMeta {
        id: file_id,
        sender: sender.to_string(),
        receiver: receiver.to_string(),
        filename: filename.to_string(),
        size: body.len() as u64,
        expire_at: expire.to_rfc3339(),
        created_at: now.to_rfc3339(),
    });
    storage::write_json_array(&meta_path, &pending);

    Ok(serde_json::json!({
        "success": true,
        "message": "文件已暂存",
        "fileId": file_id_clone,
        "receiverOnline": false,
        "pending": true
    }).into())
}

pub async fn get_pending_files(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let receiver = params.get("receiver").map(|s| s.as_str()).unwrap_or("");
    if receiver.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "接收者用户名不能为空"
        }).into());
    }

    let meta_path = base.join("pending_files_meta.json");
    let pending: Vec<PendingFileMeta> = if meta_path.exists() {
        storage::read_json_array(&meta_path)
    } else {
        vec![]
    };

    let files: Vec<serde_json::Value> = pending
        .iter()
        .filter(|f| f.receiver == receiver)
        .map(|f| serde_json::json!({
            "id": f.id,
            "sender": f.sender,
            "filename": f.filename,
            "size": f.size,
            "createdAt": f.created_at,
            "expireAt": f.expire_at
        }))
        .collect();

    Ok(serde_json::json!({
        "success": true,
        "files": files
    }).into())
}

pub async fn download_pending_file(
    axum::extract::Path(file_id): axum::extract::Path<String>,
    State(base): State<PathBuf>,
) -> Result<(StatusCode, [(String, String); 2], Vec<u8>), StatusCode> {
    let meta_path = base.join("pending_files_meta.json");
    let pending: Vec<PendingFileMeta> = if meta_path.exists() {
        storage::read_json_array(&meta_path)
    } else {
        vec![]
    };

    let meta = pending.iter().find(|f| f.id == file_id).ok_or(StatusCode::NOT_FOUND)?;
    let file_path = base.join("pending_files").join(&file_id);

    let data = std::fs::read(&file_path).map_err(|_| StatusCode::NOT_FOUND)?;

    Ok((
        StatusCode::OK,
        [
            ("Content-Disposition".to_string(), format!("attachment; filename=\"{}\"", urlencode(&meta.filename))),
            ("Content-Type".to_string(), "application/octet-stream".to_string()),
        ],
        data,
    ))
}

pub async fn delete_pending_file(
    axum::extract::Path(file_id): axum::extract::Path<String>,
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let meta_path = base.join("pending_files_meta.json");
    let mut pending: Vec<PendingFileMeta> = if meta_path.exists() {
        storage::read_json_array(&meta_path)
    } else {
        vec![]
    };

    let fi = pending.iter().position(|f| f.id == file_id).ok_or(StatusCode::NOT_FOUND)?;
    pending.remove(fi);
    storage::write_json_array(&meta_path, &pending);

    let file_path = base.join("pending_files").join(&file_id);
    if file_path.exists() {
        let _ = std::fs::remove_file(&file_path);
    }

    Ok(serde_json::json!({
        "success": true, "message": "文件已送达，服务器文件已删除"
    }).into())
}

pub async fn file_status(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let filename = params.get("filename").map(|s| s.as_str()).unwrap_or("");
    let receiver = params.get("receiver").map(|s| s.as_str()).unwrap_or("");

    let meta_path = base.join("pending_files_meta.json");
    let pending: Vec<PendingFileMeta> = if meta_path.exists() {
        storage::read_json_array(&meta_path)
    } else {
        vec![]
    };

    if let Some(meta) = pending.iter().find(|f| f.filename == filename && f.receiver == receiver) {
        Ok(serde_json::json!({
            "success": true,
            "status": "pending",
            "expireAt": meta.expire_at,
            "sender": meta.sender
        }).into())
    } else {
        Ok(serde_json::json!({
            "success": true,
            "status": "none"
        }).into())
    }
}

fn urlencode(s: &str) -> String {
    s.chars().map(|c| {
        if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
            c.to_string()
        } else {
            format!("%{:02X}", c as u8)
        }
    }).collect()
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
