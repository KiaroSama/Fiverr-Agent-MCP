use crate::core::{Result, storage_error};
use rmcp::model::Tool;
use serde_json::{Value, json};
use std::sync::Arc;

pub const TOOLS: &[(&str, &str, &str, bool)] = &[
    (
        "fiverr_status",
        "Inspect local route and runtime status without contacting Fiverr.",
        "empty",
        true,
    ),
    (
        "fiverr_health",
        "Read process health; no account/network probe.",
        "empty",
        true,
    ),
    (
        "fiverr_capabilities",
        "Read truthful implementation/verification/availability dimensions.",
        "empty",
        true,
    ),
    (
        "fiverr_settings_get",
        "Read sanitized owner settings; no proxy credentials.",
        "empty",
        true,
    ),
    (
        "fiverr_account_get",
        "Read local workspace account status, not verified live identity.",
        "empty",
        true,
    ),
    (
        "fiverr_session_status",
        "Read live-authentication prerequisites without collecting credentials.",
        "empty",
        true,
    ),
    (
        "fiverr_auth_start",
        "Return official-site manual sign-in instructions; does not authenticate.",
        "empty",
        true,
    ),
    (
        "fiverr_record_save",
        "Save a validated local draft; never uploads or sends.",
        "save",
        false,
    ),
    (
        "fiverr_record_get",
        "Read an imported record/draft within selected account.",
        "get",
        true,
    ),
    (
        "fiverr_records_list",
        "List bounded imported records/local drafts, not live account collections.",
        "list",
        true,
    ),
    (
        "fiverr_draft_get",
        "Read an explicit local draft by kind and ID.",
        "get",
        true,
    ),
    (
        "fiverr_drafts_list",
        "List local records with draft flags and source provenance.",
        "list",
        true,
    ),
    (
        "fiverr_record_history",
        "Read immutable local revisions, not platform moderation history.",
        "get",
        true,
    ),
    (
        "fiverr_import_validate",
        "Validate explicit user-import records without writing.",
        "import",
        true,
    ),
    (
        "fiverr_import",
        "Import explicit user records atomically; collisions roll back.",
        "import",
        false,
    ),
    (
        "fiverr_export_prepare",
        "Write an explicit bounded local export; existing files are not overwritten.",
        "export",
        false,
    ),
    (
        "fiverr_snapshot_diff",
        "Compare two imported/local records, not live account state.",
        "diff",
        true,
    ),
    (
        "fiverr_snapshot_create",
        "Capture immutable local record evidence; never a live refresh.",
        "get",
        false,
    ),
    (
        "fiverr_records_search",
        "Search imported/local payloads within selected account and kind.",
        "search",
        true,
    ),
    (
        "fiverr_notes_save",
        "Save a bounded private local note; never sends content.",
        "typed_save",
        false,
    ),
    (
        "fiverr_task_save",
        "Save a local task with an optional explicit RFC3339 reminder deadline.",
        "typed_save",
        false,
    ),
    (
        "fiverr_tasks_list",
        "List local task drafts with bounded scoped pagination.",
        "tasks",
        true,
    ),
    (
        "fiverr_reminders_list",
        "List due local tasks with scoped pagination; no platform deadline or notifications claimed.",
        "tasks",
        true,
    ),
    (
        "fiverr_validate",
        "Validate local gig/profile/offer/message/order draft structure.",
        "validate",
        true,
    ),
    (
        "fiverr_market_analyze",
        "Apply adapted deterministic keyword/pricing/dataset/observed metrics analysis to explicit provenance-tagged data.",
        "analysis",
        true,
    ),
    (
        "fiverr_action_prepare",
        "Prepare an immutable scoped action for human review; no remote write.",
        "prepare",
        false,
    ),
    (
        "fiverr_action_get",
        "Read prepared action and journal state.",
        "action",
        true,
    ),
    (
        "fiverr_action_cancel",
        "Cancel prepared/approved action; never reverses dispatched work.",
        "action",
        false,
    ),
    (
        "fiverr_action_commit",
        "Consume existing owner approval for manual handoff; no unverified Fiverr dispatch.",
        "action",
        false,
    ),
    (
        "fiverr_action_reconcile",
        "Inspect journal; unknown remote outcomes cannot be replayed.",
        "action",
        true,
    ),
    (
        "fiverr_manual_handoff",
        "Explain an exact official-site human operation without claiming completion.",
        "handoff",
        true,
    ),
];
pub fn schema(kind: &str) -> Value {
    let (properties, required) = match kind {
        "save" => (
            json!({"kind":{"type":"string","enum":crate::core::KINDS},"id":{"type":"string"},"payload":{"type":"object"},"expected_version":{"type":"integer","minimum":1}}),
            vec!["kind", "payload"],
        ),
        "get" => (
            json!({"kind":{"type":"string","enum":crate::core::KINDS},"id":{"type":"string"}}),
            vec!["kind", "id"],
        ),
        "typed_save" => (
            json!({"id":{"type":"string"},"payload":{"type":"object"},"expected_version":{"type":"integer","minimum":1}}),
            vec!["payload"],
        ),
        "tasks" => (
            json!({"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":{"type":"string"}}),
            vec![],
        ),
        "search" => (
            json!({"kind":{"type":"string","enum":crate::core::KINDS},"query":{"type":"string","minLength":1,"maxLength":256},"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":{"type":"string"}}),
            vec!["kind", "query"],
        ),
        "list" => (
            json!({"kind":{"type":"string","enum":crate::core::KINDS},"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":{"type":"string"}}),
            vec!["kind"],
        ),
        "import" => (
            json!({"records":{"type":"array","maxItems":1000,"items":{"type":"object"}}}),
            vec!["records"],
        ),
        "export" => (
            json!({"kind":{"type":"string","enum":crate::core::KINDS},"filename":{"type":"string"},"format":{"type":"string","enum":["json","csv","markdown","html"]}}),
            vec!["kind", "filename"],
        ),
        "diff" => (
            json!({"kind":{"type":"string"},"before_id":{"type":"string"},"after_id":{"type":"string"}}),
            vec!["kind", "before_id", "after_id"],
        ),
        "validate" => (
            json!({"kind":{"type":"string"},"payload":{"type":"object"}}),
            vec!["kind", "payload"],
        ),
        "prepare" => (
            json!({"kind":{"type":"string","enum":crate::core::KINDS},"target_id":{"type":"string"},"payload":{"type":"object"},"files":{"type":"array","maxItems":10,"items":{"type":"string"}},"record_binding":{"type":"object","required":["kind","id","version"]}}),
            vec!["kind", "target_id", "payload"],
        ),
        "action" => (json!({"action_id":{"type":"string"}}), vec!["action_id"]),
        "handoff" => (
            json!({"operation":{"type":"string","maxLength":100}}),
            vec!["operation"],
        ),
        "analysis" => {
            return json!({"type":"object","properties":{"kind":{"type":"string","enum":["keyword","pricing","dataset","metrics"]},"provenance":{"type":"object"}},"required":["kind","provenance"]});
        }
        _ => (json!({}), vec![]),
    };
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub fn tools() -> Vec<Tool> {
    TOOLS.iter().map(|(name,description,kind,readonly)|{
    let mut tool=Tool::new(*name,*description,Arc::new(schema(kind).as_object().cloned().unwrap_or_default()));
    tool.output_schema=Some(Arc::new(json!({"type":"object","properties":{"data":{},"warnings":{"type":"array","items":{"type":"string"}},"provenance":{"type":"object"},"error":{"type":"object","required":["code","message"]}},"anyOf":[{"required":["data"]},{"required":["error"]}]}).as_object().cloned().unwrap_or_default()));
    tool.annotations=serde_json::from_value(json!({"readOnlyHint":readonly,"destructiveHint":false,"idempotentHint":readonly,"openWorldHint":false})).ok();
    tool
}).collect()
}
pub fn capabilities() -> Result<Value> {
    serde_json::from_str(include_str!("../../capabilities.json")).map_err(|_| storage_error())
}
