#![cfg(target_os = "linux")]
use fiverr_agent_mcp::{
    bootstrap,
    service::actions,
    storage::{Root, Store},
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
async fn terminal(root: &Root, args: &[&str], input: &[u8]) -> String {
    let quoted = std::iter::once(env!("CARGO_BIN_EXE_fiverr-agent-mcp"))
        .chain(["--root", root.path().to_str().unwrap()])
        .chain(args.iter().copied())
        .map(|s| format!("'{}'", s.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");
    let mut command = tokio::process::Command::new("/usr/bin/script");
    command
        .args(["-q", "-e", "-c", &quoted, "/dev/null"])
        .envs(bootstrap::environment(root))
        .env("TERM", "dumb")
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input).await.unwrap();
    drop(stdin);
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}
#[tokio::test]
async fn real_owner_terminal_accepts_explicit_consent_and_keeps_model_routes_separate() {
    assert!(
        std::path::Path::new("/usr/bin/script").is_file(),
        "Native PTY prerequisite missing"
    );
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let output = terminal(&root, &["menu"], b"1\n2\n3\n4\n5\ninvalid\n0\n").await;
    assert!(output.contains("menu_closed"));
    assert!(output.contains("Choose 0-5"));
    let decline = terminal(&root, &["direct"], b"n\n").await;
    assert!(decline.contains("Owner declined"));
    assert_eq!(
        serde_json::to_value(
            fiverr_agent_mcp::network::Settings::load(&store)
                .unwrap()
                .mode
        )
        .unwrap(),
        "blocked"
    );
    let direct = terminal(&root, &["direct"], b"y\n").await;
    assert!(direct.contains("generation_changed"));
    assert_eq!(
        serde_json::to_value(
            fiverr_agent_mcp::network::Settings::load(&store)
                .unwrap()
                .mode
        )
        .unwrap(),
        "direct"
    );
    let trusted = terminal(&root, &["http-auth", "set", "loopback-trusted"], b"y\n").await;
    assert!(trusted.contains("updated"));
    let auth = terminal(&root, &["http-auth", "export", "private-auth.json"], b"y\n").await;
    assert!(auth.contains("written"));
    let value: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("exports/private-auth.json")).unwrap(),
    )
    .unwrap();
    assert!(
        value["headers"]["Authorization"]
            .as_str()
            .unwrap()
            .starts_with("Bearer ")
    );
    let client = root.path().join("config/client.json");
    root.write_new(&client, br#"{"theme":"keep"}"#).unwrap();
    assert!(
        terminal(&root, &["register", client.to_str().unwrap()], b"y\n")
            .await
            .contains("registered")
    );
    assert!(
        terminal(&root, &["unregister", client.to_str().unwrap()], b"y\n")
            .await
            .contains("registered")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(client).unwrap()).unwrap()["theme"],
        "keep"
    );
    let action = actions::prepare(
        &store,
        "note",
        &json!({"target_id":"a","payload":{"body":"Owner review"}}),
    )
    .unwrap();
    let id = action["data"]["id"].as_str().unwrap();
    let output = terminal(&root, &["actions", "approve", id], b"y\n").await;
    assert!(output.contains("approved"));
    assert_eq!(
        actions::get(&store, id).unwrap()["data"]["state"],
        "approved"
    );
    let source = tempfile::tempdir().unwrap();
    let source_root = Root::explicit(&source.path().canonicalize().unwrap()).unwrap();
    let source_store = Store::open(source_root.clone()).unwrap();
    let path = source_store.backup("empty.sqlite3").unwrap();
    let destination = tempfile::tempdir().unwrap();
    let destination_root = Root::explicit(&destination.path().canonicalize().unwrap()).unwrap();
    destination_root
        .write_new(
            &destination_root
                .safe_path("exports", "empty.sqlite3")
                .unwrap(),
            &std::fs::read(path).unwrap(),
        )
        .unwrap();
    assert!(
        terminal(
            &destination_root,
            &["backup", "restore", "empty.sqlite3"],
            b"y\n"
        )
        .await
        .contains("restored")
    );
}
