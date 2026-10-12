#[cfg(feature = "browser")]
mod fixture_process;
pub mod observation;
use crate::{
    core::{ErrorCode, Result, envelope, error, invalid},
    network::{Mode, Settings},
    storage::{Root, Store},
};
use serde_json::{Value, json};
use std::path::Path;

pub fn status(store: &Store) -> Result<Value> {
    let settings = Settings::load(store)?;
    Ok(envelope(
        json!({"configured":settings.browser_executable.is_some(),"profile_mode":"dedicated","routing_assurance":"not_verified","account_identity":"not_verified","automated_account_access":"permission_required","browser_downloads":"explicit_only","cdp_exposed_to_model":false}),
        json!({"source":"local_browser_configuration"}),
    ))
}
pub fn configure(store: &Store, executable: &Path) -> Result<()> {
    if !executable.is_absolute() || !executable.is_file() {
        return Err(invalid(
            "Browser executable must be an existing absolute path.",
        ));
    }
    crate::storage::reject_links(executable)?;
    Settings::change(store, false, |s| {
        s.browser_executable = Some(executable.to_string_lossy().to_string());
        Ok(())
    })
}
pub async fn sign_in(store: &Store) -> Result<Value> {
    let settings = Settings::load(store)?;
    if settings.mode == Mode::Blocked {
        return Err(error(
            ErrorCode::RouteBlocked,
            "Select an explicit route before sign-in. Browser routing is separate from native HTTP.",
        ));
    }
    if settings.browser_executable.is_none() {
        return Err(error(
            ErrorCode::HumanActionRequired,
            "Configure a compatible browser executable first; no automatic download occurs.",
        ));
    }
    // Process-wide browser containment has not been independently established on these OS modes.
    Err(error(
        ErrorCode::DnsPolicyUnverified,
        "Strict managed browser routing is not verified. Sign in manually in the official site using an operator-managed browser; no credentials are collected and no direct fallback occurs. Automated account tools remain permission-gated.",
    ))
}
#[cfg(feature = "browser")]
pub async fn fixture_observe(root: &Root, executable: &Path, page_url: &str) -> Result<Value> {
    if std::env::var("CI").ok().as_deref() != Some("true") {
        return Err(error(
            ErrorCode::PermissionDenied,
            "Fixture browser execution requires isolated CI; this is not a live account adapter.",
        ));
    }
    let url = url::Url::parse(page_url).map_err(|_| invalid("Invalid fixture URL."))?;
    if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") || !url.username().is_empty() {
        return Err(invalid("Fixture origin must be loopback."));
    }
    use futures::StreamExt;
    let _lease = observation::ProfileLease::acquire(root, "fixture")?;
    let profile = root.safe_path(".state/browser", "fixture")?;
    std::fs::create_dir_all(&profile).map_err(|_| crate::core::storage_error())?;
    let (mut browser, mut handler, mut child) =
        fixture_process::launch(root, executable, &profile).await?;
    eprintln!("CI owned fixture readiness and websocket connected.");
    let worker = tokio::spawn(async move {
        while let Some(result) = handler.next().await {
            if result.is_err() {
                break;
            }
        }
    });
    let result=async{
        let page=browser.new_page(page_url).await.map_err(|_|error(ErrorCode::UpstreamChanged,"Fixture page failed to open."))?;
        eprintln!("CI owned fixture page opened.");
        // Fixed-purpose fixture schema extraction; never model-supplied JavaScript.
        let observed=page.evaluate("(()=>{const e=document.querySelector('[data-fiverr-fixture]'); if(!e)throw new Error('schema missing');return JSON.parse(e.textContent);})()").await.map_err(|_|error(ErrorCode::UpstreamChanged,"Fixture schema changed or is missing."))?;
        observed.into_value::<Value>().map_err(|_|invalid("Fixture observation is not valid typed JSON."))
    }.await;
    if tokio::time::timeout(std::time::Duration::from_secs(3), browser.close())
        .await
        .is_err()
    {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), browser.kill()).await;
    }
    if tokio::time::timeout(std::time::Duration::from_secs(3), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    worker.abort();
    let _ = worker.await;
    result
}
