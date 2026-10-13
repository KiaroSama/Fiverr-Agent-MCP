use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn sql_type_mismatch_fails_reads_and_history_without_erasing_original_rows() {
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
    store.save(record.clone(), None).unwrap();
    store.save(record, Some(1)).unwrap();
    store.transaction(|tx|{tx.execute_batch("UPDATE records SET json=x'ff';UPDATE history SET json=x'ff';INSERT INTO kv VALUES('bad',x'ff');").unwrap();Ok(())}).unwrap();
    assert_eq!(
        store.get("local", "note", "a").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert!(store.list("local", "note", 0, 100).is_err());
    assert!(store.page("local", "note", None, 1, false, None).is_err());
    assert!(store.value("bad").is_err());
    assert!(store.update_value("bad", |_| Ok((json!(1), ()))).is_err());
    store
        .transaction(|tx| {
            tx.execute(
                "UPDATE records SET json=?1",
                [serde_json::to_string(&Record {
                    id: "a".into(),
                    kind: "note".into(),
                    account_id: "local".into(),
                    version: 2,
                    draft: true,
                    payload: json!({"body":"Keep"}),
                    provenance: Provenance {
                        source: "local_draft".into(),
                        observed_at: now(),
                        uri: None,
                        partial: false,
                    },
                    updated_at: now(),
                })
                .unwrap()],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
    assert!(store.history("local", "note", "a").is_err());
}
#[cfg(unix)]
#[test]
fn unreadable_file_and_unwritable_log_fail_explicitly_with_bytes_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let path = root.safe_path("exports", "unreadable").unwrap();
    root.write_new(&path, b"Keep").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = root.read_bounded(&path, 10);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::StorageUnavailable);
    assert_eq!(std::fs::read(&path).unwrap(), b"Keep");
    let logger = fiverr_agent_mcp::storage::logging::Logger::open(&root).unwrap();
    let log = std::fs::read_dir(root.path().join("logs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&log)
        .unwrap()
        .set_len(8 * 1024 * 1024)
        .unwrap();
    logger.event("INFO", "test", "Bounded", json!({}));
    drop(logger);
    assert_eq!(std::fs::metadata(&log).unwrap().len(), 8 * 1024 * 1024);
}
