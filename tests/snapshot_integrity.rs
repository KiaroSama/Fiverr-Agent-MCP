use fiverr_agent_mcp::{
    core::{ErrorCode, Record},
    service,
    storage::{Root, Store},
};
use serde_json::{Value, json};

fn workspace() -> (tempfile::TempDir, Store, Value) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let store = Store::open(Root::explicit(&root).unwrap()).unwrap();
    service::dispatch(
        &store,
        "fiverr_record_save",
        json!({"kind":"note","id":"source","payload":{"body":"Original evidence"}}),
    )
    .unwrap();
    let snapshot = service::dispatch(
        &store,
        "fiverr_snapshot_create",
        json!({"kind":"note","id":"source"}),
    )
    .unwrap()["data"]
        .clone();
    (directory, store, snapshot)
}

#[test]
fn snapshot_overwrite_fails_at_service_and_storage_boundaries() {
    let (_directory, store, snapshot) = workspace();
    let id = snapshot["id"].as_str().unwrap();
    let result = service::dispatch(
        &store,
        "fiverr_record_save",
        json!({"kind":"snapshot","id":id,"expected_version":1,"payload":{"replacement":true}}),
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::ActionStateChanged);
    let mut replacement: Record = serde_json::from_value(snapshot.clone()).unwrap();
    replacement.payload = json!({"replacement":true});
    assert_eq!(
        store.save(replacement, Some(1)).unwrap_err().code,
        ErrorCode::ActionStateChanged
    );
    let stored = store.get("local", "snapshot", id).unwrap();
    assert_eq!(serde_json::to_value(stored).unwrap(), snapshot);
    assert!(store.history("local", "snapshot", id).unwrap().is_empty());
}

#[test]
fn source_edits_and_new_snapshots_preserve_the_original_capture() {
    let (_directory, store, original) = workspace();
    let edited = service::dispatch(
        &store,
        "fiverr_record_save",
        json!({"kind":"note","id":"source","expected_version":1,"payload":{"body":"New evidence"}}),
    )
    .unwrap();
    assert_eq!(edited["data"]["version"], 2);
    let newer = service::dispatch(
        &store,
        "fiverr_snapshot_create",
        json!({"kind":"note","id":"source"}),
    )
    .unwrap();
    assert_ne!(original["id"], newer["data"]["id"]);
    assert_eq!(newer["data"]["payload"]["record"]["version"], 2);
    let id = original["id"].as_str().unwrap();
    let stored = store.get("local", "snapshot", id).unwrap();
    assert_eq!(serde_json::to_value(stored).unwrap(), original);
    assert_eq!(store.history("local", "note", "source").unwrap().len(), 1);
}
