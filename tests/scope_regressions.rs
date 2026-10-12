use fiverr_agent_mcp::{
    network::Settings,
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn absolute_root_prefix_does_not_allow_parent_traversal_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let canonical = tmp.path().canonicalize().unwrap();
    let installation = canonical.join("installation");
    std::fs::create_dir(&installation).unwrap();
    std::fs::write(canonical.join("outside.json"), b"private outside data").unwrap();
    let root = Root::explicit(&installation).unwrap();
    let traversal = root.path().join("../outside.json");
    assert!(root.read_bounded(&traversal, 1024).is_err());
    assert!(
        root.write_new(&root.path().join("../escaped.txt"), b"no write")
            .is_err()
    );
    assert!(!canonical.join("escaped.txt").exists());
}
#[test]
fn action_reads_lists_and_cancellation_stay_in_selected_account() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let prepared=actions::prepare(&store,"message",&json!({"target_id":"conversation1","payload":{"recipient_id":"buyer1","body":"Account private draft"}})).unwrap();
    let id = prepared["data"]["id"].as_str().unwrap();
    Settings::change(&store, true, |s| {
        s.account_id = "other".into();
        Ok(())
    })
    .unwrap();
    assert!(actions::get(&store, id).is_err());
    assert!(actions::cancel(&store, id).is_err());
    assert!(
        actions::list(&store).unwrap()["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    Settings::change(&store, true, |s| {
        s.account_id = "local".into();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        actions::get(&store, id).unwrap()["data"]["state"],
        "prepared"
    );
}
