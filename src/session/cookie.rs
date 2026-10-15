use super::{require_binding, status, storage};
use crate::{
    core::{ErrorCode, Result, error},
    network::RouteClient,
    storage::Store,
};
use serde_json::Value;

pub async fn verify(store: &Store) -> Result<Value> {
    let (state, cookie) = storage::load(store)?.ok_or_else(|| {
        error(
            ErrorCode::AuthRequired,
            "Import a cookie file explicitly before verification.",
        )
    })?;
    if state.mode != "cookie" {
        return Err(error(
            ErrorCode::AuthRequired,
            "Selected session mode is not cookie; no fallback to another session occurred.",
        ));
    }
    require_binding(store, &state)?;
    let cookie = cookie.ok_or_else(|| {
        error(
            ErrorCode::AuthRequired,
            "Local session credential is absent.",
        )
    })?;
    let result: Result<()> = async {
        let client = RouteClient::new(store.clone())?;
        let bytes = client
            .session_page(&cookie, state.generation, &state.account)
            .await?;
        super::observation::observe_html(&bytes, &state.username)?;
        require_binding(store, &state)?;
        Ok(())
    }
    .await;
    finish_verification(store, &state, result)
}
pub(super) fn finish_verification(
    store: &Store,
    state: &storage::State,
    result: Result<()>,
) -> Result<Value> {
    let label = match &result {
        Ok(()) => "account_observed",
        Err(e) => match e.code {
            ErrorCode::AuthRequired | ErrorCode::SessionExpired => "session_expired",
            ErrorCode::ChallengeRequired => "challenge_required",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::PermissionDenied => "account_mismatch",
            ErrorCode::RouteBlocked => "route_blocked",
            ErrorCode::ProxyUnreachable => "route_unreachable",
            _ => "verification_failed",
        },
    };
    storage::record(store, state.revision, label)?;
    result?;
    status(store)
}
