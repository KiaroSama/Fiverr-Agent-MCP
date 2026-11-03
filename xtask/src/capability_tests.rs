use super::*;

#[test]
fn capability_validator_rejects_missing_or_contradictory_dimensions() {
    let root = crate::root();
    let registry = json(&root, "capabilities.json").unwrap();
    capability_registry(&root, &registry).unwrap();
    for field in ["effect", "mode", "adapter", "evidence"] {
        let mut changed = registry.clone();
        changed["workflows"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            capability_registry(&root, &changed).is_err(),
            "accepted missing {field}"
        );
    }
    for (field, wrong) in [
        ("effect", serde_json::json!("imaginary")),
        ("mode", serde_json::json!("imaginary")),
        ("implementation", serde_json::json!("imaginary")),
        ("verification", serde_json::json!("live_verified")),
        ("availability", serde_json::json!("imaginary")),
        ("required_approval", serde_json::json!("model_boolean")),
    ] {
        let mut changed = registry.clone();
        changed["workflows"][0][field] = wrong;
        assert!(
            capability_registry(&root, &changed).is_err(),
            "accepted invalid {field}"
        );
    }
    let mut changed = registry.clone();
    changed["workflows"][1]["availability"] = serde_json::json!("available");
    assert!(
        capability_registry(&root, &changed).is_err(),
        "unsupported operation advertised as available"
    );
    let mut changed = registry.clone();
    changed["workflows"][1]["requirement_id"] = changed["workflows"][0]["requirement_id"].clone();
    assert!(capability_registry(&root, &changed).is_err());
    let mut changed = registry.clone();
    changed["workflows"][0]["evidence"] = serde_json::json!(["docs/missing-evidence.md"]);
    assert!(capability_registry(&root, &changed).is_err());
}
