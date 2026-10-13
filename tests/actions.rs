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
#[test]
fn unknown_and_executing_journal_states_never_replay_even_after_expiry() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let action = actions::prepare(
        &store,
        "message",
        &json!({"target_id":"buyer1","payload":{"recipient_id":"buyer1","body":"Do not dispatch"}}),
    )
    .unwrap();
    let id = action["data"]["id"].as_str().unwrap().to_owned();
    // Persist a crash/reply-loss fixture through the existing transactional storage seam.
    for state in ["executing", "outcome_unknown"] {
        let mut data = action["data"].clone();
        data["state"] = json!(state);
        data["expires_at"] = json!(0);
        store
            .transaction(|tx| {
                tx.execute(
                    "UPDATE actions SET state=?1,json=?2 WHERE id=?3",
                    rusqlite::params![state, data.to_string(), id],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        let reopened = Store::open(root.clone()).unwrap();
        assert_eq!(
            actions::commit(&reopened, &id).unwrap_err().code,
            ErrorCode::ActionOutcomeUnknown
        );
        assert!(actions::cancel(&reopened, &id).is_err());
        assert_eq!(
            actions::get(&reopened, &id).unwrap()["data"]["state"],
            state
        );
    }
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
