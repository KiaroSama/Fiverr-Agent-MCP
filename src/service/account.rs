use crate::{
    core::{ErrorCode, Result, envelope, error},
    network::Settings,
    storage::Store,
};
use serde_json::{Value, json};
pub fn status(store: &Store) -> Result<Value> {
    let settings = Settings::load(store)?;
    Ok(envelope(
        json!({"workspace_account":settings.account_id,"live_authenticated":false,"session":"not_verified","browser_configured":settings.browser_executable.is_some(),"roles":"unknown","mira_eligibility":"unknown","human_sign_in_required":true,"permission_required":true,"route_assurance":"not_verified"}),
        json!({"source":"local_settings","live_verified":false}),
    ))
}
pub fn handoff(operation: &str, args: &Value) -> Result<Value> {
    crate::core::bounded_text(operation, 100)?;
    if args.to_string().len() > 256 * 1024 {
        return Err(crate::core::invalid("Handoff context exceeds its limit."));
    }
    Ok(envelope(
        json!({"operation":operation,"mode":"manual_handoff","completed":false,"url":"https://www.fiverr.com/","instructions":["Open the official Fiverr website using your owner-controlled browser and chosen network policy.","Sign in yourself and verify the account, target and current platform state.","Review the local draft and perform only the exact intended operation.","Record actual platform evidence afterward; a handoff never proves completion."],"financial_execution_enabled":false}),
        json!({"source":"documented_human_workflow"}),
    ))
}
pub fn require_live() -> Result<Value> {
    Err(error(
        ErrorCode::PermissionDenied,
        "No permitted account interface and strict browser-route assurance have been verified. Use manual sign-in/handoff; do not treat a local draft or fixture as a live result.",
    ))
}
