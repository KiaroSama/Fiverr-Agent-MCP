use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    AuthRequired,
    SessionExpired,
    FeatureUnavailable,
    AccountIneligible,
    HumanActionRequired,
    ChallengeRequired,
    RouteBlocked,
    ProxyUnreachable,
    DnsPolicyUnverified,
    TlsValidationFailed,
    RateLimited,
    UpstreamChanged,
    InvalidInput,
    ResourceNotFound,
    PermissionDenied,
    ConfirmationRequired,
    ConfirmationExpired,
    ActionStateChanged,
    ActionOutcomeUnknown,
    FilePolicyViolation,
    PortInUse,
    StorageUnavailable,
}
impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::SessionExpired => "SESSION_EXPIRED",
            Self::FeatureUnavailable => "FEATURE_UNAVAILABLE",
            Self::AccountIneligible => "ACCOUNT_INELIGIBLE",
            Self::HumanActionRequired => "HUMAN_ACTION_REQUIRED",
            Self::ChallengeRequired => "CHALLENGE_REQUIRED",
            Self::RouteBlocked => "ROUTE_BLOCKED",
            Self::ProxyUnreachable => "PROXY_UNREACHABLE",
            Self::DnsPolicyUnverified => "DNS_POLICY_UNVERIFIED",
            Self::TlsValidationFailed => "TLS_VALIDATION_FAILED",
            Self::RateLimited => "RATE_LIMITED",
            Self::UpstreamChanged => "UPSTREAM_CHANGED",
            Self::InvalidInput => "INVALID_INPUT",
            Self::ResourceNotFound => "RESOURCE_NOT_FOUND",
            Self::PermissionDenied => "PERMISSION_DENIED",
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            Self::ConfirmationExpired => "CONFIRMATION_EXPIRED",
            Self::ActionStateChanged => "ACTION_STATE_CHANGED",
            Self::ActionOutcomeUnknown => "ACTION_OUTCOME_UNKNOWN",
            Self::FilePolicyViolation => "FILE_POLICY_VIOLATION",
            Self::PortInUse => "PORT_IN_USE",
            Self::StorageUnavailable => "STORAGE_UNAVAILABLE",
        }
    }
}
#[derive(Debug, Clone, thiserror::Error)]
#[error("{code}: {message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
}
impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
pub type Result<T> = std::result::Result<T, AppError>;
pub fn error(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError {
        code,
        message: message.into(),
    }
}
pub fn invalid(message: impl Into<String>) -> AppError {
    error(ErrorCode::InvalidInput, message)
}
pub fn storage_error() -> AppError {
    error(
        ErrorCode::StorageUnavailable,
        "Private storage operation failed; preserve the original and check permissions, disk space and database integrity.",
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Money {
    pub minor: i64,
    pub currency: String,
    pub scale: u8,
}
impl Money {
    pub fn validate(&self) -> Result<()> {
        if self.currency.len() != 3
            || !self.currency.bytes().all(|b| b.is_ascii_uppercase())
            || self.scale > 6
        {
            return Err(invalid(
                "Money requires a three-letter uppercase currency and a scale between 0 and 6.",
            ));
        }
        Ok(())
    }
    pub fn decimal(&self) -> Result<String> {
        self.validate()?;
        let magnitude = self.minor.unsigned_abs();
        let divisor = 10_u64.pow(self.scale.into());
        let sign = if self.minor < 0 { "-" } else { "" };
        if self.scale == 0 {
            return Ok(format!("{sign}{magnitude}"));
        }
        Ok(format!(
            "{sign}{}.{:0width$}",
            magnitude / divisor,
            magnitude % divisor,
            width = self.scale as usize
        ))
    }
}
pub fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err(invalid(
            "IDs must contain 1-128 ASCII letters, digits, hyphens or underscores.",
        ));
    }
    Ok(())
}
pub const KINDS: &[&str] = &[
    "profile",
    "portfolio",
    "gig",
    "conversation",
    "message",
    "opportunity",
    "legacy_brief",
    "offer",
    "order",
    "delivery",
    "revision",
    "analytics",
    "earnings",
    "transaction",
    "invoice",
    "collection",
    "buyer_project",
    "note",
    "task",
    "template",
    "snapshot",
    "search",
    "support",
    "availability",
    "subscription",
    "ads",
];
pub fn validate_kind(kind: &str) -> Result<()> {
    if KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(invalid("Unsupported record kind."))
    }
}
pub fn bounded_text(value: &str, max: usize) -> Result<()> {
    if value.len() > max
        || value
            .chars()
            .any(|c| c == '\0' || c == '\u{1b}' || (c.is_control() && !"\n\r\t".contains(c)))
    {
        return Err(invalid(
            "Text exceeds its limit or contains unsafe control characters.",
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: String,
    pub observed_at: String,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub partial: bool,
}
impl Provenance {
    pub fn validate(&self) -> Result<()> {
        if ![
            "user_import",
            "local_draft",
            "public_page",
            "browser",
            "fixture",
        ]
        .contains(&self.source.as_str())
        {
            return Err(invalid("Unknown observation source."));
        }
        chrono::DateTime::parse_from_rfc3339(&self.observed_at)
            .map_err(|_| invalid("observed_at must be an RFC3339 timestamp."))?;
        if let Some(uri) = &self.uri {
            bounded_text(uri, 2048)?;
            if uri.contains('@') {
                return Err(invalid("Credential-bearing provenance URLs are refused."));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub kind: String,
    pub account_id: String,
    pub version: u64,
    pub draft: bool,
    pub payload: Value,
    pub provenance: Provenance,
    pub updated_at: String,
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
pub fn hash(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}
pub fn envelope(data: Value, provenance: Value) -> Value {
    serde_json::json!({"data":data,"warnings":[],"provenance":provenance})
}
