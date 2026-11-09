use fiverr_agent_mcp::{
    bootstrap,
    server::http,
    storage::{Root, Store},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const WAIT: Duration = Duration::from_secs(5);
fn metadata() -> Value {
    json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"package-fixture","version":"1"}})
}
fn request(id: u64, method: &str, mut params: Value) -> Value {
    params["_meta"] = metadata();
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn command(binary: &Path, root: &Root) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--root")
        .arg(root.path())
        .envs(bootstrap::environment(root))
        .env_remove("FIVERR_AGENT_TEMP_MODE")
        .current_dir(root.path().join("exports"))
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
        let system = std::env::var_os("SystemRoot").unwrap();
        command.env("PATH", Path::new(&system).join("System32"));
    }
    #[cfg(unix)]
    command.env("PATH", "");
    command
}
async fn rpc(
    input: &mut tokio::process::ChildStdin,
    output: &mut BufReader<tokio::process::ChildStdout>,
    message: Value,
) -> Value {
    tokio::time::timeout(WAIT, input.write_all(format!("{message}\n").as_bytes()))
        .await
        .unwrap()
        .unwrap();
    let mut line = String::new();
    tokio::time::timeout(WAIT, output.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["id"], message["id"]);
    assert!(response.get("error").is_none(), "{response}");
    response
}
async fn bridge(
    binary: &Path,
    root: &Root,
) -> (
    tokio::process::Child,
    tokio::process::ChildStdin,
    BufReader<tokio::process::ChildStdout>,
) {
    let mut child = command(binary, root)
        .arg("bridge")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let discover = rpc(
        &mut input,
        &mut output,
        request(1, "server/discover", json!({})),
    )
    .await;
    assert!(
        discover["result"]["supportedVersions"]
            .as_array()
            .unwrap()
            .contains(&json!("2026-07-28"))
    );
    (child, input, output)
}
async fn close_bridge(
    mut child: tokio::process::Child,
    mut input: tokio::process::ChildStdin,
    output: BufReader<tokio::process::ChildStdout>,
) {
    input.shutdown().await.unwrap();
    drop(input);
    drop(output);
    assert!(
        tokio::time::timeout(WAIT, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn extracted_package_roundtrip_preserves_local_workspace() {
    let temporary = tempfile::tempdir().unwrap();
    let supplied = std::env::var_os("FIVERR_PACKAGE_BINARY");
    let installation = temporary.path().join("relocated package space");
    let binary: PathBuf = if let Some(path) = supplied {
        PathBuf::from(path).canonicalize().unwrap()
    } else {
        std::fs::create_dir(&installation).unwrap();
        let name = if cfg!(windows) {
            "fiverr-agent-mcp.exe"
        } else {
            "fiverr-agent-mcp"
        };
        let binary = installation.join(name);
        std::fs::copy(env!("CARGO_BIN_EXE_fiverr-agent-mcp"), &binary).unwrap();
        binary.canonicalize().unwrap()
    };
    let workspace = temporary.path().join("private workspace space");
    std::fs::create_dir(&workspace).unwrap();
    let root = Root::explicit(&workspace.canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let token = http::token(&store).unwrap();
    drop(store);
    let (child, mut input, mut output) = bridge(&binary, &root).await;
    let saved = rpc(&mut input, &mut output, request(2, "tools/call", json!({"name":"fiverr_record_save","arguments":{"kind":"gig","id":"package-draft","payload":{"title":"Synthetic package draft α"}}}))).await;
    let record = &saved["result"]["structuredContent"]["data"];
    assert_eq!(record["draft"], true);
    assert_eq!(record["version"], 1);
    assert_eq!(
        saved["result"]["structuredContent"]["provenance"]["remote_effect"],
        false
    );
    close_bridge(child, input, output).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = command(&binary, &root)
        .args(["http", "--port", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    tokio::time::timeout(WAIT, stderr.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert!(line.contains("listening"));
    let result = async move {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(WAIT)
            .build()
            .unwrap();
        let url = format!("http://127.0.0.1:{port}/mcp");
        for (id, name, args) in [
            (
                3,
                "fiverr_record_get",
                json!({"kind":"gig","id":"package-draft"}),
            ),
            (
                4,
                "fiverr_export_prepare",
                json!({"kind":"gig","format":"json","filename":"package-export.json"}),
            ),
            (
                5,
                "fiverr_export_prepare",
                json!({"kind":"gig","format":"json","filename":"package-export.json"}),
            ),
        ] {
            let response = client
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", "application/json, text/event-stream")
                .header("MCP-Protocol-Version", "2026-07-28")
                .header("Mcp-Method", "tools/call")
                .json(&request(
                    id,
                    "tools/call",
                    json!({"name":name,"arguments":args}),
                ))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let response: Value = response.json().await.unwrap();
            assert_eq!(response["id"], id);
            let result = &response["result"];
            if id == 3 {
                assert_eq!(
                    result["structuredContent"]["data"]["payload"]["title"],
                    "Synthetic package draft α"
                );
            } else if id == 4 {
                assert_eq!(result["structuredContent"]["data"]["count"], 1);
            } else {
                assert_eq!(result["isError"], true, "export must not overwrite");
            }
        }
    };
    // Always reap the exact server even when an assertion panics inside the workload.
    let mut workload = tokio::spawn(result);
    let outcome = tokio::time::timeout(Duration::from_secs(18), &mut workload).await;
    if outcome.is_err() {
        workload.abort();
        let _ = workload.await;
    }
    child.kill().await.unwrap();
    tokio::time::timeout(WAIT, child.wait())
        .await
        .unwrap()
        .unwrap();
    outcome.unwrap().unwrap();
    let exported: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("exports/package-export.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(exported.as_array().unwrap().len(), 1);
    assert_eq!(exported[0]["id"], "package-draft");
    assert_eq!(exported[0]["payload"]["title"], "Synthetic package draft α");
    assert!(
        tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .is_ok()
    );
}
