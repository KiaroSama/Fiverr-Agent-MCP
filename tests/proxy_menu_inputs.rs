use fiverr_agent_mcp::{
    cli::{self, Command, LocalProxy, Proxy},
    network::{Settings, profiles},
    storage::{Root, Store},
};

#[test]
fn reference_named_proxy_syntax_preserves_credentials_ipv6_and_existing_formats() {
    let input = "socks5://proxy.example:1595:user:pass:Owner profile\nhttps://[::1]:8443:::IPv6 profile\nLegacy | http://127.0.0.1:1596\n";
    let output = profiles::parse_import(input).unwrap();
    assert_eq!(output.len(), 3);
    assert_eq!(output[0].label, "Owner profile");
    assert_eq!(output[0].url, "socks5h://user:pass@proxy.example:1595");
    assert_eq!(output[1].url, "https://[::1]:8443/");
    for valid in [
        "http://host",
        "http://[::1]",
        "http://[::1]/",
        "https://host:8443",
        "host:1595:user:pass",
        "# comment\n",
        "Label | http://host:80",
    ] {
        assert!(profiles::parse_import(valid).is_ok());
    }
    for bad in [
        "socks5://host:0:::Bad",
        "socks5://host:1595:::Bad\u{1b}name",
        "socks4://host:1595:::Bad",
        "http://[::1:80:::Bad",
    ] {
        assert!(profiles::parse_import(bad).is_err());
    }
}
#[tokio::test]
async fn repeat_import_is_idempotent_conflict_rolls_back_and_local_selection_reuses_id() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let file = root.path().join("Proxy list.txt");
    root.write_new(&file, b"socks5://127.0.0.1:1595:::Owner\n")
        .unwrap();
    for expected in [1, 0] {
        let value = cli::execute(
            root.clone(),
            store.clone(),
            Command::Proxy {
                command: Proxy::Import { file: file.clone() },
            },
        )
        .await
        .unwrap();
        assert_eq!(value["imported"], expected);
    }
    let before = serde_json::to_value(Settings::load(&store).unwrap()).unwrap();
    let conflict = root.path().join("conflict.txt");
    root.write_new(
        &conflict,
        b"http://127.0.0.1:1596:::New\nsocks5://127.0.0.1:1597:::Owner\n",
    )
    .unwrap();
    assert!(
        cli::execute(
            root.clone(),
            store.clone(),
            Command::Proxy {
                command: Proxy::Import { file: conflict }
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        before,
        serde_json::to_value(Settings::load(&store).unwrap()).unwrap()
    );
    for _ in 0..2 {
        cli::execute(
            root.clone(),
            store.clone(),
            Command::LocalProxy {
                command: LocalProxy::Select,
            },
        )
        .await
        .unwrap();
    }
    let settings = Settings::load(&store).unwrap();
    assert_eq!(settings.profiles.len(), 1);
    assert_eq!(
        settings.selected_profile_id,
        Some(settings.profiles[0].id.clone())
    );
}
