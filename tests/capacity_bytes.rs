use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn aggregate_json_capacity_rolls_back_new_record_and_new_action_without_data_loss() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let record = Record {
        id: "seed".into(),
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
    // Synthetic journal payload exhausts aggregate bytes in one fixture transaction, not 200 ACL runs.
    store
        .transaction(|tx| {
            tx.execute(
                "INSERT INTO actions(id,state,json) VALUES('capacity','prepared',?1)",
                ["x".repeat(48 * 1024 * 1024)],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
    let mut extra = record;
    extra.id = "extra".into();
    assert_eq!(
        store.save(extra, None).unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(
        actions::prepare(
            &store,
            "note",
            &json!({"target_id":"a","payload":{"body":"Review"}})
        )
        .unwrap_err()
        .code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(
        store.get("local", "note", "seed").unwrap().payload["body"],
        "Keep"
    );
    assert!(store.get("local", "note", "extra").is_err());
}
