use fiverr_agent_mcp::{
    network::{Settings, approved_destination, profiles},
    storage::{Root, Store},
};
use url::Url;
#[test]
fn proxy_credential_and_import_limits_fail_atomically_without_secret_errors() {
    for bad in [
        "http://localhost/path",
        "http://localhost?x=1",
        "http://localhost#x",
        "socks5h://localhost:0",
        "socks5://u:%ff@localhost:1080",
        "socks5://%ff:p@localhost:1080",
        "socks5://u:%00@localhost:1080",
        "socks5://:password@localhost:1080",
        "\nhttp://localhost",
    ] {
        assert!(profiles::normalize(bad).is_err(), "{bad}");
    }
    assert!(profiles::normalize(&format!("http://{}:p@localhost", "u".repeat(256))).is_err());
    assert!(profiles::normalize(&format!("http://u:{}@localhost", "p".repeat(256))).is_err());
    assert!(profiles::normalize(&"x".repeat(2049)).is_err());
    for (input, scheme, port) in [
        ("http://127.0.0.1", "http", 80),
        ("https://127.0.0.1", "https", 443),
        ("socks5://127.0.0.1", "socks5h", 1080),
    ] {
        let u = profiles::normalize(input).unwrap();
        assert_eq!(u.scheme(), scheme);
        assert_eq!(u.port_or_known_default(), Some(port));
    }
    let parsed = profiles::parse_import("127.0.0.1:1080:owner:p@ss").unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].label, "Proxy 1");
    assert!(parsed[0].url.contains("p%40ss"));
    for text in [
        "|http://127.0.0.1",
        "one|http://127.0.0.1:1\nONE|http://127.0.0.1:2",
        "[::1]:1080:a:b",
        "host:port:secret",
    ] {
        let error = profiles::parse_import(text).err().unwrap();
        assert!(!error.message.contains("secret"));
    }
    assert!(profiles::parse_import(&"x".repeat(256 * 1024 + 1)).is_err());
    let over = (0..101)
        .map(|i| format!("p{i}|socks5h://127.0.0.1:{}", 1000 + i))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(profiles::parse_import(&over).is_err());
    let invalid = profiles::Profile {
        id: "bad".into(),
        label: "Bad".into(),
        url: "not-url".into(),
    }
    .sanitized();
    assert!(invalid["host"].is_null());
    assert_eq!(invalid["credentials_present"], false);
    for host in ["www.fiverr.com", "mira.fiverr.com", "help.fiverr.com"] {
        assert!(approved_destination(&Url::parse(&format!("https://{host}/")).unwrap()).is_ok());
    }
    assert!(approved_destination(&Url::parse("https://www.fiverr.com:444/").unwrap()).is_err());
}
#[test]
fn port_precedence_and_exhausted_settings_never_reset_state() {
    let s = Settings::default();
    assert_eq!(s.effective_port(None, None).unwrap(), 1597);
    assert_eq!(s.effective_port(Some(1111), Some("invalid")).unwrap(), 1111);
    assert_eq!(s.effective_port(None, Some("2222")).unwrap(), 2222);
    for value in ["", " 1597", "-1", "65536", "0", "۱۲۳"] {
        assert!(s.effective_port(None, Some(value)).is_err());
    }
    assert!(s.effective_port(Some(0), None).is_err());
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    store
        .update_value("settings", |_| {
            Ok((
                serde_json::to_value(Settings {
                    route_generation: u64::MAX,
                    ..Settings::default()
                })
                .unwrap(),
                (),
            ))
        })
        .unwrap();
    assert!(
        Settings::change(&store, true, |s| {
            s.http_port = 4444;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(Settings::load(&store).unwrap().http_port, 1597);
    store
        .update_value("settings", |_| {
            Ok((serde_json::json!({"corrupt":true}), ()))
        })
        .unwrap();
    assert!(Settings::load(&store).is_err());
    assert!(Settings::change(&store, false, |_| Ok(())).is_err());
}
