use fiverr_agent_mcp::{
    core::ErrorCode,
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn virtual_tables_integrity_damage_and_typed_backup_columns_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let p = store.backup("virtual.sqlite3").unwrap();
    let c = rusqlite::Connection::open(&p).unwrap();
    c.execute_batch("DROP TABLE records; CREATE VIRTUAL TABLE records USING fts5(account,kind,id,version,json);").unwrap();
    drop(c);
    assert!(store.validate_backup("virtual.sqlite3").is_err());
    let action = actions::prepare(
        &store,
        "note",
        &json!({"target_id":"a","payload":{"body":"Keep"}}),
    )
    .unwrap();
    let p = store.backup("typed.sqlite3").unwrap();
    let original = std::fs::read(&p).unwrap();
    for sql in [
        "UPDATE actions SET id=NULL;",
        "UPDATE actions SET state=NULL;",
        "UPDATE actions SET json=NULL;",
    ] {
        // Rebuild the ordinary table without NOT NULL solely to model a hostile backup schema.
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch("ALTER TABLE actions RENAME TO saved;CREATE TABLE actions(id TEXT,state TEXT,json TEXT);INSERT INTO actions SELECT * FROM saved;DROP TABLE saved;").unwrap();
        c.execute_batch(sql).unwrap();
        drop(c);
        assert!(store.validate_backup("typed.sqlite3").is_err());
        std::fs::write(&p, &original).unwrap();
    }
    let c = rusqlite::Connection::open(&p).unwrap();
    let page_size: i64 = c
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .unwrap();
    let page: i64 = c
        .query_row(
            "SELECT rootpage FROM sqlite_schema WHERE name='actions'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(c);
    let mut bytes = original.clone();
    bytes[((page - 1) * page_size) as usize] = 0xff;
    std::fs::write(&p, bytes).unwrap();
    assert!(store.validate_backup("typed.sqlite3").is_err());
    std::fs::write(&p, &original).unwrap();
    assert_eq!(
        actions::get(&store, action["data"]["id"].as_str().unwrap()).unwrap()["data"]["state"],
        "prepared"
    );
    assert_eq!(
        store.validate_backup("missing.sqlite3").unwrap_err().code,
        ErrorCode::ResourceNotFound
    );
}
#[test]
fn missing_destination_tables_and_invalid_settings_cannot_commit_restore() {
    let temp = tempfile::tempdir().unwrap();
    let source =
        Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let path = source.backup("source.sqlite3").unwrap();
    let bytes = std::fs::read(path).unwrap();
    let other = tempfile::tempdir().unwrap();
    let dest = Store::open(Root::explicit(&other.path().canonicalize().unwrap()).unwrap()).unwrap();
    dest.root
        .write_new(
            &dest.root.safe_path("exports", "source.sqlite3").unwrap(),
            &bytes,
        )
        .unwrap();
    dest.update_value("settings", |_| Ok((json!({"invalid":true}), ())))
        .unwrap();
    assert_eq!(
        dest.restore_backup("source.sqlite3").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    dest.transaction(|tx| {
        tx.execute_batch("DELETE FROM kv;ALTER TABLE actions RENAME TO hidden;")
            .unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        dest.restore_backup("source.sqlite3").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert!(dest.list("local", "note", 0, 100).unwrap().is_empty());
}
