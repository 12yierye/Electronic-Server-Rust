use std::sync::Arc;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::models::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut auth_user: Option<String> = None;

    let (tx, _rx) = broadcast::channel::<String>(256);
    let mut rx = tx.subscribe();

    let _send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(msg) => {
                            if sender.send(Message::Text(msg.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                else => break,
            }
        }
    });

    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            if let Ok(val) = serde_json::from_str::<Value>(&text) {
                if val.get("type").and_then(|v| v.as_str()) == Some("auth") {
                    if let Some(username) = val.get("username").and_then(|v| v.as_str()) {
                        auth_user = Some(username.to_string());
                        state.ws_clients.entry(username.to_string()).or_default();
                        tracing::info!("[WS] User online: {}", username);
                        broadcast_online_status(&state, username, true);
                    }
                }
            }
        }
    }

    if let Some(username) = auth_user {
        tracing::info!("[WS] User offline: {}", username);
        broadcast_online_status(&state, &username, false);
    }
}

fn broadcast_online_status(_state: &Arc<AppState>, username: &str, online: bool) {
    let _data = serde_json::json!({
        "type": "online_status",
        "username": username,
        "online": online,
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    tracing::debug!("[WS] Online status: {} online={}", username, online);
}
