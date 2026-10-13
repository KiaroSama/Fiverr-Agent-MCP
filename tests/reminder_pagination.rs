use fiverr_agent_mcp::{
    core::{Provenance, Record, now},
    service,
    storage::{Root, Store},
};
use serde_json::{Value, json};

fn workspace() -> (tempfile::TempDir, Store) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let store = Store::open(Root::explicit(&root).unwrap()).unwrap();
    (directory, store)
}
fn task(store: &Store, id: &str, status: &str, due_at: &str) {
    service::dispatch(
        store,
        "fiverr_task_save",
        json!({"id":id,"payload":{"title":"Synthetic reminder","status":status,"due_at":due_at}}),
    )
    .unwrap();
}
fn reminders(store: &Store, args: Value) -> Value {
    service::dispatch(store, "fiverr_reminders_list", args).unwrap()
}
fn encode(value: &Value) -> String {
    value
        .to_string()
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn due_tasks_after_a_full_ineligible_prefix_are_not_lost() {
    let (_directory, store) = workspace();
    for index in 0..100 {
        task(
            &store,
            &format!("a{index:03}"),
            "done",
            "2020-01-01T00:00:00Z",
        );
    }
    task(&store, "b-future", "open", "2099-01-01T00:00:00Z");
    task(&store, "c-cancelled", "cancelled", "2020-01-01T00:00:00Z");
    task(&store, "z-due", "open", "2020-01-01T03:30:00+03:30");
    let result = reminders(&store, json!({}));
    assert_eq!(result["data"].as_array().unwrap().len(), 1);
    assert_eq!(result["data"][0]["id"], "z-due");
    assert_eq!(result["partial"], false);
    assert!(result["pagination"]["next_cursor"].is_null());
    assert_eq!(result["notifications_sent"], false);
}

#[test]
fn due_collections_over_the_default_limit_are_fully_pageable() {
    let (_directory, store) = workspace();
    for index in 0..101 {
        task(
            &store,
            &format!("t{index:03}"),
            "open",
            "2020-01-01T00:00:00Z",
        );
    }
    let first = reminders(&store, json!({}));
    assert_eq!(first["data"].as_array().unwrap().len(), 100);
    assert_eq!(first["partial"], true);
    let second = reminders(&store, json!({"cursor":first["pagination"]["next_cursor"]}));
    assert_eq!(second["data"].as_array().unwrap().len(), 1);
    assert_eq!(second["data"][0]["id"], "t100");
    assert_eq!(second["partial"], false);
    assert!(second["pagination"]["next_cursor"].is_null());
    assert_eq!(
        first["provenance"]["observed_at"],
        second["provenance"]["observed_at"]
    );
}

#[test]
fn reminder_cursors_are_bound_to_account_view_and_cutoff() {
    let (_directory, store) = workspace();
    task(&store, "a", "open", "2020-01-01T00:00:00Z");
    task(&store, "b", "open", "2020-01-01T00:00:00Z");
    let first = reminders(&store, json!({"limit":1}));
    let valid = first["pagination"]["next_cursor"].as_str().unwrap();
    let second = reminders(&store, json!({"cursor":valid,"limit":1}));
    assert_eq!(second["data"][0]["id"], "b");
    let invalid = [
        json!({"version":1,"account":"other","after":"a","observed_at":now()}),
        json!({"version":2,"account":"local","after":"a","observed_at":now()}),
        json!({"version":1,"account":"local","after":"a","observed_at":"2099-01-01T00:00:00Z"}),
        json!({"version":1,"account":"local","after":"../a","observed_at":now()}),
        json!({"version":1,"account":"local","after":"a","observed_at":"bad-date"}),
    ];
    for cursor in invalid {
        assert!(
            service::dispatch(
                &store,
                "fiverr_reminders_list",
                json!({"cursor":encode(&cursor)})
            )
            .is_err()
        );
    }
    let collection = service::dispatch(
        &store,
        "fiverr_drafts_list",
        json!({"kind":"task","limit":1}),
    )
    .unwrap();
    assert!(
        service::dispatch(
            &store,
            "fiverr_reminders_list",
            json!({"cursor":collection["pagination"]["next_cursor"]})
        )
        .is_err()
    );
    for limit in [0, 101] {
        assert!(
            service::dispatch(&store, "fiverr_reminders_list", json!({"limit":limit})).is_err()
        );
    }
}

#[test]
fn imports_other_accounts_and_future_deadlines_stay_out_of_reminders() {
    let (_directory, store) = workspace();
    let payload = json!({"title":"Observed task","due_at":"2020-01-01T00:00:00Z"});
    let record = Record {
        id: "imported".into(),
        kind: "task".into(),
        account_id: "local".into(),
        version: 1,
        draft: false,
        payload,
        provenance: Provenance {
            source: "user_import".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    };
    store.import(std::slice::from_ref(&record)).unwrap();
    let mut other = record;
    other.account_id = "other".into();
    other.draft = true;
    other.provenance.source = "local_draft".into();
    store.save(other, None).unwrap();
    task(&store, "future", "open", "2099-01-01T00:00:00Z");
    assert!(
        reminders(&store, json!({}))["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
