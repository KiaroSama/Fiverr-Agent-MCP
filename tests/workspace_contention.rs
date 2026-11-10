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
fn async_http_auth_does_not_occupy_runtime_workers_during_sqlite_contention() {
    use axum::{body::Body, http::Request};
    use fiverr_agent_mcp::server::http;
    use tokio_util::sync::CancellationToken;
    use tower::ServiceExt;
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    Settings::load(&store).unwrap();
    http::token(&store).unwrap();
    let app = http::router(store.clone(), 1597, CancellationToken::new());
    let locker = rusqlite::Connection::open(
        store
            .root
            .safe_path(".state/workspace", "workspace.sqlite3")
            .unwrap(),
    )
    .unwrap();
    locker.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (entered, ready) = mpsc::channel();
    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let app = app.clone();
            let entered = entered.clone();
            runtime.spawn(async move {
                entered.send(()).unwrap();
                app.oneshot(
                    Request::builder()
                        .uri("/healthz")
                        .header("host", "127.0.0.1:1597")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
            })
        })
        .collect();
    for _ in 0..2 {
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    let (progress, observed) = mpsc::channel();
    let probe = runtime.spawn(async move {
        progress.send(()).unwrap();
    });
    let progressed = observed.recv_timeout(Duration::from_millis(250)).is_ok();
    locker.execute_batch("ROLLBACK").unwrap();
    runtime.block_on(async {
        for task in tasks {
            task.await.unwrap();
        }
        probe.await.unwrap();
    });
    assert!(
        progressed,
        "HTTP authentication blocked both Tokio workers on SQLite"
    );
}

#[tokio::test]
async fn queued_workspace_writes_are_bounded_and_cancelled_before_execution() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let second = Store::open(root).unwrap();
    let worker = store.worker().unwrap();
    assert!(Arc::ptr_eq(&worker, &second.worker().unwrap()));
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = mpsc::channel();
    let active = worker
        .submit(move |_| {
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), ready)
        .await
        .unwrap()
        .unwrap();
    let queued: Vec<_> = (0..16)
        .map(|_| {
            worker
                .submit(|store| store.update_value("cancelled-fixture", |_| Ok((json!(true), ()))))
                .unwrap()
        })
        .collect();
    assert!(worker.submit(|_| Ok(())).is_err());
    drop(queued);
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), active.wait())
        .await
        .unwrap()
        .unwrap();
    store.shutdown_worker().unwrap();
    assert!(worker.submit(|_| Ok(())).is_err());
    assert!(store.value("cancelled-fixture").unwrap().is_none());
}

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
