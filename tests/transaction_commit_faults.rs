use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;

#[test]
fn deferred_commit_failure_rolls_back_value_and_store_remains_usable() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    store.transaction(|tx| {
        tx.execute_batch("CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(parent_id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED);").unwrap();
        Ok(())
    }).unwrap();
    let result = store.transaction(|tx| {
        tx.execute("INSERT INTO child VALUES(7)", []).unwrap();
        tx.execute("INSERT INTO kv VALUES('commit_fixture','true')", [])
            .unwrap();
        Ok(())
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::StorageUnavailable);
    assert!(store.value("commit_fixture").unwrap().is_none());
    store
        .transaction(|tx| {
            let children: i64 = tx
                .query_row("SELECT count(*) FROM child", [], |row| row.get(0))
                .unwrap();
            assert_eq!(children, 0);
            tx.execute("INSERT INTO parent VALUES(7)", []).unwrap();
            tx.execute("INSERT INTO child VALUES(7)", []).unwrap();
            Ok(())
        })
        .unwrap();
    store
        .update_value("commit_fixture", |_| Ok((json!(true), ())))
        .unwrap();
    assert_eq!(store.value("commit_fixture").unwrap(), Some(json!(true)));
}

#[test]
fn each_restore_insert_and_settings_failure_rolls_back_the_entire_snapshot() {
    let source_dir = tempfile::tempdir().unwrap();
    let source =
        Store::open(Root::explicit(&source_dir.path().canonicalize().unwrap()).unwrap()).unwrap();
    let mut record = source
        .save(
            Record {
                id: "a".into(),
                kind: "note".into(),
                account_id: "local".into(),
                version: 0,
                draft: true,
                payload: json!({"body":"Original"}),
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
    record.payload = json!({"body":"Current"});
    source.save(record, Some(1)).unwrap();
    actions::prepare(
        &source,
        "note",
        &json!({"target_id":"a","payload":{"body":"Prepared"}}),
    )
    .unwrap();
    let backup = std::fs::read(source.backup("fixture.sqlite3").unwrap()).unwrap();
    for table in ["records", "history", "actions", "kv"] {
        let destination_dir = tempfile::tempdir().unwrap();
        let destination =
            Store::open(Root::explicit(&destination_dir.path().canonicalize().unwrap()).unwrap())
                .unwrap();
        destination
            .root
            .write_new(
                &destination
                    .root
                    .safe_path("exports", "fixture.sqlite3")
                    .unwrap(),
                &backup,
            )
            .unwrap();
        destination.transaction(|tx| {
            tx.execute_batch(&format!("CREATE TRIGGER refuse_restore BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'controlled restore failure'); END;")).unwrap();
            Ok(())
        }).unwrap();
        assert_eq!(
            destination
                .restore_backup("fixture.sqlite3")
                .unwrap_err()
                .code,
            ErrorCode::StorageUnavailable,
            "{table}"
        );
        assert!(destination.list("local", "note", 0, 1).unwrap().is_empty());
        assert!(
            actions::list(&destination).unwrap()["data"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(destination.value("settings").unwrap().is_none());
        destination
            .transaction(|tx| {
                let revisions: i64 = tx
                    .query_row("SELECT count(*) FROM history", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(revisions, 0, "{table}");
                tx.execute_batch("DROP TRIGGER refuse_restore;").unwrap();
                Ok(())
            })
            .unwrap();
        destination.restore_backup("fixture.sqlite3").unwrap();
        assert_eq!(
            destination.get("local", "note", "a").unwrap().payload["body"],
            "Current"
        );
        assert_eq!(destination.history("local", "note", "a").unwrap().len(), 1);
        assert_eq!(
            actions::list(&destination).unwrap()["data"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
