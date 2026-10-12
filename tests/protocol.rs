use fiverr_agent_mcp::{
    server::{Handler, framing::BoundedStdio, http},
    storage::{Root, Store},
};
use rmcp::{ServerHandler, ServiceExt};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
struct OwnedTask<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
fn metadata() -> Value {
    json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"fixture","version":"1"}})
}
#[tokio::test]
async fn modern_stdio_discovery_has_no_initialize_requirement() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let (server, client) = tokio::io::duplex(64 * 1024);
    let (r, w) = tokio::io::split(server);
    let task = tokio::spawn(async move {
        Handler { store }
            .serve(BoundedStdio::new(r, w))
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap()
    });
    let (r, mut w) = tokio::io::split(client);
    let mut reader = BufReader::new(r);
    let message =
        json!({"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":metadata()}});
    w.write_all(format!("{message}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        reader.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["id"], 1);
    assert!(
        response["result"]["supportedVersions"]
            .as_array()
            .unwrap()
            .contains(&json!("2026-07-28"))
    );
    let request =
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":metadata()}});
    w.write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    line.clear();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        reader.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["id"], 2);
    assert!(!response["result"]["tools"].as_array().unwrap().is_empty());
    w.shutdown().await.unwrap();
    drop(reader);
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn loopback_http_enforces_token_and_modern_header_mirrors() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let token = http::token(&store).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let app = http::router(store, port, cancel);
    let mut task = OwnedTask(tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
            .unwrap()
    }));
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let url = format!("http://127.0.0.1:{port}/mcp");
    assert_eq!(
        client.post(&url).body("{}").send().await.unwrap().status(),
        401
    );
    let body =
        json!({"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":metadata()}});
    let response = client
        .post(&url)
        .bearer_auth(&token)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "server/discover")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers().get("mcp-session-id").is_none());
    let response = client
        .post(&url)
        .bearer_auth(&token)
        .header("Origin", "https://evil.invalid")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    let response = client
        .post(&url)
        .bearer_auth(&token)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/call")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    task.0.abort();
    let _ = (&mut task.0).await;
}
#[test]
fn discovery_does_not_expose_owner_approval_or_arbitrary_execution() {
    let tmp = tempfile::tempdir().unwrap();
    let handler = Handler {
        store: Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap(),
    };
    assert!(handler.get_tool("execute_js").is_none());
    assert!(handler.get_tool("fiverr_action_approve").is_none());
    assert!(handler.get_tool("direct").is_none());
}
