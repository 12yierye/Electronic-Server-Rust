use std::path::{Path, PathBuf};

use crate::models::Credential;
use crate::security::{issue_token, now_ms, TOKEN_TTL_MS};
use crate::storage::JsonStore;

fn cred_path(base: &Path) -> PathBuf {
    base.join("credentials.json")
}

pub fn read_credentials(base: &Path) -> Vec<Credential> {
    JsonStore::new(cred_path(base)).read()
}

pub fn write_credentials(base: &Path, creds: &[Credential]) {
    JsonStore::new(cred_path(base)).write(&creds.to_vec());
}

/// 同一用户只保留最新一个 token
pub fn issue_and_store(base: &Path, username: &str) -> String {
    let token = issue_token(username);
    let mut creds = read_credentials(base);
    creds.retain(|c| c.username != username);
    creds.push(Credential {
        username: username.to_string(),
        token: token.clone(),
        created_at: now_ms(),
    });
    write_credentials(base, &creds);
    token
}

pub fn verify_token(base: &Path, token: &str) -> Option<String> {
    if token.is_empty() {
        return None;
    }
    let creds = read_credentials(base);
    let cred = creds.iter().find(|c| c.token == token)?;
    if now_ms().saturating_sub(cred.created_at) > TOKEN_TTL_MS {
        let mut remaining = creds;
        remaining.retain(|c| c.token != token);
        write_credentials(base, &remaining);
        return None;
    }
    Some(cred.username.clone())
}

pub fn revoke_token(base: &Path, token: &str) {
    let mut creds = read_credentials(base);
    creds.retain(|c| c.token != token);
    write_credentials(base, &creds);
}

pub fn revoke_user(base: &Path, username: &str) {
    let mut creds = read_credentials(base);
    creds.retain(|c| c.username != username);
    write_credentials(base, &creds);
}
