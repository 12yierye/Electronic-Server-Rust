use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde_json::Value;

use crate::auth_store;
use crate::models::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    State(base): State<PathBuf>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, base))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>, base: PathBuf) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let mut auth_user: Option<String> = None;
    let mut conn_id: Option<u64> = None;

    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            if let Ok(val) = serde_json::from_str::<Value>(&text) {
                match val.get("type").and_then(|v| v.as_str()) {
                    Some("auth") => {
                        let username = val.get("username").and_then(|v| v.as_str()).unwrap_or("");
                        let token = val.get("token").and_then(|v| v.as_str()).unwrap_or("");
                        let valid = !username.is_empty()
                            && auth_store::verify_token(&base, token).as_deref() == Some(username);

                        if !valid {
                            tracing::warn!("[WS] Rejected unauthenticated client: {}", username);
                            break;
                        }

                        let id = state.ws_seq.fetch_add(1, Ordering::SeqCst);
                        state
                            .ws_clients
                            .entry(username.to_string())
                            .or_default()
                            .push((id, tx.clone()));

                        conn_id = Some(id);
                        auth_user = Some(username.to_string());
                        tracing::info!("[WS] User online: {}", username);
                    }
                    Some("ping") => {
                        let _ = tx.send(serde_json::json!({ "type": "pong" }).to_string());
                    }
                    _ => {}
                }
            }
        }
    }

    if let (Some(username), Some(id)) = (auth_user, conn_id) {
        let empty = match state.ws_clients.get_mut(&username) {
            Some(mut list) => {
                list.retain(|(cid, _)| *cid != id);
                list.is_empty()
            }
            None => false,
        };
        if empty {
            state.ws_clients.remove(&username);
        }
        tracing::info!("[WS] User offline: {}", username);
    }

    send_task.abort();
}

/// 向指定用户当前所有在线连接推送消息
pub fn notify_user(state: &Arc<AppState>, username: &str, payload: &str) {
    if let Some(list) = state.ws_clients.get(username) {
        for (_, tx) in list.iter() {
            let _ = tx.send(payload.to_string());
        }
    }
}
