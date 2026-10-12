use crate::core::{Result, invalid};
use serde_json::Value;

// Validate the advertised top-level contract before dispatch; domain validators
// own nested records and the optimizer's deliberately variant-specific payload.
pub fn arguments(name: &str, value: &Value) -> Result<()> {
    let (_, _, kind, _) = super::catalog::TOOLS
        .iter()
        .find(|tool| tool.0 == name)
        .ok_or_else(|| invalid("Unknown implemented tool."))?;
    let schema = super::catalog::schema(kind);
    let object = value
        .as_object()
        .ok_or_else(|| invalid("Tool arguments must be an object."))?;
    if let Some(required) = schema["required"].as_array() {
        for key in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(key) {
                return Err(invalid(format!("{key} is required.")));
            }
        }
    }
    let properties = schema["properties"]
        .as_object()
        .ok_or_else(|| invalid("Bundled argument schema is invalid."))?;
    for (key, value) in object {
        let Some(property) = properties.get(key) else {
            if schema["additionalProperties"] == false {
                return Err(invalid("Unexpected tool argument."));
            }
            continue;
        };
        let valid = match property["type"].as_str() {
            Some("string") => value.is_string(),
            Some("object") => value.is_object(),
            Some("array") => value.is_array(),
            Some("integer") => value.as_u64().is_some() || value.as_i64().is_some(),
            Some("boolean") => value.is_boolean(),
            _ => true,
        };
        if !valid {
            return Err(invalid(format!("{key} has an invalid type.")));
        }
        if let Some(values) = property["enum"].as_array()
            && !values.contains(value)
        {
            return Err(invalid(format!("{key} has an unsupported value.")));
        }
        if let Some(number) = value
            .as_u64()
            .map(i128::from)
            .or_else(|| value.as_i64().map(i128::from))
            && (property["minimum"]
                .as_i64()
                .is_some_and(|n| number < i128::from(n))
                || property["maximum"]
                    .as_i64()
                    .is_some_and(|n| number > i128::from(n)))
        {
            return Err(invalid(format!("{key} is outside its allowed range.")));
        }
        if let Some(text) = value.as_str() {
            let length = text.chars().count() as u64;
            if property["minLength"].as_u64().is_some_and(|n| length < n)
                || property["maxLength"].as_u64().is_some_and(|n| length > n)
            {
                return Err(invalid(format!("{key} has an invalid length.")));
            }
        }
        if let Some(items) = value.as_array()
            && property["maxItems"]
                .as_u64()
                .is_some_and(|n| items.len() as u64 > n)
        {
            return Err(invalid(format!("{key} has too many items.")));
        }
    }
    Ok(())
}
