#![cfg(windows)]
use std::{path::Path, process::Stdio, time::Duration};

async fn output(command: &mut tokio::process::Command) -> std::process::Output {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command.creation_flags(0x08000000);
    tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .expect("Owned launcher fixture exceeded its deadline")
        .unwrap()
}
async fn junction(link: &Path, target: &Path) {
    let mut command = tokio::process::Command::new("pwsh");
    command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
        "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:FIVERR_FIXTURE_LINK -Target $env:FIVERR_FIXTURE_TARGET | Out-Null"])
        .env("FIVERR_FIXTURE_LINK", link).env("FIVERR_FIXTURE_TARGET", target);
    let result = output(&mut command).await;
    assert!(
        result.status.success(),
        "Required Windows junction capability unavailable: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_ne!(
        std::fs::symlink_metadata(link).unwrap().file_attributes() & 0x400,
        0
    );
}
use std::os::windows::fs::MetadataExt;

#[tokio::test]
async fn powershell_launcher_rejects_linked_root_before_any_write() {
    for child_name in ["logs", ".cache"] {
        for installed in [false, true] {
            let temp = tempfile::Builder::new()
                .prefix("launcher root policy space-")
                .tempdir()
                .unwrap();
            let root = temp.path().join("installation");
            let outside = temp.path().join("outside");
            std::fs::create_dir(&root).unwrap();
            std::fs::create_dir(&outside).unwrap();
            std::fs::write(outside.join("sentinel.txt"), b"unchanged").unwrap();
            std::fs::copy("run.ps1", root.join("run.ps1")).unwrap();
            if installed {
                std::fs::create_dir(root.join("bin")).unwrap();
                std::fs::copy(
                    env!("CARGO_BIN_EXE_fiverr-agent-mcp"),
                    root.join("bin/fiverr-agent-mcp.exe"),
                )
                .unwrap();
            }
            let link = root.join(child_name);
            junction(&link, &outside).await;
            let mut command = tokio::process::Command::new("pwsh");
            command
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
                .arg(root.join("run.ps1"))
                .arg("-Preview")
                .current_dir(temp.path())
                .env_remove("FIVERR_AGENT_TEMP_MODE");
            let result = output(&mut command).await;
            let outside_files: Vec<_> = std::fs::read_dir(&outside)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            let sentinel = std::fs::read(outside.join("sentinel.txt")).unwrap();
            let early_log = child_name != "logs" && root.join("logs").exists();
            // Remove exactly the owned junction before TempDir cleanup; never traverse its target.
            std::fs::remove_dir(&link).unwrap();
            assert!(!result.status.success(), "Linked root must be refused");
            assert_eq!(sentinel, b"unchanged");
            assert_eq!(
                outside_files,
                [std::ffi::OsString::from("sentinel.txt")],
                "Refused launcher wrote through {child_name}; installed={installed}"
            );
            assert!(
                !early_log,
                "Launcher wrote logs before validating {child_name}"
            );
        }
    }
}
