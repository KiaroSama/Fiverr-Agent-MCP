use fiverr_agent_mcp::server::catalog;
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

#[test]
fn implemented_capability_effects_match_catalog_and_dispatch() {
    let registry = catalog::capabilities().unwrap();
    let rows = registry["workflows"].as_array().unwrap();
    assert_eq!(rows.len(), 122);
    let implemented: Vec<_> = rows
        .iter()
        .filter(|row| row["implementation"] == "implemented")
        .collect();
    assert_eq!(implemented.len(), catalog::TOOLS.len());
    for (name, _, _, readonly) in catalog::TOOLS {
        let matches: Vec<_> = implemented
            .iter()
            .filter(|row| row["tool"] == *name)
            .collect();
        assert_eq!(matches.len(), 1, "{name}");
        let expected = if *readonly {
            "read_only"
        } else {
            "local_write"
        };
        assert_eq!(
            matches[0]["effect"], expected,
            "{name}: actual local effect, not intended remote consequence"
        );
        assert_eq!(
            matches[0]["required_approval"],
            if *name == "fiverr_action_commit" {
                "human_owner"
            } else {
                "host_tool_consent"
            },
            "{name}: preparation and instructions do not enforce owner approval"
        );
    }
    assert_eq!(registry["live_read_verified"], false);
    assert_eq!(registry["live_write_verified"], false);
}

#[test]
fn capability_fixture_evidence_never_promotes_live_access() {
    let receipt: Value =
        serde_json::from_str(include_str!("../docs/testing/capability-evidence.json")).unwrap();
    assert_eq!(receipt["schema_version"], 1);
    assert_eq!(receipt["live_read_verified"], false);
    assert_eq!(receipt["live_write_verified"], false);
    assert_eq!(receipt["current_acceptance"], "pending");
    let historical = &receipt["historical_acceptance"];
    let sha = historical["source_sha"].as_str().unwrap();
    assert_eq!(sha.len(), 40);
    assert!(sha.bytes().all(|value| value.is_ascii_hexdigit()));
    assert!(
        historical["run_url"]
            .as_str()
            .unwrap()
            .starts_with("https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/runs/")
    );
    assert_eq!(historical["native_platforms"].as_array().unwrap().len(), 4);
    let cases = receipt["test_cases"].as_object().unwrap();
    assert_eq!(cases.len(), catalog::TOOLS.len());
    let registry = catalog::capabilities().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (tool, _, _, _) in catalog::TOOLS {
        let mapping = cases[*tool].as_array().unwrap();
        assert_eq!(mapping.len(), 2);
        let path = mapping[0].as_str().unwrap();
        let case = mapping[1].as_str().unwrap();
        assert!(path.starts_with("tests/") && path.ends_with(".rs"));
        assert!(!path.contains("..") && !path.contains('\\'));
        let source = std::fs::read_to_string(root.join(path)).unwrap();
        let body = source
            .split_once(&format!("fn {case}("))
            .unwrap_or_else(|| panic!("{tool}: missing mapped test {case}"))
            .1
            .split("\n#[")
            .next()
            .unwrap();
        assert!(
            body.contains(&format!("\"{tool}\"")),
            "{tool}: test does not call named dispatch"
        );
        assert!(body.contains("assert"), "{tool}: test lacks an assertion");
        let row = registry["workflows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["tool"] == *tool && row["implementation"] == "implemented")
            .unwrap();
        assert!(
            row["tests"]
                .as_array()
                .unwrap()
                .contains(&Value::from(path)),
            "{tool}: mapping drift"
        );
        assert_eq!(
            row["verification"], "not_verified",
            "historical execution is not current acceptance"
        );
    }
    let ids: BTreeSet<_> = registry["workflows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["requirement_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 122);
}
