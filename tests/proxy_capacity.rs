use fiverr_agent_mcp::{
    cli::{self, Command, Proxy},
    network::{Settings, profiles::Profile},
    storage::{Root, Store},
};
use serde_json::Value;

fn workspace(count: usize) -> (tempfile::TempDir, Store) {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root).unwrap();
    let settings = Settings {
        profiles: (0..count)
            .map(|i| Profile {
                id: format!("profile-{i}"),
                label: format!("Profile {i}"),
                url: format!("socks5h://127.0.0.1:{}", 2000 + i),
            })
            .collect(),
        ..Settings::default()
    };
    store
        .update_value("settings", |_| {
            Ok((serde_json::to_value(settings).unwrap(), ()))
        })
        .unwrap();
    (temp, store)
}
fn snapshot(store: &Store) -> Value {
    store.value("settings").unwrap().unwrap()
}
async fn add(store: &Store, label: &str) -> fiverr_agent_mcp::core::Result<Value> {
    let file = store.root.path().join(format!("{label}.txt"));
    store
        .root
        .write_new(&file, b"socks5h://127.0.0.1:9000")
        .unwrap();
    cli::execute(
        store.root.clone(),
        store.clone(),
        Command::Proxy {
            command: Proxy::Add {
                label: label.into(),
                file,
            },
        },
    )
    .await
}

#[tokio::test]
async fn accepted_proxy_growth_must_remain_backup_recoverable() {
    let (_temp, store) = workspace(99);
    add(&store, "Hundredth").await.unwrap();
    assert_eq!(Settings::load(&store).unwrap().profiles.len(), 100);
    store.backup("control.sqlite3").unwrap();
    assert_eq!(
        store.validate_backup("control.sqlite3").unwrap()["valid"],
        true
    );
    let before = snapshot(&store);
    assert!(
        add(&store, "Overflow").await.is_err(),
        "A 101st profile must be rejected before settings commit"
    );
    assert_eq!(snapshot(&store), before);
}
