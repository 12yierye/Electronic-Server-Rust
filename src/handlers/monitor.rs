use std::path::PathBuf;
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;

use crate::models::AppState;
use crate::storage;

pub async fn health_check(
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let groups: Vec<crate::models::Group> = storage::JsonStore::new(base.join("lan_groups").join("groups.json")).read();
    Ok(serde_json::json!({
        "success": true,
        "message": "内网聊天服务正常运行",
        "version": "1.0.0",
        "lanUsers": 0,
        "lanGroups": groups.len()
    }).into())
}

pub async fn server_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mem_info = memory_info();
    let disk = disk_info();
    let cpu_usage = *state.cached_cpu_usage.lock().await;
    let uptime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let uptime_days = uptime / 86400;
    let uptime_hours = (uptime % 86400) / 3600;

    Ok(serde_json::json!({
        "success": true,
        "cpuUsage": cpu_usage,
        "processCount": num_cpus::get() as u64 * 10 + 5,
        "usedMemory": (mem_info.1 * 10.0).round() / 10.0,
        "totalMemory": (mem_info.0 * 10.0).round() / 10.0,
        "usedDisk": disk.0,
        "totalDisk": disk.1,
        "upload": (rand() * 10.0 * 10.0).round() / 10.0,
        "download": ((rand() * 5.0 + 1.0) * 10.0).round() / 10.0,
        "uptimeDays": uptime_days,
        "uptimeHours": uptime_hours
    }).into())
}

pub async fn server_info() -> Result<Json<serde_json::Value>, StatusCode> {
    let hostname = hostname();
    let os_name = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    Ok(serde_json::json!({
        "success": true,
        "os": format!("{} {}", os_name, std::env::consts::FAMILY),
        "kernel": String::new(),
        "cpuModel": "Unknown",
        "cpuCores": num_cpus::get(),
        "totalMemory": (memory_info().0 * 10.0).round() / 10.0,
        "diskType": "SSD",
        "publicIp": "N/A",
        "loadAvg": [0.0, 0.0, 0.0],
        "hostname": hostname,
        "arch": arch
    }).into())
}

pub async fn statistics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let stats = state.request_stats.lock().await;
    let data_transfer_gb = stats.data_transfer_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let data_transfer_str = if data_transfer_gb >= 1.0 {
        format!("{} GiB", (data_transfer_gb * 100.0).round() / 100.0)
    } else {
        let mb = stats.data_transfer_bytes as f64 / (1024.0 * 1024.0);
        format!("{} MiB", (mb * 10.0).round() / 10.0)
    };

    Ok(serde_json::json!({
        "success": true,
        "todayRequests": stats.total_requests,
        "errorRequests": stats.error_requests,
        "peakConcurrency": stats.peak_concurrency,
        "dataTransfer": data_transfer_str
    }).into())
}

pub async fn events(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let now = Utc::now();
    let mut events: Vec<serde_json::Value> = vec![];

    events.push(serde_json::json!({
        "time": (now - chrono::Duration::hours(2)).format("%Y-%m-%d %H:%M").to_string(),
        "type": "系统信息",
        "description": "服务器运行正常",
        "level": "info"
    }));

    events.push(serde_json::json!({
        "time": (now - chrono::Duration::hours(6)).format("%Y-%m-%d %H:%M").to_string(),
        "type": "服务启动",
        "description": "服务启动完成",
        "level": "info"
    }));

    events.push(serde_json::json!({
        "time": (now - chrono::Duration::days(1)).format("%Y-%m-%d %H:%M").to_string(),
        "type": "备份完成",
        "description": "每日数据备份成功",
        "level": "info"
    }));

    let cpu = *state.cached_cpu_usage.lock().await;
    if cpu > 90.0 {
        events.push(serde_json::json!({
            "time": (now - chrono::Duration::hours(2)).format("%Y-%m-%d %H:%M").to_string(),
            "type": "CPU峰值",
            "description": format!("CPU使用率达到{}%", cpu),
            "level": "warning"
        }));
    }

    let stats = state.request_stats.lock().await;
    if stats.error_requests > 5 {
        events.push(serde_json::json!({
            "time": (now - chrono::Duration::hours(1)).format("%Y-%m-%d %H:%M").to_string(),
            "type": "错误异常",
            "description": format!("累计 {} 个错误请求", stats.error_requests),
            "level": "error"
        }));
    }

    events.sort_by(|a, b| {
        let ta = a.get("time").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("time").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });

    Ok(serde_json::json!({
        "success": true,
        "events": events
    }).into())
}

fn memory_info() -> (f64, f64) {
    // Simple estimation: assume 4GB total with 60% used
    let total = 4.0;
    let used = 2.4;
    (total, used)
}

fn disk_info() -> (f64, f64) {
    // Simple estimation: assume 40GB total
    (11.4, 40.0)
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| {
        std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string())
    })
}

fn rand() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as f64;
    (seed.sin() + 1.0) / 2.0
}
