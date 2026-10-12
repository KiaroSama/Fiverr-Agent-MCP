use super::Root;
use crate::core::{Result, storage_error};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::Write,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Logger {
    file: Arc<Mutex<File>>,
}
impl Logger {
    pub fn open(root: &Root) -> Result<Self> {
        let name = format!(
            "fiverr-agent-mcp_{}_UTC_{}.log",
            chrono::Utc::now().format("%Y-%m-%d_%H-%M-%S"),
            uuid::Uuid::new_v4().simple()
        );
        let path = root.safe_path("logs", &name)?;
        root.write_new(&path, b"")?;
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|_| storage_error())?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|_| storage_error())?;
        // Retention never deletes a log still held by another execution.
        let mut logs: Vec<_> = fs::read_dir(root.path().join("logs"))
            .map_err(|_| storage_error())?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("fiverr-agent-mcp_")
                    && e.path().extension().is_some_and(|x| x == "log")
            })
            .collect();
        logs.sort_by_key(|e| e.file_name());
        let excess = logs.len().saturating_sub(30);
        for entry in logs.into_iter().take(excess) {
            if entry.path() != path && crate::storage::reject_links(&entry.path()).is_ok() {
                if let Ok(candidate) = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(entry.path())
                {
                    if fs2::FileExt::try_lock_exclusive(&candidate).is_ok() {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }
    pub fn event(&self, level: &str, category: &str, event: &str, fields: Value) {
        let level = match level {
            "INFO" | "WARNING" | "ERROR" | "DEBUG" => level,
            _ => "ERROR",
        };
        let record = json!({"timestamp":crate::core::now(),"level":level,"category":category,"message":event,"context":redact(fields)});
        if let Ok(mut file) = self.file.lock() {
            if file.metadata().is_ok_and(|m| m.len() < 8 * 1024 * 1024) {
                let _ = writeln!(file, "{record}");
                let _ = file.flush();
            }
        }
    }
}
pub fn redact(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    let sensitive = [
                        "token",
                        "password",
                        "cookie",
                        "authorization",
                        "secret",
                        "csrf",
                        "session",
                        "message_body",
                        "payload",
                        "content",
                        "otp",
                        "credential",
                        "username",
                    ]
                    .iter()
                    .any(|s| lower.contains(s));
                    (
                        key,
                        if sensitive {
                            json!("[REDACTED]")
                        } else {
                            redact(value)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().take(32).map(redact).collect()),
        Value::String(text) => {
            if text.contains("://")
                || text.to_ascii_lowercase().contains("bearer ")
                || text.contains('\n')
                || text.contains('\r')
                || text.len() > 256
            {
                json!("[REDACTED]")
            } else {
                json!(text.chars().filter(|c| !c.is_control()).collect::<String>())
            }
        }
        other => other,
    }
}
