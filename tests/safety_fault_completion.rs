use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;

#[test]
fn financial_plan_is_persisted_as_unapproved_and_never_remote_success() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let plan = actions::prepare(&store, "offer", &json!({
        "target_id":"buyer",
        "payload":{"scope":"Review","recipient_id":"buyer","money":{"minor":100,"currency":"USD","scale":2},"delivery_days":1}
    })).unwrap();
    let id = plan["data"]["id"].as_str().unwrap();
    assert_eq!(plan["data"]["effect"], "financial_or_contractual");
    assert_eq!(plan["data"]["state"], "prepared");
    assert_eq!(plan["provenance"]["remote_executed"], false);
    assert_eq!(actions::get(&store, id).unwrap()["data"], plan["data"]);
}

#[test]
fn retention_delete_failure_rolls_back_current_record_and_new_revision() {
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
    let mut saved = store.save(record, None).unwrap();
    // One transaction builds the minimal 100-revision boundary; no repeated filesystem setup.
    store.transaction(|tx| {
        for version in 1..=100 {
            let mut historical = saved.clone();
            historical.version = version;
            tx.execute("INSERT INTO history(account,kind,id,version,json) VALUES('local','note','a',?1,?2)", rusqlite::params![version, serde_json::to_string(&historical).unwrap()]).unwrap();
        }
        saved.version = 101;
        tx.execute("UPDATE records SET version=101,json=?1", [serde_json::to_string(&saved).unwrap()]).unwrap();
        tx.execute_batch("CREATE TRIGGER refuse_retention BEFORE DELETE ON history BEGIN SELECT RAISE(ABORT,'controlled retention failure'); END;").unwrap();
        Ok(())
    }).unwrap();
    let before = store.history("local", "note", "a").unwrap();
    let mut changed = saved.clone();
    changed.payload = json!({"body":"New"});
    assert_eq!(
        store.save(changed, Some(101)).unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(store.get("local", "note", "a").unwrap().version, 101);
    assert_eq!(
        store.get("local", "note", "a").unwrap().payload["body"],
        "Keep"
    );
    let after = store.history("local", "note", "a").unwrap();
    assert_eq!(
        serde_json::to_value(after).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    store
        .transaction(|tx| {
            tx.execute_batch("DROP TRIGGER refuse_retention;").unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(store.save(saved, Some(101)).unwrap().version, 102);
    assert_eq!(store.history("local", "note", "a").unwrap().len(), 100);
}

#[cfg(unix)]
#[test]
fn linked_restore_staging_is_refused_before_writing_outside_root() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    store.backup("empty.sqlite3").unwrap();
    let staging = store.root.path().join(".cache/tmp");
    let outside = tempfile::tempdir().unwrap();
    std::fs::remove_dir(&staging).unwrap();
    std::os::unix::fs::symlink(outside.path(), &staging).unwrap();
    let result = store.validate_backup("empty.sqlite3");
    std::fs::remove_file(&staging).unwrap();
    std::fs::create_dir(&staging).unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::FilePolicyViolation);
    assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    assert!(store.list("local", "note", 0, 1).unwrap().is_empty());
}
