use fiverr_agent_mcp::{
    core::{ErrorCode, Money, validate_id},
    storage::Root,
};
use serde_json::json;

#[test]
fn exact_money_and_untrusted_ids_have_load_bearing_limits() {
    let money: Money =
        serde_json::from_value(json!({"minor":1999,"currency":"USD","scale":2})).unwrap();
    assert_eq!(money.decimal().unwrap(), "19.99");
    assert!(
        serde_json::from_value::<Money>(json!({"minor":1.5,"currency":"USD","scale":2})).is_err()
    );
    for id in ["../cookie", "x:y", "", "x/y", "a\nsecret"] {
        assert!(validate_id(id).is_err(), "{id:?}");
    }
    assert!(validate_id("order-123_Ab").is_ok());
    assert_eq!(ErrorCode::RouteBlocked.as_str(), "ROUTE_BLOCKED");
}

#[test]
fn rooted_files_refuse_escape_and_preserve_existing_user_content() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    for name in [
        "../outside",
        "C:/secret",
        "x:ads",
        "CON",
        "NUL.txt",
        "a/../../b",
        "\\\\server\\share",
        "a\\..\\b",
    ] {
        assert!(root.safe_path("exports", name).is_err(), "{name}");
    }
    let path = root.safe_path("exports", "safe.json").unwrap();
    root.write_new(&path, b"original").unwrap();
    assert!(root.write_new(&path, b"replacement").is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"original");
}

#[cfg(unix)]
#[test]
fn physical_link_escape_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("exports/linked")).unwrap();
    assert!(root.safe_path("exports", "linked/secret.json").is_err());
}
