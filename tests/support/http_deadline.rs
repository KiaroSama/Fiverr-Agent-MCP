use super::*;
use tower::ServiceExt;

fn protected(
    app: Router,
) -> (
    tempfile::TempDir,
    Router,
    String,
    Arc<tokio::sync::Semaphore>,
) {
    let temp = tempfile::tempdir().unwrap();
    let store =
        Store::open(crate::storage::Root::explicit(&temp.path().canonicalize().unwrap()).unwrap())
            .unwrap();
    let bearer = token(&store).unwrap();
    let capacity = Arc::new(tokio::sync::Semaphore::new(1));
    let app = app.layer(middleware::from_fn_with_state(
        Access {
            store,
            port: 1597,
            capacity: capacity.clone(),
        },
        guard,
    ));
    (temp, app, bearer, capacity)
}

fn request(bearer: &str, body: Body) -> Request {
    Request::builder()
        .uri("/healthz")
        .header("host", "127.0.0.1:1597")
        .header("authorization", format!("Bearer {bearer}"))
        .body(body)
        .unwrap()
}

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

#[tokio::test(start_paused = true)]
async fn pending_request_body_expires_before_handler_and_releases_capacity() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let called = Arc::new(AtomicBool::new(false));
    let observed = called.clone();
    let app = Router::new().route(
        "/healthz",
        get(move || {
            let observed = observed.clone();
            async move {
                observed.store(true, Ordering::SeqCst);
                "unexpected"
            }
        }),
    );
    let (_temp, app, bearer, capacity) = protected(app);
    let body = Body::from_stream(futures::stream::pending::<
        std::result::Result<Vec<u8>, std::io::Error>,
    >());
    let start = tokio::time::Instant::now();
    let response = app.oneshot(request(&bearer, body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(start.elapsed(), Duration::from_secs(5));
    assert!(!called.load(Ordering::SeqCst));
    assert_eq!(capacity.available_permits(), 1);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        b"Request body exceeds size/time limits."
    );
}

#[tokio::test(start_paused = true)]
async fn idle_response_expires_and_releases_its_owned_permit() {
    let app = Router::new().route(
        "/healthz",
        get(|| async {
            Body::from_stream(futures::stream::pending::<
                std::result::Result<Vec<u8>, std::io::Error>,
            >())
        }),
    );
    let (_temp, app, bearer, capacity) = protected(app);
    let response = app.oneshot(request(&bearer, Body::empty())).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(capacity.available_permits(), 0);
    let start = tokio::time::Instant::now();
    assert!(
        to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(start.elapsed(), Duration::from_secs(30));
    assert_eq!(capacity.available_permits(), 1);
}

#[tokio::test(start_paused = true)]
async fn response_deadline_releases_capacity_before_any_body_poll() {
    let app = Router::new().route(
        "/healthz",
        get(|| async {
            Body::from_stream(futures::stream::pending::<
                std::result::Result<Vec<u8>, std::io::Error>,
            >())
        }),
    );
    let (_temp, app, bearer, capacity) = protected(app);
    let retained = app
        .clone()
        .oneshot(request(&bearer, Body::empty()))
        .await
        .unwrap();
    assert_eq!(retained.status(), StatusCode::OK);
    assert_eq!(capacity.available_permits(), 0);
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::task::yield_now().await;
    assert_eq!(
        capacity.available_permits(),
        1,
        "The response deadline must release capacity without downstream polling"
    );
    let next = app.oneshot(request(&bearer, Body::empty())).await.unwrap();
    assert_eq!(next.status(), StatusCode::OK);
    drop(next);
    drop(retained);
    assert_eq!(capacity.available_permits(), 1);
}

#[tokio::test(start_paused = true)]
async fn expired_unpolled_and_dropped_responses_release_capacity() {
    let (_temp, app, bearer, capacity) =
        protected(Router::new().route("/healthz", get(|| async { "alive" })));
    let response = app
        .clone()
        .oneshot(request(&bearer, Body::empty()))
        .await
        .unwrap();
    assert_eq!(capacity.available_permits(), 0);
    tokio::time::advance(Duration::from_secs(31)).await;
    assert!(
        to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(capacity.available_permits(), 1);
    let response = app.oneshot(request(&bearer, Body::empty())).await.unwrap();
    assert_eq!(capacity.available_permits(), 0);
    drop(response);
    assert_eq!(capacity.available_permits(), 1);
}
