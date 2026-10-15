use crate::{
    core::{ErrorCode, Result, error, invalid},
    session::validate_username,
};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use url::Url;

pub const MANAGEMENT_URL: &str = "https://www.fiverr.com/users/manage_gigs";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub username: String,
    pub management: bool,
    pub logout: bool,
}
impl Observation {
    pub fn validate(&self, expected: &str) -> Result<()> {
        validate_username(&self.username)?;
        validate_username(expected)?;
        let url = Url::parse(&self.url).map_err(|_| invalid("Invalid account observation URL."))?;
        let path = format!("/users/{expected}/manage_gigs");
        if self.kind != "account_observation"
            || !same_origin(&url)
            || url.query().is_some()
            || url.fragment().is_some()
            || !["/users/manage_gigs", path.as_str()]
                .contains(&url.path().strip_suffix('/').unwrap_or(url.path()))
            || !self.management
            || !self.logout
        {
            return Err(error(
                ErrorCode::UpstreamChanged,
                "Expected Fiverr management account observation is missing or changed.",
            ));
        }
        if self.username != expected {
            return Err(error(
                ErrorCode::PermissionDenied,
                "Observed account does not match the explicitly paired username.",
            ));
        }
        Ok(())
    }
}
fn same_origin(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("www.fiverr.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
}
fn selector(value: &str) -> Result<Selector> {
    Selector::parse(value).map_err(|_| invalid("Account selector configuration is invalid."))
}
pub fn observe_html(bytes: &[u8], expected: &str) -> Result<Observation> {
    validate_username(expected)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(invalid("Account page exceeds the 2 MiB limit."));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        error(
            ErrorCode::UpstreamChanged,
            "Account page is not valid UTF-8.",
        )
    })?;
    let page = Html::parse_document(text);
    if page
        .select(&selector(
            ".g-recaptcha, #challenge-form, iframe[src*='captcha'], form[action*='challenge']",
        )?)
        .next()
        .is_some()
    {
        return Err(error(
            ErrorCode::ChallengeRequired,
            "Human sign-in challenge required; no bypass attempted.",
        ));
    }
    if page
        .select(&selector("form[action*='login'], input[type='password']")?)
        .next()
        .is_some()
    {
        return Err(error(
            ErrorCode::SessionExpired,
            "Account page requires human sign-in; local credential is not authenticated.",
        ));
    }
    let base = Url::parse(MANAGEMENT_URL).map_err(|_| invalid("Account URL is invalid."))?;
    let mut usernames = HashSet::new();
    let mut logout = false;
    for anchor in page.select(&selector(
        "nav a[href], header a[href], [role='navigation'] a[href]",
    )?) {
        let Some(raw) = anchor.value().attr("href") else {
            continue;
        };
        let Ok(url) = base.join(raw) else {
            continue;
        };
        if !same_origin(&url) || url.query().is_some() || url.fragment().is_some() {
            continue;
        }
        if ["/logout", "/users/logout"]
            .contains(&url.path().strip_suffix('/').unwrap_or(url.path()))
        {
            logout = true;
        }
        let segments: Vec<_> = url.path().split('/').collect();
        if segments.len() == 4
            && segments[1] == "sellers"
            && segments[3] == "edit"
            && validate_username(segments[2]).is_ok()
        {
            usernames.insert(segments[2].to_owned());
        }
    }
    if usernames.len() != 1 {
        return Err(error(
            ErrorCode::UpstreamChanged,
            "Account navigation identity is missing or ambiguous; no authentication claimed.",
        ));
    }
    let observed = Observation {
        kind: "account_observation".into(),
        url: MANAGEMENT_URL.into(),
        username: usernames
            .into_iter()
            .next()
            .ok_or_else(|| invalid("Account identity missing."))?,
        management: page.select(&selector(".js-db-table")?).next().is_some(),
        logout,
    };
    observed.validate(expected)?;
    Ok(observed)
}
