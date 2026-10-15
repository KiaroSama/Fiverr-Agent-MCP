use fiverr_agent_mcp::{
    core::ErrorCode,
    session::observation::{Observation, observe_html},
};

const PAGE: &str = r#"<html><nav><a href="/sellers/fixture_owner/edit">Profile</a><a href="/logout">Logout</a></nav><main class="js-db-table"></main></html>"#;
#[test]
fn identity_requires_positive_management_navigation_and_exact_account() {
    assert!(observe_html(&vec![b'x'; 2097153], "fixture_owner").is_err());
    assert!(observe_html(&[0xff], "fixture_owner").is_err());
    assert!(observe_html(PAGE.as_bytes(), "bad.name").is_err());
    let observed = observe_html(PAGE.as_bytes(), "fixture_owner").unwrap();
    assert_eq!(observed.username, "fixture_owner");
    assert_eq!(observed.url, "https://www.fiverr.com/users/manage_gigs");
    assert_eq!(
        observe_html(PAGE.as_bytes(), "other").unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    for page in [
        "",
        "<html>OK</html>",
        "<form action='/login'></form>",
        "<div class='g-recaptcha'></div>",
        &PAGE.replace("js-db-table", "unknown"),
        &PAGE.replace("/logout", "https://evil.invalid/logout"),
        &PAGE.replace(
            "/sellers/fixture_owner/edit",
            "https://evil.invalid/sellers/fixture_owner/edit",
        ),
    ] {
        assert!(observe_html(page.as_bytes(), "fixture_owner").is_err());
    }
    for href in [
        "http://[",
        "https://www.fiverr.com/sellers/fixture_owner/edit?x=1",
        "/sellers/bad.name/edit",
    ] {
        assert!(
            observe_html(
                PAGE.replace("/sellers/fixture_owner/edit", href).as_bytes(),
                "fixture_owner"
            )
            .is_err()
        );
    }
    let ambiguous = PAGE.replace("</nav>", "<a href='/sellers/other/edit'>Other</a></nav>");
    assert!(observe_html(ambiguous.as_bytes(), "fixture_owner").is_err());
}
#[test]
fn native_observations_reject_untrusted_shapes_and_urls() {
    let valid = r#"{"type":"account_observation","url":"https://www.fiverr.com/users/manage_gigs","username":"fixture_owner","management":true,"logout":true}"#;
    let observation: Observation = serde_json::from_str(valid).unwrap();
    observation.validate("fixture_owner").unwrap();
    for url in [
        "http://www.fiverr.com/users/manage_gigs",
        "https://evil.invalid/users/manage_gigs",
        "https://www.fiverr.com/users/manage_gigs?token=x",
        "https://www.fiverr.com/users/other/manage_gigs",
        "https://name@www.fiverr.com/users/manage_gigs",
        "https://www.fiverr.com:444/users/manage_gigs",
        "https://www.fiverr.com/users/manage_gigs#x",
    ] {
        let mut bad = observation.clone();
        bad.url = url.into();
        assert!(bad.validate("fixture_owner").is_err());
    }
    let mut bad = observation.clone();
    bad.management = false;
    assert!(bad.validate("fixture_owner").is_err());
    bad = observation.clone();
    bad.logout = false;
    assert!(bad.validate("fixture_owner").is_err());
    assert!(observation.validate("other").is_err());
    assert!(
        serde_json::from_str::<Observation>(&valid.replace(
            "\"logout\":true",
            "\"logout\":true,\"cookie\":\"not-allowed\""
        ))
        .is_err()
    );
}
