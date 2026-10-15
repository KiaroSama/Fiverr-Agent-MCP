use serde_json::{Value, json};

const MANIFEST: &str = include_str!("../extension/manifest.json");
const OBSERVATION: &str = include_str!("../extension/observation.js");
const POPUP: &str = include_str!("../extension/popup.js");

#[test]
fn extension_has_only_gesture_scoped_permissions_and_fixed_native_protocol() {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(manifest["manifest_version"], 3);
    assert_eq!(
        manifest["permissions"],
        json!(["activeTab", "scripting", "nativeMessaging"])
    );
    assert_eq!(manifest["action"]["default_popup"], "popup.html");
    for key in [
        "host_permissions",
        "optional_host_permissions",
        "background",
        "content_scripts",
        "externally_connectable",
        "web_accessible_resources",
    ] {
        assert!(manifest.get(key).is_none(), "unexpected manifest key {key}");
    }
    assert!(POPUP.contains("chrome.runtime.connectNative('com.fiverr_agent_mcp.session')"));
    assert!(POPUP.contains("func: extractAccountObservation"));
    assert!(POPUP.contains("active: true, currentWindow: true"));
    assert!(POPUP.contains("10_000"));
    assert!(POPUP.contains("port.disconnect()"));
    assert!(POPUP.contains("chrome.runtime.lastError"));
    for forbidden in [
        "eval(",
        "new Function",
        "chrome.cookies",
        "chrome.webRequest",
        "fetch(",
        "XMLHttpRequest",
        "innerHTML",
        "localStorage",
        "sessionStorage",
    ] {
        assert!(!POPUP.contains(forbidden), "popup contains {forbidden}");
        assert!(
            !OBSERVATION.contains(forbidden),
            "extractor contains {forbidden}"
        );
    }
}

// Executes the exact exported extractor body in CI Chromium with a synthetic DOM/location.
// This does not load an extension, use an account, or verify native-host registration.
#[cfg(feature = "browser")]
#[tokio::test]
async fn exact_extension_extractor_rejects_unsafe_or_unknown_account_fixtures() {
    use axum::{Router, response::Html, routing::get};
    use fiverr_agent_mcp::{browser, storage::Root};
    use std::time::Duration;

    let browser = std::env::var_os("FIVERR_FIXTURE_BROWSER")
        .expect("CI must provide the required fixture browser");
    let workspace = std::env::var_os("GITHUB_WORKSPACE").expect("CI workspace is required");
    let tmp = tempfile::Builder::new()
        .prefix("e")
        .tempdir_in(workspace)
        .unwrap();
    let root = Root::explicit(&tmp.path().canonicalize().unwrap()).unwrap();
    let own_nav = r#"<nav><a href="/sellers/owner_1/edit">Profile</a><a href="/users/logout">Log out</a></nav>"#;
    let valid = format!("{own_nav}<div class='js-db-table'></div>");
    let base = "https://www.fiverr.com/users/manage_gigs";
    let mut cases = vec![
        json!({"name":"valid table", "url":base, "html":valid, "username":"owner_1"}),
        json!({"name":"valid named route", "url":"https://www.fiverr.com/users/owner_1/manage_gigs/", "html":valid, "username":"owner_1"}),
        json!({"name":"absolute profile and logout", "url":base, "html":"<header><a href='https://www.fiverr.com/sellers/owner-1/edit'>Profile</a><a href='https://www.fiverr.com/logout'>Exit</a></header><div class='js-db-table'></div>", "username":"owner-1"}),
        json!({"name":"relative URLs", "url":base, "html":"<nav><a href='../sellers/owner_1/edit'>Profile</a><a href='logout'>Exit</a></nav><div class='js-db-table'></div>", "username":"owner_1"}),
        json!({"name":"duplicate same identity", "url":base, "html":format!("{valid}{own_nav}"), "username":"owner_1"}),
        json!({"name":"management link alone not proof", "url":base, "html":format!("{own_nav}<a href='/users/owner_1/manage_gigs'>Manage</a>")}),
        json!({"name":"orders table not proof", "url":base, "html":format!("{own_nav}<div class='orders-table'></div>")}),
        json!({"name":"missing markup", "url":base, "html":""}),
        json!({"name":"missing management", "url":base, "html":own_nav}),
        json!({"name":"missing logout", "url":base, "html":"<nav><a href='/sellers/owner_1/edit'>Profile</a></nav><div class='js-db-table'></div>"}),
        json!({"name":"public seller outside navigation", "url":base, "html":"<a href='/sellers/owner_1/edit'>Public seller</a><nav><a href='/logout'>Exit</a></nav><div class='js-db-table'></div>"}),
        json!({"name":"ambiguous identity", "url":base, "html":format!("{valid}<nav><a href='/sellers/other/edit'>Other</a></nav>")}),
        json!({"name":"named route mismatch", "url":"https://www.fiverr.com/users/other/manage_gigs", "html":valid}),
        json!({"name":"cross origin profile", "url":base, "html":"<nav><a href='https://evil.invalid/sellers/owner_1/edit'>Profile</a><a href='/logout'>Exit</a></nav><div class='js-db-table'></div>"}),
        json!({"name":"cross origin logout", "url":base, "html":"<nav><a href='/sellers/owner_1/edit'>Profile</a><a href='https://evil.invalid/logout'>Exit</a></nav><div class='js-db-table'></div>"}),
        json!({"name":"logout substring not path", "url":base, "html":"<nav><a href='/sellers/owner_1/edit'>Profile</a><a href='/help?next=/logout'>Exit</a></nav><div class='js-db-table'></div>"}),
        json!({"name":"cross origin management", "url":base, "html":format!("{own_nav}<a href='https://evil.invalid/users/manage_gigs'>Manage</a>")}),
        json!({"name":"management link wrong account", "url":base, "html":format!("{own_nav}<a href='/users/other/manage_gigs'>Manage</a>")}),
        json!({"name":"malicious base ignored", "url":base, "html":format!("<base href='https://evil.invalid/'>{valid}"), "username":"owner_1"}),
    ];
    for markup in [
        "<input type='password'>",
        "<form action='/login'></form>",
        "<div class='g-recaptcha'></div>",
        "<form id='challenge-form'></form>",
    ] {
        cases.push(json!({"name":format!("refused overlay {markup}"),"url":base,"html":format!("{valid}{markup}")}));
    }
    for (name, url) in [
        ("http", "http://www.fiverr.com/users/manage_gigs"),
        ("other host", "https://fiverr.com/users/manage_gigs"),
        ("subdomain", "https://help.fiverr.com/users/manage_gigs"),
        (
            "nondefault port",
            "https://www.fiverr.com:444/users/manage_gigs",
        ),
        (
            "credentials",
            "https://owner:example@www.fiverr.com/users/manage_gigs",
        ),
        (
            "query not forwarded",
            "https://www.fiverr.com/users/manage_gigs?private=example",
        ),
        (
            "fragment not forwarded",
            "https://www.fiverr.com/users/manage_gigs#private",
        ),
        ("public profile", "https://www.fiverr.com/owner_1"),
        (
            "path suffix",
            "https://www.fiverr.com/users/manage_gigs/unexpected",
        ),
        (
            "encoded identity route",
            "https://www.fiverr.com/users/owner%5f1/manage_gigs",
        ),
    ] {
        cases.push(json!({"name":name,"url":url,"html":valid}));
    }
    for username in [
        "",
        "owner.name",
        "owner%5f1",
        "ümlaut",
        "a/b",
        &"a".repeat(81),
    ] {
        cases.push(json!({"name":format!("invalid username {username}"), "url":base,
            "html":format!("<nav><a href='/sellers/{username}/edit'>Profile</a><a href='/logout'>Exit</a></nav><div class='js-db-table'></div>")}));
    }
    for username in ["a", &"a".repeat(80)] {
        cases.push(json!({"name":format!("valid username length {}", username.len()), "url":base,
            "html":format!("<nav><a href='/sellers/{username}/edit'>Profile</a><a href='/logout'>Exit</a></nav><div class='js-db-table'></div>"), "username":username}));
    }
    let source = OBSERVATION.replacen("export function ", "function ", 1);
    assert!(!source.contains("export function"));
    let validator = POPUP
        .split("const connectButton =")
        .next()
        .unwrap()
        .strip_prefix("import { extractAccountObservation } from './observation.js';\n")
        .unwrap()
        .replacen("export function ", "function ", 1);
    let fixture = format!(
        r#"<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'"><script type="application/json" data-fiverr-fixture>{{}}</script><script>
const cases = {};
const run = (document, location) => {{ {} return extractAccountObservation(); }};
const results = cases.map(c => {{
  const fixture = new DOMParser().parseFromString(c.html, 'text/html');
  return {{name:c.name, result:run(fixture, new URL(c.url))}};
}});
{}
const observation = {{username:'owner_1'}};
const valid = {{ok:true,status:{{configured:true,mode:'extension',state:'account_observed',
  username:'owner_1',live_authenticated:false,account_observed:true,fresh:true,
  automation_permission:'not_verified',route_assurance:'operator_managed_unverified',
  observed_at:new Date().toISOString(),verification_ttl_seconds:300}}}};
const clone = () => JSON.parse(JSON.stringify(valid));
const native = [{{name:'valid observed response',response:valid,expected:true}},
  {{name:'missing response',response:null,expected:false}},
  {{name:'only ok flag',response:{{ok:true}},expected:false}},
  {{name:'refused',response:{{ok:false,error:'untrusted native text'}},expected:false}}];
for (const [key,value] of [['configured',false],['mode','cookie'],['state','connected'],
  ['username','other'],['live_authenticated',true],['account_observed',false],['fresh',false],
  ['automation_permission','verified'],['route_assurance','verified'],
  ['observed_at','invalid'],['verification_ttl_seconds',0]]) {{
  const response=clone(); response.status[key]=value;
  native.push({{name:'invalid '+key,response,expected:false}});
}}
for (const [name,change] of [
  ['missing field',r=>delete r.status.state],
  ['extra status field',r=>r.status.private='not allowed'],
  ['extra response field',r=>r.private='not allowed'],
  ['stale',r=>r.status.observed_at=new Date(Date.now()-301000).toISOString()],
  ['future',r=>r.status.observed_at=new Date(Date.now()+60000).toISOString()]]) {{
  const response=clone(); change(response); native.push({{name,response,expected:false}});
}}
const responses=native.map(c=>({{name:c.name,expected:c.expected,
  actual:validateNativeStatus(c.response,observation)}}));
document.querySelector('[data-fiverr-fixture]').textContent = JSON.stringify({{results,responses}});
</script>"#,
        serde_json::to_string(&cases).unwrap(),
        source,
        validator,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new().route(
        "/",
        get(move || {
            let fixture = fixture.clone();
            async move { Html(fixture) }
        }),
    );
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let mut server = Server(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let observed = tokio::time::timeout(
        Duration::from_secs(30),
        browser::fixture_observe(
            &root,
            std::path::Path::new(&browser),
            &format!("http://127.0.0.1:{port}/"),
        ),
    )
    .await
    .expect("bounded CI browser extraction")
    .unwrap();
    let results = observed["results"]
        .as_array()
        .expect("extractor fixture results");
    assert_eq!(results.len(), cases.len());
    for (case, observed) in cases.iter().zip(results) {
        assert_eq!(observed["name"], case["name"]);
        let expected = match case.get("username") {
            Some(username) => json!({"type":"account_observation", "url":case["url"],
                "username":username, "management":true,"logout":true}),
            None => Value::Null,
        };
        assert_eq!(observed["result"], expected, "fixture {}", case["name"]);
    }
    let responses = observed["responses"]
        .as_array()
        .expect("native response fixtures");
    assert_eq!(responses.len(), 20);
    for response in responses {
        assert_eq!(
            response["actual"], response["expected"],
            "native fixture {}",
            response["name"]
        );
    }
    server.0.abort();
    let _ = (&mut server.0).await;
}
