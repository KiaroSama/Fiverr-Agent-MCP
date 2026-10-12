use fiverr_agent_mcp::{
    core::ErrorCode,
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn model_cannot_approve_and_stale_route_cannot_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let action = actions::prepare(
        &store,
        "message",
        &json!({"target_id":"conversation1","payload":{"recipient_id":"buyer1","body":"Hello"}}),
    )
    .unwrap();
    let id = action["data"]["id"].as_str().unwrap();
    assert_eq!(
        actions::commit(&store, id).unwrap_err().code,
        ErrorCode::ConfirmationRequired
    );
    assert_eq!(
        actions::approve(&store, id, false).unwrap_err().code,
        ErrorCode::ConfirmationRequired
    );
    actions::approve(&store, id, true).unwrap();
    crate_route_change(&store);
    assert_eq!(
        actions::commit(&store, id).unwrap_err().code,
        ErrorCode::ActionStateChanged
    );
}
fn crate_route_change(store: &Store) {
    fiverr_agent_mcp::network::Settings::change(store, true, |_| Ok(())).unwrap();
}
#[test]
fn approval_does_not_convert_a_handoff_into_remote_success() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let action = actions::prepare(
        &store,
        "delivery",
        &json!({"target_id":"order1","payload":{"order_id":"order1","body":"Deliverable review"}}),
    )
    .unwrap();
    let id = action["data"]["id"].as_str().unwrap();
    actions::approve(&store, id, true).unwrap();
    let result = actions::commit(&store, id).unwrap();
    assert_eq!(result["data"]["remote_executed"], false);
    assert_eq!(result["data"]["mode"], "manual_handoff");
    assert!(actions::commit(&store, id).is_err());
}
