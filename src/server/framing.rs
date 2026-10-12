use rmcp::{
    RoleServer,
    model::{ClientJsonRpcMessage, ServerJsonRpcMessage},
    transport::Transport,
};
use std::{io, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::Mutex,
};

pub struct BoundedStdio<R, W> {
    reader: BufReader<R>,
    pending: Vec<u8>,
    writer: Arc<Mutex<W>>,
}
impl<R: AsyncRead, W> BoundedStdio<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            pending: Vec::new(),
            writer: Arc::new(Mutex::new(writer)),
        }
    }
}
impl<R, W> Transport<RoleServer> for BoundedStdio<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    type Error = io::Error;
    fn send(
        &mut self,
        message: ServerJsonRpcMessage,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let writer = self.writer.clone();
        async move {
            let data = serde_json::to_vec(&message).map_err(io::Error::other)?;
            if data.len() > 1024 * 1024 {
                return Err(io::Error::other("MCP response exceeds frame limit."));
            }
            let mut writer = writer.lock().await;
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                writer.write_all(&data).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await
            })
            .await
            .map_err(|_| io::Error::other("MCP writer deadline exceeded."))?
        }
    }
    async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
        loop {
            let buffer = self.reader.fill_buf().await.ok()?;
            if buffer.is_empty() {
                return None;
            }
            let end = buffer.iter().position(|b| *b == b'\n');
            let consumed = end.map_or(buffer.len(), |i| i + 1);
            if self.pending.len() + consumed > 1024 * 1024 {
                return None;
            }
            self.pending.extend_from_slice(&buffer[..consumed]);
            self.reader.consume(consumed);
            if end.is_some() {
                let data = std::mem::take(&mut self.pending);
                match serde_json::from_slice(&data) {
                    Ok(message) => return Some(message),
                    Err(_) => return None,
                }
            }
        }
    }
    async fn close(&mut self) -> io::Result<()> {
        self.writer.lock().await.shutdown().await
    }
}
