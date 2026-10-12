use fiverr_agent_mcp::{
    server::{Handler, framing::BoundedStdio},
    storage::{Root, Store},
};
use rmcp::ServiceExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
struct Owned(tokio::task::JoinHandle<()>);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}
#[tokio::test]
async fn legacy_initialize_then_tools_resources_prompts_and_local_draft() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let (server, client) = tokio::io::duplex(1024 * 1024);
    let (r, w) = tokio::io::split(server);
    let task = Owned(tokio::spawn(async move {
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
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"resources/list"}),
        json!({"jsonrpc":"2.0","id":4,"method":"prompts/list"}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"fiverr_record_save","arguments":{"kind":"gig","id":"legacy1","payload":{"title":"Local legacy draft"}}}}),
        json!({"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"fiverr://records/gig/legacy1"}}),
    ];
    for request in requests {
        w.write_all(format!("{request}\n").as_bytes())
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
        let result: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(result["id"], request["id"]);
        assert!(result.get("error").is_none(), "{result}");
        match request["id"].as_u64().unwrap() {
            1 => {
                assert_eq!(result["result"]["protocolVersion"], "2025-11-25");
                w.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
                    .await
                    .unwrap();
            }
            2 => assert!(!result["result"]["tools"].as_array().unwrap().is_empty()),
            3 => assert!(!result["result"]["resources"].as_array().unwrap().is_empty()),
            4 => assert_eq!(
                result["result"]["prompts"][0]["name"],
                "fiverr-gig-optimizer"
            ),
            5 => assert_eq!(result["result"]["structuredContent"]["data"]["draft"], true),
            6 => assert!(
                result["result"]["contents"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("Local legacy draft")
            ),
            _ => unreachable!(),
        }
    }
    w.shutdown().await.unwrap();
    drop(reader);
    task.0.abort();
}
