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
    let entry = json!({"command":executable,"args":["bridge"],"env":{"FIVERR_AGENT_TEMP_MODE":"rooted","TEMP":root.path().join(".cache/tmp"),"TMP":root.path().join(".cache/tmp"),"TMPDIR":root.path().join(".cache/tmp"),"XDG_CACHE_HOME":root.path().join(".cache")}});
    if entry_only {
        json!({"Fiverr-Agent-mcp":entry})
    } else {
        json!({"mcpServers":{"Fiverr-Agent-mcp":entry}})
    }
}
fn validate_existing(existing: &Value, generated: &Value) -> Result<()> {
    let conflict = || {
        error(
            ErrorCode::ActionStateChanged,
            "Existing Fiverr-Agent-mcp arguments or temporary policy conflict; review the explicit client entry without replacing custom options.",
        )
    };
    if existing["args"] != generated["args"] {
        return Err(conflict());
    }
    let Some(env) = existing.get("env") else {
        return Ok(());
    };
    let env = env.as_object().ok_or_else(conflict)?;
    if env.values().any(|value| !value.is_string()) {
        return Err(conflict());
    }
    let generated_env = generated["env"].as_object().ok_or_else(storage_error)?;
    for key in env.keys() {
        if generated_env
            .keys()
            .any(|owned| key.eq_ignore_ascii_case(owned) && key != owned)
        {
            return Err(conflict());
        }
    }
    let mode = env
        .get("FIVERR_AGENT_TEMP_MODE")
        .and_then(Value::as_str)
        .unwrap_or("rooted");
    if !matches!(mode, "rooted" | "system") {
        return Err(conflict());
    }
    for key in ["TEMP", "TMP", "TMPDIR", "XDG_CACHE_HOME"] {
        if let Some(value) = env.get(key)
            && (mode == "system" || Some(value) != generated_env.get(key))
        {
            return Err(conflict());
        }
    }
    Ok(())
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
    let entry = desktop(root, true)["Fiverr-Agent-mcp"].clone();
    let object = config.as_object_mut().ok_or_else(storage_error)?;
    let servers = object
        .entry("mcpServers")
        .or_insert(json!({}))
        .as_object_mut()
        .ok_or_else(storage_error)?;
    if let Some(existing) = servers.get("Fiverr-Agent-mcp")
        && existing["command"] != entry["command"]
    {
        return Err(error(
            ErrorCode::ActionStateChanged,
            "Existing Fiverr-Agent-mcp belongs to another executable; explicit conflict resolution required.",
        ));
    }
    if !remove && let Some(existing) = servers.get("Fiverr-Agent-mcp") {
        validate_existing(existing, &entry)?;
        if std::fs::read(file).map_err(|_| storage_error())? != bytes {
            return Err(error(
                ErrorCode::ActionStateChanged,
                "Client configuration changed concurrently; review it again.",
            ));
        }
        return Ok(
            json!({"registered":true,"preserved_other_entries":true,"backup_created":false,"restart_client_required":false}),
        );
    }
    if remove {
        servers.remove("Fiverr-Agent-mcp");
    } else {
        servers.insert("Fiverr-Agent-mcp".into(), entry);
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
