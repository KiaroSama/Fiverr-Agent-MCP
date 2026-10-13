use fiverr_agent_mcp::server::framing::BoundedStdio;
use rmcp::{RoleServer, model::ServerJsonRpcMessage, transport::Transport};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

struct PendingWriter;
impl AsyncWrite for PendingWriter {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Pending
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Pending
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct NewlineFailure {
    writes: usize,
}
impl AsyncWrite for NewlineFailure {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.writes += 1;
        if self.writes == 1 {
            Poll::Ready(Ok(bytes.len()))
        } else {
            assert_eq!(bytes, b"\n");
            Poll::Ready(Err(io::Error::other("controlled newline failure")))
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct FailedReader;
impl AsyncRead for FailedReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("controlled read failure")))
    }
}
fn response() -> ServerJsonRpcMessage {
    serde_json::from_value(serde_json::json!({"jsonrpc":"2.0","id":1,"result":{}})).unwrap()
}

#[tokio::test(start_paused = true)]
async fn pending_stdio_writer_expires_at_real_transport_deadline_and_can_close() {
    let mut transport = BoundedStdio::new(tokio::io::empty(), PendingWriter);
    let start = tokio::time::Instant::now();
    let error = Transport::<RoleServer>::send(&mut transport, response())
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "MCP writer deadline exceeded.");
    assert_eq!(start.elapsed(), Duration::from_secs(10));
    Transport::<RoleServer>::close(&mut transport)
        .await
        .unwrap();
}

#[tokio::test]
async fn framing_newline_and_reader_failures_never_return_success_or_partial_message() {
    let mut transport = BoundedStdio::new(FailedReader, NewlineFailure { writes: 0 });
    assert_eq!(
        Transport::<RoleServer>::send(&mut transport, response())
            .await
            .unwrap_err()
            .to_string(),
        "controlled newline failure"
    );
    assert!(
        Transport::<RoleServer>::receive(&mut transport)
            .await
            .is_none()
    );
    Transport::<RoleServer>::close(&mut transport)
        .await
        .unwrap();
}
