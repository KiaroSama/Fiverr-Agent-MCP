use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    service::actions,
    storage::{Root, Store},
};
use rusqlite::params;
use serde_json::{Value, json};

fn workspace() -> (tempfile::TempDir, Store) {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    (temp, store)
}
fn record(id: &str, kind: &str) -> Record {
    Record {
        id: id.into(),
        kind: kind.into(),
        account_id: "local".into(),
        version: 0,
        draft: true,
        payload: json!({"body":"Keep", "due_at":"2020-01-01T00:00:00Z","status":"open"}),
        provenance: Provenance {
            source: "local_draft".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    }
}

#[test]
fn persisted_record_identity_mismatch_fails_closed_without_repair() {
    let (_temp, store) = workspace();
    let saved = store.save(record("a", "note"), None).unwrap();
    store.save(record("sibling", "note"), None).unwrap();
    for (field, value) in [
        ("id", json!("other")),
        ("account_id", json!("foreign")),
        ("kind", json!("task")),
        ("version", json!(99)),
    ] {
        let mut damaged = serde_json::to_value(&saved).unwrap();
        damaged[field] = value;
        let bytes = damaged.to_string();
        store
            .transaction(|tx| {
                tx.execute("UPDATE records SET json=?1 WHERE id='a'", [&bytes])
                    .unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.get("local", "note", "a").unwrap_err().code,
            ErrorCode::StorageUnavailable,
            "get {field}"
        );
        assert_eq!(
            store.list("local", "note", 0, 100).unwrap_err().code,
            ErrorCode::StorageUnavailable,
            "list {field}"
        );
        assert_eq!(
            store
                .page("local", "note", None, 100, false, None)
                .unwrap_err()
                .code,
            ErrorCode::StorageUnavailable,
            "page {field}"
        );
        assert_eq!(
            store.save(saved.clone(), Some(1)).unwrap_err().code,
            ErrorCode::StorageUnavailable,
            "save {field}"
        );
        assert_eq!(store.get("local", "note", "sibling").unwrap().id, "sibling");
        store
            .transaction(|tx| {
                let current: String = tx
                    .query_row("SELECT json FROM records WHERE id='a'", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(current, bytes);
                let history: i64 = tx
                    .query_row("SELECT count(*) FROM history", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(history, 0);
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn history_and_due_task_reads_bind_authoritative_columns() {
    let (_temp, store) = workspace();
    let first = store.save(record("a", "task"), None).unwrap();
    store.save(first.clone(), Some(1)).unwrap();
    let mut damaged = serde_json::to_value(first).unwrap();
    damaged["version"] = json!(2);
    store
        .transaction(|tx| {
            tx.execute("UPDATE history SET json=?1", [damaged.to_string()])
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(
        store.history("local", "task", "a").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    damaged["account_id"] = json!("foreign");
    store
        .transaction(|tx| {
            tx.execute("UPDATE records SET json=?1", [damaged.to_string()])
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(
        store
            .due_tasks_page("local", None, 100, chrono::Utc::now())
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
}

#[test]
fn persisted_action_identity_and_state_mismatch_never_target_another_row() {
    let (_temp, store) = workspace();
    let prepared = actions::prepare(
        &store,
        "note",
        &json!({"target_id":"a","payload":{"body":"Keep"}}),
    )
    .unwrap();
    let sibling = actions::prepare(
        &store,
        "note",
        &json!({"target_id":"b","payload":{"body":"Keep sibling"}}),
    )
    .unwrap();
    let id = prepared["data"]["id"].as_str().unwrap();
    let sibling_id = sibling["data"]["id"].as_str().unwrap();
    for (field, value) in [("id", json!(sibling_id)), ("state", json!("approved"))] {
        let mut damaged = prepared["data"].clone();
        damaged[field] = value;
        let bytes = damaged.to_string();
        store
            .transaction(|tx| {
                tx.execute("UPDATE actions SET json=?1 WHERE id=?2", params![bytes, id])
                    .unwrap();
                Ok(())
            })
            .unwrap();
        for result in [
            actions::get(&store, id),
            actions::list(&store),
            actions::approve(&store, id, true),
            actions::cancel(&store, id),
            actions::commit(&store, id),
        ] {
            assert_eq!(
                result.unwrap_err().code,
                ErrorCode::StorageUnavailable,
                "{field}"
            );
        }
        assert_eq!(
            actions::get(&store, sibling_id).unwrap()["data"]["state"],
            "prepared"
        );
        store
            .transaction(|tx| {
                let current: String = tx
                    .query_row("SELECT json FROM actions WHERE id=?1", [id], |r| r.get(0))
                    .unwrap();
                assert_eq!(serde_json::from_str::<Value>(&current).unwrap(), damaged);
                Ok(())
            })
            .unwrap();
    }
}
