use fiverr_agent_mcp::{
    cli::{
        self, Actions, Auth, Backup, Browser, Cache, Command, Config, Diagnostics, HttpAuth,
        LocalProxy, Logs, Plugins, Proxy,
    },
    core::ErrorCode,
    network::{Mode, Settings},
    service::actions,
    storage::{Root, Store},
};
use serde_json::{Value, json};
use std::path::Path;

async fn run(store: &Store, command: Command) -> Value {
    cli::execute(store.root.clone(), store.clone(), command)
        .await
        .unwrap()
}
fn input(store: &Store, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = store.root.safe_path("exports", name).unwrap();
    store.root.write_new(&path, bytes).unwrap();
    path
}
#[tokio::test]
async fn owner_management_commands_preserve_other_state_and_refuse_model_consent() {
    let temporary = tempfile::tempdir().unwrap();
    let store =
        Store::open(Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap()).unwrap();
    assert_eq!(
        run(&store, Command::Version).await["name"],
        "fiverr-agent-mcp"
    );
    for command in [Command::Status, Command::Doctor] {
        assert_eq!(
            run(&store, command).await["data"]["route"]["mode"],
            "blocked"
        );
    }
    assert!(run(&store, Command::Capabilities).await.is_object());
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::HttpPort { port: 0 }
        )
        .await
        .is_err()
    );
    assert_eq!(
        run(&store, Command::HttpPort { port: 1777 }).await["port"],
        1777
    );
    assert_eq!(
        run(&store, Command::HttpConfig).await["url"],
        "http://127.0.0.1:1777/mcp"
    );
    let config = run(&store, Command::DesktopConfig { entry_only: true }).await;
    assert!(
        config["Fiverr-Agent-mcp"]["command"]
            .as_str()
            .unwrap()
            .contains("bin")
    );
    assert_eq!(
        run(
            &store,
            Command::Config {
                command: Config::Validate
            }
        )
        .await["valid"],
        true
    );
    for command in [
        Command::Config {
            command: Config::Export {
                filename: "settings.json".into(),
            },
        },
        Command::Diagnostics {
            command: Diagnostics::Export {
                filename: "diagnostics.json".into(),
            },
        },
    ] {
        let result = run(&store, command).await;
        let data = std::fs::read(
            store
                .root
                .safe_path("exports", result["file"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&data).unwrap();
        assert!(value.is_object());
        assert!(!value.to_string().contains("http_token"));
    }
    assert_eq!(
        run(
            &store,
            Command::Plugins {
                command: Plugins::Manifest
            }
        )
        .await["name"],
        "fiverr-agent-mcp"
    );
    assert_eq!(
        run(
            &store,
            Command::Plugins {
                command: Plugins::Validate
            }
        )
        .await["manifest_valid"],
        true
    );
    assert_eq!(
        run(
            &store,
            Command::Cache {
                command: Cache::Inspect
            }
        )
        .await["contents_disclosed"],
        false
    );
    assert_eq!(
        run(
            &store,
            Command::Cache {
                command: Cache::Clean { apply: false }
            }
        )
        .await["deleted"],
        false
    );
    assert_eq!(
        run(
            &store,
            Command::Logs {
                command: Logs::Inspect
            }
        )
        .await["contents_disclosed"],
        false
    );
    for command in [
        Command::Menu,
        Command::Direct,
        Command::Register {
            file: temporary.path().join("client.json"),
        },
        Command::Unregister {
            file: temporary.path().join("client.json"),
        },
        Command::HttpAuth {
            command: HttpAuth::Set {
                mode: "loopback-trusted".into(),
            },
        },
        Command::HttpAuth {
            command: HttpAuth::Export {
                filename: "auth.json".into(),
            },
        },
    ] {
        assert!(
            cli::execute(store.root.clone(), store.clone(), command)
                .await
                .is_err()
        );
    }
    assert!(Settings::load(&store).unwrap().mode == Mode::Blocked);
    assert_eq!(
        run(
            &store,
            Command::HttpAuth {
                command: HttpAuth::Status
            }
        )
        .await["mode"],
        "token"
    );
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::HttpAuth {
                command: HttpAuth::Set {
                    mode: "invalid".into()
                }
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        run(
            &store,
            Command::HttpAuth {
                command: HttpAuth::Set {
                    mode: "token".into()
                }
            }
        )
        .await["updated"],
        true
    );
    let old = fiverr_agent_mcp::server::http::token(&store).unwrap();
    assert_eq!(
        run(
            &store,
            Command::HttpAuth {
                command: HttpAuth::Rotate
            }
        )
        .await["rotated"],
        true
    );
    assert_ne!(old, fiverr_agent_mcp::server::http::token(&store).unwrap());
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::Cache {
                command: Cache::Clean { apply: true }
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        run(
            &store,
            Command::Auth {
                command: Auth::Status
            }
        )
        .await["data"]["live_authenticated"],
        false
    );
    assert_eq!(
        run(
            &store,
            Command::Auth {
                command: Auth::Logout
            }
        )
        .await["data"]["completed"],
        false
    );
    for command in [
        Command::Auth {
            command: Auth::RemoveLocalSession,
        },
        Command::Auth {
            command: Auth::SignIn,
        },
    ] {
        assert!(
            cli::execute(store.root.clone(), store.clone(), command)
                .await
                .is_err()
        );
    }
    assert_eq!(
        run(
            &store,
            Command::Browser {
                command: Browser::Status
            }
        )
        .await["data"]["configured"],
        false
    );
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::Browser {
                command: Browser::Configure {
                    executable: Path::new("relative").into()
                }
            }
        )
        .await
        .is_err()
    );
    let executable = std::env::current_exe().unwrap();
    assert_eq!(
        run(
            &store,
            Command::Browser {
                command: Browser::Configure { executable }
            }
        )
        .await["route_assurance"],
        "not_verified"
    );
    assert_eq!(run(&store, Command::Block).await["mode"], "blocked");
}

#[tokio::test]
async fn file_based_proxy_and_action_commands_are_atomic_and_sanitized() {
    let temporary = tempfile::tempdir().unwrap();
    let store =
        Store::open(Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap()).unwrap();
    let first = input(
        &store,
        "proxy.txt",
        b"socks5://fixture-user:fixture-pass@127.0.0.1:1080",
    );
    assert_eq!(
        run(
            &store,
            Command::Proxy {
                command: Proxy::Add {
                    label: "one".into(),
                    file: first.clone()
                }
            }
        )
        .await["selected"],
        false
    );
    let list = run(
        &store,
        Command::Proxy {
            command: Proxy::List,
        },
    )
    .await;
    assert_eq!(list[0]["scheme"], "socks5h");
    assert!(!list.to_string().contains("fixture-pass"));
    let id = list[0]["id"].as_str().unwrap().to_string();
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::Proxy {
                command: Proxy::Add {
                    label: "ONE".into(),
                    file: first
                }
            }
        )
        .await
        .is_err()
    );
    let imported = input(&store, "profiles.txt", b"two|http://127.0.0.1:1888\n");
    assert_eq!(
        run(
            &store,
            Command::Proxy {
                command: Proxy::Import {
                    file: imported.clone()
                }
            }
        )
        .await["imported"],
        1
    );
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::Proxy {
                command: Proxy::Import { file: imported }
            }
        )
        .await
        .is_err()
    );
    let edited = input(&store, "edit.txt", b"http://127.0.0.1:1999");
    assert_eq!(
        run(
            &store,
            Command::Proxy {
                command: Proxy::Edit {
                    id: id.clone(),
                    file: edited.clone()
                }
            }
        )
        .await["updated"],
        true
    );
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::Proxy {
                command: Proxy::Edit {
                    id: "missing".into(),
                    file: edited.clone()
                }
            }
        )
        .await
        .is_err()
    );
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::Proxy {
                command: Proxy::Select {
                    id: "missing".into()
                }
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        run(
            &store,
            Command::Proxy {
                command: Proxy::Select { id: id.clone() }
            }
        )
        .await["mode"],
        "proxy"
    );
    assert_eq!(
        run(
            &store,
            Command::Proxy {
                command: Proxy::Test
            }
        )
        .await["external_probe"],
        false
    );
    assert_eq!(
        run(
            &store,
            Command::Proxy {
                command: Proxy::Delete { id }
            }
        )
        .await["deleted"],
        true
    );
    assert!(Settings::load(&store).unwrap().mode == Mode::Blocked);
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::Proxy {
                command: Proxy::Delete {
                    id: "missing".into()
                }
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        run(
            &store,
            Command::LocalProxy {
                command: LocalProxy::Settings { file: Some(edited) }
            }
        )
        .await["activated"],
        false
    );
    assert_eq!(
        run(
            &store,
            Command::LocalProxy {
                command: LocalProxy::Settings { file: None }
            }
        )
        .await["active"],
        false
    );
    assert_eq!(
        run(
            &store,
            Command::LocalProxy {
                command: LocalProxy::Select
            }
        )
        .await["selected"],
        true
    );
    assert_eq!(
        run(
            &store,
            Command::LocalProxy {
                command: LocalProxy::Settings { file: None }
            }
        )
        .await["active"],
        true
    );
    run(&store, Command::Block).await;
    let invalid = input(&store, "invalid.txt", b"\xff");
    for command in [
        Proxy::Import {
            file: invalid.clone(),
        },
        Proxy::Add {
            label: "bad".into(),
            file: invalid.clone(),
        },
        Proxy::Edit {
            id: "missing".into(),
            file: invalid.clone(),
        },
    ] {
        assert!(
            cli::execute(
                store.root.clone(),
                store.clone(),
                Command::Proxy { command }
            )
            .await
            .is_err()
        );
    }
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::LocalProxy {
                command: LocalProxy::Settings {
                    file: Some(invalid)
                }
            }
        )
        .await
        .is_err()
    );
    let call = input(
        &store,
        "call.json",
        br#"{"kind":"note","id":"cli-note","payload":{"body":"Kept"}}"#,
    );
    assert_eq!(
        run(
            &store,
            Command::Call {
                name: "fiverr_record_save".into(),
                input: Some(call)
            }
        )
        .await["data"]["id"],
        "cli-note"
    );
    assert_eq!(
        run(
            &store,
            Command::Call {
                name: "fiverr_health".into(),
                input: None
            }
        )
        .await["data"]["alive"],
        true
    );
    let bad = input(&store, "bad.json", b"not JSON");
    assert!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::Call {
                name: "fiverr_health".into(),
                input: Some(bad)
            }
        )
        .await
        .is_err()
    );
    let action = actions::prepare(
        &store,
        "note",
        &json!({"target_id":"note1","payload":{"body":"Review only"}}),
    )
    .unwrap();
    let id = action["data"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        run(
            &store,
            Command::Actions {
                command: Actions::List
            }
        )
        .await["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        run(
            &store,
            Command::Actions {
                command: Actions::Review { id: id.clone() }
            }
        )
        .await["data"]["state"],
        "prepared"
    );
    assert_eq!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::Actions {
                command: Actions::Approve { id: id.clone() }
            }
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::ConfirmationRequired
    );
    assert_eq!(
        run(
            &store,
            Command::Actions {
                command: Actions::Reject { id }
            }
        )
        .await["data"]["state"],
        "cancelled"
    );
    assert_eq!(
        run(
            &store,
            Command::Backup {
                command: Backup::Create {
                    filename: "cli.sqlite3".into()
                }
            }
        )
        .await["private"],
        true
    );
    assert_eq!(
        cli::execute(
            store.root.clone(),
            store.clone(),
            Command::Backup {
                command: Backup::Restore {
                    filename: "cli.sqlite3".into()
                }
            }
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::ConfirmationRequired
    );
    assert_eq!(
        store.get("local", "note", "cli-note").unwrap().payload["body"],
        "Kept"
    );
}
