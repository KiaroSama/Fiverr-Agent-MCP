mod cookie;
pub mod native;
pub mod observation;
mod storage;
#[cfg(test)]
mod wire_tests;

use crate::{
    core::{Result, invalid},
    network::Settings,
    storage::{Store, private_permissions},
};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};
use storage::State;

pub use cookie::verify;
pub const FRESHNESS_SECONDS: i64 = 300;

pub fn validate_username(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        return Err(invalid(
            "Expected username must contain 1-80 ASCII letters, digits, underscores or hyphens.",
        ));
    }
    Ok(())
}
fn validate_cookie(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 16384
        || !value.is_ascii()
        || value.bytes().any(|b| b.is_ascii_control())
    {
        return Err(invalid(
            "Cookie header must be 1-16384 ASCII bytes without control characters; use a private local file.",
        ));
    }
    let mut names = HashSet::new();
    for pair in value.split(';') {
        let (name, value) = pair
            .trim_start_matches(' ')
            .split_once('=')
            .ok_or_else(|| invalid("Malformed cookie header; original session preserved."))?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || !names.insert(name)
            || !value.bytes().all(|b| {
                b == 0x21
                    || (0x23..=0x2b).contains(&b)
                    || (0x2d..=0x3a).contains(&b)
                    || (0x3c..=0x5b).contains(&b)
                    || (0x5d..=0x7e).contains(&b)
            })
        {
            return Err(invalid(
                "Malformed or duplicate cookie; original session preserved.",
            ));
        }
    }
    Ok(())
}
fn initial(settings: &Settings, mode: &str, username: &str) -> State {
    State {
        revision: 0,
        mode: mode.into(),
        account: settings.account_id.clone(),
        generation: settings.route_generation,
        username: username.into(),
        extension_id: None,
        observed_at: None,
        state: if mode == "cookie" {
            "credential_saved"
        } else {
            "extension_paired"
        }
        .into(),
    }
}
pub fn import_cookie(store: &Store, path: &Path, username: &str) -> Result<Value> {
    validate_username(username)?;
    store.root.assert_owned(path)?;
    private_permissions(path)?;
    let bytes = store.root.read_bounded(path, 16384)?;
    let cookie = std::str::from_utf8(&bytes)
        .map_err(|_| invalid("Cookie file must be ASCII UTF-8; original session preserved."))?;
    validate_cookie(cookie)?;
    let settings = Settings::load(store)?;
    storage::replace(store, initial(&settings, "cookie", username), Some(cookie))?;
    status(store)
}
pub fn status(store: &Store) -> Result<Value> {
    let settings = Settings::load(store)?;
    let Some((saved, _)) = storage::load(store)? else {
        return Ok(
            json!({"configured":false,"state":"not_configured","live_authenticated":false,"account_observed":false,"fresh":false,"automation_permission":"not_verified","route_assurance":"not_verified"}),
        );
    };
    let age = saved
        .observed_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|s| chrono::Utc::now().signed_duration_since(s).num_seconds());
    let fresh = age.is_some_and(|age| (0..FRESHNESS_SECONDS).contains(&age));
    let state = if saved.account != settings.account_id {
        "account_changed"
    } else if saved.generation != settings.route_generation {
        "route_changed"
    } else if saved.state == "account_observed" && !fresh {
        "observation_expired"
    } else {
        &saved.state
    };
    let observed = state == "account_observed";
    Ok(
        json!({"configured":true,"mode":saved.mode,"state":state,"username":saved.username,"live_authenticated":observed && saved.mode == "cookie","account_observed":observed,"fresh":observed && fresh,"observed_at":saved.observed_at,"verification_ttl_seconds":FRESHNESS_SECONDS,"automation_permission":"not_verified","route_assurance":if saved.mode == "extension" {"operator_managed_unverified"} else {"selected_native_route_only"}}),
    )
}
pub fn forget(store: &Store) -> Result<Value> {
    storage::forget(store)?;
    status(store)
}
fn require_binding(store: &Store, state: &State) -> Result<()> {
    let settings = Settings::load(store)?;
    if settings.account_id != state.account || settings.route_generation != state.generation {
        return Err(invalid(
            "Account or route changed; explicitly import or pair the session again before verification.",
        ));
    }
    Ok(())
}
