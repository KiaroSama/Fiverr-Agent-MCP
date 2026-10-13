use fiverr_agent_mcp::server::framing::BoundedStdio;
use rmcp::{RoleServer, model::ServerJsonRpcMessage, transport::Transport};
use tokio::io::AsyncWriteExt;
#[tokio::test]
async fn malformed_and_overlong_frames_fail_without_partial_dispatch() {
    for bytes in [b"not json\n".to_vec(), vec![b'x'; 1024 * 1024 + 1]] {
        let (reader, mut writer) = tokio::io::duplex(2 * 1024 * 1024);
        writer.write_all(&bytes).await.unwrap();
        writer.shutdown().await.unwrap();
        let mut transport = BoundedStdio::new(reader, tokio::io::sink());
        assert!(
            Transport::<RoleServer>::receive(&mut transport)
                .await
                .is_none()
        );
    }
    let mut transport = BoundedStdio::new(tokio::io::empty(), tokio::io::sink());
    let message: ServerJsonRpcMessage = serde_json::from_value(
        serde_json::json!({"jsonrpc":"2.0","id":1,"result":{"data":"x".repeat(1024*1024)}}),
    )
    .unwrap();
    assert!(
        Transport::<RoleServer>::send(&mut transport, message)
            .await
            .is_err()
    );
    Transport::<RoleServer>::close(&mut transport)
        .await
        .unwrap();
}
