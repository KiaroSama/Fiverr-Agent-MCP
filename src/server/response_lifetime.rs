use axum::body::{Body, BodyDataStream, Bytes};
use futures::{Stream, task::AtomicWaker};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{sync::OwnedSemaphorePermit, task::JoinHandle, time::Instant};
use tokio_util::sync::CancellationToken;

struct Upstream {
    body: BodyDataStream,
    // Drop the actual upstream body before returning its capacity.
    _permit: OwnedSemaphorePermit,
}

struct Shared {
    upstream: Mutex<Option<Upstream>>,
    waker: AtomicWaker,
}

impl Shared {
    fn close(&self) {
        self.upstream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.waker.wake();
    }
}

struct ResponseLifetime {
    shared: Arc<Shared>,
    deadline: Instant,
    cancel: CancellationToken,
    timer: JoinHandle<()>,
}

impl Stream for ResponseLifetime {
    type Item = std::result::Result<Bytes, axum::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.shared.waker.register(cx.waker());
        let mut upstream = self
            .shared
            .upstream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if Instant::now() >= self.deadline || self.cancel.is_cancelled() {
            upstream.take();
        }
        let Some(body) = upstream.as_mut() else {
            self.timer.abort();
            return Poll::Ready(None);
        };
        let result = Pin::new(&mut body.body).poll_next(cx);
        if matches!(result, Poll::Ready(None | Some(Err(_)))) {
            upstream.take();
            self.timer.abort();
        }
        result
    }
}

impl Drop for ResponseLifetime {
    fn drop(&mut self) {
        self.shared.close();
        self.timer.abort();
    }
}

pub(super) fn bounded(body: Body, permit: OwnedSemaphorePermit, cancel: CancellationToken) -> Body {
    let deadline = Instant::now() + Duration::from_secs(30);
    let shared = Arc::new(Shared {
        upstream: Mutex::new(Some(Upstream {
            body: body.into_data_stream(),
            _permit: permit,
        })),
        waker: AtomicWaker::new(),
    });
    let owned = shared.clone();
    let shutdown = cancel.clone();
    let timer = tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => {},
            _ = tokio::time::sleep_until(deadline) => {},
        }
        owned.close();
    });
    Body::from_stream(ResponseLifetime {
        shared,
        deadline,
        cancel,
        timer,
    })
}
