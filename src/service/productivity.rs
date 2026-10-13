use crate::{
    core::{Provenance, Record, Result, bounded_text, envelope, hash, invalid, now},
    storage::Store,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    account: String,
    kind: String,
    drafts: bool,
    query_hash: String,
    after: String,
}
pub fn collection(store: &Store, args: &Value, drafts: bool, search: bool) -> Result<Value> {
    let account = super::account_scope(store, args)?;
    let kind = super::local::string(args, "kind")?;
    crate::core::validate_kind(kind)?;
    let query = if search {
        let q = super::local::string(args, "query")?;
        bounded_text(q, 256)?;
        if q.trim().is_empty() {
            return Err(invalid("Search query must not be empty."));
        }
        Some(q)
    } else {
        None
    };
    let query_hash = hash(query.unwrap_or("").as_bytes());
    let after = match args.get("cursor") {
        None => None,
        Some(value) => {
            let value = value
                .as_str()
                .ok_or_else(|| invalid("Cursor must be opaque text."))?;
            let cursor: Cursor = serde_json::from_slice(&decode(value)?)
                .map_err(|_| invalid("Invalid collection cursor."))?;
            if cursor.account != account
                || cursor.kind != kind
                || cursor.drafts != drafts
                || cursor.query_hash != query_hash
            {
                return Err(invalid(
                    "Cursor does not belong to this account/kind/filter/query.",
                ));
            }
            Some(cursor.after)
        }
    };
    let limit = match args.get("limit") {
        None => 20,
        Some(value) => value
            .as_u64()
            .filter(|n| *n > 0 && *n <= 100)
            .ok_or_else(|| invalid("Limit must be an integer between 1 and 100."))?
            as usize,
    };
    let (records, more) = store.page(&account, kind, after.as_deref(), limit, drafts, query)?;
    let cursor = if more {
        records
            .last()
            .map(|r| Cursor {
                account: account.clone(),
                kind: kind.into(),
                drafts,
                query_hash,
                after: r.id.clone(),
            })
            .map(|c| {
                serde_json::to_vec(&c)
                    .map(|v| encode(&v))
                    .map_err(|_| invalid("Cannot serialize collection cursor."))
            })
            .transpose()?
    } else {
        None
    };
    Ok(
        json!({"data":records,"warnings":["Only explicit local drafts/imports; never a live Fiverr collection."],"provenance":{"source":"local_workspace"},"pagination":{"limit":limit,"next_cursor":cursor,"partial":false,"live_collection":false}}),
    )
}
pub fn draft(store: &Store, args: &Value) -> Result<Value> {
    let value = super::local::get(store, args)?;
    if value["data"]["draft"] != true {
        return Err(crate::core::error(
            crate::core::ErrorCode::ResourceNotFound,
            "This record is an imported observation, not a local draft.",
        ));
    }
    Ok(value)
}
pub fn save_as(store: &Store, args: &Value, kind: &str) -> Result<Value> {
    let mut args = args.clone();
    args["kind"] = json!(kind);
    super::local::save(store, &args)
}
pub fn snapshot(store: &Store, args: &Value) -> Result<Value> {
    let account = super::account_scope(store, args)?;
    let record = store.get(
        &account,
        super::local::string(args, "kind")?,
        super::local::string(args, "id")?,
    )?;
    let snapshot = Record {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "snapshot".into(),
        account_id: account,
        version: 0,
        draft: true,
        payload: json!({"record":record,"captured_at":now(),"live_verified":false}),
        provenance: Provenance {
            source: "local_draft".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    };
    let saved = store.save(snapshot, None)?;
    Ok(envelope(
        json!(saved),
        json!({"source":"local_snapshot","live_verified":false}),
    ))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReminderCursor {
    version: u8,
    account: String,
    after: String,
    observed_at: String,
}
pub fn reminders(store: &Store, args: &Value) -> Result<Value> {
    let account = super::account_scope(store, args)?;
    let current = chrono::Utc::now();
    let limit = match args.get("limit") {
        None => 100,
        Some(value) => value
            .as_u64()
            .filter(|n| *n > 0 && *n <= 100)
            .ok_or_else(|| invalid("Limit must be an integer between 1 and 100."))?
            as usize,
    };
    let (after, cutoff) = match args.get("cursor") {
        None => (None, current),
        Some(value) => {
            let value = value
                .as_str()
                .ok_or_else(|| invalid("Cursor must be opaque text."))?;
            let cursor: ReminderCursor = serde_json::from_slice(&decode(value)?)
                .map_err(|_| invalid("Invalid reminder cursor."))?;
            if cursor.version != 1 || cursor.account != account {
                return Err(invalid(
                    "Cursor does not belong to this account/reminder view.",
                ));
            }
            let cutoff = chrono::DateTime::parse_from_rfc3339(&cursor.observed_at)
                .map_err(|_| invalid("Invalid reminder observation time."))?
                .with_timezone(&chrono::Utc);
            if cutoff > current {
                return Err(invalid(
                    "Reminder observation time must not be in the future.",
                ));
            }
            (Some(cursor.after), cutoff)
        }
    };
    let (due, more) = store.due_tasks_page(&account, after.as_deref(), limit, cutoff)?;
    let cursor = if more {
        due.last()
            .map(|record| ReminderCursor {
                version: 1,
                account: account.clone(),
                after: record.id.clone(),
                observed_at: cutoff.to_rfc3339(),
            })
            .map(|cursor| {
                serde_json::to_vec(&cursor)
                    .map(|value| encode(&value))
                    .map_err(|_| invalid("Cannot serialize reminder cursor."))
            })
            .transpose()?
    } else {
        None
    };
    Ok(
        json!({"data":due,"warnings":["Local deadline inference; not a Fiverr deadline/notification. Pagination fixes the cutoff, not a mutable task snapshot."],"provenance":{"source":"local_reminder_inference","observed_at":cutoff.to_rfc3339()},"partial":more,"notifications_sent":false,"pagination":{"limit":limit,"next_cursor":cursor,"partial":false,"live_collection":false}}),
    )
}
fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode(value: &str) -> Result<Vec<u8>> {
    if value.is_empty()
        || value.len() > 2048
        || !value.len().is_multiple_of(2)
        || !value.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid("Invalid opaque cursor."));
    }
    (0..value.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&value[i..i + 2], 16).map_err(|_| invalid("Invalid opaque cursor."))
        })
        .collect()
}
