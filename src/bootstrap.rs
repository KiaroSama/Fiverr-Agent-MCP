use crate::{
    core::{ErrorCode, Result, error},
    storage::Root,
};
use std::{ffi::OsString, process::Command};

pub fn environment(root: &Root) -> [(&'static str, OsString); 4] {
    let temporary = root.path().join(".cache/tmp").into_os_string();
    [
        ("TEMP", temporary.clone()),
        ("TMP", temporary.clone()),
        ("TMPDIR", temporary),
        (
            "XDG_CACHE_HOME",
            root.path().join(".cache").into_os_string(),
        ),
    ]
}

// Restart before SQLite, TLS, logging or Tokio can create threads/read OS temp paths.
// Equality checks are authoritative; a forged marker cannot bypass containment.
pub fn rooted_process(root: &Root) -> Result<Option<i32>> {
    let variables = environment(root);
    if variables
        .iter()
        .all(|(key, value)| std::env::var_os(key).as_ref() == Some(value))
    {
        return Ok(None);
    }
    let executable = std::env::current_exe().map_err(|_| failure())?;
    let mut command = Command::new(executable);
    command.args(std::env::args_os().skip(1)).envs(variables);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _ = command.exec();
        Err(failure())
    }
    #[cfg(not(unix))]
    {
        // Inherit the real streams: bridge stays a native stream, never an HTTP wrapper.
        let status = command.status().map_err(|_| failure())?;
        Ok(Some(status.code().unwrap_or(1)))
    }
}
fn failure() -> crate::core::AppError {
    error(
        ErrorCode::StorageUnavailable,
        "Cannot start the same native executable with installation-owned temporary paths; no fallback to OS temporary storage occurred.",
    )
}
