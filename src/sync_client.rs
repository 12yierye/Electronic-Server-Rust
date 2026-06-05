use std::path::PathBuf;
use std::time::Duration;

use crate::models::User;
use crate::storage;

async fn sync_users_from_master(base: &PathBuf, master_url: &str, secret: &str) {
    let url = format!(
        "{}/api/sync/users?secret={}",
        master_url.trim_end_matches('/'),
        secret
    );

    tracing::info!("[Sync] Pulling users from master");

    match reqwest::get(&url).await {
        Ok(resp) => {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                if data.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                    if let Some(users) = data.get("users").and_then(|v| v.as_array()) {
                        let typed_users: Vec<User> =
                            serde_json::from_value(serde_json::json!(users)).unwrap_or_default();
                        let store = storage::JsonStore::<Vec<User>>::new(base.join("users.json"));
                        store.write(&typed_users);
                        tracing::info!("[Sync] Synced {} users from master", typed_users.len());
                    }
                } else {
                    tracing::warn!("[Sync] Master returned error: {:?}", data);
                }
            }
        }
        Err(e) => {
            tracing::warn!("[Sync] Failed to connect to master: {}", e);
        }
    }
}

/// Periodically sync users from master server (called only in SLAVE mode)
pub async fn start_sync_loop(base: PathBuf, master_url: String, secret: String) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    // Initial sync after 1 second
    tokio::time::sleep(Duration::from_secs(1)).await;
    sync_users_from_master(&base, &master_url, &secret).await;

    loop {
        interval.tick().await;
        sync_users_from_master(&base, &master_url, &secret).await;
    }
}
