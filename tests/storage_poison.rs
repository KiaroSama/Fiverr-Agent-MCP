use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn interrupted_transaction_poison_is_a_visible_storage_error_never_a_reset() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let record = Record {
        id: "a".into(),
        kind: "note".into(),
        account_id: "local".into(),
        version: 0,
        draft: true,
        payload: json!({"body":"Keep"}),
        provenance: Provenance {
            source: "local_draft".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    };
    store.save(record.clone(), None).unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: fiverr_agent_mcp::core::Result<()> = store.transaction(|tx| {
            tx.execute_batch("UPDATE records SET json='{}';").unwrap();
            panic!("Controlled interrupted transaction");
        });
    }));
    assert!(failure.is_err());
    assert_eq!(
        store.value("settings").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(
        store.get("local", "note", "a").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert!(store.list("local", "note", 0, 100).is_err());
    assert!(store.page("local", "note", None, 1, false, None).is_err());
    assert!(
        store
            .due_tasks_page("local", None, 1, chrono::Utc::now())
            .is_err()
    );
    assert!(store.save(record, Some(1)).is_err());
    assert!(store.update_value("x", |_| Ok((json!(1), ()))).is_err());
    assert!(store.backup("failed.sqlite3").is_err());
    assert!(actions::list(&store).is_err());
    let reopened = Store::open(store.root.clone()).unwrap();
    assert_eq!(
        reopened.get("local", "note", "a").unwrap().payload["body"],
        "Keep"
    );
    assert_eq!(reopened.get("local", "note", "a").unwrap().version, 1);
}
