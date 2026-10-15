use super::*;
use crate::{
    core::ErrorCode,
    network::{Mode, RouteClient},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn observation_revisions_and_expiry_preserve_session_fences() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(crate::storage::Root::explicit(temp.path()).unwrap()).unwrap();
    native::pair(&store, "abcdefghijklmnopabcdefghijklmnop", "fixture_owner").unwrap();
    let (first, _) = storage::load(&store).unwrap().unwrap();
    storage::record(&store, first.revision, "account_observed").unwrap();
    assert!(storage::record(&store, first.revision, "verification_failed").is_err());
    assert_eq!(status(&store).unwrap()["account_observed"], true);
    let (mut state, _) = storage::load(&store).unwrap().unwrap();
    state.observed_at = Some((chrono::Utc::now() - chrono::Duration::seconds(301)).to_rfc3339());
    storage::replace(&store, state, None).unwrap();
    assert_eq!(status(&store).unwrap()["state"], "observation_expired");
    let (state, _) = storage::load(&store).unwrap().unwrap();
    forget(&store).unwrap();
    native::pair(&store, "abcdefghijklmnopabcdefghijklmnop", "fixture_owner").unwrap();
    assert!(storage::record(&store, state.revision, "account_observed").is_err());
    Settings::change(&store, false, |s| {
        s.account_id = "other".into();
        Ok(())
    })
    .unwrap();
    assert_eq!(status(&store).unwrap()["state"], "account_changed");
}

#[tokio::test]
async fn changed_route_cannot_receive_an_old_session_cookie() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(crate::storage::Root::explicit(temp.path()).unwrap()).unwrap();
    Settings::change(&store, true, |s| {
        s.mode = Mode::Direct;
        Ok(())
    })
    .unwrap();
    let old = Settings::load(&store).unwrap();
    Settings::change(&store, true, |_| Ok(())).unwrap();
    let client = RouteClient::new(store.clone()).unwrap();
    assert_eq!(
        client
            .session_page("session=synthetic", old.route_generation, &old.account_id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::RouteBlocked
    );
    let current = Settings::load(&store).unwrap();
    assert!(
        client
            .session_page(
                "invalid\r\nheader",
                current.route_generation,
                &current.account_id
            )
            .await
            .is_err()
    );
    assert!(
        client
            .fixture_session_page(
                url::Url::parse("http://127.0.0.1/").unwrap(),
                "invalid\r\nheader"
            )
            .await
            .is_err()
    );
    assert_eq!(
        client
            .session_page("session=synthetic", current.route_generation, "other")
            .await
            .unwrap_err()
            .code,
        ErrorCode::RouteBlocked
    );
}
#[tokio::test]
async fn cookie_verification_uses_same_routed_wire_and_fails_closed_on_status_or_shape() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(crate::storage::Root::explicit(temp.path()).unwrap()).unwrap();
    Settings::change(&store, true, |s| {
        s.mode = Mode::Direct;
        Ok(())
    })
    .unwrap();
    let input = store.root.safe_path("config", "cookie.txt").unwrap();
    store
        .root
        .write_new(&input, b"session=synthetic-wire")
        .unwrap();
    import_cookie(&store, &input, "fixture_owner").unwrap();
    let valid = "<nav><a href='/sellers/fixture_owner/edit'>Profile</a><a href='/logout'>Exit</a></nav><div class='js-db-table'></div>";
    for (status_code, body, code, label) in [
        (200, valid, None, "account_observed"),
        (
            200,
            "<html>unknown</html>",
            Some(ErrorCode::UpstreamChanged),
            "verification_failed",
        ),
        (
            200,
            "<input type='password'>",
            Some(ErrorCode::SessionExpired),
            "session_expired",
        ),
        (401, "", Some(ErrorCode::AuthRequired), "session_expired"),
        (
            403,
            "",
            Some(ErrorCode::ChallengeRequired),
            "challenge_required",
        ),
        (429, "", Some(ErrorCode::RateLimited), "rate_limited"),
        (
            302,
            "",
            Some(ErrorCode::UpstreamChanged),
            "verification_failed",
        ),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        struct Worker(tokio::task::JoinHandle<()>);
        impl Drop for Worker {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let mut worker = Worker(tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 1024];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0 && bytes.len() < 8192);
                bytes.extend_from_slice(&chunk[..count]);
            }
            let request = String::from_utf8(bytes).unwrap();
            assert!(request.contains("cookie: session=synthetic-wire\r\n"));
            assert!(request.starts_with("GET /users/manage_gigs HTTP/1.1"));
            let response = format!(
                "HTTP/1.1 {status_code} Fixture\r\nContent-Length: {}\r\nConnection: close\r\nLocation: https://must-not-follow.invalid/\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }));
        let route = RouteClient::new(store.clone()).unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            route.fixture_session_page(
                url::Url::parse(&format!("http://{address}/users/manage_gigs")).unwrap(),
                "session=synthetic-wire",
            ),
        )
        .await
        .unwrap();
        let result =
            result.and_then(|bytes| observation::observe_html(&bytes, "fixture_owner").map(|_| ()));
        match code {
            None => assert!(result.is_ok()),
            Some(code) => assert_eq!(result.as_ref().unwrap_err().code, code),
        };
        let (state, _) = storage::load(&store).unwrap().unwrap();
        let completed = cookie::finish_verification(&store, &state, result);
        assert_eq!(completed.is_ok(), code.is_none());
        assert_eq!(status(&store).unwrap()["state"], label);
        assert_eq!(
            status(&store).unwrap()["live_authenticated"],
            status_code == 200 && body == valid
        );
        tokio::time::timeout(Duration::from_secs(3), &mut worker.0)
            .await
            .unwrap()
            .unwrap();
    }
}
