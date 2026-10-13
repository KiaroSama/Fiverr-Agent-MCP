#![cfg(unix)]
use fiverr_agent_mcp::{
    bootstrap,
    server::http,
    storage::{Root, Store},
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[tokio::test]
async fn native_bridge_stdio_and_http_shutdown_are_owned_and_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fiverr-agent-mcp"))
        .arg("--root")
        .arg(root.path())
        .arg("bridge")
        .envs(bootstrap::environment(&root))
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    for (id, method) in [(1, "server/discover"), (2, "tools/list")] {
        let message = json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"native-fixture","version":"1"}}}});
        stdin
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), stdout.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["id"], id);
    }
    stdin.shutdown().await.unwrap();
    drop(stdin);
    drop(stdout);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let token = http::token(&store).unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fiverr-agent-mcp"))
        .arg("--root")
        .arg(root.path())
        .args(["http", "--port", &port.to_string()])
        .envs(bootstrap::environment(&root))
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), stderr.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert!(line.contains("listening"));
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let response = client
        .get(format!("http://127.0.0.1:{port}/healthz"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "alive");
    let pid = child.id().unwrap();
    let status = tokio::process::Command::new("/bin/kill")
        .args(["-INT", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .status()
        .await
        .unwrap();
    assert!(status.success());
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(
        tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .is_ok()
    );
}
