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
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::io::AsyncReadExt;
    let phase = if !wrapper {
        "native"
    } else if root.join("bin/fiverr-agent-mcp.exe").is_file() {
        "installed_binary"
    } else {
        "missing_binary"
    };
    let started = std::time::Instant::now();
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    eprintln!("launcher phase={phase} event=spawned arguments={arguments:?}");
    let out_count = Arc::new(AtomicUsize::new(0));
    let err_count = Arc::new(AtomicUsize::new(0));
    let preview_seen = Arc::new(AtomicBool::new(false));
    async fn drain(
        mut stream: impl tokio::io::AsyncRead + Unpin,
        count: Arc<AtomicUsize>,
        preview: Arc<AtomicBool>,
    ) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let size = stream.read(&mut buffer).await?;
            if size == 0 {
                return Ok(bytes);
            }
            count.fetch_add(size, Ordering::Relaxed);
            if bytes.len() + size > 65536 {
                return Err(std::io::Error::other("Launcher stream exceeded 64 KiB"));
            }
            bytes.extend_from_slice(&buffer[..size]);
            if bytes
                .windows(b"Fiverr Main menu:".len())
                .any(|part| part == b"Fiverr Main menu:")
            {
                preview.store(true, Ordering::Relaxed);
            }
        }
    }
    let mut out = tokio::spawn(drain(
        child.stdout.take().unwrap(),
        out_count.clone(),
        preview_seen.clone(),
    ));
    let mut err = tokio::spawn(drain(
        child.stderr.take().unwrap(),
        err_count.clone(),
        Arc::new(AtomicBool::new(false)),
    ));
    let mut exit_code = None;
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let status = child.wait().await.unwrap();
        exit_code = Some(status.code());
        let (stdout, stderr) = tokio::join!(&mut out, &mut err);
        std::process::Output {
            status,
            stdout: stdout.unwrap().unwrap(),
            stderr: stderr.unwrap().unwrap(),
        }
    })
    .await;
    eprintln!(
        "launcher phase={phase} elapsed_ms={} exit={exit_code:?} stdout_bytes={} stderr_bytes={} native_preview_reached={} deadline={}",
        started.elapsed().as_millis(),
        out_count.load(Ordering::Relaxed),
        err_count.load(Ordering::Relaxed),
        preview_seen.load(Ordering::Relaxed),
        result.is_err()
    );
    match result {
        Ok(output) => output,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            out.abort();
            err.abort();
            let _ = out.await;
            let _ = err.await;
            panic!("Launcher deadline exceeded phase={phase} for {arguments:?}");
        }
    }
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
    assert!(
        !root.join("logs").exists(),
        "missing binary must not create unvalidated launcher logs"
    );
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
    let mut native_logs = 0;
    for entry in std::fs::read_dir(root.join("logs")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(
            !name.starts_with("run_"),
            "wrapper must delegate private first writes to native Root"
        );
        if !name.starts_with("fiverr-agent-mcp_") {
            continue;
        }
        native_logs += 1;
        let text = std::fs::read_to_string(path).unwrap();
        assert!(!text.is_empty());
        for line in text.lines() {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(event["timestamp"].as_str().unwrap().ends_with('Z'));
        }
    }
    assert!(
        native_logs >= 2,
        "preview/status must each retain native private file logging"
    );
    assert!(!root.join("elsewhere/logs").exists());
}
