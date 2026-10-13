use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    network::{Mode, RouteClient, Settings},
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;

#[test]
fn backup_action_identifiers_are_validated_before_any_destination_write() {
    let temp = tempfile::tempdir().unwrap();
    let source =
        Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let action = actions::prepare(
        &source,
        "note",
        &json!({"target_id":"a","payload":{"body":"Keep"}}),
    )
    .unwrap();
    let id = action["data"]["id"].as_str().unwrap();
    let path = source.backup("journal.sqlite3").unwrap();
    let original = std::fs::read(&path).unwrap();
    let other = tempfile::tempdir().unwrap();
    let destination =
        Store::open(Root::explicit(&other.path().canonicalize().unwrap()).unwrap()).unwrap();
    let candidate = destination
        .root
        .safe_path("exports", "journal.sqlite3")
        .unwrap();
    destination.root.write_new(&candidate, &original).unwrap();
    for (field, value) in [
        ("id", "bad/id"),
        ("account_id", "bad/id"),
        ("target_id", "bad/id"),
        ("kind", "unsupported"),
    ] {
        let mut bad = action["data"].clone();
        bad[field] = json!(value);
        let connection = rusqlite::Connection::open(&candidate).unwrap();
        connection
            .execute("UPDATE actions SET json=?1", [bad.to_string()])
            .unwrap();
        drop(connection);
        assert_eq!(
            destination
                .restore_backup("journal.sqlite3")
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput,
            "{field}"
        );
        assert!(
            actions::list(&destination).unwrap()["data"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(destination.list("local", "note", 0, 1).unwrap().is_empty());
        assert!(destination.value("settings").unwrap().is_none());
        assert!(
            std::fs::read_dir(destination.root.path().join(".cache/tmp"))
                .unwrap()
                .next()
                .is_none()
        );
        std::fs::write(&candidate, &original).unwrap();
    }
    destination.restore_backup("journal.sqlite3").unwrap();
    assert_eq!(
        actions::get(&destination, id).unwrap()["data"]["state"],
        "cancelled"
    );
    assert_eq!(
        actions::get(&source, id).unwrap()["data"]["state"],
        "prepared"
    );
}

#[test]
fn mistyped_existing_record_refuses_optimistic_save_without_erasing_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let saved = store
        .save(
            Record {
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
            },
            None,
        )
        .unwrap();
    let original = serde_json::to_string(&saved).unwrap();
    for damaged in [vec![0xff_u8], b"{}".to_vec()] {
        store
            .transaction(|tx| {
                tx.execute("UPDATE records SET json=?1", [&damaged])
                    .unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.save(saved.clone(), Some(1)).unwrap_err().code,
            ErrorCode::StorageUnavailable
        );
        assert_eq!(
            store.history("local", "note", "a").unwrap_err().code,
            ErrorCode::StorageUnavailable
        );
        store
            .transaction(|tx| {
                let bytes: Vec<u8> = tx
                    .query_row("SELECT json FROM records", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(bytes, damaged);
                tx.execute("UPDATE records SET json=?1", [&original])
                    .unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.get("local", "note", "a").unwrap().payload["body"],
            "Keep"
        );
        assert!(store.history("local", "note", "a").unwrap().is_empty());
    }
}

#[test]
fn corrupt_route_settings_refuse_existing_and_new_clients_without_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    Settings::change(&store, true, |settings| {
        settings.mode = Mode::Direct;
        Ok(())
    })
    .unwrap();
    let route = RouteClient::new(store.clone()).unwrap();
    store
        .update_value("settings", |_| Ok((json!({"corrupt":true}), ())))
        .unwrap();
    assert_eq!(
        route.check().unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    match RouteClient::new(store.clone()) {
        Ok(_) => panic!("Corrupt settings unexpectedly created a routed client"),
        Err(error) => assert_eq!(error.code, ErrorCode::StorageUnavailable),
    }
    assert_eq!(
        store.value("settings").unwrap(),
        Some(json!({"corrupt":true}))
    );
}
