use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;

use crate::auth_store;
use crate::models::{AppContext, AppState};

/// 无需登录即可访问的接口
const PUBLIC_EXACT: &[&str] = &[
    "/",
    "/health",
    "/login",
    "/register",
    "/credential/verify",
    "/api/login",
    "/api/logout",
    "/api/sync/users",
    "/api/status",
    "/api/server-info",
    "/api/statistics",
    "/api/events",
    "/api/accounts/login",
    // 局域网模式接口（对端自建服务，暂保持免鉴权）
    "/api/friends",
    "/api/messages",
    "/api/online",
    "/api/groups",
    "/api/groups/join",
    "/api/groups/leave",
    "/api/group-messages",
];

/// 只对 GET 放行（图片链接无法携带请求头）
const PUBLIC_GET_PREFIX: &[&str] = &["/user/avatar/", "/ws"];

#[derive(Clone, Debug)]
pub struct AuthUser(pub String);

fn is_public(method: &Method, path: &str) -> bool {
    if PUBLIC_EXACT.contains(&path) {
        return true;
    }
    if method == Method::GET && PUBLIC_GET_PREFIX.iter().any(|p| path.starts_with(p)) {
        return true;
    }
    if method == Method::DELETE && path.starts_with("/api/groups/") {
        return true;
    }
    false
}

pub async fn require_auth(
    State(ctx): State<AppContext>,
    mut req: Request,
    next: Next,
) -> Response {
    if is_public(req.method(), req.uri().path()) {
        return next.run(req).await;
    }

    let token = auth_header(req.headers()).unwrap_or_default();
    match auth_store::verify_token(&ctx.base, &token) {
        Some(username) => {
            req.extensions_mut().insert(AuthUser(username));
            next.run(req).await
        }
        None => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "success": false, "message": "未提供凭证或凭证无效" })),
        )
            .into_response(),
    }
}

pub async fn stats_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let today = Utc::now().date_naive().to_string();
    {
        let mut stats = state.request_stats.lock().await;
        if stats.today_date != today {
            stats.today_date = today.clone();
            stats.total_requests = 0;
            stats.error_requests = 0;
            stats.peak_concurrency = 0;
            stats.current_concurrency = 0;
            stats.data_transfer_bytes = 0;
        }
        stats.total_requests += 1;
        stats.current_concurrency += 1;
        if stats.current_concurrency > stats.peak_concurrency {
            stats.peak_concurrency = stats.current_concurrency;
        }
    }

    let res = next.run(req).await;

    {
        let mut stats = state.request_stats.lock().await;
        stats.current_concurrency = stats.current_concurrency.saturating_sub(1);
        if res.status().as_u16() >= 400 {
            stats.error_requests += 1;
        }
    }

    res
}

pub fn auth_header(req: &axum::http::HeaderMap) -> Option<String> {
    req.get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

pub fn sync_secret(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-sync-secret")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}
