use fiverr_agent_mcp::{
    session,
    storage::{Root, Store},
};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

async fn command(root: &Path, args: &[&str], input: Option<Vec<u8>>) -> std::process::Output {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_fiverr-agent-mcp"));
    command
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().unwrap();
    if let Some(bytes) = input {
        let mut stdin = child.stdin.take().unwrap();
        tokio::time::timeout(Duration::from_secs(2), stdin.write_all(&bytes))
            .await
            .unwrap()
            .unwrap();
        drop(stdin);
    }
    tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap()
}
#[tokio::test]
async fn actual_session_cli_and_native_frames_preserve_secret_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(temp.path()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    fiverr_agent_mcp::network::Settings::change(&store, true, |s| {
        s.mode = fiverr_agent_mcp::network::Mode::Blocked;
        Ok(())
    })
    .unwrap();
    let file = root.safe_path("config", "cookie.txt").unwrap();
    root.write_new(&file, b"session=synthetic-cli-private")
        .unwrap();
    let out = command(
        root.path(),
        &["session", "import", file.to_str().unwrap(), "fixture_owner"],
        None,
    )
    .await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["state"], "credential_saved");
    assert!(
        !String::from_utf8(out.stdout)
            .unwrap()
            .contains("synthetic-cli-private")
    );
    let out = command(root.path(), &["session", "verify"], None).await;
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("ROUTE_BLOCKED"));
    assert_eq!(session::status(&store).unwrap()["state"], "route_blocked");
    let id = "abcdefghijklmnopabcdefghijklmnop";
    assert!(
        command(
            root.path(),
            &["session", "extension-pair", id, "fixture_owner"],
            None
        )
        .await
        .status
        .success()
    );
    let message=serde_json::to_vec(&json!({"type":"account_observation","url":"https://www.fiverr.com/users/manage_gigs","username":"fixture_owner","management":true,"logout":true})).unwrap();
    let mut frame = (message.len() as u32).to_ne_bytes().to_vec();
    frame.extend_from_slice(&message);
    let caller = format!("chrome-extension://{id}/");
    let out = command(root.path(), &["native-session", &caller], Some(frame)).await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let length = u32::from_ne_bytes(out.stdout[..4].try_into().unwrap()) as usize;
    assert_eq!(out.stdout.len(), length + 4);
    let response: Value = serde_json::from_slice(&out.stdout[4..]).unwrap();
    assert_eq!(response["status"]["account_observed"], true);
    assert_eq!(response["status"]["live_authenticated"], false);
    assert_eq!(response["status"]["mode"], "extension");
    let installed = root.path().join("bin");
    std::fs::create_dir(&installed).unwrap();
    let exe = installed.join(if cfg!(windows) {
        "fiverr-agent-mcp.exe"
    } else {
        "fiverr-agent-mcp"
    });
    std::fs::copy(env!("CARGO_BIN_EXE_fiverr-agent-mcp"), &exe).unwrap();
    root.write_new(
        &root.path().join("fiverr.install.json"),
        include_bytes!("../fiverr.install.json"),
    )
    .unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(&exe)
            .args(["session", "extension-manifest"])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("config/com.fiverr_agent_mcp.session.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["allowed_origins"], json!([caller.clone()]));
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(&exe)
            .args(["session", "extension-manifest"])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success());
    let manifest_path = root.path().join("config/com.fiverr_agent_mcp.session.json");
    std::fs::write(&manifest_path, b"{}").unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(&exe)
            .args(["session", "extension-manifest"])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    assert_eq!(std::fs::read(&manifest_path).unwrap(), b"{}");
    let mut host = tokio::process::Command::new(&exe)
        .arg(&caller)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = host.stdin.take().unwrap();
    let mut frame = (message.len() as u32).to_ne_bytes().to_vec();
    frame.extend_from_slice(&message);
    stdin.write_all(&frame).await.unwrap();
    // Keep input open like Chrome: the one-frame native host must still exit.
    let output = tokio::time::timeout(Duration::from_secs(10), host.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout[4..]).unwrap()["ok"],
        true
    );
    drop(stdin);
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(&exe)
            .arg(&caller)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout[4..]).unwrap()["ok"],
        false
    );
    assert!(
        command(root.path(), &["session", "forget"], None)
            .await
            .status
            .success()
    );
    assert_eq!(session::status(&store).unwrap()["configured"], false);
    for entry in std::fs::read_dir(root.path().join("logs")).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !bytes
                .windows(b"synthetic-cli-private".len())
                .any(|w| w == b"synthetic-cli-private")
        );
    }
    assert_eq!(
        std::fs::read(file).unwrap(),
        b"session=synthetic-cli-private"
    );
}
