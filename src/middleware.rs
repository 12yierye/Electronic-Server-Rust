use std::sync::Arc;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use chrono::Utc;

use crate::models::AppState;

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
