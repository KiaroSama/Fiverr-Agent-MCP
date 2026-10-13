use axum::{
    body::{Body, to_bytes},
    http::{HeaderValue, Request, StatusCode},
};
use fiverr_agent_mcp::{
    server::http,
    storage::{Root, Store},
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
#[tokio::test]
async fn protected_http_header_storage_and_body_failures_return_exact_status() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let token = http::token(&store).unwrap();
    let router = http::router(store.clone(), 1597, CancellationToken::new());
    let mut request = Request::builder()
        .uri("/healthz")
        .header("host", "127.0.0.1:1597")
        .body(Body::empty())
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", HeaderValue::from_bytes(b"\xff").unwrap());
    assert_eq!(
        router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let body = Body::from_stream(futures::stream::once(async {
        Err::<Vec<u8>, _>(std::io::Error::other("controlled body failure"))
    }));
    let request = Request::builder()
        .uri("/healthz")
        .header("host", "127.0.0.1:1597")
        .header("authorization", format!("Bearer {token}"))
        .body(body)
        .unwrap();
    assert_eq!(
        router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    store.transaction(|tx|{tx.execute_batch("CREATE TRIGGER refuse_token BEFORE UPDATE ON kv BEGIN SELECT RAISE(ABORT,'controlled'); END;").unwrap();Ok(())}).unwrap();
    let request = Request::builder()
        .uri("/healthz")
        .header("host", "127.0.0.1:1597")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    store
        .transaction(|tx| {
            tx.execute_batch(
                "DROP TRIGGER refuse_token;INSERT INTO kv VALUES('settings','not-json');",
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
    let request = Request::builder()
        .uri("/healthz")
        .header("host", "127.0.0.1:1597")
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        b"Settings unavailable."
    );
}
#[tokio::test]
async fn actual_http_bind_refuses_occupied_port_without_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), http::run(store, Some(port)))
            .await
            .unwrap()
            .unwrap_err()
            .code,
        fiverr_agent_mcp::core::ErrorCode::PortInUse
    );
    assert!(listener.local_addr().is_ok());
}
