#![cfg(feature = "browser")]
use axum::{Router, response::Html, routing::get};
use fiverr_agent_mcp::{browser, storage::Root};
use serde_json::json;

#[tokio::test]
async fn native_browser_reads_fixed_fixture_without_exposing_credentials() {
    let executable = std::env::var_os("FIVERR_FIXTURE_BROWSER")
        .expect("CI must configure an installed native browser for this required fixture test");
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let page = r#"<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'"><script type="application/json" data-fiverr-fixture>{"page_state":"account","account":{"account_id":"fixture","username":"sample-owner","roles":["buyer","seller"],"observed_at":"2026-10-12T00:00:00Z","source":"fixture","mira_eligible":null}}</script>"#;
    let app = Router::new().route("/", get(move || async move { Html(page) }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    struct Owned(tokio::task::JoinHandle<()>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let owned = Owned(server);
    let observed = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        browser::fixture_observe(
            &root,
            std::path::Path::new(&executable),
            &format!("http://127.0.0.1:{port}/"),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(observed["page_state"], "account");
    let account = browser::observation::decode(&observed, "fixture").unwrap();
    assert_eq!(account.roles, vec!["buyer", "seller"]);
    assert_eq!(
        serde_json::to_value(account).unwrap()["source"],
        json!("fixture")
    );
    owned.0.abort();
}
