use fiverr_agent_mcp::{
    core::ErrorCode,
    storage::{Root, Store},
};
#[test]
fn unavailable_private_directories_never_fall_back_outside_root() {
    for folder in [".cache", ".state", "config", "logs", "exports"] {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        std::fs::write(base.join(folder), b"Owner data").unwrap();
        assert_eq!(
            Root::explicit(&base).unwrap_err().code,
            ErrorCode::StorageUnavailable
        );
        assert_eq!(std::fs::read(base.join(folder)).unwrap(), b"Owner data");
    }
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let db = root
        .safe_path(".state/workspace", "workspace.sqlite3")
        .unwrap();
    std::fs::create_dir(&db).unwrap();
    assert!(Store::open(root).is_err());
    assert!(db.is_dir());
}
#[cfg(unix)]
#[test]
fn inaccessible_owned_parents_report_canonicalization_and_write_failure() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let directory = root.path().join("exports/locked");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("keep.txt");
    std::fs::write(&path, b"Keep").unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o000)).unwrap();
    let results = (
        root.safe_path("exports", "locked/new.txt"),
        root.assert_owned(&path),
        root.read_bounded(&path, 10),
        root.write_new(&directory.join("new.txt"), b"No"),
    );
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(results.0.unwrap(), directory.join("new.txt"));
    assert!(results.1.is_ok());
    assert!(results.2.is_err());
    assert!(results.3.is_ok());
    assert_eq!(std::fs::read(directory.join("new.txt")).unwrap(), b"No");
    assert_eq!(std::fs::read(path).unwrap(), b"Keep");
}
