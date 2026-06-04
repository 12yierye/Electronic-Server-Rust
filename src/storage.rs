use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

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
        if !self.path.exists() {
            return T::default();
        }
        match fs::read_to_string(&self.path) {
            Ok(data) => match serde_json::from_str(&data) {
                Ok(val) => val,
                Err(e) => {
                    tracing::error!("JSON parse error at {}: {}", self.path.display(), e);
                    T::default()
                }
            },
            Err(e) => {
                tracing::error!("Read error at {}: {}", self.path.display(), e);
                T::default()
            }
        }
    }

    pub fn write(&self, data: &T) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(data) {
            Ok(json) => {
                if let Err(e) = fs::write(&self.path, &json) {
                    tracing::error!("Write error at {}: {}", self.path.display(), e);
                }
            }
            Err(e) => tracing::error!("Serialize error: {}", e),
        }
    }
}

pub fn ensure_dir<P: AsRef<Path>>(dir: P) {
    let _ = fs::create_dir_all(dir.as_ref());
}

pub fn read_json_array<P: AsRef<Path>, T: DeserializeOwned>(path: P) -> Vec<T> {
    if !path.as_ref().exists() {
        return vec![];
    }
    match fs::read_to_string(path.as_ref()) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => vec![],
    }
}

pub fn write_json_array<P: AsRef<Path>, T: Serialize>(path: P, data: &[T]) {
    if let Some(parent) = path.as_ref().parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = fs::write(path.as_ref(), &json);
    }
}

pub fn make_conv_path(base: &Path, a: &str, b: &str) -> PathBuf {
    let mut participants = vec![a, b];
    participants.sort();
    base.join(format!("{}_{}.json", participants[0], participants[1]))
}
