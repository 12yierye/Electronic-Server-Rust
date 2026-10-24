use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand_core::OsRng;

pub const TOKEN_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;

pub fn is_hashed(stored: &str) -> bool {
    stored.starts_with("$argon2")
}

fn hash_sync(plain: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .unwrap_or_default()
}

fn verify_sync(stored: &str, plain: &str) -> bool {
    if stored.is_empty() {
        return false;
    }
    if is_hashed(stored) {
        return PasswordHash::new(stored)
            .map(|parsed| Argon2::default().verify_password(plain.as_bytes(), &parsed).is_ok())
            .unwrap_or(false);
    }
    // 兼容历史明文密码，登录成功后由调用方升级为哈希
    stored == plain
}

pub async fn hash_password(plain: String) -> String {
    tokio::task::spawn_blocking(move || hash_sync(&plain))
        .await
        .unwrap_or_default()
}

pub async fn verify_password(stored: String, plain: String) -> bool {
    tokio::task::spawn_blocking(move || verify_sync(&stored, &plain))
        .await
        .unwrap_or(false)
}

pub fn issue_token(username: &str) -> String {
    format!(
        "{}:{}:{}",
        username,
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4().simple()
    )
}

pub fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

/// 启动时把历史明文密码统一升级为 Argon2 哈希，避免明文落盘
pub async fn migrate_plaintext_passwords(base: &std::path::Path) {
    use crate::models::User;
    use crate::storage::JsonStore;

    let path = base.join("users.json");
    let store = JsonStore::<Vec<User>>::new(&path);
    let mut users: Vec<User> = store.read();
    let mut changed = 0;

    for user in users.iter_mut() {
        if !user.password.is_empty() && !is_hashed(&user.password) {
            user.password = hash_password(std::mem::take(&mut user.password)).await;
            changed += 1;
        }
    }

    if changed > 0 {
        store.write(&users);
        // 同步覆盖历史文件，避免明文密码继续留在磁盘上
        if let Ok(json) = serde_json::to_string_pretty(&users) {
            let _ = std::fs::write(&path, json);
        }
        tracing::info!("Migrated {} plaintext passwords to Argon2", changed);
    }
}

