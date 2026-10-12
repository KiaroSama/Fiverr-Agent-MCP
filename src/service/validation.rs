use crate::core::{Money, Result, bounded_text, invalid, validate_id, validate_kind};
use serde_json::{Value, json};
pub fn validate(kind: &str, payload: &Value) -> Result<Value> {
    validate_kind(kind)?;
    if !payload.is_object() || payload.to_string().len() > 256 * 1024 {
        return Err(invalid("Payload must be an object at most 256 KiB."));
    }
    walk(payload, 0)?;
    let mut warnings = vec![];
    match kind {
        "gig" => {
            required_text(payload, "title", 200)?;
            if let Some(tags) = payload.get("tags") {
                let tags = tags
                    .as_array()
                    .ok_or_else(|| invalid("tags must be an array."))?;
                if tags.len() > 20 {
                    return Err(invalid(
                        "More than 20 tags are refused; actual category limits require current evidence.",
                    ));
                }
            }
            warnings.push("Category/account-specific publication limits are not verified; this is local structural validation only.");
        }
        "message" => {
            required_text(payload, "body", 20000)?;
            validate_id(
                payload
                    .get("recipient_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("recipient_id is required."))?,
            )?;
        }
        "offer" => {
            required_text(payload, "scope", 20000)?;
            required_text(payload, "recipient_id", 128)?;
            let money: Money = serde_json::from_value(
                payload
                    .get("money")
                    .cloned()
                    .ok_or_else(|| invalid("Exact money is required."))?,
            )
            .map_err(|_| {
                invalid("Offer money must use integer minor units, currency and scale.")
            })?;
            money.validate()?;
            if money.minor <= 0 {
                return Err(invalid("Offer amount must be positive."));
            }
            let days = payload
                .get("delivery_days")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("delivery_days is required."))?;
            if days == 0 || days > 365 {
                return Err(invalid(
                    "Delivery period must be 1-365 days; account limits still require verification.",
                ));
            }
        }
        "delivery" => {
            required_text(payload, "order_id", 128)?;
            required_text(payload, "body", 20000)?;
            warnings.push("Local validation is not upload or submitted delivery; order ownership/state and file hashes need fresh verification.");
        }
        "task" => {
            required_text(payload, "title", 1000)?;
            if let Some(deadline) = payload.get("due_at") {
                let deadline = deadline
                    .as_str()
                    .ok_or_else(|| invalid("due_at must be RFC3339 text."))?;
                chrono::DateTime::parse_from_rfc3339(deadline)
                    .map_err(|_| invalid("due_at must be RFC3339."))?;
            }
            if let Some(status) = payload.get("status") {
                if !status
                    .as_str()
                    .is_some_and(|s| ["open", "done", "cancelled"].contains(&s))
                {
                    return Err(invalid("Task status must be open, done or cancelled."));
                }
            }
        }
        "note" => {
            required_text(payload, "body", 20000)?;
        }
        "template" => {
            required_text(payload, "title", 200)?;
            required_text(payload, "body", 20000)?;
        }
        "profile" => {
            if let Some(bio) = payload.get("bio") {
                bounded_text(
                    bio.as_str().ok_or_else(|| invalid("bio must be text."))?,
                    10000,
                )?;
            }
            warnings.push("Identity, certifications and account level must be factual; no identity/KYC modification is performed.");
        }
        _ => {}
    }
    Ok(
        json!({"valid":true,"validation_scope":"local_structural","warnings":warnings,"remote_validated":false}),
    )
}
fn required_text(payload: &Value, key: &str, max: usize) -> Result<()> {
    let value = payload
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{key} is required text.")))?;
    if value.trim().is_empty() {
        return Err(invalid(format!("{key} must not be empty.")));
    }
    bounded_text(value, max)
}
fn walk(value: &Value, depth: usize) -> Result<()> {
    if depth > 16 {
        return Err(invalid("Payload nesting exceeds 16 levels."));
    }
    match value {
        Value::String(s) => bounded_text(s, 64000)?,
        Value::Array(a) => {
            if a.len() > 1000 {
                return Err(invalid("Array exceeds 1000 items."));
            }
            for v in a {
                walk(v, depth + 1)?;
            }
        }
        Value::Object(o) => {
            if o.len() > 200 {
                return Err(invalid("Object exceeds 200 fields."));
            }
            for (key, v) in o {
                bounded_text(key, 128)?;
                let lower = key.to_ascii_lowercase();
                if [
                    "password",
                    "cookie",
                    "authorization",
                    "token",
                    "csrf",
                    "otp",
                    "private_key",
                ]
                .iter()
                .any(|k| lower.contains(k))
                {
                    return Err(invalid(
                        "Credential/security fields are not permitted in workspace records.",
                    ));
                }
                walk(v, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
