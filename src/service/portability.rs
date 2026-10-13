use crate::{
    core::{Result, envelope, hash, invalid, now},
    storage::Store,
};
use serde_json::{Value, json};
pub fn export(store: &Store, args: &Value) -> Result<Value> {
    let account = super::account_scope(store, args)?;
    let kind = super::local::string(args, "kind")?;
    let records = store.list(&account, kind, 0, 100)?;
    let format = args.get("format").and_then(Value::as_str).unwrap_or("json");
    let filename = super::local::string(args, "filename")?;
    validate_export_extension(format, filename)?;
    let content = match format {
        "json" => serde_json::to_string_pretty(&records)
            .map_err(|_| invalid("Export serialization failed."))?,
        "csv" => {
            let mut out = String::from("id,kind,version,payload\n");
            for r in &records {
                out.push_str(&format!(
                    "{},{},{},{}\n",
                    csv(&r.id),
                    csv(&r.kind),
                    r.version,
                    csv(&r.payload.to_string())
                ));
            }
            out
        }
        "markdown" => {
            let mut out = format!("# Imported {kind} records\n\n");
            for r in &records {
                let payload = serde_json::to_string_pretty(&r.payload)
                    .map_err(|_| invalid("Export serialization failed."))?;
                let longest = payload.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                let fence = "`".repeat(longest.max(2) + 1);
                out.push_str(&format!(
                    "## {}\n\n{fence}json\n{payload}\n{fence}\n\n",
                    r.id
                ));
            }
            out
        }
        "html" => {
            let data = serde_json::to_string_pretty(&records)
                .map_err(|_| invalid("Export serialization failed."))?;
            format!(
                "<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'none'\"><title>Fiverr local export</title><h1>Imported records</h1><pre>{}</pre>",
                escape(&data)
            )
        }
        _ => {
            return Err(invalid(
                "Export format must be json, csv, markdown or html.",
            ));
        }
    };
    let path = store.root.safe_path("exports", filename)?;
    store.root.write_new(&path, content.as_bytes())?;
    Ok(envelope(
        json!({"file":filename,"format":format,"bytes":content.len(),"sha256":hash(content.as_bytes()),"count":records.len(),"partial":records.len()==100}),
        json!({"source":"local_export","observed_at":now()}),
    ))
}
pub fn diff(store: &Store, args: &Value) -> Result<Value> {
    let account = super::account_scope(store, args)?;
    let kind = super::local::string(args, "kind")?;
    let a = store.get(&account, kind, super::local::string(args, "before_id")?)?;
    let b = store.get(&account, kind, super::local::string(args, "after_id")?)?;
    let mut changes = vec![];
    let a = a
        .payload
        .as_object()
        .ok_or_else(|| invalid("Snapshot is not an object."))?;
    let b = b
        .payload
        .as_object()
        .ok_or_else(|| invalid("Snapshot is not an object."))?;
    let keys: std::collections::BTreeSet<_> = a.keys().chain(b.keys()).collect();
    for key in keys {
        if a.get(key) != b.get(key) {
            changes.push(json!({"field":key,"before":a.get(key),"after":b.get(key)}));
        }
    }
    Ok(envelope(
        json!({"changes":changes}),
        json!({"source":"local_observation_diff","live_verified":false}),
    ))
}
fn csv(text: &str) -> String {
    let safe = if text
        .trim_start()
        .starts_with(['=', '+', '-', '@', '\t', '\r'])
    {
        format!("'{text}")
    } else {
        text.to_string()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('`', "&#96;")
}

fn validate_export_extension(format: &str, filename: &str) -> Result<()> {
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let valid = match format {
        "json" => extension == "json",
        "csv" => extension == "csv",
        "markdown" => matches!(extension.as_str(), "md" | "markdown"),
        "html" => matches!(extension.as_str(), "html" | "htm"),
        _ => false,
    };
    if !valid {
        return Err(invalid(
            "Export filename extension must match json (.json), csv (.csv), markdown (.md/.markdown) or html (.html/.htm).",
        ));
    }
    Ok(())
}
