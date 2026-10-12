use crate::{
    core::{ErrorCode, Result, error, storage_error},
    storage::{Root, reject_links},
};
use serde_json::{Value, json};
use std::path::Path;
pub fn desktop(root: &Root, entry_only: bool) -> Value {
    let executable = root.path().join("bin").join(if cfg!(windows) {
        "fiverr-agent-mcp.exe"
    } else {
        "fiverr-agent-mcp"
    });
    let entry = json!({"command":executable,"args":["bridge"],"env":{"TEMP":root.path().join(".cache/tmp"),"TMP":root.path().join(".cache/tmp"),"TMPDIR":root.path().join(".cache/tmp"),"XDG_CACHE_HOME":root.path().join(".cache")}});
    if entry_only {
        json!({"fiverr-local":entry})
    } else {
        json!({"mcpServers":{"fiverr-local":entry}})
    }
}
pub fn register(root: &Root, file: &Path, remove: bool) -> Result<Value> {
    reject_links(file)?;
    if !file.is_absolute() {
        return Err(error(
            ErrorCode::FilePolicyViolation,
            "Registration needs an explicit absolute client configuration file.",
        ));
    }
    if !std::fs::metadata(file)
        .map_err(|_| storage_error())?
        .is_file()
        || std::fs::metadata(file).map_err(|_| storage_error())?.len() > 2 * 1024 * 1024
    {
        return Err(error(
            ErrorCode::FilePolicyViolation,
            "Client file must be a bounded regular JSON file.",
        ));
    }
    let bytes = std::fs::read(file).map_err(|_| storage_error())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(error(
            ErrorCode::FilePolicyViolation,
            "Client configuration exceeds 2 MiB.",
        ));
    }
    let mut config: Value = serde_json::from_slice(&bytes).map_err(|_| {
        error(
            ErrorCode::InvalidInput,
            "Existing client configuration is not valid JSON; nothing overwritten.",
        )
    })?;
    let entry = desktop(root, true)["fiverr-local"].clone();
    let object = config.as_object_mut().ok_or_else(storage_error)?;
    let servers = object
        .entry("mcpServers")
        .or_insert(json!({}))
        .as_object_mut()
        .ok_or_else(storage_error)?;
    if let Some(existing) = servers.get("fiverr-local") {
        if existing["command"] != entry["command"] {
            return Err(error(
                ErrorCode::ActionStateChanged,
                "Existing fiverr-local belongs to another executable; explicit conflict resolution required.",
            ));
        }
    }
    if remove {
        servers.remove("fiverr-local");
    } else {
        servers.insert("fiverr-local".into(), entry);
    }
    let backup = file.with_extension(format!("json.backup-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&backup, &bytes).map_err(|_| storage_error())?;
    crate::storage::private_permissions(&backup)?;
    let temp = file.with_extension(format!("json.pending-{}", uuid::Uuid::new_v4().simple()));
    let updated = serde_json::to_vec_pretty(&config).map_err(|_| storage_error())?;
    std::fs::write(&temp, updated).map_err(|_| storage_error())?;
    crate::storage::private_permissions(&temp)?;
    if std::fs::read(file).map_err(|_| storage_error())? != bytes {
        let _ = std::fs::remove_file(&temp);
        return Err(error(
            ErrorCode::ActionStateChanged,
            "Client configuration changed concurrently; backup retained, no overwrite.",
        ));
    }
    std::fs::rename(&temp, file).map_err(|_| storage_error())?;
    Ok(
        json!({"registered":!remove,"preserved_other_entries":true,"backup_created":true,"restart_client_required":true}),
    )
}
