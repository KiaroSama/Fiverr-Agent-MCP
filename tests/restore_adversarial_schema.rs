use fiverr_agent_mcp::{
    core::{Provenance, Record, now},
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn virtual_schema_hidden_shadows_and_integrity_mismatches_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let r = Record {
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
    store.save(r, None).unwrap();
    let path = store.backup("virtual.sqlite3").unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("DROP TABLE records;CREATE VIRTUAL TABLE records USING fts5(account,kind,id,version,json);PRAGMA writable_schema=ON;DELETE FROM sqlite_schema WHERE name LIKE 'records_%';PRAGMA writable_schema=OFF;").unwrap();
    drop(c);
    assert!(store.validate_backup("virtual.sqlite3").is_err());
    let path = store.backup("integrity.sqlite3").unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("PRAGMA writable_schema=ON;UPDATE sqlite_schema SET sql=replace(sql,'version INTEGER NOT NULL','version INTEGER NOT NULL CHECK(version>100)') WHERE name='records';PRAGMA writable_schema=OFF;").unwrap();
    drop(c);
    assert!(store.validate_backup("integrity.sqlite3").is_err());
    assert_eq!(
        store.get("local", "note", "a").unwrap().payload["body"],
        "Keep"
    );
}
#[test]
fn missing_staging_directory_cannot_make_a_valid_backup_appear_restored() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    store.backup("empty.sqlite3").unwrap();
    let staging = store.root.path().join(".cache/tmp");
    std::fs::remove_dir(&staging).unwrap();
    std::fs::write(&staging, b"Owner blocker").unwrap();
    assert!(store.validate_backup("empty.sqlite3").is_err());
    assert_eq!(std::fs::read(staging).unwrap(), b"Owner blocker");
    assert!(store.list("local", "note", 0, 1).unwrap().is_empty());
}
