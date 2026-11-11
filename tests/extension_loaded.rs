#![cfg(target_os = "linux")]
//! Real loaded extension and native host, isolated dummy profile, never Fiverr.
use chromiumoxide::{Browser, BrowserConfig};
use fiverr_agent_mcp::{
    session,
    storage::{Root, Store},
};
use futures::{FutureExt, StreamExt};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::Duration};

#[tokio::test]
async fn loaded_extension_native_host_preserves_pairing_and_refuses_unknown_caller() {
    assert_eq!(std::env::var("CI").as_deref(), Ok("true"));
    let browser_path = std::env::var_os("FIVERR_EXTENSION_BROWSER")
        .expect("required dedicated Chrome for Testing path");
    let workspace = std::env::var_os("GITHUB_WORKSPACE").unwrap();
    let temp = tempfile::Builder::new()
        .prefix("e")
        .tempdir_in(&workspace)
        .unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    fs::create_dir(root.path().join("bin")).unwrap();
    let native = root.path().join("bin/fiverr-agent-mcp");
    fs::copy(env!("CARGO_BIN_EXE_fiverr-agent-mcp"), &native).unwrap();
    fs::write(
        root.path().join("fiverr.install.json"),
        include_bytes!("../fiverr.install.json"),
    )
    .unwrap();
    let extension = Path::new(&workspace)
        .join("extension")
        .canonicalize()
        .unwrap();
    let digest = Sha256::digest(extension.as_os_str().as_encoded_bytes());
    let id: String = digest[..16]
        .iter()
        .flat_map(|b| [(b'a' + (b >> 4)) as char, (b'a' + (b & 15)) as char])
        .collect();
    let store = Store::open(root.clone()).unwrap();
    session::native::pair(&store, &id, "owner_1").unwrap();
    let profile = root.path().join(".cache/browser/loaded-profile");
    let hosts = profile.join("NativeMessagingHosts");
    fs::create_dir_all(&hosts).unwrap();
    fs::write(hosts.join(format!("{}.json", session::native::HOST)), serde_json::to_vec(&json!({
        "name":session::native::HOST,"description":"Owned CI fixture","path":native,"type":"stdio",
        "allowed_origins":[format!("chrome-extension://{id}/")]
    })).unwrap()).unwrap();
    let config = BrowserConfig::builder()
        .chrome_executable(browser_path)
        .new_headless_mode()
        .user_data_dir(&profile)
        .extension(extension.to_str().unwrap())
        .respect_https_errors()
        .launch_timeout(Duration::from_secs(20))
        .request_timeout(Duration::from_secs(5))
        .arg("--disable-background-networking")
        .arg("--disable-component-update")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .env("HOME", root.path().to_str().unwrap())
        .env("TMPDIR", root.path().to_str().unwrap())
        .build()
        .unwrap();
    let (mut browser, mut handler) = Browser::launch(config).await.unwrap();
    let worker = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });
    let outcome = std::panic::AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(15), async {
        let page = browser.new_page(format!("chrome-extension://{id}/popup.html")).await.unwrap();
        let loaded: Value = page.evaluate("({id:chrome.runtime.id,manifest:chrome.runtime.getManifest(),label:document.querySelector('#connect').textContent})").await.unwrap().into_value().unwrap();
        assert_eq!(loaded["id"], id);
        assert_eq!(loaded["manifest"]["name"], "Fiverr Agent MCP Session Observation");
        assert_eq!(loaded["label"], "Connect this tab");
        // The real extension API launches the real native host. This synthetic
        // observation is not authenticated-site or activeTab extraction proof.
        let reply: Value = page.evaluate(r#"new Promise((resolve,reject)=>{
          const port=chrome.runtime.connectNative('com.fiverr_agent_mcp.session');
          const timer=setTimeout(()=>{port.disconnect();reject(new Error('fixture deadline'));},5000);
          port.onMessage.addListener(reply=>{clearTimeout(timer);port.disconnect();resolve(reply);});
          port.onDisconnect.addListener(()=>{void chrome.runtime.lastError;clearTimeout(timer);reject(new Error('native fixture disconnected'));});
          port.postMessage({type:'account_observation',url:'https://www.fiverr.com/users/manage_gigs',username:'owner_1',management:true,logout:true});
        })"#).await.unwrap().into_value().unwrap();
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["status"]["account_observed"], true);
        assert_eq!(reply["status"]["live_authenticated"], false);
        assert_eq!(reply["status"]["automation_permission"], "not_verified");
        assert_eq!(session::status(&store).unwrap()["account_observed"], true);
        let before = session::status(&store).unwrap();
        assert!(session::native::receive(&store, "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/", b"{}").is_err());
        assert_eq!(session::status(&store).unwrap(), before);
        page.close().await.unwrap();
    })).catch_unwind().await;
    let _ = tokio::time::timeout(Duration::from_secs(3), browser.close()).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), browser.kill()).await;
    tokio::time::timeout(Duration::from_secs(3), browser.wait())
        .await
        .unwrap()
        .unwrap();
    worker.abort();
    let _ = worker.await;
    outcome
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        .expect("loaded extension fixture exceeded bound");
}
