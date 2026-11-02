use super::response_lifetime::bounded;
use axum::body::{Body, Bytes, to_bytes};
use futures::{Stream, StreamExt};
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

struct Observed {
    polls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
    remaining: Option<usize>,
    error: bool,
}
impl Stream for Observed {
    type Item = Result<Bytes, std::io::Error>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        match self.remaining {
            None => Poll::Pending,
            Some(0) => Poll::Ready(None),
            Some(count) => {
                self.remaining = Some(count - 1);
                Poll::Ready(Some(if self.error {
                    Err(std::io::Error::other("controlled upstream failure"))
                } else {
                    Ok(Bytes::from_static(b"frame"))
                }))
            }
        }
    }
}
impl Drop for Observed {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
fn observed(remaining: Option<usize>, error: bool) -> (Body, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let polls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    (
        Body::from_stream(Observed {
            polls: polls.clone(),
            drops: drops.clone(),
            remaining,
            error,
        }),
        polls,
        drops,
    )
}
async fn settle() {
    // Give the owned deadline task its scheduling turn; never wait on real time.
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
}

#[tokio::test(start_paused = true)]
async fn retained_unpolled_body_is_dropped_at_absolute_deadline_without_prefetch() {
    let capacity = Arc::new(Semaphore::new(1));
    let (upstream, polls, drops) = observed(None, false);
    let body = bounded(
        upstream,
        capacity.clone().acquire_owned().await.unwrap(),
        CancellationToken::new(),
    );
    settle().await;
    tokio::time::advance(Duration::from_secs(29)).await;
    settle().await;
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(capacity.available_permits(), 0);
    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(capacity.available_permits(), 1);
    assert!(to_bytes(body, 32).await.unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn finite_eof_and_error_discard_upstream_before_releasing_capacity() {
    for (remaining, error) in [(0, false), (2, false), (1, true)] {
        let capacity = Arc::new(Semaphore::new(1));
        let (upstream, _, drops) = observed(Some(remaining), error);
        let body = bounded(
            upstream,
            capacity.clone().acquire_owned().await.unwrap(),
            CancellationToken::new(),
        );
        let result = to_bytes(body, 32).await;
        if error {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap().len(), remaining * 5);
        }
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(capacity.available_permits(), 1);
    }
}

#[tokio::test(start_paused = true)]
async fn partial_consumption_does_not_reset_deadline_or_buffer_upstream() {
    let capacity = Arc::new(Semaphore::new(1));
    let (upstream, polls, drops) = observed(Some(4), false);
    let body = bounded(
        upstream,
        capacity.clone().acquire_owned().await.unwrap(),
        CancellationToken::new(),
    );
    let mut stream = body.into_data_stream();
    assert_eq!(stream.next().await.unwrap().unwrap(), b"frame"[..]);
    tokio::time::advance(Duration::from_secs(29)).await;
    assert_eq!(stream.next().await.unwrap().unwrap(), b"frame"[..]);
    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(polls.load(Ordering::SeqCst), 2);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(capacity.available_permits(), 1);
    assert!(stream.next().await.is_none());
}

#[tokio::test(start_paused = true)]
async fn pending_poll_wakes_on_shutdown_and_discards_upstream() {
    let capacity = Arc::new(Semaphore::new(1));
    let (upstream, polls, drops) = observed(None, false);
    let cancel = CancellationToken::new();
    let body = bounded(
        upstream,
        capacity.clone().acquire_owned().await.unwrap(),
        cancel.clone(),
    );
    let reader = tokio::spawn(to_bytes(body, 32));
    settle().await;
    assert!(polls.load(Ordering::SeqCst) > 0);
    cancel.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), reader)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .is_empty()
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(capacity.available_permits(), 1);
}

#[tokio::test(start_paused = true)]
async fn saturation_drop_deadline_races_and_shutdown_never_over_release() {
    let capacity = Arc::new(Semaphore::new(16));
    let cancel = CancellationToken::new();
    let mut bodies = Vec::new();
    let mut dropped = Vec::new();
    for _ in 0..16 {
        let (upstream, _, drops) = observed(None, false);
        bodies.push(bounded(
            upstream,
            capacity.clone().try_acquire_owned().unwrap(),
            cancel.clone(),
        ));
        dropped.push(drops);
    }
    assert!(capacity.clone().try_acquire_owned().is_err());
    for body in bodies.drain(..8) {
        drop(body);
    }
    assert_eq!(capacity.available_permits(), 8);
    tokio::time::advance(Duration::from_secs(30)).await;
    cancel.cancel();
    drop(bodies);
    settle().await;
    assert!(
        dropped
            .iter()
            .all(|count| count.load(Ordering::SeqCst) == 1)
    );
    assert_eq!(capacity.available_permits(), 16);
    let reused = capacity.clone().try_acquire_many_owned(16).unwrap();
    assert_eq!(capacity.available_permits(), 0);
    drop(reused);
    assert_eq!(capacity.available_permits(), 16);
}
