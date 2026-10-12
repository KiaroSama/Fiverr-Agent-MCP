use fiverr_agent_mcp::{
    core::ErrorCode,
    service::{self, actions},
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn changed_attachment_or_bound_draft_invalidates_approval() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let file = root.safe_path(".state/workspace", "delivery.txt").unwrap();
    root.write_new(&file, b"original delivery").unwrap();
    let draft=service::dispatch(&store,"fiverr_record_save",json!({"kind":"delivery","id":"draft1","payload":{"order_id":"order1","body":"Review only"}})).unwrap();
    assert_eq!(draft["data"]["version"], 1);
    let input = json!({"target_id":"order1","payload":{"order_id":"order1","body":"Review only"},"files":["delivery.txt"],"record_binding":{"kind":"delivery","id":"draft1","version":1}});
    let prepared = actions::prepare(&store, "delivery", &input).unwrap();
    let id = prepared["data"]["id"].as_str().unwrap();
    actions::approve(&store, id, true).unwrap();
    std::fs::write(&file, b"changed delivery").unwrap();
    assert_eq!(
        actions::commit(&store, id).unwrap_err().code,
        ErrorCode::ActionStateChanged
    );
    std::fs::write(&file, b"original delivery").unwrap();
    service::dispatch(&store,"fiverr_record_save",json!({"kind":"delivery","id":"draft1","expected_version":1,"payload":{"order_id":"order1","body":"Changed draft"}})).unwrap();
    assert_eq!(
        actions::commit(&store, id).unwrap_err().code,
        ErrorCode::ActionStateChanged
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"original delivery");
}
#[test]
fn concurrent_commits_consume_one_approval_only() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let action=actions::prepare(&store,"message",&json!({"target_id":"conversation1","payload":{"recipient_id":"buyer1","body":"Local review"}})).unwrap();
    let id = action["data"]["id"].as_str().unwrap().to_string();
    actions::approve(&store, &id, true).unwrap();
    let a = store.clone();
    let ai = id.clone();
    let first = std::thread::spawn(move || actions::commit(&a, &ai));
    let b = store.clone();
    let second = std::thread::spawn(move || actions::commit(&b, &id));
    let successes = [first.join().unwrap(), second.join().unwrap()]
        .into_iter()
        .filter(Result::is_ok)
        .count();
    assert_eq!(successes, 1);
}
