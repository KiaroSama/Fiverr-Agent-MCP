use crate::core::{ErrorCode, Result, bounded_text, error, invalid, validate_id};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountObservation {
    pub account_id: String,
    pub username: String,
    pub roles: Vec<String>,
    pub observed_at: String,
    pub source: String,
    pub mira_eligible: Option<bool>,
}
// Typed observations consume the fixed adapter schema only. Page content and
// account claims remain untrusted and never confer automation permission.
pub fn decode(value: &Value, expected_account: &str) -> Result<AccountObservation> {
    match value.get("page_state").and_then(Value::as_str) {
        Some("challenge") => {
            return Err(error(
                ErrorCode::ChallengeRequired,
                "Human verification required; no bypass.",
            ));
        }
        Some("expired") => {
            return Err(error(
                ErrorCode::SessionExpired,
                "Account session expired; sign in manually.",
            ));
        }
        Some("signed_out") => {
            return Err(error(ErrorCode::AuthRequired, "Manual sign-in required."));
        }
        Some("account") => {}
        _ => {
            return Err(error(
                ErrorCode::UpstreamChanged,
                "Fixed account schema is absent or has changed.",
            ));
        }
    }
    let account: AccountObservation = serde_json::from_value(
        value
            .get("account")
            .cloned()
            .ok_or_else(|| invalid("Account observation missing."))?,
    )
    .map_err(|_| {
        error(
            ErrorCode::UpstreamChanged,
            "Account observation schema changed.",
        )
    })?;
    validate_id(&account.account_id)?;
    validate_id(expected_account)?;
    if account.account_id != expected_account {
        return Err(error(
            ErrorCode::ActionStateChanged,
            "Observed account differs from owner-selected scope; no silent switch.",
        ));
    }
    bounded_text(&account.username, 128)?;
    if account.username.is_empty()
        || account.roles.len() > 2
        || account
            .roles
            .iter()
            .any(|r| !["buyer", "seller"].contains(&r.as_str()))
    {
        return Err(invalid("Account name/roles are invalid."));
    }
    if !["fixture", "user_import", "browser_observation"].contains(&account.source.as_str()) {
        return Err(invalid("Unverified account source claim refused."));
    }
    chrono::DateTime::parse_from_rfc3339(&account.observed_at)
        .map_err(|_| invalid("Account observation needs an RFC3339 time."))?;
    Ok(account)
}

pub struct ProfileLease(std::fs::File);
impl ProfileLease {
    pub fn acquire(root: &crate::storage::Root, account: &str) -> Result<Self> {
        validate_id(account)?;
        let path = root.safe_path(".state/browser", &format!("{account}.lock"))?;
        if !path.exists() {
            match root.write_new(&path, b"") {
                Ok(()) => {}
                Err(_) if path.is_file() => {}
                Err(e) => return Err(e),
            }
        }
        crate::storage::reject_links(&path)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| crate::core::storage_error())?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|_|error(ErrorCode::HumanActionRequired,"Dedicated account profile is already owned by another process; no stale lock is deleted."))?;
        Ok(Self(file))
    }
}
impl Drop for ProfileLease {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}
