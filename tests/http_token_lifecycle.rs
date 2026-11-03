use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use fiverr_agent_mcp::{
    core::ErrorCode,
    server::http,
    storage::{Root, Store},
};
use serde_json::json;
use std::sync::{Arc, Barrier};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

fn workspace() -> (tempfile::TempDir, Store) {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    (temp, store)
}

#[tokio::test]
async fn existing_http_token_authentication_does_not_write() {
    let (_temp, store) = workspace();
    store
        .update_value("http_token", |_| Ok((json!("dummy-established-token"), ())))
        .unwrap();
    store.transaction(|tx| {
        tx.execute_batch("CREATE TRIGGER refuse_token BEFORE UPDATE ON kv WHEN OLD.key='http_token' BEGIN SELECT RAISE(ABORT,'controlled'); END;").unwrap();
        Ok(())
    }).unwrap();
    let router = http::router(store.clone(), 1597, CancellationToken::new());
    for (bearer, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some("Bearer wrong"), StatusCode::UNAUTHORIZED),
        (Some("Bearer dummy-established-token"), StatusCode::OK),
    ] {
        let mut request = Request::builder()
            .uri("/healthz")
            .header("host", "127.0.0.1:1597");
        if let Some(bearer) = bearer {
            request = request.header("authorization", bearer);
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert!(store.value("http_token").unwrap() == Some(json!("dummy-established-token")));
}

#[test]
fn malformed_persisted_token_is_not_replaced() {
    let (_temp, store) = workspace();
    for value in [
        json!(null),
        json!(false),
        json!({"token":"dummy"}),
        json!(""),
        json!("bad\nheader"),
    ] {
        store
            .update_value("http_token", |_| Ok((value.clone(), ())))
            .unwrap();
        let result = http::token(&store);
        assert!(result.is_err(), "malformed authority must fail closed");
        assert_eq!(result.unwrap_err().code, ErrorCode::StorageUnavailable);
        assert!(store.value("http_token").unwrap() == Some(value));
    }
}

#[test]
fn concurrent_first_creation_returns_one_persisted_token() {
    let (_temp, store) = workspace();
    let other = Store::open(store.root.clone()).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let first = barrier.clone();
    let worker = std::thread::spawn(move || {
        first.wait();
        http::token(&other).unwrap()
    });
    barrier.wait();
    let token = http::token(&store).unwrap();
    assert!(worker.join().unwrap() == token);
    assert!(store.value("http_token").unwrap() == Some(json!(token)));
}

#[tokio::test]
async fn token_rotation_is_visible_without_router_restart() {
    let (_temp, store) = workspace();
    let old = http::token(&store).unwrap();
    let router = http::router(store.clone(), 1597, CancellationToken::new());
    http::rotate(&store).unwrap();
    let new = http::token(&store).unwrap();
    assert!(old != new);
    for (token, expected) in [(old, StatusCode::UNAUTHORIZED), (new, StatusCode::OK)] {
        let request = Request::builder()
            .uri("/healthz")
            .header("host", "127.0.0.1:1597")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            expected
        );
    }
}
