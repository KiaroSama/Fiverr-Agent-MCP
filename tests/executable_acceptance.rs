use fiverr_agent_mcp::{bootstrap, storage::Root};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

async fn command(root: &Root, args: &[&str], rooted: bool) -> std::process::Output {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fiverr-agent-mcp"));
    child
        .arg("--root")
        .arg(root.path())
        .args(args)
        .current_dir(root.path().join("exports"));
    child.kill_on_drop(true).stdin(std::process::Stdio::null());
    if rooted {
        child.envs(bootstrap::environment(root));
    } else {
        for (name, _) in bootstrap::environment(root) {
            child.env_remove(name);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        child.as_std_mut().creation_flags(0x08000000);
    }
    tokio::time::timeout(Duration::from_secs(10), child.output())
        .await
        .unwrap()
        .unwrap()
}
#[tokio::test]
async fn native_entrypoint_roots_itself_preserves_exit_codes_and_writes_private_logs() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let version = command(&root, &["version"], true).await;
    assert!(version.status.success());
    assert!(
        String::from_utf8(version.stdout)
            .unwrap()
            .starts_with("fiverr-agent-mcp 0.1.0")
    );
    let status = command(&root, &["status"], false).await;
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let value: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(value["data"]["route"]["mode"], "blocked");
    assert!(
        root.path()
            .join(".state/workspace/workspace.sqlite3")
            .is_file()
    );
    let input = root.safe_path("exports", "input.json").unwrap();
    root.write_new(
        &input,
        serde_json::to_string(
            &json!({"kind":"note","id":"binary-note","payload":{"body":"Native entrypoint"}}),
        )
        .unwrap()
        .as_bytes(),
    )
    .unwrap();
    let path = input.to_str().unwrap();
    let save = command(
        &root,
        &["call", "fiverr_record_save", "--input", path],
        true,
    )
    .await;
    assert!(save.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&save.stdout).unwrap()["data"]["id"],
        "binary-note"
    );
    let error = command(&root, &["http-port", "0"], true).await;
    assert_eq!(error.status.code(), Some(1));
    assert!(error.stdout.is_empty());
    assert!(
        String::from_utf8(error.stderr)
            .unwrap()
            .contains("INVALID_INPUT")
    );
    let unknown = command(&root, &["call", "execute_js"], true).await;
    assert_eq!(unknown.status.code(), Some(1));
    assert!(unknown.stdout.is_empty());
    let menu = command(&root, &["menu"], true).await;
    assert_eq!(menu.status.code(), Some(1));
    assert!(
        String::from_utf8(menu.stderr)
            .unwrap()
            .contains("interactive")
    );
    let logs: Vec<_> = std::fs::read_dir(root.path().join("logs"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(logs.len() >= 5);
    let mut failures = 0;
    for p in logs {
        let text = std::fs::read_to_string(p).unwrap();
        for line in text.lines() {
            let v: Value = serde_json::from_str(line).unwrap();
            assert!(v["timestamp"].as_str().unwrap().ends_with('Z'));
            if v["level"] == "ERROR" {
                failures += 1;
            }
        }
    }
    assert!(failures >= 3);
    assert!(
        bootstrap::environment(&root)
            .iter()
            .all(|(_, v)| Path::new(v).starts_with(root.path()))
    );
}
