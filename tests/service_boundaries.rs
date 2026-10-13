use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    service::{self, local, portability},
    storage::{Root, Store},
};
use serde_json::{Value, json};

#[test]
fn exports_and_snapshot_diffs_preserve_inputs_and_escape_active_text() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    service::dispatch(&store, "fiverr_record_save", json!({"kind":"note","id":"before","payload":{"body":"<script>\"&` فارسی","same":1,"removed":true}})).unwrap();
    service::dispatch(
        &store,
        "fiverr_record_save",
        json!({"kind":"note","id":"after","payload":{"body":"Revised","same":1,"added":2}}),
    )
    .unwrap();
    let diff = portability::diff(
        &store,
        &json!({"kind":"note","before_id":"before","after_id":"after"}),
    )
    .unwrap();
    let fields: Vec<_> = diff["data"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["added", "body", "removed"]);
    for format in ["json", "csv", "markdown", "html"] {
        let suffix = if format == "markdown" { "md" } else { format };
        let filename = format!("records.{suffix}");
        let args = json!({"kind":"note","filename":filename,"format":format});
        let result = service::dispatch(&store, "fiverr_export_prepare", args.clone()).unwrap();
        assert_eq!(result["data"]["count"], 2);
        let bytes = store
            .root
            .read_bounded(&store.root.safe_path("exports", &filename).unwrap(), 10000)
            .unwrap();
        assert_eq!(
            result["data"]["sha256"],
            fiverr_agent_mcp::core::hash(&bytes)
        );
        let text = String::from_utf8(bytes).unwrap();
        match format {
            "json" => assert_eq!(serde_json::from_str::<Vec<Record>>(&text).unwrap().len(), 2),
            "csv" => assert!(text.starts_with("id,kind,version,payload\n\"after\",\"note\",1,")),
            "html" => {
                assert!(text.contains("default-src 'none'"));
                assert!(!text.contains("<script>"));
                assert!(text.contains("&lt;script&gt;"));
            }
            _ => {
                assert!(text.starts_with("# Imported note records"));
                assert!(text.contains("```json"));
                assert!(text.contains("<script>"));
            }
        }
        assert!(service::dispatch(&store, "fiverr_export_prepare", args).is_err());
    }
    assert!(
        portability::export(
            &store,
            &json!({"kind":"note","filename":"bad","format":"exe"})
        )
        .is_err()
    );
    assert_eq!(
        store.get("local", "note", "before").unwrap().payload["body"],
        "<script>\"&` فارسی"
    );
}

#[test]
fn import_validation_is_atomic_scoped_and_distinct_from_commit() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let record = Record {
        id: "import1".into(),
        kind: "note".into(),
        account_id: "local".into(),
        version: 77,
        draft: true,
        payload: json!({"body":"From owner"}),
        provenance: Provenance {
            source: "user_import".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    };
    let args = json!({"records":[record]});
    assert_eq!(
        local::import(&store, &args, false).unwrap()["data"]["imported"],
        false
    );
    assert!(store.get("local", "note", "import1").is_err());
    assert_eq!(
        local::import(&store, &args, true).unwrap()["data"]["count"],
        1
    );
    let imported = store.get("local", "note", "import1").unwrap();
    assert_eq!(imported.version, 1);
    assert!(!imported.draft);
    let mut duplicate = args.clone();
    duplicate["records"][0]["id"] = json!("second");
    duplicate["records"]
        .as_array_mut()
        .unwrap()
        .push(args["records"][0].clone());
    assert!(local::import(&store, &duplicate, true).is_err());
    assert!(store.get("local", "note", "second").is_err());
    for (field, value) in [
        ("account_id", json!("other")),
        (
            "provenance",
            json!({"source":"fixture","observed_at":now()}),
        ),
        ("payload", json!({"body":""})),
        (
            "provenance",
            json!({"source":"user_import","observed_at":"invalid"}),
        ),
    ] {
        let mut bad = args.clone();
        bad["records"][0][field] = value;
        assert!(local::import(&store, &bad, false).is_err());
    }
    for value in [
        json!({}),
        json!({"records":false}),
        json!({"records":[{}]}),
        json!({"records":vec![Value::Null;1001]}),
    ] {
        assert!(local::import(&store, &value, false).is_err());
    }
    assert_eq!(
        service::dispatch(
            &store,
            "fiverr_record_get",
            json!({"kind":"note","id":"import1"})
        )
        .unwrap()["data"]["payload"]["body"],
        "From owner"
    );
    assert!(local::list(&store, &json!({"kind":"note","cursor":"not-a-number"})).is_err());
    let page = local::list(&store, &json!({"kind":"note","limit":1})).unwrap();
    assert_eq!(page["pagination"]["next_cursor"], "1");
    assert!(local::list(&store, &json!({"kind":"note","limit":0})).is_err());
    assert_eq!(
        service::account_scope(
            &store,
            &json!({"account_id":"other","kind":"note","id":"import1"})
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn shared_dispatch_reports_handoffs_and_rejects_unimplemented_or_hostile_input() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    assert!(
        service::dispatch(&store, "fiverr_capabilities", json!({})).unwrap()["data"].is_object()
    );
    assert_eq!(
        service::dispatch(&store, "fiverr_account_capabilities", json!({}))
            .unwrap_err()
            .code,
        ErrorCode::FeatureUnavailable
    );
    assert_eq!(
        service::dispatch(&store, "fiverr_settings_get", json!({})).unwrap()["data"]["mode"],
        "blocked"
    );
    for name in ["fiverr_account_get", "fiverr_session_status"] {
        assert_eq!(
            service::dispatch(&store, name, json!({})).unwrap()["data"]["live_authenticated"],
            false
        );
    }
    assert_eq!(
        service::dispatch(&store, "fiverr_auth_start", json!({})).unwrap()["data"]["completed"],
        false
    );
    assert_eq!(
        service::dispatch(
            &store,
            "fiverr_manual_handoff",
            json!({"operation":"review"})
        )
        .unwrap()["data"]["financial_execution_enabled"],
        false
    );
    assert_eq!(
        service::account::require_live().unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert!(service::account::handoff("bad\0", &json!({})).is_err());
    assert!(service::account::handoff("review", &json!({"body":"x".repeat(256*1024)})).is_err());
    assert_eq!(
        service::dispatch(&store, "execute_js", json!({}))
            .unwrap_err()
            .code,
        ErrorCode::FeatureUnavailable
    );
    assert!(
        service::dispatch(
            &store,
            "fiverr_health",
            json!({"body":"x".repeat(1024*1024)})
        )
        .is_err()
    );
    assert_eq!(
        service::dispatch(
            &store,
            "fiverr_validate",
            json!({"kind":"note","payload":{"body":"Valid"}})
        )
        .unwrap()["data"]["valid"],
        true
    );
    let prepared = service::dispatch(
        &store,
        "fiverr_action_prepare",
        json!({"kind":"note","target_id":"review1","payload":{"body":"Review"}}),
    )
    .unwrap();
    let id = prepared["data"]["id"].clone();
    assert_eq!(
        service::dispatch(&store, "fiverr_action_get", json!({"action_id":id})).unwrap()["data"]["state"],
        "prepared"
    );
    assert_eq!(
        service::dispatch(&store, "fiverr_action_reconcile", json!({"action_id":id})).unwrap()["data"]
            ["state"],
        "prepared"
    );
    assert!(service::dispatch(&store, "fiverr_action_commit", json!({"action_id":id})).is_err());
    assert_eq!(
        service::dispatch(&store, "fiverr_action_cancel", json!({"action_id":id})).unwrap()["data"]
            ["state"],
        "cancelled"
    );
}
