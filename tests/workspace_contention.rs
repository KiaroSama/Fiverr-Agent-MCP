use fiverr_agent_mcp::{
    network::Settings,
    service,
    storage::{Root, Store},
};
use serde_json::json;
use std::{
    sync::{Arc, Barrier, mpsc},
    time::{Duration, Instant},
};

#[test]
fn workspace_contention_records_actual_lock_and_transport_progress() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    Settings::load(&store).unwrap();
    let path = store
        .root
        .safe_path(".state/workspace", "workspace.sqlite3")
        .unwrap();
    let locker = rusqlite::Connection::open(path).unwrap();
    locker.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let ready = Arc::new(Barrier::new(3));
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let started = Instant::now();
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let ready = ready.clone();
            let store = store.clone();
            let started_tx = started_tx.clone();
            let done_tx = done_tx.clone();
            runtime.spawn(async move {
                ready.wait();
                started_tx.send(()).unwrap();
                let result = service::dispatch(&store, "fiverr_settings_get", json!({}));
                done_tx.send(result.is_ok()).unwrap();
            })
        })
        .collect();
    ready.wait();
    for _ in 0..2 {
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    let (progress_tx, progress_rx) = mpsc::channel();
    let probe = runtime.spawn(async move {
        progress_tx.send(()).unwrap();
    });
    // This is a characterization, not a runtime scheduling guarantee or an artificial sleep.
    let progressed_while_locked = progress_rx.recv_timeout(Duration::from_millis(200)).is_ok();
    locker.execute_batch("ROLLBACK").unwrap();
    for _ in 0..2 {
        assert!(done_rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }
    runtime.block_on(async {
        for handle in handles {
            handle.await.unwrap();
        }
        probe.await.unwrap();
    });
    assert!(started.elapsed() < Duration::from_secs(8));
    println!(
        "fixture=exclusive_sqlite workers=2 reads=2 progress_while_locked={progressed_while_locked} elapsed_ms={}",
        started.elapsed().as_millis()
    );
    assert!(store.value("unmodified").unwrap().is_none());
}
