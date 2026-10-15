use fiverr_agent_mcp::{
    network::Settings,
    session,
    storage::{Root, Store},
};

#[test]
fn local_cookie_import_is_private_unverified_route_bound_and_forgettable() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(temp.path()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let input = root.safe_path("config", "session-input.txt").unwrap();
    root.write_new(&input, b"session=synthetic-private-value; other=value")
        .unwrap();
    let imported = session::import_cookie(&store, &input, "fixture_owner").unwrap();
    assert_eq!(imported["state"], "credential_saved");
    assert_eq!(imported["live_authenticated"], false);
    assert!(!imported.to_string().contains("synthetic-private-value"));
    assert!(
        session::status(&store).unwrap()["configured"]
            .as_bool()
            .unwrap()
    );
    let backup = store.backup("without-session.sqlite3").unwrap();
    assert!(
        !std::fs::read(backup)
            .unwrap()
            .windows(b"synthetic-private-value".len())
            .any(|s| s == b"synthetic-private-value")
    );
    Settings::change(&store, true, |_| Ok(())).unwrap();
    assert_eq!(session::status(&store).unwrap()["state"], "route_changed");
    session::forget(&store).unwrap();
    assert_eq!(session::status(&store).unwrap()["configured"], false);
    assert_eq!(
        std::fs::read(input).unwrap(),
        b"session=synthetic-private-value; other=value"
    );
}

#[test]
fn malformed_cookie_and_username_do_not_replace_saved_session() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(temp.path()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let good = root.safe_path("config", "good.txt").unwrap();
    root.write_new(&good, b"session=synthetic-original")
        .unwrap();
    session::import_cookie(&store, &good, "fixture_owner").unwrap();
    for username in ["", "../bad", "user.name", "ümlaut", &"a".repeat(81)] {
        assert!(session::import_cookie(&store, &good, username).is_err());
    }
    let large = root.safe_path("config", "large.txt").unwrap();
    root.write_new(&large, &vec![b'x'; 16385]).unwrap();
    assert!(session::import_cookie(&store, &large, "fixture_owner").is_err());
    for (i, value) in [
        "",
        "not-a-cookie",
        "session=x\r\nX-Evil: yes",
        "session=x\n",
        "session=x;",
        "bad name=x",
        "session=x; session=y",
    ]
    .into_iter()
    .enumerate()
    {
        let input = root.safe_path("config", &format!("bad{i}.txt")).unwrap();
        root.write_new(&input, value.as_bytes()).unwrap();
        assert!(session::import_cookie(&store, &input, "fixture_owner").is_err());
    }
    assert_eq!(
        session::status(&store).unwrap()["username"],
        "fixture_owner"
    );
    assert_eq!(
        session::status(&store).unwrap()["state"],
        "credential_saved"
    );
}
