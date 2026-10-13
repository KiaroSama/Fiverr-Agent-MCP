use fiverr_agent_mcp::{core::ErrorCode, server::validation};
use serde_json::json;

#[test]
fn variant_specific_analysis_fields_are_permitted_but_strict_tools_reject_extras() {
    validation::arguments(
        "fiverr_market_analyze",
        &json!({"kind":"keyword","provenance":{},"keywords":[]}),
    )
    .unwrap();
    assert_eq!(
        validation::arguments(
            "fiverr_validate",
            &json!({"kind":"note","payload":{"body":"Keep"},"keywords":[]}),
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );
}
