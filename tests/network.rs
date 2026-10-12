use fiverr_agent_mcp::{
    core::ErrorCode,
    network::{Mode, RouteClient, Settings, profiles},
    storage::{Root, Store},
};
#[test]
fn proxy_import_is_atomic_bounded_and_remotely_resolving() {
    let parsed = profiles::parse_import(
        "\u{feff}# profiles\nUK|socks5://a:p%3Ass@[::1]:1595\nUK|socks5h://a:p%3Ass@[::1]:1595",
    )
    .unwrap();
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].url.starts_with("socks5h://"));
    assert!(!parsed[0].sanitized().to_string().contains("p%3Ass"));
    assert!(profiles::parse_import("ok|http://127.0.0.1:8\nbad|not:a:proxy").is_err());
    assert!(profiles::normalize("socks4a://user@127.0.0.1:8").is_err());
}
#[test]
fn block_and_generation_changes_never_construct_a_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap()).unwrap();
    assert!(RouteClient::new(store.clone()).is_err());
    Settings::change(&store, true, |s| {
        s.mode = Mode::Direct;
        Ok(())
    })
    .unwrap();
    let client = RouteClient::new(store.clone()).unwrap();
    Settings::change(&store, true, |s| {
        s.mode = Mode::Blocked;
        Ok(())
    })
    .unwrap();
    assert_eq!(client.check().unwrap_err().code, ErrorCode::RouteBlocked);
}
#[test]
fn production_destination_allowlist_rejects_internal_and_credential_urls() {
    for url in [
        "https://127.0.0.1/a",
        "https://169.254.169.254/",
        "https://www.fiverr.com.evil.invalid/",
        "http://www.fiverr.com/",
        "https://x:y@www.fiverr.com/",
    ] {
        assert!(
            fiverr_agent_mcp::network::approved_destination(&url::Url::parse(url).unwrap())
                .is_err()
        );
    }
}
