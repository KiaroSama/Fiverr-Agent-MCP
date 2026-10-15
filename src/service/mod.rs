pub mod account;
pub mod actions;
pub mod analysis;
pub mod bindings;
pub mod local;
pub mod portability;
pub mod productivity;
pub mod validation;
use crate::{
    core::{ErrorCode, Result, envelope, error, invalid},
    network::Settings,
    storage::Store,
};
use serde_json::{Value, json};

pub fn account_scope(store: &Store, args: &Value) -> Result<String> {
    let selected = Settings::load(store)?.account_id;
    if args
        .get("account_id")
        .and_then(Value::as_str)
        .is_some_and(|v| v != selected)
    {
        return Err(error(
            ErrorCode::PermissionDenied,
            "Account scope does not match the owner-selected workspace.",
        ));
    }
    Ok(selected)
}
pub fn dispatch(store: &Store, name: &str, args: Value) -> Result<Value> {
    if args.to_string().len() > 1024 * 1024 {
        return Err(invalid("Tool arguments exceed 1 MiB."));
    }
    if !crate::server::catalog::TOOLS
        .iter()
        .any(|tool| tool.0 == name)
    {
        return Err(error(
            ErrorCode::FeatureUnavailable,
            "Dedicated workflow not implemented; inspect capability dispositions.",
        ));
    }
    crate::server::validation::arguments(name, &args)?;
    match name {
        "fiverr_status" | "fiverr_health" => Ok(envelope(
            json!({"alive":true,"version":env!("CARGO_PKG_VERSION"),"route":Settings::load(store)?.sanitized(),"session":crate::session::status(store)?,"live_verified":false}),
            json!({"source":"local_runtime"}),
        )),
        "fiverr_capabilities" | "fiverr_account_capabilities" => Ok(envelope(
            crate::server::catalog::capabilities()?,
            json!({"source":"capability_registry"}),
        )),
        "fiverr_settings_get" => Ok(envelope(
            Settings::load(store)?.sanitized(),
            json!({"source":"local_settings"}),
        )),
        "fiverr_account_get" | "fiverr_session_status" => account::status(store),
        "fiverr_auth_start" => account::handoff("manual_sign_in", &args),
        "fiverr_record_save" | "fiverr_draft_save" => local::save(store, &args),
        "fiverr_record_get" => local::get(store, &args),
        "fiverr_draft_get" => productivity::draft(store, &args),
        "fiverr_records_list" => productivity::collection(store, &args, false, false),
        "fiverr_drafts_list" => productivity::collection(store, &args, true, false),
        "fiverr_records_search" => productivity::collection(store, &args, false, true),
        "fiverr_notes_save" => productivity::save_as(store, &args, "note"),
        "fiverr_task_save" => productivity::save_as(store, &args, "task"),
        "fiverr_tasks_list" => {
            let mut args = args;
            args["kind"] = json!("task");
            productivity::collection(store, &args, true, false)
        }
        "fiverr_snapshot_create" => productivity::snapshot(store, &args),
        "fiverr_reminders_list" => productivity::reminders(store, &args),
        "fiverr_record_history" | "fiverr_gig_history" => local::history(store, &args),
        "fiverr_import_validate" => local::import(store, &args, false),
        "fiverr_import" => local::import(store, &args, true),
        "fiverr_export_prepare" => portability::export(store, &args),
        "fiverr_snapshot_diff" => portability::diff(store, &args),
        "fiverr_validate" => Ok(envelope(
            validation::validate(
                local::string(&args, "kind")?,
                args.get("payload")
                    .ok_or_else(|| invalid("payload is required."))?,
            )?,
            json!({"source":"local_validation"}),
        )),
        "fiverr_market_analyze" => analysis::analyze(&args).map_err(invalid),
        "fiverr_action_prepare" => actions::prepare(store, local::string(&args, "kind")?, &args),
        "fiverr_action_get" => actions::get(store, local::string(&args, "action_id")?),
        "fiverr_action_cancel" => actions::cancel(store, local::string(&args, "action_id")?),
        "fiverr_action_commit" => actions::commit(store, local::string(&args, "action_id")?),
        "fiverr_action_reconcile" => actions::reconcile(store, local::string(&args, "action_id")?),
        "fiverr_manual_handoff" => account::handoff(local::string(&args, "operation")?, &args),
        _ => Err(error(
            ErrorCode::FeatureUnavailable,
            "Tool is not an implemented workflow. Inspect fiverr_capabilities for local alternatives and actual prerequisites.",
        )),
    }
}
