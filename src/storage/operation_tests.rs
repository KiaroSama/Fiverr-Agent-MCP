use super::*;
use crate::storage::Root;

fn workspace() -> (tempfile::TempDir, Store) {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    (temp, store)
}

#[tokio::test]
async fn started_write_returns_pending_then_a_private_durable_receipt() {
    let (_temp, store) = workspace();
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let owner = store.clone();
    let request = tokio::spawn(async move {
        owner.dispatch_write("fiverr_record_save".into(), move |store| {
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(8)).unwrap();
            crate::service::dispatch(store, "fiverr_record_save", json!({"kind":"note","id":"receipt-fixture","payload":{"body":"private fixture body"}}))
        }, Duration::from_secs(2)).await
    });
    tokio::time::timeout(Duration::from_secs(3), ready)
        .await
        .unwrap()
        .unwrap();
    let pending = tokio::time::timeout(Duration::from_secs(3), request)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(pending["data"]["state"], "pending");
    let id = pending["data"]["operation_id"].as_str().unwrap();
    assert_eq!(store.operation(id).unwrap()["data"]["state"], "executing");
    release.send(()).unwrap();
    let receipt = store
        .dispatch("fiverr_status".into(), json!({"operation_id":id}))
        .await
        .unwrap();
    assert_eq!(receipt["data"]["state"], "succeeded");
    assert_eq!(
        receipt["data"]["completion"]["summary"]["id"],
        "receipt-fixture"
    );
    assert!(!receipt.to_string().contains("private fixture body"));
    assert_eq!(
        store
            .get("local", "note", "receipt-fixture")
            .unwrap()
            .version,
        1
    );
    store.shutdown_worker().unwrap();
    let reopened = Store::open(store.root.clone()).unwrap();
    assert_eq!(reopened.operation(id).unwrap(), receipt);
}

#[tokio::test]
async fn unavailable_journal_never_executes_a_write() {
    let (_temp, store) = workspace();
    let path = store
        .root
        .safe_path(".state/audit", "operations.sqlite3")
        .unwrap();
    std::fs::create_dir(&path).unwrap();
    let failure = store
        .dispatch("fiverr_record_save".into(), note("unavailable"))
        .await
        .unwrap_err();
    assert_eq!(failure.code, ErrorCode::StorageUnavailable);
    assert_eq!(
        store.get("local", "note", "unavailable").unwrap_err().code,
        ErrorCode::ResourceNotFound
    );
    assert!(path.is_dir());
}

#[test]
fn malformed_receipt_is_refused_without_repair() {
    let (_temp, store) = workspace();
    mutation_lock(&store).unwrap();
    let mut connection = journal(&store).unwrap();
    begin(&mut connection, "malformed", "local", "fiverr_record_save").unwrap();
    for (state, result) in [("invalid", "{}"), ("succeeded", "{")] {
        connection
            .execute(
                "UPDATE operations SET state=?1,result=?2 WHERE id='malformed'",
                params![state, result],
            )
            .unwrap();
        assert_eq!(
            store.operation("malformed").unwrap_err().code,
            ErrorCode::StorageUnavailable
        );
        let actual: (String, String) = connection
            .query_row(
                "SELECT state,result FROM operations WHERE id='malformed'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(actual, (state.into(), result.into()));
    }
}

#[tokio::test]
async fn receipt_dispatch_validates_before_creating_a_journal() {
    let (_temp, store) = workspace();
    let input = json!({"kind":"note","payload":{"body":"x".repeat(1024 * 1024)}});
    assert_eq!(
        store
            .dispatch("fiverr_record_save".into(), input)
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        store
            .dispatch("fiverr_status".into(), json!({"operation_id":false}))
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert!(
        !store
            .root
            .safe_path(".state/audit", "operations.sqlite3")
            .unwrap()
            .exists()
    );
}

#[tokio::test]
async fn refused_receipt_insert_prevents_local_effect() {
    let (_temp, store) = workspace();
    mutation_lock(&store).unwrap();
    let connection = journal(&store).unwrap();
    connection.execute_batch("CREATE TRIGGER refuse_admission BEFORE INSERT ON operations BEGIN SELECT RAISE(ABORT,'controlled admission failure'); END;").unwrap();
    assert_eq!(
        store
            .dispatch("fiverr_record_save".into(), note("refused"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(
        store.get("local", "note", "refused").unwrap_err().code,
        ErrorCode::ResourceNotFound
    );
    let count: u32 = connection
        .query_row("SELECT count(*) FROM operations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn unreadable_receipt_inputs_fail_closed() {
    let (_temp, store) = workspace();
    let lock_path = store
        .root
        .safe_path(".state/audit", "operations.lock")
        .unwrap();
    std::fs::create_dir(&lock_path).unwrap();
    assert_eq!(
        store
            .dispatch("fiverr_record_save".into(), note("locked"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
    assert!(store.get("local", "note", "locked").is_err());
    std::fs::remove_dir(&lock_path).unwrap();
    let connection = journal(&store).unwrap();
    connection.execute_batch("DROP TABLE operations;").unwrap();
    assert_eq!(
        store.operation("missing-lock").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    mutation_lock(&store).unwrap();
    assert_eq!(
        store.operation("missing-table").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
}

#[tokio::test]
async fn recovery_failure_prevents_admission() {
    let (_temp, store) = workspace();
    mutation_lock(&store).unwrap();
    let mut connection = journal(&store).unwrap();
    begin(&mut connection, "abandoned", "local", "fiverr_record_save").unwrap();
    connection.execute_batch("CREATE TRIGGER refuse_recovery BEFORE UPDATE OF state ON operations BEGIN SELECT RAISE(ABORT,'controlled recovery failure'); END;").unwrap();
    assert_eq!(
        store
            .dispatch("fiverr_record_save".into(), note("not-recovered"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
    assert!(store.get("local", "note", "not-recovered").is_err());
    let state: String = connection
        .query_row(
            "SELECT state FROM operations WHERE id='abandoned'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "executing");
}

#[tokio::test]
async fn shutdown_finishes_started_work_without_running_queued_work() {
    let (_temp, store) = workspace();
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let worker = store.worker().unwrap();
    let started = worker
        .submit(move |store| {
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            store.update_value("shutdown-started", |_| Ok((json!(true), ())))
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), ready)
        .await
        .unwrap()
        .unwrap();
    let queued = worker
        .submit(|store| store.update_value("shutdown-queued", |_| Ok((json!(true), ()))))
        .unwrap();
    worker.close();
    assert!(worker.submit(|_| Ok(())).is_err());
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), started.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), queued.wait())
            .await
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
    store.shutdown_worker().unwrap();
    assert_eq!(store.value("shutdown-started").unwrap(), Some(json!(true)));
    assert!(store.value("shutdown-queued").unwrap().is_none());
}

#[test]
fn completion_cannot_overwrite_a_terminal_receipt() {
    let (_temp, store) = workspace();
    let mut connection = journal(&store).unwrap();
    begin(&mut connection, "terminal", "local", "fiverr_record_save").unwrap();
    complete(
        &connection,
        "terminal",
        &Ok(json!({"data":{"id":"original"}})),
    )
    .unwrap();
    let error = complete(&connection, "terminal", &Err(not_started())).unwrap_err();
    assert_eq!(error.code, ErrorCode::StorageUnavailable);
    let row: (String, String) = connection
        .query_row(
            "SELECT state,result FROM operations WHERE id='terminal'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row.0, "succeeded");
    assert_eq!(
        serde_json::from_str::<Value>(&row.1).unwrap()["summary"]["id"],
        "original"
    );
}

fn note(id: &str) -> Value {
    json!({"kind":"note","id":id,"payload":{"body":"synthetic note"}})
}

#[tokio::test]
async fn receipt_admission_refuses_competing_writer_and_full_journal() {
    let (_temp, store) = workspace();
    let lock = mutation_lock(&store).unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    let failure = store
        .dispatch("fiverr_record_save".into(), note("contended"))
        .await
        .unwrap_err();
    assert_eq!(failure.code, ErrorCode::StorageUnavailable);
    assert!(failure.message.contains("Another process"));
    drop(lock);
    let connection = journal(&store).unwrap();
    connection.execute_batch("WITH RECURSIVE fixture(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM fixture WHERE n<10000) INSERT INTO operations(id,account,tool,state,created) SELECT 'fixture-'||n,'local','fiverr_record_save','outcome_unknown','fixture' FROM fixture;").unwrap();
    let failure = store
        .dispatch("fiverr_record_save".into(), note("at-capacity"))
        .await
        .unwrap_err();
    assert_eq!(failure.code, ErrorCode::StorageUnavailable);
    assert!(failure.message.contains("capacity"));
    for id in ["contended", "at-capacity"] {
        assert_eq!(
            store.get("local", "note", id).unwrap_err().code,
            ErrorCode::ResourceNotFound
        );
    }
    let count: u32 = connection
        .query_row(
            "SELECT count(*) FROM operations WHERE state='outcome_unknown'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 10000);
}

#[tokio::test]
async fn completion_failure_preserves_unknown_and_does_not_undo_the_write() {
    let (_temp, store) = workspace();
    mutation_lock(&store).unwrap();
    let connection = journal(&store).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_completion BEFORE UPDATE OF result ON operations BEGIN SELECT RAISE(ABORT,'controlled completion failure'); END;").unwrap();
    let result = store
        .dispatch(
            "fiverr_record_save".into(),
            note("saved-before-receipt-failure"),
        )
        .await
        .unwrap();
    assert_eq!(result["data"]["state"], "outcome_unknown");
    let id = result["data"]["operation_id"].as_str().unwrap();
    assert_eq!(
        store.operation(id).unwrap()["data"]["state"],
        "outcome_unknown"
    );
    assert_eq!(
        store
            .get("local", "note", "saved-before-receipt-failure")
            .unwrap()
            .version,
        1
    );
    connection
        .execute_batch("DROP TRIGGER fail_completion;")
        .unwrap();
    let failure = store
        .dispatch(
            "fiverr_record_save".into(),
            json!({"kind":"note","id":"invalid","payload":{}}),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, ErrorCode::InvalidInput);
    let failed_id: String = connection
        .query_row("SELECT id FROM operations WHERE state='failed'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let failed = store.operation(&failed_id).unwrap();
    assert_eq!(
        failed["data"]["completion"]["error"]["code"],
        "INVALID_INPUT"
    );
    assert_eq!(
        store
            .get("local", "note", "saved-before-receipt-failure")
            .unwrap()
            .version,
        1
    );
}

#[tokio::test]
async fn queued_timeout_reports_no_operation_and_missing_receipt_does_not_create_one() {
    let (_temp, store) = workspace();
    assert_eq!(
        store.operation("absent").unwrap_err().code,
        ErrorCode::ResourceNotFound
    );
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let worker = store.worker().unwrap();
    let active = worker
        .submit(move |_| {
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        })
        .unwrap();
    ready.await.unwrap();
    let result = store
        .dispatch_write(
            "fiverr_record_save".into(),
            |store| crate::service::dispatch(store, "fiverr_record_save", note("expired")),
            Duration::ZERO,
        )
        .await;
    assert!(
        result
            .unwrap_err()
            .message
            .contains("no operation executed")
    );
    release.send(()).unwrap();
    active.wait().await.unwrap();
    store.shutdown_worker().unwrap();
    assert!(store.get("local", "note", "expired").is_err());
    assert!(
        !store
            .root
            .safe_path(".state/audit", "operations.sqlite3")
            .unwrap()
            .exists()
    );
}

#[tokio::test]
async fn abandoned_receipt_reopens_unknown_without_replay_or_account_leak() {
    let (_temp, store) = workspace();
    let root = store.root.clone();
    {
        let lock = mutation_lock(&store).unwrap();
        fs2::FileExt::try_lock_exclusive(&lock).unwrap();
        let mut connection = journal(&store).unwrap();
        begin(
            &mut connection,
            "unfinished-fixture",
            "local",
            "fiverr_record_save",
        )
        .unwrap();
    }
    drop(store);
    let store = Store::open(root).unwrap();
    let value = store
        .dispatch(
            "fiverr_status".into(),
            json!({"operation_id":"unfinished-fixture"}),
        )
        .await
        .unwrap();
    assert_eq!(value["data"]["state"], "outcome_unknown");
    assert_eq!(value["data"]["automatic_replay"], false);
    assert!(store.get("local", "note", "unfinished-fixture").is_err());
    crate::network::Settings::change(&store, true, |settings| {
        settings.account_id = "other".into();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        store.operation("unfinished-fixture").unwrap_err().code,
        ErrorCode::ResourceNotFound
    );
}
