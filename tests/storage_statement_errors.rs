use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    network::Settings,
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn nonnormal_owned_paths_and_missing_private_files_fail_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    // Construct the lexical input directly: Windows verbatim PathBuf::join may normalize '..'.
    let path = std::path::PathBuf::from(format!("{}/exports/../outside", root.path().display()));
    assert_eq!(
        root.assert_owned(&path).unwrap_err().code,
        ErrorCode::FilePolicyViolation
    );
    assert_eq!(
        root.write_new(&path, b"No").unwrap_err().code,
        ErrorCode::FilePolicyViolation
    );
    assert!(!root.path().join("outside").exists());
    assert!(fiverr_agent_mcp::storage::private_permissions(&root.path().join("missing")).is_err());
}
#[test]
fn corrupt_statement_shapes_fail_every_journal_settings_and_collection_operation() {
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
    let saved = store.save(r, None).unwrap();
    for table in ["kv", "history", "actions"] {
        store
            .transaction(|tx| {
                tx.execute_batch(&format!(
                    "ALTER TABLE {table} RENAME TO old;CREATE TABLE {table}(bad);"
                ))
                .unwrap();
                Ok(())
            })
            .unwrap();
        match table {
            "kv" => {
                assert!(store.transaction(Settings::in_transaction).is_err());
                assert!(Settings::change(&store, false, |_| Ok(())).is_err());
            }
            "history" => {
                assert!(store.history("local", "note", "a").is_err());
                assert!(store.save(saved.clone(), Some(1)).is_err());
            }
            _ => {
                assert!(
                    actions::prepare(
                        &store,
                        "note",
                        &json!({"target_id":"a","payload":{"body":"Keep"}})
                    )
                    .is_err()
                );
                assert!(actions::get(&store, "a").is_err());
                assert!(actions::list(&store).is_err());
            }
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
    store.transaction(|tx|{tx.execute_batch(r#"ALTER TABLE records RENAME TO old;CREATE TABLE records(account TEXT,kind TEXT,id TEXT,version INTEGER,json TEXT);INSERT INTO records VALUES('local','task','a',1,'{"draft":true}');"#).unwrap();Ok(())}).unwrap();
    assert!(
        store
            .due_tasks_page("local", None, 1, chrono::Utc::now())
            .is_err()
    );
    assert!(store.get("local", "task", "a").is_err());
    assert!(store.list("local", "task", 0, 1).is_err());
    store
        .transaction(|tx| {
            tx.execute_batch("DROP TABLE records;ALTER TABLE old RENAME TO records;")
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(store.get("local", "note", "a").unwrap().version, 1);
}
