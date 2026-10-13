use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use fiverr_agent_mcp::{
    server::{Handler, framing::BoundedStdio, http},
    storage::{Root, Store},
};
use rmcp::ServiceExt;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as TowerServiceExt;
struct Owned(tokio::task::JoinHandle<()>);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}
fn metadata() -> Value {
    json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"boundary-fixture","version":"1"}})
}

#[tokio::test]
async fn modern_stdio_exercises_resources_prompts_tools_and_operational_errors() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let (server, client) = tokio::io::duplex(65536);
    let (r, w) = tokio::io::split(server);
    let mut task = Owned(tokio::spawn(async move {
        Handler { store }
            .serve(BoundedStdio::new(r, w))
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    }));
    let (r, mut w) = tokio::io::split(client);
    let mut reader = BufReader::new(r);
    let requests = [
        ("server/discover", json!({}), false),
        ("tools/list", json!({"cursor":"unexpected"}), true),
        ("resources/list", json!({}), false),
        ("resources/templates/list", json!({}), false),
        ("prompts/list", json!({}), false),
        ("prompts/get", json!({"name":"fiverr-gig-optimizer"}), false),
        ("prompts/get", json!({"name":"missing"}), true),
        ("resources/read", json!({"uri":"fiverr://policy"}), false),
        (
            "resources/read",
            json!({"uri":"fiverr://capabilities"}),
            false,
        ),
        ("resources/read", json!({"uri":"fiverr://account"}), false),
        (
            "resources/read",
            json!({"uri":"fiverr://records/note/a/b"}),
            true,
        ),
        ("resources/read", json!({"uri":"fiverr://unknown"}), true),
        (
            "tools/call",
            json!({"name":"execute_js","arguments":{}}),
            true,
        ),
        (
            "tools/call",
            json!({"name":"fiverr_record_save","arguments":{"kind":"note","id":"protocol-note","payload":{"body":"Kept through MCP"}}}),
            false,
        ),
        (
            "resources/read",
            json!({"uri":"fiverr://records/note/protocol-note"}),
            false,
        ),
        (
            "tools/call",
            json!({"name":"fiverr_record_get","arguments":{"kind":"note","id":"absent"}}),
            false,
        ),
        (
            "resources/read",
            json!({"uri":"fiverr://records/note/absent"}),
            true,
        ),
        (
            "tools/call",
            json!({"name":"fiverr_action_prepare","arguments":{"kind":"note","target_id":"buyer","payload":{"body":"Review"}}}),
            false,
        ),
    ];
    let mut action = None;
    for (index, (method, mut params, error)) in requests.into_iter().enumerate() {
        params["_meta"] = metadata();
        let request = json!({"jsonrpc":"2.0","id":index+1,"method":method,"params":params});
        w.write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], index + 1);
        assert_eq!(
            response.get("error").is_some(),
            error,
            "{method}: {response}"
        );
        if method == "resources/list" {
            assert_eq!(response["result"]["resources"].as_array().unwrap().len(), 3);
        }
        if method == "resources/templates/list" {
            assert_eq!(
                response["result"]["resourceTemplates"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
        }
        if method == "prompts/list" {
            assert_eq!(
                response["result"]["prompts"][0]["name"],
                "fiverr-gig-optimizer"
            );
        }
        if method == "tools/call" && params["name"] == "fiverr_record_get" {
            assert_eq!(response["result"]["isError"], true);
            assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                "RESOURCE_NOT_FOUND"
            );
        }
        if method == "tools/call" && params["name"] == "fiverr_action_prepare" {
            action = Some(
                response["result"]["structuredContent"]["data"]["id"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            );
        }
    }
    let request = json!({"jsonrpc":"2.0","id":100,"method":"resources/read","params":{"uri":format!("fiverr://actions/{}",action.unwrap()),"_meta":metadata()}});
    w.write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert!(
        serde_json::from_str::<Value>(&line)
            .unwrap()
            .get("result")
            .is_some()
    );
    w.shutdown().await.unwrap();
    drop(reader);
    tokio::time::timeout(Duration::from_secs(5), &mut task.0)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn http_guards_reject_duplicate_invalid_headers_and_preserve_private_responses() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let token = http::token(&store).unwrap();
    let port = 1597;
    let router = http::router(store.clone(), port, CancellationToken::new());
    let auth = format!("Bearer {token}");
    let cases = [
        (vec![], "/healthz", StatusCode::FORBIDDEN),
        (
            vec![("host", "127.0.0.1:1597"), ("host", "127.0.0.1:1597")],
            "/healthz",
            StatusCode::FORBIDDEN,
        ),
        (
            vec![
                ("host", "127.0.0.1:1597"),
                ("origin", "http://127.0.0.1:1597"),
                ("origin", "http://127.0.0.1:1597"),
            ],
            "/healthz",
            StatusCode::FORBIDDEN,
        ),
        (
            vec![("host", "127.0.0.1:1597")],
            "/healthz?token=no",
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![("host", "127.0.0.1:1597")],
            "/healthz",
            StatusCode::UNAUTHORIZED,
        ),
        (
            vec![("host", "127.0.0.1:1597"), ("authorization", "Bad")],
            "/healthz",
            StatusCode::UNAUTHORIZED,
        ),
        (
            vec![("host", "127.0.0.1:1597"), ("authorization", "Basic value")],
            "/healthz",
            StatusCode::UNAUTHORIZED,
        ),
        (
            vec![
                ("host", "127.0.0.1:1597"),
                ("authorization", auth.as_str()),
                ("authorization", auth.as_str()),
            ],
            "/healthz",
            StatusCode::UNAUTHORIZED,
        ),
        (
            vec![("host", "127.0.0.1:1597"), ("authorization", auth.as_str())],
            "/healthz",
            StatusCode::OK,
        ),
        (
            vec![
                ("host", "127.0.0.1:1597"),
                ("authorization", auth.as_str()),
                ("origin", "http://127.0.0.1:1597"),
            ],
            "/readyz",
            StatusCode::OK,
        ),
    ];
    for (headers, path, expected) in cases {
        let mut request = Request::builder().uri(path).body(Body::empty()).unwrap();
        for (name, value) in headers {
            request.headers_mut().append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        let response =
            tokio::time::timeout(Duration::from_secs(5), router.clone().oneshot(request))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(&token));
    }
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "127.0.0.1:1597")
        .header("authorization", &auth)
        .body(Body::from(vec![b'x'; 1024 * 1024 + 1]))
        .unwrap();
    assert_eq!(
        router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    fiverr_agent_mcp::network::Settings::change(&store, false, |s| {
        s.http_auth_mode = "invalid".into();
        Ok(())
    })
    .unwrap();
    let request = Request::builder()
        .uri("/healthz")
        .header("host", "127.0.0.1:1597")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    fiverr_agent_mcp::network::Settings::change(&store, false, |s| {
        s.http_auth_mode = "loopback-trusted".into();
        Ok(())
    })
    .unwrap();
    let request = Request::builder()
        .uri("/healthz")
        .header("host", "127.0.0.1:1597")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.oneshot(request).await.unwrap().status(),
        StatusCode::OK
    );
}
