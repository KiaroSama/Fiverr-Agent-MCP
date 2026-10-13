use fiverr_agent_mcp::{
    core::ErrorCode,
    network::{Mode, Settings},
    service::{self, actions},
    storage::{Root, Store},
};
use serde_json::json;

fn workspace() -> (tempfile::TempDir, Store) {
    let temporary = tempfile::tempdir().unwrap();
    let store =
        Store::open(Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap()).unwrap();
    (temporary, store)
}

#[test]
fn empty_restore_keeps_records_history_and_blocks_old_authority() {
    let (_source_root, source) = workspace();
    let saved = service::dispatch(
        &source,
        "fiverr_record_save",
        json!({"kind":"note","id":"a","payload":{"body":"Original فارسی"}}),
    )
    .unwrap();
    service::dispatch(&source, "fiverr_record_save", json!({"kind":"note","id":"a","expected_version":saved["data"]["version"],"payload":{"body":"Updated"}})).unwrap();
    Settings::change(&source, true, |s| {
        s.mode = Mode::Direct;
        Ok(())
    })
    .unwrap();
    source
        .update_value("http_token", |_| Ok((json!("private-test-token"), ())))
        .unwrap();
    let prepared = actions::prepare(
        &source,
        "message",
        &json!({"target_id":"buyer1","payload":{"recipient_id":"buyer1","body":"Never send this"}}),
    )
    .unwrap();
    let id = prepared["data"]["id"].as_str().unwrap();
    actions::approve(&source, id, true).unwrap();
    let backup = source.backup("sample.sqlite3").unwrap();
    assert_eq!(
        source.validate_backup("sample.sqlite3").unwrap()["valid"],
        true
    );
    let (_destination_root, destination) = workspace();
    destination
        .root
        .write_new(
            &destination
                .root
                .safe_path("exports", "sample.sqlite3")
                .unwrap(),
            &source.root.read_bounded(&backup, 64 * 1024 * 1024).unwrap(),
        )
        .unwrap();
    assert_eq!(
        destination
            .restore_backup_checked("sample.sqlite3", Some("changed-after-preview"))
            .unwrap_err()
            .code,
        ErrorCode::ActionStateChanged
    );
    assert!(
        destination
            .list("local", "note", 0, 100)
            .unwrap()
            .is_empty()
    );
    destination.restore_backup("sample.sqlite3").unwrap();
    assert_eq!(
        destination.get("local", "note", "a").unwrap().payload["body"],
        "Updated"
    );
    assert_eq!(
        destination.history("local", "note", "a").unwrap()[0].payload["body"],
        "Original فارسی"
    );
    assert!(Settings::load(&destination).unwrap().mode == Mode::Blocked);
    assert!(destination.value("http_token").unwrap().is_none());
    assert_eq!(
        actions::get(&destination, id).unwrap()["data"]["state"],
        "cancelled"
    );
    assert!(actions::commit(&destination, id).is_err());
    assert_eq!(
        destination
            .restore_backup("sample.sqlite3")
            .unwrap_err()
            .code,
        ErrorCode::ActionStateChanged
    );
    assert_eq!(
        destination.get("local", "note", "a").unwrap().payload["body"],
        "Updated"
    );
}

#[test]
fn exhausted_counters_and_colliding_history_are_rejected_before_restore() {
    let (_temporary, source) = workspace();
    service::dispatch(
        &source,
        "fiverr_record_save",
        json!({"kind":"note","id":"a","payload":{"body":"Keep"}}),
    )
    .unwrap();
    for (name, corruption) in [
        ("counter.sqlite3", "counter"),
        ("history.sqlite3", "history"),
    ] {
        let path = source.backup(name).unwrap();
        let connection = rusqlite::Connection::open(path).unwrap();
        if corruption == "counter" {
            let settings = Settings {
                revision: u64::MAX - 1,
                ..Settings::default()
            };
            connection
                .execute(
                    "INSERT INTO kv(key,value) VALUES('settings',?1)",
                    [serde_json::to_string(&settings).unwrap()],
                )
                .unwrap();
        } else {
            connection
                .execute_batch("INSERT INTO history SELECT * FROM records;")
                .unwrap();
        }
        drop(connection);
        assert!(source.validate_backup(name).is_err());
    }
    source
        .update_value("settings", |_| {
            Ok((
                serde_json::to_value(Settings {
                    revision: u64::MAX,
                    ..Settings::default()
                })
                .unwrap(),
                (),
            ))
        })
        .unwrap();
    assert!(Settings::change(&source, false, |_| Ok(())).is_err());
    assert_eq!(
        source.get("local", "note", "a").unwrap().payload["body"],
        "Keep"
    );
}

#[test]
fn malformed_and_foreign_backups_never_change_current_workspace() {
    let (_temporary, store) = workspace();
    for (name, bytes) in [
        ("bad.sqlite3", b"SQLite format 3\0not a database".as_slice()),
        ("text.sqlite3", b"not sqlite".as_slice()),
    ] {
        store
            .root
            .write_new(&store.root.safe_path("exports", name).unwrap(), bytes)
            .unwrap();
        assert!(store.validate_backup(name).is_err());
        assert!(store.restore_backup(name).is_err());
    }
    let foreign = store.root.safe_path("exports", "foreign.sqlite3").unwrap();
    store.root.write_new(&foreign, b"").unwrap();
    let connection = rusqlite::Connection::open(&foreign).unwrap();
    connection
        .execute_batch("CREATE TABLE records(x); PRAGMA user_version=1;")
        .unwrap();
    drop(connection);
    assert!(store.restore_backup("foreign.sqlite3").is_err());
    assert!(store.list("local", "note", 0, 100).unwrap().is_empty());
    assert!(store.value("settings").unwrap().is_none());
    assert!(store.restore_backup("../outside.sqlite3").is_err());
}
