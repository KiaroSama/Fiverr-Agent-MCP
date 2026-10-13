use super::*;
use tower::ServiceExt;

#[tokio::test(start_paused = true)]
async fn slow_handler_expires_without_retaining_a_request_permit() {
    let temp = tempfile::tempdir().unwrap();
    let store =
        Store::open(crate::storage::Root::explicit(&temp.path().canonicalize().unwrap()).unwrap())
            .unwrap();
    let bearer = token(&store).unwrap();
    let capacity = Arc::new(tokio::sync::Semaphore::new(1));
    let app = Router::new()
        .route(
            "/healthz",
            get(|| async { std::future::pending::<&'static str>().await }),
        )
        .layer(middleware::from_fn_with_state(
            Access {
                store,
                port: 1597,
                capacity: capacity.clone(),
            },
            guard,
        ));
    let request = Request::builder()
        .uri("/healthz")
        .header("host", "127.0.0.1:1597")
        .header("authorization", format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap();
    let start = tokio::time::Instant::now();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(start.elapsed(), Duration::from_secs(30));
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        b"Request deadline exceeded."
    );
    assert_eq!(capacity.available_permits(), 1);
}
