#![cfg(windows)]
use std::{process::Stdio, time::Duration};

async fn launch(root: &std::path::Path, arguments: &[&str]) -> std::process::Output {
    run(root, arguments, false).await
}

async fn run(root: &std::path::Path, arguments: &[&str], wrapper: bool) -> std::process::Output {
    let mut command = if wrapper {
        let mut command = tokio::process::Command::new("pwsh");
        command
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
            .arg(root.join("run.ps1"));
        command
    } else {
        let mut command = tokio::process::Command::new(root.join("bin/fiverr-agent-mcp.exe"));
        command.arg("--root").arg(root);
        command
    };
    command
        .args(arguments)
        .current_dir(root.join("elsewhere"))
        .env("NO_COLOR", "1")
        .envs(fiverr_agent_mcp::bootstrap::environment(
            &fiverr_agent_mcp::storage::Root::explicit(root).unwrap(),
        ))
        .stdin(Stdio::null())
        .kill_on_drop(true);
    command.creation_flags(0x08000000);
    tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .unwrap_or_else(|_| panic!("Launcher deadline exceeded for {arguments:?}"))
        .unwrap()
}
#[tokio::test]
async fn native_windows_entrypoint_handles_modes_root_and_plain_preview() {
    let temp = tempfile::Builder::new()
        .prefix("launcher space-")
        .tempdir()
        .unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("elsewhere")).unwrap();
    std::fs::create_dir(root.join("bin")).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_fiverr-agent-mcp"),
        root.join("bin/fiverr-agent-mcp.exe"),
    )
    .unwrap();
    let preview = launch(root, &["preview"]).await;
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let text = String::from_utf8(preview.stdout).unwrap();
    assert!(text.contains("Fiverr Main menu:"));
    assert!(text.contains("Account status (no login required)"));
    assert!(!text.contains("Sign in to Fiverr"));
    assert!(!text.contains('\u{1b}'));
    assert!(!text.ends_with("null\r\n"));
    let status = launch(root, &["status"]).await;
    assert!(status.status.success());
    let value: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(value["data"]["route"]["mode"], "direct");
    let signin = launch(root, &["-SignIn"]).await;
    assert_eq!(signin.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&signin.stdout).contains("Fiverr session"));
    assert_eq!(
        launch(root, &["status", "preview"]).await.status.code(),
        Some(2)
    );
    let logs = std::fs::read_dir(root.join("logs")).unwrap().count();
    assert!(logs >= 2);
}

#[tokio::test]
async fn powershell_launcher_restores_modes_errors_and_rooted_utf8_logs() {
    let temp = tempfile::Builder::new()
        .prefix("PowerShell launcher space-")
        .tempdir()
        .unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("elsewhere")).unwrap();
    std::fs::copy("run.ps1", root.join("run.ps1")).unwrap();
    let missing = run(root, &["-Preview"], true).await;
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Native executable missing"));
    std::fs::create_dir(root.join("bin")).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_fiverr-agent-mcp"),
        root.join("bin/fiverr-agent-mcp.exe"),
    )
    .unwrap();
    let preview = run(root, &["-Preview"], true).await;
    assert!(preview.status.success(), "{:?}", preview);
    let text = String::from_utf8(preview.stdout).unwrap();
    assert!(text.contains("Fiverr Main menu:"));
    assert!(text.contains("Account status (no login required)"));
    assert!(!text.contains("Sign in to Fiverr"));
    assert!(!text.contains('\u{1b}'));
    let status = run(root, &["-Status"], true).await;
    assert!(status.status.success(), "{:?}", status);
    let value: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(value["data"]["route"]["mode"], "direct");
    for args in [vec!["-Status", "-Preview"], vec!["-Preview", "status"]] {
        let refused = run(root, &args, true).await;
        assert_eq!(refused.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("Launcher mode")
                || String::from_utf8_lossy(&refused.stderr).contains("Choose only one")
        );
    }
    assert_eq!(run(root, &["-SignIn"], true).await.status.code(), Some(2));
    let menu = run(root, &[], true).await;
    assert_eq!(menu.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&menu.stderr).contains("interactive owner terminal"));
    let mut launcher_logs = 0;
    for entry in std::fs::read_dir(root.join("logs")).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("run_")
        {
            continue;
        }
        launcher_logs += 1;
        let text = std::fs::read_to_string(path).unwrap();
        for line in text.lines() {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(event["category"], "LAUNCHER");
            assert!(event["timestamp"].as_str().unwrap().ends_with('Z'));
        }
    }
    assert_eq!(launcher_logs, 7);
    assert!(!root.join("elsewhere/logs").exists());
}
