use crate::{
    core::{ErrorCode, Result, envelope, error, hash, invalid, now, storage_error, validate_id},
    network::Settings,
    storage::Store,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub kind: String,
    pub target_id: String,
    pub account_id: String,
    pub payload: Value,
    pub payload_hash: String,
    pub route_generation: u64,
    pub expires_at: i64,
    pub state: String,
    pub created_at: String,
    pub approved_at: Option<String>,
    pub effect: String,
    #[serde(default)]
    pub files: Vec<super::bindings::FileBinding>,
    #[serde(default)]
    pub record_binding: Option<super::bindings::RecordBinding>,
}
impl Action {
    fn value(&self) -> Result<Value> {
        serde_json::to_value(self).map_err(|_| storage_error())
    }
}
pub fn prepare(store: &Store, kind: &str, args: &Value) -> Result<Value> {
    let settings = Settings::load(store)?;
    let target = super::local::string(args, "target_id")?;
    validate_id(target)?;
    let payload = args
        .get("payload")
        .cloned()
        .ok_or_else(|| invalid("Action payload is required."))?;
    if crate::core::KINDS.contains(&kind) {
        super::validation::validate(kind, &payload)?;
    } else {
        return Err(invalid("Unknown prepared action kind."));
    }
    let files = super::bindings::files(store, args)?;
    let record_binding = super::bindings::record(store, &settings.account_id, args)?;
    let action = Action {
        files,
        record_binding,
        id: uuid::Uuid::new_v4().to_string(),
        kind: kind.into(),
        target_id: target.into(),
        account_id: settings.account_id,
        payload_hash: hash(payload.to_string().as_bytes()),
        payload,
        route_generation: settings.route_generation,
        expires_at: chrono::Utc::now().timestamp() + 300,
        state: "prepared".into(),
        created_at: now(),
        approved_at: None,
        effect: if ["offer", "order", "earnings", "subscription", "ads"].contains(&kind) {
            "financial_or_contractual".into()
        } else {
            "remote_write".into()
        },
    };
    store.transaction(|tx| {
        tx.execute(
            "INSERT INTO actions(id,state,json) VALUES(?1,?2,?3)",
            params![action.id, action.state, action.value()?.to_string()],
        )
        .map_err(|_| storage_error())?;
        Store::check_capacity(tx)?;
        Ok(())
    })?;
    Ok(envelope(
        action.value()?,
        json!({"source":"local_preparation","remote_executed":false,"approval":"owner_cli_required","adapter":"manual_handoff_until_permitted"}),
    ))
}
fn read(tx: &rusqlite::Transaction<'_>, id: &str) -> Result<Action> {
    validate_id(id)?;
    let text: Option<String> = tx
        .query_row("SELECT json FROM actions WHERE id=?1", [id], |r| r.get(0))
        .optional()
        .map_err(|_| storage_error())?;
    serde_json::from_str(
        &text.ok_or_else(|| error(ErrorCode::ResourceNotFound, "Prepared action not found."))?,
    )
    .map_err(|_| storage_error())
}
fn write(tx: &rusqlite::Transaction<'_>, action: &Action) -> Result<()> {
    tx.execute(
        "UPDATE actions SET state=?1,json=?2 WHERE id=?3",
        params![action.state, action.value()?.to_string(), action.id],
    )
    .map_err(|_| storage_error())?;
    Ok(())
}
fn fresh(action: &Action, settings: &Settings) -> Result<()> {
    if action.expires_at < chrono::Utc::now().timestamp() {
        return Err(error(
            ErrorCode::ConfirmationExpired,
            "Action expired; prepare again and review its current state.",
        ));
    }
    if action.route_generation != settings.route_generation
        || action.account_id != settings.account_id
        || action.payload_hash != hash(action.payload.to_string().as_bytes())
    {
        return Err(error(
            ErrorCode::ActionStateChanged,
            "Action/account/route/payload changed; prepare again.",
        ));
    }
    Ok(())
}
fn validate_bindings(store: &Store, tx: &rusqlite::Transaction<'_>, action: &Action) -> Result<()> {
    super::bindings::unchanged(store, &action.files)?;
    if let Some(binding) = &action.record_binding {
        let version: Option<i64> = tx
            .query_row(
                "SELECT version FROM records WHERE account=?1 AND kind=?2 AND id=?3",
                params![action.account_id, binding.kind, binding.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| storage_error())?;
        if version
            .map(|value| u64::try_from(value).map_err(|_| storage_error()))
            .transpose()?
            != Some(binding.version)
        {
            return Err(error(
                ErrorCode::ActionStateChanged,
                "Prepared draft changed; review a new plan.",
            ));
        }
    }
    Ok(())
}
pub fn get(store: &Store, id: &str) -> Result<Value> {
    store.transaction(|tx| {
        let action = read(tx, id)?;
        if action.account_id != Settings::in_transaction(tx)?.account_id {
            return Err(error(
                ErrorCode::ResourceNotFound,
                "Action not found in selected account scope.",
            ));
        }
        Ok(envelope(
            action.value()?,
            json!({"source":"local_action_journal"}),
        ))
    })
}
pub fn list(store: &Store) -> Result<Value> {
    store.transaction(|tx| {
        let account=Settings::in_transaction(tx)?.account_id;
        let mut statement = tx
            .prepare("SELECT json FROM actions WHERE json_extract(json,'$.account_id')=?1 ORDER BY id LIMIT 100")
            .map_err(|_| storage_error())?;
        let rows = statement
            .query_map([account], |r| r.get::<_, String>(0))
            .map_err(|_| storage_error())?;
        let data: Vec<Value> = rows
            .map(|r| {
                serde_json::from_str(&r.map_err(|_| storage_error())?).map_err(|_| storage_error())
            })
            .collect::<Result<_>>()?;
        Ok(envelope(
            json!(data),
            json!({"source":"local_action_journal","limit":100}),
        ))
    })
}
pub fn approve(store: &Store, id: &str, trusted_owner_control: bool) -> Result<Value> {
    if !trusted_owner_control {
        return Err(error(
            ErrorCode::ConfirmationRequired,
            "Only the interactive owner control can approve; model confirmations are refused.",
        ));
    }
    store.transaction(|tx| {
        let settings = Settings::in_transaction(tx)?;
        let mut action = read(tx, id)?;
        fresh(&action, &settings)?;
        validate_bindings(store, tx, &action)?;
        if action.state != "prepared" {
            return Err(error(
                ErrorCode::ActionStateChanged,
                "Action is not prepared; approval is single-use.",
            ));
        }
        action.state = "approved".into();
        action.approved_at = Some(now());
        write(tx, &action)?;
        Ok(envelope(action.value()?, json!({"source":"owner_control"})))
    })
}
pub fn cancel(store: &Store, id: &str) -> Result<Value> {
    store.transaction(|tx| {
        let mut action = read(tx, id)?;
        if action.account_id != Settings::in_transaction(tx)?.account_id {
            return Err(error(
                ErrorCode::ResourceNotFound,
                "Action not found in selected account scope.",
            ));
        }
        if !["prepared", "approved"].contains(&action.state.as_str()) {
            return Err(error(
                ErrorCode::ActionStateChanged,
                "Dispatched or completed actions cannot be cancelled or replayed.",
            ));
        }
        action.state = "cancelled".into();
        write(tx, &action)?;
        Ok(envelope(action.value()?, json!({"source":"local_cancel"})))
    })
}
pub fn commit(store: &Store, id: &str) -> Result<Value> {
    let action = store.transaction(|tx| {
        let settings = Settings::in_transaction(tx)?;
        let mut action = read(tx, id)?;
        if action.account_id != settings.account_id {
            return Err(error(
                ErrorCode::ResourceNotFound,
                "Action not found in selected account scope.",
            ));
        }
        if ["executing", "outcome_unknown"].contains(&action.state.as_str()) {
            return Err(error(
                ErrorCode::ActionOutcomeUnknown,
                "Outcome is unknown; reconciliation is required, no replay.",
            ));
        }
        fresh(&action, &settings)?;
        validate_bindings(store, tx, &action)?;
        if action.state != "approved" {
            return Err(error(
                ErrorCode::ConfirmationRequired,
                "Action requires fresh server-side human approval and must not be committed twice.",
            ));
        }
        // No unverified Fiverr write adapter can dispatch. Approval is consumed for the explicit handoff.
        action.state = "manual_handoff".into();
        write(tx, &action)?;
        Ok(action)
    })?;
    let mut data = super::account::handoff(&action.kind, &action.payload)?;
    data["data"]["action_id"] = json!(id);
    data["data"]["remote_executed"] = json!(false);
    data["data"]["journal_state"] = json!("manual_handoff");
    Ok(data)
}
pub fn reconcile(store: &Store, id: &str) -> Result<Value> {
    let current = get(store, id)?;
    if current["data"]["state"] == "outcome_unknown" || current["data"]["state"] == "executing" {
        return Err(error(
            ErrorCode::ActionOutcomeUnknown,
            "No verified remote postcondition adapter is available. Owner must inspect the exact target; automatic replay remains blocked.",
        ));
    }
    Ok(current)
}
