use fiverr_agent_mcp::{
    bootstrap,
    core::{Provenance, Record, now},
    storage::{Root, Store},
};
use serde_json::{Value, json};
use std::time::Duration;
async fn call(root: &Root, name: &str, input: &Value) -> std::process::Output {
    let path = root
        .safe_path("exports", &format!("input-{}.json", uuid::Uuid::new_v4()))
        .unwrap();
    root.write_new(&path, &serde_json::to_vec(input).unwrap())
        .unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_fiverr-agent-mcp"));
    command
        .arg("--root")
        .arg(root.path())
        .args(["call", name, "--input"])
        .arg(path)
        .envs(bootstrap::environment(root))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(0x08000000);
    }
    tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .unwrap()
        .unwrap()
}
#[tokio::test]
async fn fixed_native_export_snapshot_and_reminder_regressions_preserve_real_workspace_data() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let note=call(&root,"fiverr_record_save",&json!({"kind":"note","id":"note","payload":{"body":"![fixture](https://example.invalid/image) [link](https://example.invalid/link) <script>synthetic</script> ```` فارسی"}})).await;
    assert!(note.status.success());
    let mismatch = call(
        &root,
        "fiverr_export_prepare",
        &json!({"kind":"note","format":"json","filename":"unsafe.html"}),
    )
    .await;
    assert_eq!(mismatch.status.code(), Some(1));
    assert!(!root.path().join("exports/unsafe.html").exists());
    let markdown = call(
        &root,
        "fiverr_export_prepare",
        &json!({"kind":"note","format":"markdown","filename":"literal.md"}),
    )
    .await;
    assert!(markdown.status.success());
    let text = std::fs::read_to_string(root.path().join("exports/literal.md")).unwrap();
    let fence = text
        .lines()
        .find(|line| line.ends_with("json"))
        .unwrap()
        .trim_end_matches("json");
    assert!(fence.len() >= 5);
    assert_eq!(text.lines().filter(|l| *l == fence).count(), 1);
    assert!(text.contains("![fixture]"));
    let snapshot = call(
        &root,
        "fiverr_snapshot_create",
        &json!({"kind":"note","id":"note"}),
    )
    .await;
    assert!(snapshot.status.success());
    let snapshot: Value = serde_json::from_slice(&snapshot.stdout).unwrap();
    let id = snapshot["data"]["id"].as_str().unwrap();
    let overwrite = call(
        &root,
        "fiverr_record_save",
        &json!({"kind":"snapshot","id":id,"expected_version":1,"payload":{"replacement":true}}),
    )
    .await;
    assert_eq!(overwrite.status.code(), Some(1));
    let store = Store::open(root.clone()).unwrap();
    assert_eq!(store.get("local", "snapshot", id).unwrap().version, 1);
    assert_eq!(
        store.get("local", "snapshot", id).unwrap().payload["record"]["id"],
        "note"
    );
    // One transactional fixture batch avoids 100 child launches/ACL resets.
    for index in 0..101 {
        let due = index == 100;
        let record = Record {
            id: if due {
                "z-due".into()
            } else {
                format!("a{index:03}")
            },
            kind: "task".into(),
            account_id: "local".into(),
            version: 0,
            draft: true,
            payload: json!({"title":"Synthetic","status":if due{"open"}else{"done"},"due_at":"2020-01-01T00:00:00Z"}),
            provenance: Provenance {
                source: "local_draft".into(),
                observed_at: now(),
                uri: None,
                partial: false,
            },
            updated_at: now(),
        };
        store.save(record, None).unwrap();
    }
    drop(store);
    let reminders = call(&root, "fiverr_reminders_list", &json!({})).await;
    assert!(reminders.status.success());
    let result: Value = serde_json::from_slice(&reminders.stdout).unwrap();
    assert_eq!(result["data"].as_array().unwrap().len(), 1);
    assert_eq!(result["data"][0]["id"], "z-due");
    assert_eq!(result["partial"], false);
}
