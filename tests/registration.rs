use fiverr_agent_mcp::{cli::registration, storage::Root};
use serde_json::json;

#[test]
fn registration_rejects_conflicts_and_invalid_documents_without_overwriting() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap();
    assert!(registration::register(&root, std::path::Path::new("relative.json"), false).is_err());
    assert!(registration::register(&root, root.path(), false).is_err());
    for (index, bytes) in [
        b"not JSON".as_slice(),
        b"[]",
        br#"{"mcpServers":false}"#,
        br#"{"mcpServers":{"Fiverr-Agent-mcp":{"command":"another-install"}}}"#,
    ]
    .into_iter()
    .enumerate()
    {
        let path = root
            .safe_path("config", &format!("bad-{index}.json"))
            .unwrap();
        root.write_new(&path, bytes).unwrap();
        assert!(registration::register(&root, &path, false).is_err());
        assert!(registration::register(&root, &path, true).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let path = root.safe_path("config", "new-entry.json").unwrap();
    root.write_new(&path, br#"{"theme":"preserved"}"#).unwrap();
    registration::register(&root, &path, false).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap()).unwrap()["theme"],
        "preserved"
    );
}

#[test]
fn claude_entry_uses_owner_requested_name_and_preserves_other_servers() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap();
    let config = root.safe_path("config", "client.json").unwrap();
    root.write_new(
        &config,
        br#"{"mcpServers":{"unrelated":{"command":"unchanged"}},"theme":"dark"}"#,
    )
    .unwrap();
    let entry = registration::desktop(&root, false);
    assert_eq!(
        entry["mcpServers"]["Fiverr-Agent-mcp"]["args"],
        json!(["bridge"])
    );
    assert!(entry["mcpServers"].get("fiverr-local").is_none());
    registration::register(&root, &config, false).unwrap();
    let read = || {
        serde_json::from_slice::<serde_json::Value>(&root.read_bounded(&config, 4096).unwrap())
            .unwrap()
    };
    assert_eq!(read()["mcpServers"]["unrelated"]["command"], "unchanged");
    assert_eq!(read()["theme"], "dark");
    assert_eq!(
        read()["mcpServers"]["Fiverr-Agent-mcp"],
        entry["mcpServers"]["Fiverr-Agent-mcp"]
    );
    registration::register(&root, &config, true).unwrap();
    assert!(read()["mcpServers"].get("Fiverr-Agent-mcp").is_none());
    assert_eq!(read()["mcpServers"]["unrelated"]["command"], "unchanged");
}
