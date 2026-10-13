use fiverr_agent_mcp::{
    core::ErrorCode,
    storage::{Root, logging::Logger},
};
#[test]
fn unavailable_log_directory_reports_failure_instead_of_claiming_a_file() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let logs = root.path().join("logs");
    std::fs::remove_dir(&logs).unwrap();
    std::fs::write(&logs, b"Owner blocker").unwrap();
    let result = Logger::open(&root);
    match result {
        Ok(_) => panic!("Unexpected logger success"),
        Err(e) => assert_eq!(e.code, ErrorCode::StorageUnavailable),
    }
    assert_eq!(std::fs::read(logs).unwrap(), b"Owner blocker");
}
