use fiverr_agent_mcp::{
    cli::{self, Command, LocalProxy, Proxy},
    network::{Mode, Settings, profiles::Profile},
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

async fn execute(store: &Store, command: Command) -> fiverr_agent_mcp::core::Result<Value> {
    cli::execute(store.root.clone(), store.clone(), command).await
}
async fn import(store: &Store, name: &str, text: &str) -> fiverr_agent_mcp::core::Result<Value> {
    let file = store.root.path().join(name);
    store.root.write_new(&file, text.as_bytes()).unwrap();
    execute(
        store,
        Command::Proxy {
            command: Proxy::Import { file },
        },
    )
    .await
}

#[tokio::test]
async fn merged_imports_are_atomic_at_capacity_and_keep_noop_revision_policy() {
    for (count, text) in [
        (
            99,
            "New A | socks5h://127.0.0.1:9001\nNew B | socks5h://127.0.0.1:9002\n",
        ),
        (
            100,
            "Profile 0 | socks5h://127.0.0.1:2000\nNew | socks5h://127.0.0.1:9001\n",
        ),
        (100, "Profile 0 | socks5h://127.0.0.1:9001\n"),
    ] {
        let (_temp, store) = workspace(count);
        let before = snapshot(&store);
        assert!(import(&store, "overflow.txt", text).await.is_err());
        assert_eq!(snapshot(&store), before);
    }
    let (_temp, store) = workspace(100);
    let before = Settings::load(&store).unwrap();
    let result = import(
        &store,
        "identical.txt",
        "Profile 0 | socks5h://127.0.0.1:2000\n",
    )
    .await
    .unwrap();
    assert_eq!(result["imported"], 0);
    assert_eq!(result["skipped"], 1);
    let after = Settings::load(&store).unwrap();
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(after.route_generation, before.route_generation);
    assert_eq!(after.profiles.len(), 100);
}

#[tokio::test]
async fn local_selection_reuses_existing_endpoint_but_never_commits_overflow() {
    let (_temp, store) = workspace(100);
    Settings::change(&store, false, |s| {
        s.local_proxy = s.profiles[0].url.clone();
        Ok(())
    })
    .unwrap();
    execute(
        &store,
        Command::LocalProxy {
            command: LocalProxy::Select,
        },
    )
    .await
    .unwrap();
    let selected = Settings::load(&store).unwrap();
    assert_eq!(selected.profiles.len(), 100);
    assert_eq!(selected.selected_profile_id.as_deref(), Some("profile-0"));
    assert!(selected.mode == Mode::Proxy);
    Settings::change(&store, false, |s| {
        s.local_proxy = "socks5h://127.0.0.1:9999".into();
        Ok(())
    })
    .unwrap();
    let before = snapshot(&store);
    assert!(
        execute(
            &store,
            Command::LocalProxy {
                command: LocalProxy::Select
            }
        )
        .await
        .is_err()
    );
    assert_eq!(snapshot(&store), before);
}

#[tokio::test]
async fn legacy_overflow_remains_readable_and_deletable_until_backup_recovers() {
    let (_temp, store) = workspace(102);
    Settings::change(&store, true, |s| {
        s.selected_profile_id = Some("profile-0".into());
        s.mode = Mode::Proxy;
        Ok(())
    })
    .unwrap();
    let listed = execute(
        &store,
        Command::Proxy {
            command: Proxy::List,
        },
    )
    .await
    .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 102);
    let before = snapshot(&store);
    assert!(add(&store, "LegacyOverflow").await.is_err());
    assert_eq!(snapshot(&store), before);
    for (id, expected) in [("profile-0", 101), ("profile-1", 100)] {
        execute(
            &store,
            Command::Proxy {
                command: Proxy::Delete { id: id.into() },
            },
        )
        .await
        .unwrap();
        let settings = Settings::load(&store).unwrap();
        assert_eq!(settings.profiles.len(), expected);
        assert!(settings.mode == Mode::Blocked);
        assert!(settings.selected_profile_id.is_none());
    }
    let backup = store.backup("recovered.sqlite3").unwrap();
    assert_eq!(
        store.validate_backup("recovered.sqlite3").unwrap()["valid"],
        true
    );
    let (_destination_temp, destination) = workspace(0);
    let path = destination
        .root
        .safe_path("exports", "recovered.sqlite3")
        .unwrap();
    destination
        .root
        .write_new(
            &path,
            &store.root.read_bounded(&backup, 64 * 1024 * 1024).unwrap(),
        )
        .unwrap();
    destination.restore_backup("recovered.sqlite3").unwrap();
    let restored = Settings::load(&destination).unwrap();
    assert_eq!(restored.profiles.len(), 100);
    assert_eq!(restored.profiles[0].id, "profile-2");
    assert!(restored.mode == Mode::Blocked);
    assert!(restored.selected_profile_id.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_cli_growth_serializes_at_the_recovery_boundary() {
    let (_temp, store) = workspace(99);
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let mut jobs = Vec::new();
    for label in ["ConcurrentA", "ConcurrentB"] {
        let store = store.clone();
        let barrier = barrier.clone();
        jobs.push(tokio::spawn(async move {
            barrier.wait().await;
            add(&store, label).await.is_ok()
        }));
    }
    let mut accepted = 0;
    for job in jobs {
        accepted += usize::from(
            tokio::time::timeout(std::time::Duration::from_secs(5), job)
                .await
                .unwrap()
                .unwrap(),
        );
    }
    assert_eq!(accepted, 1);
    let settings = Settings::load(&store).unwrap();
    assert_eq!(settings.profiles.len(), 100);
    assert_eq!(settings.revision, 1);
    assert_eq!(settings.route_generation, 0);
    store.backup("concurrent.sqlite3").unwrap();
}
