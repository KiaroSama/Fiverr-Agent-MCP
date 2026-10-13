use fiverr_agent_mcp::{
    cli::{self, Backup, Command},
    storage::{Root, Store},
};
use serde_json::json;

#[tokio::test]
async fn backup_cli_validates_real_snapshot_without_changing_workspace() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    fiverr_agent_mcp::service::dispatch(
        &store,
        "fiverr_record_save",
        json!({"kind":"note","id":"a","payload":{"body":"CLI snapshot"}}),
    )
    .unwrap();
    let created = cli::execute(
        root.clone(),
        store.clone(),
        Command::Backup {
            command: Backup::Create {
                filename: "cli.sqlite3".into(),
            },
        },
    )
    .await
    .unwrap();
    assert_eq!(created["private"], true);
    let checked = cli::execute(
        root.clone(),
        store.clone(),
        Command::Backup {
            command: Backup::Validate {
                filename: "cli.sqlite3".into(),
            },
        },
    )
    .await
    .unwrap();
    assert_eq!(checked["valid"], true);
    assert_eq!(checked["records"], 1);
    assert_eq!(
        store.get("local", "note", "a").unwrap().payload["body"],
        "CLI snapshot"
    );
    assert!(
        cli::execute(
            root,
            store,
            Command::Backup {
                command: Backup::Validate {
                    filename: "missing.sqlite3".into()
                }
            }
        )
        .await
        .is_err()
    );
}
