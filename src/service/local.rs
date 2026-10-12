use crate::{
    core::{Provenance, Record, Result, envelope, invalid, now},
    storage::Store,
};
use serde_json::{Value, json};
pub fn save(store: &Store, arguments: &Value) -> Result<Value> {
    let kind = string(arguments, "kind")?;
    let payload = arguments
        .get("payload")
        .ok_or_else(|| invalid("payload is required."))?
        .clone();
    super::validation::validate(kind, &payload)?;
    let account = super::account_scope(store, arguments)?;
    let id = arguments
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let record = Record {
        id,
        kind: kind.into(),
        account_id: account,
        version: 0,
        draft: true,
        payload,
        provenance: Provenance {
            source: "local_draft".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    };
    let saved = store.save(
        record,
        arguments.get("expected_version").and_then(Value::as_u64),
    )?;
    Ok(envelope(
        serde_json::to_value(saved).map_err(|_| invalid("Record serialization failed."))?,
        json!({"source":"local_draft","remote_effect":false}),
    ))
}
pub fn get(store: &Store, arguments: &Value) -> Result<Value> {
    let account = super::account_scope(store, arguments)?;
    let record = store.get(
        &account,
        string(arguments, "kind")?,
        string(arguments, "id")?,
    )?;
    Ok(envelope(
        serde_json::to_value(&record).map_err(|_| invalid("Serialization failed."))?,
        serde_json::to_value(record.provenance)
            .map_err(|_| invalid("Provenance serialization failed."))?,
    ))
}
pub fn list(store: &Store, arguments: &Value) -> Result<Value> {
    let account = super::account_scope(store, arguments)?;
    let kind = string(arguments, "kind")?;
    let offset = arguments
        .get("cursor")
        .and_then(Value::as_str)
        .map(|s| {
            s.parse::<usize>()
                .map_err(|_| invalid("Invalid pagination cursor."))
        })
        .transpose()?
        .unwrap_or(0);
    let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
    let records = store.list(&account, kind, offset, limit)?;
    let has_more = records.len() == limit;
    Ok(
        json!({"data":records,"warnings":["This collection contains only explicit imports/local drafts; it is not a live account list."],"provenance":{"source":"local_workspace"},"pagination":{"limit":limit,"next_cursor":if has_more{Some((offset+limit).to_string())}else{None},"partial":true}}),
    )
}
pub fn history(store: &Store, arguments: &Value) -> Result<Value> {
    let account = super::account_scope(store, arguments)?;
    Ok(envelope(
        json!(store.history(
            &account,
            string(arguments, "kind")?,
            string(arguments, "id")?
        )?),
        json!({"source":"local_history"}),
    ))
}
pub fn import(store: &Store, arguments: &Value, commit: bool) -> Result<Value> {
    let raw = arguments
        .get("records")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("records must be an explicit array."))?;
    if raw.len() > 1000 || arguments.to_string().len() > 2 * 1024 * 1024 {
        return Err(invalid("Import exceeds 1000 records or 2 MiB."));
    }
    let account = super::account_scope(store, arguments)?;
    let records: Vec<Record> = raw
        .iter()
        .map(|v| {
            serde_json::from_value(v.clone())
                .map_err(|_| invalid("Invalid imported record schema."))
        })
        .collect::<Result<_>>()?;
    for record in &records {
        if record.account_id != account {
            return Err(invalid(
                "Import account does not match the selected workspace.",
            ));
        }
        if record.provenance.source != "user_import" {
            return Err(invalid(
                "Imports must be labeled user_import, never live_verified/fixture.",
            ));
        }
        record.provenance.validate()?;
        super::validation::validate(&record.kind, &record.payload)?;
    }
    let count = if commit {
        store.import(&records)?
    } else {
        records.len()
    };
    Ok(envelope(
        json!({"valid":true,"count":count,"imported":commit,"remote_verified":false}),
        json!({"source":"user_import"}),
    ))
}
pub fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{key} must be text.")))
}
