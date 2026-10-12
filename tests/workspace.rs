use fiverr_agent_mcp::{
    core::{Provenance, Record, now},
    storage::{Root, Store},
};
use serde_json::json;
fn record() -> Record {
    Record {
        id: "draft1".into(),
        kind: "gig".into(),
        account_id: "local".into(),
        version: 0,
        draft: true,
        payload: json!({"title":"Build a useful service"}),
        provenance: Provenance {
            source: "local_draft".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    }
}
#[test]
fn drafts_keep_revisions_and_reject_lost_updates() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let saved = store.save(record(), None).unwrap();
    assert_eq!(saved.version, 1);
    assert!(store.save(record(), None).is_err());
    let mut next = saved.clone();
    next.payload = json!({"title":"Updated"});
    assert_eq!(store.save(next, Some(1)).unwrap().version, 2);
    assert_eq!(
        store.history("local", "gig", "draft1").unwrap()[0].payload["title"],
        "Build a useful service"
    );
    assert!(store.get("other", "gig", "draft1").is_err());
}
#[test]
fn import_conflict_rolls_back_the_entire_file() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    store.save(record(), None).unwrap();
    let mut a = record();
    a.id = "new1".into();
    a.provenance.source = "user_import".into();
    let mut b = a.clone();
    b.id = "draft1".into();
    assert!(store.import(&[a, b]).is_err());
    assert!(store.get("local", "gig", "new1").is_err());
}
#[test]
fn nested_logs_remove_sensitive_fields_before_serialization() {
    let redacted = fiverr_agent_mcp::storage::logging::redact(
        json!({"proxy_url":"socks5h://alice:encoded%3Asecret@host","nested":{"authorization":"Bearer dummy","message_body":"private"},"ok":12}),
    );
    let text = redacted.to_string();
    assert!(!text.contains("dummy"));
    assert!(!text.contains("private"));
    assert!(!text.contains("alice"));
    assert_eq!(redacted["ok"], 12);
}
