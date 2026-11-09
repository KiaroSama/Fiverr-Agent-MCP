use super::Root;
use crate::core::{Result, storage_error};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone)]
pub struct Logger {
    file: Arc<Mutex<File>>,
    degraded: Arc<AtomicBool>,
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
            .read(true)
            .write(true)
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
            if entry.path() != path
                && crate::storage::reject_links(&entry.path()).is_ok()
                && let Ok(candidate) = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(entry.path())
                && fs2::FileExt::try_lock_exclusive(&candidate).is_ok()
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            degraded: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn event(&self, level: &str, category: &str, event: &str, fields: Value) {
        let level = match level {
            "INFO" | "WARNING" | "ERROR" | "DEBUG" => level,
            _ => "ERROR",
        };
        let record = json!({"timestamp":crate::core::now(),"level":level,"category":category,"message":event,"context":redact(fields)});
        let written = self.file.lock().ok().is_some_and(|mut file| {
            file.metadata().is_ok_and(|m| m.len() < 8 * 1024 * 1024)
                && writeln!(file, "{record}").is_ok()
                && file.flush().is_ok()
        });
        if !written && !self.degraded.swap(true, Ordering::Relaxed) {
            // Fixed text only: never echo the rejected event or recurse into this logger.
            let _ = writeln!(
                std::io::stderr(),
                "File logging degraded; some events may not be recorded."
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_open_write_failure_and_poison_share_one_degraded_state() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("read-only-sink");
        fs::write(&path, b"unchanged").unwrap();
        let logger = Logger {
            file: Arc::new(Mutex::new(File::open(&path).unwrap())),
            degraded: Arc::new(AtomicBool::new(false)),
        };
        logger.event(
            "INFO",
            "fixture",
            "Write failure",
            json!({"token":"dummy-secret"}),
        );
        assert!(logger.degraded.load(Ordering::Relaxed));
        assert!(Arc::ptr_eq(&logger.degraded, &logger.clone().degraded));
        assert_eq!(fs::read(&path).unwrap(), b"unchanged");
        let file = logger.file.clone();
        assert!(
            std::panic::catch_unwind(move || {
                let _guard = file.lock().unwrap();
                panic!("controlled logger poison");
            })
            .is_err()
        );
        logger.event("INFO", "fixture", "Poison", json!({}));
        assert!(logger.degraded.load(Ordering::Relaxed));
    }
}
