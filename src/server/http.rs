use crate::{
    core::{ErrorCode, Result, error},
    network::Settings,
    storage::Store,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::get,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::{sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio_util::sync::CancellationToken;
#[derive(Clone)]
struct Access {
    store: Store,
    port: u16,
    capacity: Arc<tokio::sync::Semaphore>,
}
pub fn token(store: &Store) -> Result<String> {
    store.update_value("http_token", |old| {
        let value = old
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| {
                format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                )
            });
        Ok((serde_json::json!(value), value))
    })
}
fn refuse(status: StatusCode, text: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(text))
        .expect("fixed error response")
}
async fn guard(State(access): State<Access>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let expected = format!("127.0.0.1:{}", access.port);
    if headers.get_all(header::HOST).iter().count() != 1
        || headers.get(header::HOST).and_then(|v| v.to_str().ok()) != Some(expected.as_str())
    {
        return refuse(StatusCode::FORBIDDEN, "Unapproved Host.");
    }
    if headers.get_all(header::ORIGIN).iter().count() > 1
        || headers
            .get(header::ORIGIN)
            .is_some_and(|v| v.to_str().ok() != Some(format!("http://{expected}").as_str()))
    {
        return refuse(StatusCode::FORBIDDEN, "Unapproved Origin.");
    }
    if request.uri().query().is_some() || request.uri().authority().is_some() {
        return refuse(
            StatusCode::BAD_REQUEST,
            "Only the fixed endpoint without a query is supported.",
        );
    }
    let settings = match Settings::load(&access.store) {
        Ok(s) => s,
        Err(_) => return refuse(StatusCode::SERVICE_UNAVAILABLE, "Settings unavailable."),
    };
    if settings.http_auth_mode == "token" {
        let token = match token(&access.store) {
            Ok(t) => t,
            Err(_) => {
                return refuse(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Access control unavailable.",
                );
            }
        };
        if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
            return refuse(
                StatusCode::UNAUTHORIZED,
                "Protected bearer header required.",
            );
        }
        let Some(raw) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        else {
            return refuse(StatusCode::UNAUTHORIZED, "Invalid access header.");
        };
        let Some((scheme, value)) = raw.split_once(' ') else {
            return refuse(StatusCode::UNAUTHORIZED, "Invalid access header.");
        };
        if !scheme.eq_ignore_ascii_case("bearer")
            || value.as_bytes().ct_eq(token.as_bytes()).unwrap_u8() != 1
        {
            return refuse(StatusCode::UNAUTHORIZED, "Access denied.");
        }
    } else if settings.http_auth_mode != "loopback-trusted" {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "Invalid access-control mode.",
        );
    }
    let permit = match access.capacity.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return refuse(StatusCode::TOO_MANY_REQUESTS, "Request capacity reached."),
    };
    let (parts, body) = request.into_parts();
    let bytes =
        match tokio::time::timeout(Duration::from_secs(5), to_bytes(body, 1024 * 1024)).await {
            Ok(Ok(b)) => b,
            _ => {
                return refuse(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Request body exceeds size/time limits.",
                );
            }
        };
    let request = Request::from_parts(parts, Body::from(bytes));
    match tokio::time::timeout(Duration::from_secs(30), next.run(request)).await {
        Ok(response) => {
            use futures::StreamExt;
            let (mut parts, body) = response.into_parts();
            parts.headers.insert(
                header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            );
            let stream = futures::stream::unfold(
                (body.into_data_stream(), permit, tokio::time::Instant::now()),
                |(mut stream, permit, start)| async move {
                    let remaining = Duration::from_secs(30).checked_sub(start.elapsed())?;
                    let item = tokio::time::timeout(remaining, stream.next())
                        .await
                        .ok()
                        .flatten()?;
                    Some((item, (stream, permit, start)))
                },
            );
            Response::from_parts(parts, Body::from_stream(stream))
        }
        Err(_) => refuse(StatusCode::REQUEST_TIMEOUT, "Request deadline exceeded."),
    }
}
pub fn router(store: Store, port: u16, cancel: CancellationToken) -> Router {
    let handler = super::Handler {
        store: store.clone(),
    };
    let mut config = StreamableHttpServerConfig::default()
        .with_cancellation_token(cancel)
        .with_json_response(true);
    config.max_request_body_bytes = 1024 * 1024;
    config.allowed_hosts = vec![format!("127.0.0.1:{port}")];
    config.allowed_origins = vec![format!("http://127.0.0.1:{port}")];
    let service: StreamableHttpService<super::Handler, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(handler.clone()), Default::default(), config);
    Router::new()
        .nest_service("/mcp", service)
        .route("/healthz", get(|| async { "alive" }))
        .route(
            "/readyz",
            get(|| async { "local-workspace-ready; account-not-verified" }),
        )
        .layer(middleware::from_fn_with_state(
            Access {
                store,
                port,
                capacity: Arc::new(tokio::sync::Semaphore::new(16)),
            },
            guard,
        ))
}
pub async fn run(store: Store, override_port: Option<u16>) -> Result<()> {
    let settings = Settings::load(&store)?;
    let environment = std::env::var("FIVERR_AGENT_HTTP_PORT").ok();
    let port = settings.effective_port(override_port, environment.as_deref())?;
    let _ = token(&store)?;
    let listener=tokio::net::TcpListener::bind(("127.0.0.1",port)).await.map_err(|_|error(ErrorCode::PortInUse,"Loopback port is occupied or unavailable; change http-port and restart. No alternate port selected."))?;
    let cancel = CancellationToken::new();
    let app = router(store, port, cancel.clone());
    eprintln!("Local MCP HTTP listening at http://127.0.0.1:{port}/mcp");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            cancel.cancel();
        })
        .await
        .map_err(|_| {
            error(
                ErrorCode::StorageUnavailable,
                "HTTP server stopped unexpectedly.",
            )
        })?;
    Ok(())
}
pub fn rotate(store: &Store) -> Result<()> {
    store.update_value("http_token", |_| {
        Ok((
            serde_json::json!(format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )),
            (),
        ))
    })?;
    Ok(())
}
