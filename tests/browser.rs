use fiverr_agent_mcp::{
    browser::{
        observation::{ProfileLease, decode},
        sign_in,
    },
    core::ErrorCode,
    storage::{Root, Store},
};
use serde_json::json;

#[test]
fn typed_account_observation_stops_challenges_and_identity_switches() {
    let account = json!({"page_state":"account","account":{"account_id":"owner1","username":"fixture-seller","roles":["buyer","seller"],"observed_at":"2026-10-12T00:00:00Z","source":"fixture","mira_eligible":null}});
    assert_eq!(decode(&account, "owner1").unwrap().account_id, "owner1");
    assert_eq!(
        decode(&account, "owner2").unwrap_err().code,
        ErrorCode::ActionStateChanged
    );
    for (state, code) in [
        ("challenge", ErrorCode::ChallengeRequired),
        ("expired", ErrorCode::SessionExpired),
        ("signed_out", ErrorCode::AuthRequired),
        ("new_schema", ErrorCode::UpstreamChanged),
    ] {
        assert_eq!(
            decode(&json!({"page_state":state}), "owner1")
                .unwrap_err()
                .code,
            code
        );
    }
    let mut injected = account;
    injected["account"]["cookie"] = json!("example-refused");
    assert!(decode(&injected, "owner1").is_err());
}
#[test]
fn dedicated_profile_lease_is_exclusive_and_released_without_deletion() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap();
    let lease = ProfileLease::acquire(&root, "owner1").unwrap();
    assert!(ProfileLease::acquire(&root, "owner1").is_err());
    drop(lease);
    assert!(ProfileLease::acquire(&root, "owner1").is_ok());
    assert!(
        root.safe_path(".state/browser", "owner1.lock")
            .unwrap()
            .is_file()
    );
}
#[tokio::test]
async fn sign_in_never_launches_from_blocked_unverified_route() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    assert_eq!(
        sign_in(&store).await.unwrap_err().code,
        ErrorCode::RouteBlocked
    );
}
