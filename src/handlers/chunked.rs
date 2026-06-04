use std::collections::HashMap;
use std::path::PathBuf;

use axum::{body::Bytes, extract::State, http::StatusCode, Json};
use axum_extra::extract::Query;

use crate::models::FileMeta;
use crate::storage;

pub async fn query_chunks(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let md5 = params.get("md5").map(|s| s.as_str()).unwrap_or("");
    if md5.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "md5参数不能为空"
        }).into());
    }

    let chunk_dir = base.join("temp_chunks").join(md5);
    if !chunk_dir.exists() {
        return Ok(serde_json::json!({
            "success": true,
            "chunks": []
        }).into());
    }

    let mut indices: Vec<u64> = vec![];
    if let Ok(entries) = std::fs::read_dir(&chunk_dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".chunk") {
                if let Ok(idx) = fname.trim_end_matches(".chunk").parse::<u64>() {
                    indices.push(idx);
                }
            }
        }
    }
    indices.sort();

    Ok(serde_json::json!({
        "success": true,
        "chunks": indices
    }).into())
}

pub async fn upload_chunk(
    State(base): State<PathBuf>,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let md5 = params.get("md5").map(|s| s.as_str()).unwrap_or("");
    let index = params.get("index").and_then(|s| s.parse::<u64>().ok());
    let _total_chunks = params.get("totalChunks").and_then(|s| s.parse::<u64>().ok());
    let file_name = params.get("fileName").map(|s| s.as_str()).unwrap_or("");

    if md5.is_empty() || index.is_none() || file_name.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "参数无效"
        }).into());
    }

    let chunk_dir = base.join("temp_chunks").join(md5);
    storage::ensure_dir(&chunk_dir);

    let chunk_path = chunk_dir.join(format!("{}.chunk", index.unwrap()));
    let _ = std::fs::write(&chunk_path, &body);

    Ok(serde_json::json!({
        "success": true,
        "index": index.unwrap(),
        "message": "分片上传成功"
    }).into())
}

pub async fn merge_chunks(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let md5 = body.get("md5").and_then(|v| v.as_str()).unwrap_or("");
    let file_name = body.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
    let total_chunks = body.get("totalChunks").and_then(|v| v.as_u64());
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");

    if md5.is_empty() || file_name.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "参数无效"
        }).into());
    }

    let chunk_dir = base.join("temp_chunks").join(md5);
    if !chunk_dir.exists() {
        return Ok(serde_json::json!({
            "success": false, "message": "未找到分片数据"
        }).into());
    }

    // Check all chunks present
    if let Some(total) = total_chunks {
        let count = std::fs::read_dir(&chunk_dir)
            .map(|e| e.flatten().filter(|f| f.file_name().to_string_lossy().ends_with(".chunk")).count())
            .unwrap_or(0);
        if (count as u64) < total {
            return Ok(serde_json::json!({
                "success": false,
                "message": format!("分片不完整，期望{}个，实际{}个", total, count)
            }).into());
        }
    }

    // Collect and sort chunks
    let mut chunks: Vec<(u64, PathBuf)> = vec![];
    if let Ok(entries) = std::fs::read_dir(&chunk_dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".chunk") {
                if let Ok(idx) = fname.trim_end_matches(".chunk").parse::<u64>() {
                    chunks.push((idx, entry.path()));
                }
            }
        }
    }
    chunks.sort_by_key(|k| k.0);

    // Merge
    let target_dir = if username.is_empty() {
        base.join("uploaded_files").join("shared")
    } else {
        base.join("uploaded_files").join(username)
    };
    storage::ensure_dir(&target_dir);
    let final_path = target_dir.join(&file_name);

    let mut combined = Vec::new();
    for (_, path) in &chunks {
        if let Ok(data) = std::fs::read(path) {
            combined.extend_from_slice(&data);
        }
    }

    // Verify md5
    use md5::{Md5, Digest};
    let actual_md5 = format!("{:x}", Md5::digest(&combined));
    if actual_md5 != md5 {
        return Ok(serde_json::json!({
            "success": false, "message": "文件校验失败，MD5不匹配"
        }).into());
    }

    let _ = std::fs::write(&final_path, &combined);

    // Cleanup temp chunks
    let _ = std::fs::remove_dir_all(&chunk_dir);

    // Update metadata
    if !username.is_empty() {
        let mut user_files: Vec<FileMeta> = storage::read_json_array(&base.join("user_files").join(format!("{}.json", username)));
        let existing_idx = user_files.iter().position(|f| f.name == file_name);
        let meta = FileMeta {
            name: file_name.to_string(),
            size: combined.len() as u64,
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
        storage::write_json_array(&base.join("user_files").join(format!("{}.json", username)), &user_files);
    }

    Ok(serde_json::json!({
        "success": true,
        "md5": actual_md5,
        "path": final_path.to_string_lossy(),
        "message": "文件合并完成"
    }).into())
}
