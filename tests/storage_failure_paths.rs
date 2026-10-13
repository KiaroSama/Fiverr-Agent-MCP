use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    network::Settings,
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;
fn record() -> Record {
    Record {
        id: "a".into(),
        kind: "note".into(),
        account_id: "local".into(),
        version: 0,
        draft: true,
        payload: json!({"body":"Preserved"}),
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
fn missing_tables_return_storage_errors_without_destroying_other_data() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let r = store.save(record(), None).unwrap();
    store.save(r.clone(), Some(1)).unwrap();
    for table in ["records", "history", "kv", "actions"] {
        store
            .transaction(|tx| {
                tx.execute_batch(&format!("ALTER TABLE {table} RENAME TO unavailable;"))
                    .unwrap();
                Ok(())
            })
            .unwrap();
        match table {
            "records" => {
                assert_eq!(
                    store.save(r.clone(), Some(2)).unwrap_err().code,
                    ErrorCode::StorageUnavailable
                );
                assert_eq!(
                    store
                        .due_tasks_page("local", None, 1, chrono::Utc::now())
                        .unwrap_err()
                        .code,
                    ErrorCode::StorageUnavailable
                );
            }
            "history" => {
                assert_eq!(
                    store.history("local", "note", "a").unwrap_err().code,
                    ErrorCode::StorageUnavailable
                );
                assert_eq!(
                    store.save(r.clone(), Some(2)).unwrap_err().code,
                    ErrorCode::StorageUnavailable
                );
            }
            "kv" => {
                assert_eq!(
                    store.value("settings").unwrap_err().code,
                    ErrorCode::StorageUnavailable
                );
                assert_eq!(
                    store
                        .update_value("settings", |_| Ok((json!(1), ())))
                        .unwrap_err()
                        .code,
                    ErrorCode::StorageUnavailable
                );
                assert!(Settings::change(&store, false, |_| Ok(())).is_err());
                assert!(actions::list(&store).is_err());
            }
            _ => {
                assert_eq!(
                    actions::list(&store).unwrap_err().code,
                    ErrorCode::StorageUnavailable
                );
                assert_eq!(
                    actions::get(&store, "a").unwrap_err().code,
                    ErrorCode::StorageUnavailable
                );
                assert_eq!(
                    actions::prepare(
                        &store,
                        "note",
                        &json!({"target_id":"a","payload":{"body":"Keep"}})
                    )
                    .unwrap_err()
                    .code,
                    ErrorCode::StorageUnavailable
                );
            }
        }
        store
            .transaction(|tx| {
                tx.execute_batch(&format!("ALTER TABLE unavailable RENAME TO {table};"))
                    .unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.get("local", "note", "a").unwrap().payload["body"],
            "Preserved"
        );
    }
    for limit in [0, 101] {
        assert_eq!(
            store
                .due_tasks_page("local", None, limit, chrono::Utc::now())
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
}
#[test]
fn backup_restore_transaction_failures_keep_destination_empty_and_source_intact() {
    let temp = tempfile::tempdir().unwrap();
    let source =
        Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    source.save(record(), None).unwrap();
    source.save(record(), Some(1)).unwrap();
    let action = actions::prepare(
        &source,
        "note",
        &json!({"target_id":"a","payload":{"body":"Terminal"}}),
    )
    .unwrap();
    let id = action["data"]["id"].as_str().unwrap();
    actions::cancel(&source, id).unwrap();
    let path = source.backup("source.sqlite3").unwrap();
    let bytes = std::fs::read(path).unwrap();
    let other = tempfile::tempdir().unwrap();
    let destination =
        Store::open(Root::explicit(&other.path().canonicalize().unwrap()).unwrap()).unwrap();
    let candidate = destination
        .root
        .safe_path("exports", "source.sqlite3")
        .unwrap();
    destination.root.write_new(&candidate, &bytes).unwrap();
    for (table, event) in [
        ("records", "INSERT"),
        ("history", "INSERT"),
        ("actions", "INSERT"),
        ("kv", "INSERT"),
    ] {
        destination.transaction(|tx|{tx.execute_batch(&format!("CREATE TRIGGER refuse BEFORE {event} ON {table} BEGIN SELECT RAISE(ABORT,'controlled'); END;")).unwrap();Ok(())}).unwrap();
        assert_eq!(
            destination
                .restore_backup("source.sqlite3")
                .unwrap_err()
                .code,
            ErrorCode::StorageUnavailable
        );
        assert!(
            destination
                .list("local", "note", 0, 100)
                .unwrap()
                .is_empty()
        );
        assert!(destination.value("settings").unwrap().is_none());
        destination
            .transaction(|tx| {
                tx.execute_batch("DROP TRIGGER refuse;").unwrap();
                Ok(())
            })
            .unwrap();
    }
    destination.restore_backup("source.sqlite3").unwrap();
    assert_eq!(
        actions::get(&destination, id).unwrap()["data"]["state"],
        "cancelled"
    );
    assert_eq!(source.get("local", "note", "a").unwrap().version, 2);
}
#[test]
fn typed_sql_row_and_settings_corruption_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    store.save(record(), None).unwrap();
    store
        .transaction(|tx| {
            tx.execute_batch("UPDATE records SET json=123;").unwrap();
            Ok(())
        })
        .unwrap();
    assert!(store.get("local", "note", "a").is_err());
    assert!(store.page("local", "note", None, 1, false, None).is_err());
    store
        .transaction(|tx| {
            tx.execute_batch("INSERT INTO kv VALUES('settings','not-json');")
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert!(Settings::load(&store).is_err());
    assert!(
        store
            .transaction(|tx| Settings::in_transaction(tx))
            .is_err()
    );
}
