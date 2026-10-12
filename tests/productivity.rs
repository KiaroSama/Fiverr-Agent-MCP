use fiverr_agent_mcp::{
    service,
    storage::{Root, Store},
};
use serde_json::json;

#[test]
fn drafts_search_snapshots_and_reminders_are_scoped_local_observations() {
    let temporary = tempfile::tempdir().unwrap();
    let store =
        Store::open(Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap()).unwrap();
    for (id, title) in [("a", "Translate UTF-8 فارسی"), ("b", "Other service")] {
        service::dispatch(
            &store,
            "fiverr_record_save",
            json!({"kind":"gig","id":id,"payload":{"title":title}}),
        )
        .unwrap();
    }
    let found = service::dispatch(
        &store,
        "fiverr_records_search",
        json!({"kind":"gig","query":"فارسی"}),
    )
    .unwrap();
    assert_eq!(found["data"].as_array().unwrap().len(), 1);
    assert_eq!(found["data"][0]["id"], "a");
    let snapshot = service::dispatch(
        &store,
        "fiverr_snapshot_create",
        json!({"kind":"gig","id":"a"}),
    )
    .unwrap();
    assert_eq!(
        snapshot["data"]["payload"]["record"]["payload"]["title"],
        "Translate UTF-8 فارسی"
    );
    service::dispatch(
        &store,
        "fiverr_task_save",
        json!({"id":"due1","payload":{"title":"Owner reminder","due_at":"2020-01-01T00:00:00Z"}}),
    )
    .unwrap();
    let reminders = service::dispatch(&store, "fiverr_reminders_list", json!({})).unwrap();
    assert_eq!(reminders["data"][0]["id"], "due1");
    assert_eq!(
        reminders["provenance"]["source"],
        "local_reminder_inference"
    );
    assert!(
        service::dispatch(
            &store,
            "fiverr_records_search",
            json!({"kind":"gig","query":"x","account_id":"other"})
        )
        .is_err()
    );
}

#[test]
fn pagination_cursor_is_bound_to_account_kind_and_draft_filter() {
    let temporary = tempfile::tempdir().unwrap();
    let store =
        Store::open(Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap()).unwrap();
    for id in ["a", "b"] {
        service::dispatch(
            &store,
            "fiverr_record_save",
            json!({"kind":"note","id":id,"payload":{"body":"Local only"}}),
        )
        .unwrap();
    }
    let first = service::dispatch(
        &store,
        "fiverr_drafts_list",
        json!({"kind":"note","limit":1}),
    )
    .unwrap();
    let cursor = first["pagination"]["next_cursor"].as_str().unwrap();
    assert_ne!(cursor, "1");
    let next = service::dispatch(
        &store,
        "fiverr_drafts_list",
        json!({"kind":"note","limit":1,"cursor":cursor}),
    )
    .unwrap();
    assert_eq!(next["data"][0]["id"], "b");
    assert!(
        service::dispatch(
            &store,
            "fiverr_records_list",
            json!({"kind":"note","cursor":cursor})
        )
        .is_err()
    );
    assert!(
        service::dispatch(
            &store,
            "fiverr_drafts_list",
            json!({"kind":"task","cursor":cursor})
        )
        .is_err()
    );
    assert!(
        service::dispatch(
            &store,
            "fiverr_draft_get",
            json!({"kind":"note","id":"missing"})
        )
        .is_err()
    );
}
