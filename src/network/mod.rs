pub mod profiles;
#[cfg(test)]
mod wire_tests;
use crate::{
    core::{ErrorCode, Result, error, invalid, storage_error},
    storage::Store,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Blocked,
    Direct,
    Proxy,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub route_generation: u64,
    pub mode: Mode,
    pub selected_profile_id: Option<String>,
    pub profiles: Vec<profiles::Profile>,
    pub local_proxy: String,
    pub http_port: u16,
    pub http_auth_mode: String,
    pub browser_executable: Option<String>,
    pub account_id: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            route_generation: 0,
            mode: Mode::Blocked,
            selected_profile_id: None,
            profiles: vec![],
            local_proxy: "socks5h://127.0.0.1:1595".into(),
            http_port: 1597,
            http_auth_mode: "token".into(),
            browser_executable: None,
            account_id: "local".into(),
        }
    }
}
impl Settings {
    pub fn load(store: &Store) -> Result<Self> {
        store
            .value("settings")?
            .map(|v| serde_json::from_value(v).map_err(|_| storage_error()))
            .unwrap_or(Ok(Self::default()))
    }
    pub fn change<T>(
        store: &Store,
        route: bool,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        store.update_value("settings", |old| {
            let mut settings: Self = old
                .map(|v| serde_json::from_value(v).map_err(|_| storage_error()))
                .unwrap_or(Ok(Self::default()))?;
            let result = f(&mut settings)?;
            settings.revision = settings.revision.checked_add(1).ok_or_else(|| {
                invalid("Settings revision is exhausted; preserve state and contact support.")
            })?;
            if route {
                settings.route_generation =
                    settings.route_generation.checked_add(1).ok_or_else(|| {
                        invalid("Route generation is exhausted; networking remains unchanged.")
                    })?;
            }
            Ok((
                serde_json::to_value(settings).map_err(|_| storage_error())?,
                result,
            ))
        })
    }
    pub fn in_transaction(tx: &rusqlite::Transaction<'_>) -> Result<Self> {
        use rusqlite::OptionalExtension;
        let text: Option<String> = tx
            .query_row("SELECT value FROM kv WHERE key='settings'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|_| storage_error())?;
        text.map(|t| serde_json::from_str(&t).map_err(|_| storage_error()))
            .unwrap_or(Ok(Self::default()))
    }
    pub fn sanitized(&self) -> Value {
        json!({"revision":self.revision,"route_generation":self.route_generation,"mode":self.mode,"selected_profile_id":self.selected_profile_id,"profiles":self.profiles.iter().map(profiles::Profile::sanitized).collect::<Vec<_>>(),"local_proxy_configured":true,"http_port":self.http_port,"http_auth_mode":self.http_auth_mode,"browser_configured":self.browser_executable.is_some(),"account_id":self.account_id})
    }
    pub fn effective_port(&self, cli: Option<u16>, environment: Option<&str>) -> Result<u16> {
        let port = if let Some(port) = cli {
            port
        } else if let Some(value) = environment {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid(
                    "HTTP port environment must contain ASCII digits only.",
                ));
            }
            value
                .parse()
                .map_err(|_| invalid("HTTP port is out of range."))?
        } else {
            self.http_port
        };
        if port == 0 {
            return Err(invalid(
                "HTTP port must be 1-65535; port 0 is not permitted.",
            ));
        }
        Ok(port)
    }
}
#[derive(Clone)]
pub struct RouteClient {
    store: Store,
    generation: u64,
    account: String,
    client: reqwest::Client,
}
impl RouteClient {
    pub fn new(store: Store) -> Result<Self> {
        let settings = Settings::load(&store)?;
        if settings.mode == Mode::Blocked {
            return Err(error(
                ErrorCode::RouteBlocked,
                "Outbound networking is blocked. Select an explicit route through the owner CLI.",
            ));
        }
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .pool_max_idle_per_host(0);
        if settings.mode == Mode::Proxy {
            let selected = settings
                .profiles
                .iter()
                .find(|p| Some(&p.id) == settings.selected_profile_id.as_ref())
                .ok_or_else(|| {
                    error(
                        ErrorCode::RouteBlocked,
                        "Selected proxy is missing; choose a valid profile.",
                    )
                })?;
            let url = profiles::normalize(&selected.url)?;
            let proxy = reqwest::Proxy::all(url.as_str())
                .map_err(|_| {
                    error(
                        ErrorCode::RouteBlocked,
                        "Selected proxy is invalid; networking remains blocked.",
                    )
                })?
                .no_proxy(None);
            builder = builder.proxy(proxy);
        }
        let client = builder.build().map_err(|_| {
            error(
                ErrorCode::RouteBlocked,
                "Cannot create the explicitly routed TLS client.",
            )
        })?;
        Ok(Self {
            store,
            generation: settings.route_generation,
            account: settings.account_id,
            client,
        })
    }
    pub fn check(&self) -> Result<()> {
        let current = Settings::load(&self.store)?;
        if current.mode == Mode::Blocked
            || current.route_generation != self.generation
            || current.account_id != self.account
        {
            return Err(error(
                ErrorCode::RouteBlocked,
                "Route changed or blocked; request cancelled, recreate the client.",
            ));
        }
        Ok(())
    }
    pub async fn cancelled(&self) {
        let mut timer = tokio::time::interval(Duration::from_millis(50));
        loop {
            timer.tick().await;
            if self.check().is_err() {
                return;
            }
        }
    }
    pub async fn get(&self, url: Url) -> Result<Vec<u8>> {
        approved_destination(&url)?;
        self.get_inner(url, None).await
    }
    pub(crate) async fn session_page(
        &self,
        cookie: &str,
        generation: u64,
        account: &str,
    ) -> Result<Vec<u8>> {
        if self.generation != generation || Settings::load(&self.store)?.account_id != account {
            return Err(error(
                ErrorCode::RouteBlocked,
                "Session route/account binding changed before dispatch.",
            ));
        }
        let mut header = reqwest::header::HeaderValue::from_str(cookie)
            .map_err(|_| invalid("Invalid local cookie header."))?;
        header.set_sensitive(true);
        let url = Url::parse(crate::session::observation::MANAGEMENT_URL)
            .map_err(|_| invalid("Invalid fixed account URL."))?;
        approved_destination(&url)?;
        self.get_inner(url, Some(header)).await
    }
    #[cfg(test)]
    pub(crate) async fn fixture_session_page(&self, url: Url, cookie: &str) -> Result<Vec<u8>> {
        let mut header = reqwest::header::HeaderValue::from_str(cookie)
            .map_err(|_| invalid("Invalid fixture header."))?;
        header.set_sensitive(true);
        self.get_inner(url, Some(header)).await
    }
    async fn get_inner(
        &self,
        url: Url,
        cookie: Option<reqwest::header::HeaderValue>,
    ) -> Result<Vec<u8>> {
        self.check()?;
        let request = async {
            let mut builder = self.client.get(url);
            if let Some(cookie) = cookie {
                builder = builder.header(reqwest::header::COOKIE, cookie);
            }
            let response = builder.send().await.map_err(|_| {
                error(
                    ErrorCode::ProxyUnreachable,
                    "Selected-route request failed; no direct fallback or replay occurred.",
                )
            })?;
            if response.status().as_u16() == 429 {
                return Err(error(
                    ErrorCode::RateLimited,
                    "Upstream rate limited the request; pause and retry later through the same explicit route.",
                ));
            }
            if response.status().as_u16() == 401 {
                return Err(error(
                    ErrorCode::AuthRequired,
                    "Account authentication is required.",
                ));
            }
            if response.status().as_u16() == 403 {
                return Err(error(
                    ErrorCode::ChallengeRequired,
                    "Upstream denied access or requires a human challenge; no bypass is attempted.",
                ));
            }
            if !response.status().is_success() {
                return Err(error(
                    ErrorCode::UpstreamChanged,
                    "Upstream status is not a successful read; redirects and alternate endpoints are not followed.",
                ));
            }
            if response
                .content_length()
                .is_some_and(|v| v > 2 * 1024 * 1024)
            {
                return Err(error(
                    ErrorCode::FilePolicyViolation,
                    "Upstream body exceeds the 2 MiB limit.",
                ));
            }
            use futures::StreamExt;
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| {
                    error(
                        ErrorCode::ProxyUnreachable,
                        "Selected-route response interrupted.",
                    )
                })?;
                if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                    return Err(error(
                        ErrorCode::FilePolicyViolation,
                        "Upstream body exceeds the limit.",
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            self.check()?;
            Ok(bytes)
        };
        tokio::select! {biased;_=self.cancelled()=>Err(error(ErrorCode::RouteBlocked,"Route generation changed; active read cancelled.")),result=tokio::time::timeout(Duration::from_secs(20),request)=>result.map_err(|_|error(ErrorCode::ProxyUnreachable,"Selected-route read deadline exceeded."))?}
    }
    // Only local wire fixtures can inject an origin; release tools cannot choose a base URL.
    #[cfg(test)]
    pub(crate) async fn fixture_get(&self, url: Url) -> Result<Vec<u8>> {
        self.get_inner(url, None).await
    }
}
pub fn approved_destination(url: &Url) -> Result<()> {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !["www.fiverr.com", "mira.fiverr.com", "help.fiverr.com"]
            .contains(&url.host_str().unwrap_or(""))
    {
        return Err(error(
            ErrorCode::FilePolicyViolation,
            "Only approved HTTPS Fiverr authorities on port 443 are permitted; arbitrary URLs/internal targets are refused.",
        ));
    }
    Ok(())
}
