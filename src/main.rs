use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{FromRef, State},
    middleware as axum_mw,
    routing::{delete, get, post, put},
    Json, Router,
};
use dashmap::DashMap;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

mod handlers;
mod middleware;
mod models;
mod storage;
mod sync_client;
mod ws;

#[derive(Clone)]
struct AppContext {
    pub state: Arc<models::AppState>,
    pub base: PathBuf,
}

impl FromRef<AppContext> for Arc<models::AppState> {
    fn from_ref(ctx: &AppContext) -> Self {
        ctx.state.clone()
    }
}

impl FromRef<AppContext> for PathBuf {
    fn from_ref(ctx: &AppContext) -> Self {
        ctx.base.clone()
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("electronic_server=info,tower_http=info")
        .init();

    let base = std::env::current_dir().expect("Failed to get current dir");
    tracing::info!("Data directory: {:?}", base);

    // Initialize data directories
    let dirs = [
        "chat_messages",
        "lan_chat_messages",
        "lan_groups",
        "lan_group_messages",
        "user_files",
        "uploaded_files",
        "pending_files",
        "avatars",
        "friend_requests",
        "temp_chunks",
    ];
    for d in &dirs {
        storage::ensure_dir(base.join(d));
    }

    // Initialize storage files if they don't exist
    let init_files: Vec<(&str, &str)> = vec![
        ("users.json", "[]"),
        ("broadcasts.json", "[]"),
        ("pending_files_meta.json", "[]"),
        ("credentials.json", "[]"),
        ("roles_config.json", r#"{"roles":[{"id":"admin","name":"管理员","permissions":["manage_users","broadcast_all"]},{"id":"teacher","name":"教师","permissions":["broadcast_class"]},{"id":"student","name":"学生","permissions":[]}]}"#),
        ("org_tree.json", r#"{"id":"root","name":"默认学校","type":"school","children":[]}"#),
    ];
    for (fname, default) in &init_files {
        let path = base.join(fname);
        if !path.exists() {
            std::fs::write(&path, *default).ok();
        }
    }
    let groups_file = base.join("lan_groups").join("groups.json");
    if !groups_file.exists() {
        std::fs::write(&groups_file, "[]").ok();
    }

    let app_state = Arc::new(models::AppState {
        online_users: dashmap::DashSet::new(),
        lan_online_users: dashmap::DashSet::new(),
        ws_clients: DashMap::new(),
        request_stats: Mutex::new(models::RequestStats {
            today_date: chrono::Utc::now().date_naive().to_string(),
            total_requests: 0,
            error_requests: 0,
            peak_concurrency: 0,
            current_concurrency: 0,
            data_transfer_bytes: 0,
        }),
        cached_cpu_usage: Mutex::new(0.0),
        prev_cpu: Mutex::new(None),
    });

    let ctx = AppContext {
        state: app_state.clone(),
        base: base.clone(),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Auth
        .route("/login", post(handlers::auth::login))
        .route("/register", post(handlers::auth::register))
        .route("/logout", post(handlers::auth::logout_verify))
        .route("/user/logout", post(handlers::auth::user_logout))
        .route("/user/online", post(handlers::auth::user_online))
        .route("/user/online", get(handlers::auth::check_online))
        .route("/credential/verify", post(credential_verify))
        // Users
        .route("/users", get(handlers::users::get_users))
        .route("/users/search", get(handlers::users::search_users))
        .route("/users/online", get(users_online))
        .route("/users/friends/add", post(handlers::users::add_friend))
        .route("/users/friends/remove", post(handlers::users::remove_friend))
        .route("/users/friends", get(handlers::users::get_friends))
        .route("/users/star", post(handlers::users::star_user))
        .route("/users/starred", get(handlers::users::get_starred))
        // Friend Requests
        .route("/friends/requests/send", post(handlers::users::send_friend_request))
        .route("/friends/requests", get(handlers::users::get_friend_requests))
        .route("/friends/requests/handle", post(handlers::users::handle_friend_request))
        // Profile & Avatar
        .route("/user/profile", post(handlers::users::update_profile))
        .route("/user/avatar/upload", post(handlers::users::upload_avatar))
        .route("/user/avatar/{username}", get(handlers::users::get_avatar))
        .route("/user/avatar/{username}", delete(handlers::users::delete_avatar))
        // Chat
        .route("/chat/send", post(handlers::chat::send_message))
        .route("/chat/messages", get(handlers::chat::get_messages))
        .route("/chat/unread-counts", get(handlers::chat::unread_counts_get))
        .route("/chat/unread-counts", post(handlers::chat::unread_counts_post))
        .route("/chat/mark-read", post(handlers::chat::mark_read))
        .route("/chat/mark-read-group", post(handlers::chat::mark_read_group))
        // LAN
        .route("/api/login", post(handlers::chat::lan_login))
        .route("/api/logout", post(handlers::chat::lan_logout))
        .route("/api/friends", get(handlers::chat::lan_friends))
        .route("/api/messages", get(handlers::chat::lan_get_messages))
        .route("/api/messages", post(handlers::chat::lan_send_message))
        .route("/api/online", get(handlers::chat::lan_online))
        .route("/health", get(handlers::monitor::health_check))
        // Groups
        .route("/api/groups", post(handlers::groups::create_group))
        .route("/api/groups", get(handlers::groups::get_user_groups))
        .route("/api/group-messages", get(handlers::groups::get_group_messages))
        .route("/api/group-messages", post(handlers::groups::send_group_message))
        .route("/api/groups/join", post(handlers::groups::join_group))
        .route("/api/groups/leave", post(handlers::groups::leave_group))
        .route("/api/groups/{group_id}", delete(handlers::groups::delete_group))
        // Files
        .route("/user/files", get(handlers::files::get_user_files))
        .route("/user/files", post(handlers::files::update_user_files))
        .route("/user/upload", post(handlers::files::upload_file))
        .route("/user/download/{username}/{filename}", get(handlers::files::download_file))
        .route("/user/file/{username}/{filename}", delete(handlers::files::delete_file))
        .route("/files/all", get(handlers::files::get_all_files))
        .route("/files/send", post(handlers::files::send_file))
        // Pending Files
        .route("/file/store", post(handlers::pending::store_pending_file))
        .route("/file/pending", get(handlers::pending::get_pending_files))
        .route("/file/download/{file_id}", get(handlers::pending::download_pending_file))
        .route("/file/{file_id}", delete(handlers::pending::delete_pending_file))
        .route("/file/status", get(handlers::pending::file_status))
        // Chunked Upload
        .route("/api/file/chunks", get(handlers::chunked::query_chunks))
        .route("/api/file/chunk", post(handlers::chunked::upload_chunk))
        .route("/api/file/merge", post(handlers::chunked::merge_chunks))
        // Broadcast
        .route("/api/broadcast/send", post(handlers::broadcast::send_broadcast))
        .route("/api/broadcast/list", get(handlers::broadcast::list_broadcasts))
        .route("/api/broadcast/receipts/{id}", get(handlers::broadcast::get_broadcast_receipts))
        .route("/api/broadcast/read/{id}", post(handlers::broadcast::mark_broadcast_read))
        // Organization
        .route("/api/org/tree", get(handlers::org::get_tree))
        .route("/api/org/node", post(handlers::org::add_node))
        .route("/api/org/node/{id}", delete(handlers::org::delete_node))
        .route("/api/org/node/{id}/members", put(handlers::org::update_members))
        .route("/api/org/node/{id}/members", get(handlers::org::get_node_members))
        .route("/api/org/nodes/flat", get(handlers::org::get_flat_nodes))
        // Admin
        .route("/api/accounts/login", post(handlers::admin::admin_login))
        .route("/api/accounts", get(handlers::admin::get_accounts))
        .route("/api/accounts", post(handlers::admin::add_account))
        .route("/api/accounts/{username}", delete(handlers::admin::delete_account))
        .route("/api/accounts/{username}", put(handlers::admin::update_account))
        .route("/api/accounts/roles", get(handlers::admin::get_roles))
        .route("/api/accounts/stats", get(handlers::admin::account_stats))
        // Monitor
        .route("/api/status", get(handlers::monitor::server_status))
        .route("/api/server-info", get(handlers::monitor::server_info))
        .route("/api/statistics", get(handlers::monitor::statistics))
        .route("/api/events", get(handlers::monitor::events))
        // Sync (master -> slave)
        .route("/api/sync/users", get(handlers::sync::sync_users))
        // WebSocket
        .route("/ws", get(ws::ws_handler))
        // Home
        .route("/", get(home))
        .layer(cors)
        .layer(axum_mw::from_fn_with_state(
            ctx.clone(),
            middleware::stats_middleware,
        ))
        .with_state(ctx);

    let server_mode = std::env::var("SERVER_MODE").unwrap_or_else(|_| "master".to_string());
    let is_master = server_mode == "master";

    tracing::info!("========================================");
    tracing::info!("      电子聊天系统服务端已启动 (Rust)");
    tracing::info!("========================================");
    tracing::info!("Server mode: {}", server_mode);
    if !is_master {
        let master_url = std::env::var("MASTER_URL").expect("SLAVE mode requires MASTER_URL env");
        let sync_secret = std::env::var("SYNC_SECRET").expect("SLAVE mode requires SYNC_SECRET env");
        tracing::info!("Master URL: {}", master_url);
        // Start background sync loop
        let sync_base = base.clone();
        tokio::spawn(async move {
            sync_client::start_sync_loop(sync_base, master_url, sync_secret).await;
        });
    }

    let port = std::env::var("PORT").unwrap_or_else(|_| "3001".to_string());
    let addr = format!("0.0.0.0:{}", port);

    tracing::info!("HTTP port: {}", port);
    tracing::info!("Listening on: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn home() -> axum::response::Html<&'static str> {
    axum::response::Html(
        r#"<h1>Electronic 服务器 (Rust)</h1>
<p>服务器正在运行中...</p>
<p>API 端点:</p>
<ul>
<li>POST /login - 用户登录</li>
<li>POST /register - 用户注册</li>
<li>GET /users - 获取所有用户</li>
<li>GET /user/files - 获取用户文件列表</li>
<li>POST /user/files - 更新用户文件列表</li>
<li>POST /user/upload - 上传文件</li>
<li>GET /user/download/{username}/{filename} - 下载文件</li>
<li>DELETE /user/file/{username}/{filename} - 删除文件</li>
</ul>"#,
    )
}

async fn users_online(
    State(state): State<Arc<models::AppState>>,
) -> Json<serde_json::Value> {
    let online: Vec<String> = state.online_users.iter().map(|s| s.key().clone()).collect();
    Json(serde_json::json!({
        "success": true,
        "onlineUsers": online
    }))
}

async fn credential_verify(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let token = body.get("token").and_then(|v| v.as_str()).unwrap_or("");

    if username.is_empty() || token.is_empty() {
        return Ok(Json(serde_json::json!({
            "success": false, "message": "用户名和凭证不能为空"
        })));
    }

    let credentials: Vec<models::Credential> = storage::JsonStore::new(base.join("credentials.json")).read();
    let cred = credentials.iter().find(|c| c.username == username && c.token == token);

    match cred {
        Some(c) => {
            let thirty_days: u64 = 30 * 24 * 60 * 60 * 1000;
            let now = chrono::Utc::now().timestamp_millis() as u64;
            if now - c.created_at > thirty_days {
                let mut all = credentials;
                all.retain(|x| x.token != token);
                storage::JsonStore::new(base.join("credentials.json")).write(&all);
                Ok(Json(serde_json::json!({
                    "success": false, "message": "凭证已过期"
                })))
            } else {
                Ok(Json(serde_json::json!({
                    "success": true, "message": "凭证有效"
                })))
            }
        }
        None => Ok(Json(serde_json::json!({
            "success": false, "message": "凭证无效"
        }))),
    }
}
