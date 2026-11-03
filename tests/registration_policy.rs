use fiverr_agent_mcp::{cli::registration, core::ErrorCode, storage::Root};
use serde_json::json;

#[test]
fn same_install_registration_preserves_system_temp_and_custom_options_without_backup() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    for (index, env) in [
        json!({"FIVERR_AGENT_TEMP_MODE":"system","CUSTOM_OPTION":"preserved"}),
        registration::desktop(&root, true)["Fiverr-Agent-mcp"]["env"].clone(),
        json!({"CUSTOM_OPTION":"preserved"}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut entry = registration::desktop(&root, true)["Fiverr-Agent-mcp"].clone();
        entry["env"] = env;
        entry["custom"] = json!({"enabled":false});
        let config = json!({"mcpServers":{"Fiverr-Agent-mcp":entry,"other":{"command":"keep"}},"theme":"dark"});
        let bytes = serde_json::to_vec_pretty(&config).unwrap();
        let path = root
            .safe_path("config", &format!("client-{index}.json"))
            .unwrap();
        root.write_new(&path, &bytes).unwrap();
        let before = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        let result = registration::register(&root, &path, false).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            before
        );
        assert_eq!(result["backup_created"], false);
    }
}

#[test]
fn contradictory_existing_temporary_policy_is_not_silently_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    for (index, env) in [
        json!({"FIVERR_AGENT_TEMP_MODE":"unknown"}),
        json!({"FIVERR_AGENT_TEMP_MODE":""}),
        json!({"FIVERR_AGENT_TEMP_MODE":false}),
        json!({"FIVERR_AGENT_TEMP_MODE":"system","TEMP":"rooted-temp"}),
        json!({"FIVERR_AGENT_TEMP_MODE":"rooted","TMP":"foreign-temp"}),
        json!({"fiverr_agent_temp_mode":"system"}),
        json!({"FIVERR_AGENT_TEMP_MODE":"system","tmp":"foreign-temp"}),
        json!({"CUSTOM_OPTION":false}),
        json!("not-an-environment-object"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut entry = registration::desktop(&root, true)["Fiverr-Agent-mcp"].clone();
        entry["env"] = env;
        let bytes = serde_json::to_vec(&json!({"mcpServers":{"Fiverr-Agent-mcp":entry}})).unwrap();
        let path = root
            .safe_path("config", &format!("conflict-{index}.json"))
            .unwrap();
        root.write_new(&path, &bytes).unwrap();
        let before = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(
            registration::register(&root, &path, false)
                .unwrap_err()
                .code,
            ErrorCode::ActionStateChanged
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            before
        );
    }
}
