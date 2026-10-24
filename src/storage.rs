use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use rusqlite::{params, Connection};

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

fn open_db(base: &Path) -> Mutex<Connection> {
    let db_path = base.join("data.sqlite3");
    let conn = Connection::open(&db_path).expect("failed to open sqlite database");
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=5000;
         CREATE TABLE IF NOT EXISTS documents (
             path TEXT PRIMARY KEY,
             json TEXT NOT NULL,
             updated_at INTEGER NOT NULL
         );",
    )
    .expect("failed to init sqlite schema");
    Mutex::new(conn)
}

/// 服务启动时指定数据根目录
pub fn init_db(base: &Path) {
    DB.get_or_init(|| open_db(base));
}

fn conn() -> &'static Mutex<Connection> {
    DB.get_or_init(|| {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        open_db(&cwd)
    })
}

fn db_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn lock() -> MutexGuard<'static, Connection> {
    conn().lock().unwrap_or_else(|e| e.into_inner())
}

/// 读取文档；首次访问时自动从历史 JSON 文件导入
fn read_doc(path: &Path) -> Option<String> {
    let key = db_key(path);
    let existing = {
        let conn = lock();
        conn.query_row(
            "SELECT json FROM documents WHERE path = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .ok()
    };
    if existing.is_some() {
        return existing;
    }

    if !path.exists() {
        return None;
    }
    let raw = fs::read_to_string(path).ok()?;
    write_doc(path, &raw);
    Some(raw)
}

fn write_doc(path: &Path, json: &str) {
    let key = db_key(path);
    let now = chrono::Utc::now().timestamp_millis();
    let result = lock().execute(
        "INSERT INTO documents (path, json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(path) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
        params![key, json, now],
    );
    if let Err(e) = result {
        tracing::error!("SQLite write error at {}: {}", path.display(), e);
    }
}

pub struct JsonStore<T> {
    path: PathBuf,
    _marker: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned + Default> JsonStore<T> {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        JsonStore {
            path: path.as_ref().to_path_buf(),
            _marker: PhantomData,
        }
    }

    pub fn read(&self) -> T {
        match read_doc(&self.path) {
            Some(json) => match serde_json::from_str(&json) {
                Ok(val) => val,
                Err(e) => {
                    tracing::error!("JSON parse error at {}: {}", self.path.display(), e);
                    T::default()
                }
            },
            None => T::default(),
        }
    }

    pub fn write(&self, data: &T) {
        match serde_json::to_string_pretty(data) {
            Ok(json) => write_doc(&self.path, &json),
            Err(e) => tracing::error!("Serialize error: {}", e),
        }
    }
}

pub fn ensure_dir<P: AsRef<Path>>(dir: P) {
    let _ = fs::create_dir_all(dir.as_ref());
}

pub fn read_json_array<P: AsRef<Path>, T: DeserializeOwned>(path: P) -> Vec<T> {
    match read_doc(path.as_ref()) {
        Some(json) => serde_json::from_str(&json).unwrap_or_default(),
        None => vec![],
    }
}

pub fn write_json_array<P: AsRef<Path>, T: Serialize>(path: P, data: &[T]) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        write_doc(path.as_ref(), &json);
    }
}

pub fn make_conv_path(base: &Path, a: &str, b: &str) -> PathBuf {
    let mut participants = vec![a, b];
    participants.sort();
    base.join(format!("{}_{}.json", participants[0], participants[1]))
}

/// 只接受单一文件名/用户名片段，拒绝 .. / 分隔符 / 盘符等路径注入
pub fn is_safe_segment(s: &str) -> bool {
    if s.is_empty() || s.len() > 200 || s.contains('\0') {
        return false;
    }
    let mut parts = Path::new(s).components();
    matches!(parts.next(), Some(std::path::Component::Normal(_))) && parts.next().is_none()
}

/// 安全拼接，不合法时返回 None
pub fn safe_join(base: &Path, segments: &[&str]) -> Option<PathBuf> {
    let mut path = base.to_path_buf();
    for seg in segments {
        if !is_safe_segment(seg) {
            return None;
        }
        path = path.join(seg);
    }
    Some(path)
}
