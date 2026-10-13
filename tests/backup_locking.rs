use fiverr_agent_mcp::{
    core::ErrorCode,
    storage::{Root, Store},
};
#[test]
fn exclusively_locked_source_reports_backup_failure_without_reset_or_retry() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let path = store
        .root
        .safe_path(".state/workspace", "workspace.sqlite3")
        .unwrap();
    let lock = rusqlite::Connection::open(&path).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let result = store.backup("busy.sqlite3");
    lock.execute_batch("ROLLBACK;").unwrap();
    drop(lock);
    assert_eq!(result.unwrap_err().code, ErrorCode::StorageUnavailable);
    assert!(store.value("settings").unwrap().is_none());
    assert!(store.list("local", "note", 0, 100).unwrap().is_empty());
}
