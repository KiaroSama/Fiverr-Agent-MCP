use fiverr_agent_mcp::{
    core::{Provenance, Record, now},
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn backup_record_column_types_and_settings_blobs_never_restore_partial_data() {
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
    store.save(record, None).unwrap();
    let path = store.backup("rows.sqlite3").unwrap();
    let original = std::fs::read(&path).unwrap();
    for field in ["account", "kind", "id", "version", "json"] {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("ALTER TABLE records RENAME TO old;CREATE TABLE records(account,kind,id,version,json);INSERT INTO records SELECT * FROM old;DROP TABLE old;").unwrap();
        c.execute_batch(&format!("UPDATE records SET {field}=NULL;"))
            .unwrap();
        drop(c);
        assert!(store.validate_backup("rows.sqlite3").is_err());
        std::fs::write(&path, &original).unwrap();
    }
    for sql in [
        "UPDATE records SET kind='gig';",
        "UPDATE records SET id='wrong';",
        "UPDATE records SET json=json_set(json,'$.account_id','bad/id');",
        "UPDATE records SET json=json_set(json,'$.id','bad/id');",
        "UPDATE records SET json=json_set(json,'$.kind','invalid');",
        "INSERT INTO kv VALUES('settings',x'ff');",
    ] {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(sql).unwrap();
        drop(c);
        assert!(store.validate_backup("rows.sqlite3").is_err());
        std::fs::write(&path, &original).unwrap();
    }
    assert_eq!(
        store.get("local", "note", "a").unwrap().payload["body"],
        "Keep"
    );
}
#[test]
fn closed_table_schema_refuses_every_read_and_preserves_reopenable_source() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    for table in ["records", "kv", "history"] {
        store
            .transaction(|tx| {
                tx.execute_batch(&format!(
                    "ALTER TABLE {table} RENAME TO old;CREATE TABLE {table}(wrong);"
                ))
                .unwrap();
                Ok(())
            })
            .unwrap();
        match table {
            "records" => {
                assert!(store.get("local", "note", "a").is_err());
                assert!(store.list("local", "note", 0, 1).is_err());
                assert!(store.page("local", "note", None, 1, false, None).is_err());
            }
            "kv" => {
                assert!(store.value("x").is_err());
                assert!(store.update_value("x", |_| Ok((json!(1), ()))).is_err());
            }
            _ => {}
        }
        store
            .transaction(|tx| {
                tx.execute_batch(&format!(
                    "DROP TABLE {table};ALTER TABLE old RENAME TO {table};"
                ))
                .unwrap();
                Ok(())
            })
            .unwrap();
    }
    assert!(
        Store::open(store.root.clone())
            .unwrap()
            .list("local", "note", 0, 1)
            .unwrap()
            .is_empty()
    );
}
