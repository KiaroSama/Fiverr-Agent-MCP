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
