//! Bounded synchronous work ownership outside asynchronous runtime threads.
use super::Store;
use crate::core::{ErrorCode, Result, error, storage_error};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};
use tokio::sync::oneshot;
const QUEUED: u8 = 0;
const STARTED: u8 = 1;
const CANCELLED: u8 = 2;
type Work = Box<dyn FnOnce(&Store) + Send>;
struct Job {
    state: Arc<AtomicU8>,
    work: Work,
}
pub struct Worker {
    sender: Mutex<Option<mpsc::SyncSender<Job>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
    stopping: Arc<AtomicBool>,
}
static WORKERS: OnceLock<Mutex<HashMap<PathBuf, Weak<Worker>>>> = OnceLock::new();
pub(super) fn shared(store: &Store) -> Result<Arc<Worker>> {
    let mut workers = WORKERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| storage_error())?;
    workers.retain(|_, v| v.strong_count() > 0);
    if let Some(w) = workers.get(store.root.path()).and_then(Weak::upgrade) {
        return Ok(w);
    }
    let (sender, receiver) = mpsc::sync_channel::<Job>(16);
    let stopping = Arc::new(AtomicBool::new(false));
    let stop = stopping.clone();
    let detached = store.detached();
    let handle = std::thread::Builder::new()
        .name("workspace-owner".into())
        .spawn(move || {
            for job in receiver {
                if job
                    .state
                    .compare_exchange(QUEUED, STARTED, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    if stop.load(Ordering::Acquire) {
                        job.state.store(CANCELLED, Ordering::Release);
                        continue;
                    }
                    // A panic drops its reply; an incomplete durable receipt is never replayed.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        (job.work)(&detached)
                    }));
                }
            }
        })
        .map_err(|_| storage_error())?;
    let worker = Arc::new(Worker {
        sender: Mutex::new(Some(sender)),
        handle: Mutex::new(Some(handle)),
        stopping,
    });
    workers.insert(store.root.path().to_owned(), Arc::downgrade(&worker));
    Ok(worker)
}
pub struct Pending<T> {
    receiver: oneshot::Receiver<Result<T>>,
    state: Arc<AtomicU8>,
    _owner: Arc<Worker>,
}
impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        let _ = self
            .state
            .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire);
    }
}
impl<T> Pending<T> {
    pub async fn wait(mut self) -> Result<T> {
        (&mut self.receiver).await.map_err(|_| storage_error())?
    }
}
impl Worker {
    pub fn submit<T: Send + 'static>(
        self: &Arc<Self>,
        operation: impl FnOnce(&Store) -> Result<T> + Send + 'static,
    ) -> Result<Pending<T>> {
        let state = Arc::new(AtomicU8::new(QUEUED));
        let (send, receiver) = oneshot::channel();
        let job = Job {
            state: state.clone(),
            work: Box::new(move |store| {
                let _ = send.send(operation(store));
            }),
        };
        let sender = self.sender.lock().map_err(|_| storage_error())?;
        if self.stopping.load(Ordering::Acquire) {
            return Err(storage_error());
        }
        sender
            .as_ref()
            .ok_or_else(storage_error)?
            .try_send(job)
            .map_err(|_| {
                error(
                    ErrorCode::StorageUnavailable,
                    "Workspace queue full or shutting down; no operation admitted.",
                )
            })?;
        Ok(Pending {
            receiver,
            state,
            _owner: self.clone(),
        })
    }
    pub(crate) fn close(&self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
    }
    pub fn shutdown(&self) -> Result<()> {
        self.close();
        if let Some(h) = self.handle.lock().map_err(|_| storage_error())?.take() {
            h.join().map_err(|_| storage_error())?
        }
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
