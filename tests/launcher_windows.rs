#![cfg(windows)]
use std::{process::Stdio, time::Duration};

async fn launch(root: &std::path::Path, arguments: &[&str]) -> std::process::Output {
    let mut command = tokio::process::Command::new("pwsh");
    command
        .args(["-NoLogo", "-NoProfile", "-File"])
        .arg(root.join("run.ps1"))
        .args(arguments)
        .current_dir(root.join("elsewhere"))
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    command.creation_flags(0x08000000);
    tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .unwrap()
        .unwrap()
}
#[tokio::test]
async fn real_windows_launcher_handles_missing_binary_modes_root_and_plain_preview() {
    let temp = tempfile::Builder::new()
        .prefix("launcher space-")
        .tempdir()
        .unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("elsewhere")).unwrap();
    std::fs::copy("run.ps1", root.join("run.ps1")).unwrap();
    let missing = launch(root, &["-Preview"]).await;
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Native executable missing"));
    std::fs::create_dir(root.join("bin")).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_fiverr-agent-mcp"),
        root.join("bin/fiverr-agent-mcp.exe"),
    )
    .unwrap();
    let preview = launch(root, &["-Preview"]).await;
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let text = String::from_utf8(preview.stdout).unwrap();
    assert!(text.contains("Fiverr Main menu:"));
    assert!(text.contains("Sign in to Fiverr"));
    assert!(!text.contains('\u{1b}'));
    assert!(!text.ends_with("null\r\n"));
    let status = launch(root, &["-Status"]).await;
    assert!(status.status.success());
    let value: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(value["data"]["route"]["mode"], "blocked");
    let signin = launch(root, &["-SignIn"]).await;
    assert_eq!(signin.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&signin.stderr).contains("ROUTE_BLOCKED"));
    assert_eq!(
        launch(root, &["-Status", "-Preview"]).await.status.code(),
        Some(1)
    );
    let logs = std::fs::read_dir(root.join("logs")).unwrap().count();
    assert!(logs >= 6);
}
