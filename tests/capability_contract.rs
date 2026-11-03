use fiverr_agent_mcp::server::catalog;

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
    }
    assert_eq!(registry["live_read_verified"], false);
    assert_eq!(registry["live_write_verified"], false);
}
