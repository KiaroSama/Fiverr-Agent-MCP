use super::*;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn routed_http_status_and_body_limits_are_enforced_without_redirects() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(
        crate::storage::Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    Settings::change(&store, true, |s| {
        s.mode = Mode::Direct;
        Ok(())
    })
    .unwrap();
    let client = RouteClient::new(store.clone()).unwrap();
    let cases = [
        (b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody".to_vec(), None),
        (b"HTTP/1.1 429 Rate Limited\r\nContent-Length: 0\r\n\r\n".to_vec(), Some(ErrorCode::RateLimited)),
        (b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n".to_vec(), Some(ErrorCode::AuthRequired)),
        (b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n".to_vec(), Some(ErrorCode::ChallengeRequired)),
        (b"HTTP/1.1 302 Found\r\nLocation: http://must-not-follow.invalid/\r\nContent-Length: 0\r\n\r\n".to_vec(), Some(ErrorCode::UpstreamChanged)),
        (b"HTTP/1.1 200 OK\r\nContent-Length: 2097153\r\n\r\n".to_vec(), Some(ErrorCode::FilePolicyViolation)),
        (b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort".to_vec(), Some(ErrorCode::ProxyUnreachable)),
    ];
    for (response, expected) in cases {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let mut worker = Owned(tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            let mut chunk = [0; 1024];
            while !header.windows(4).any(|v| v == b"\r\n\r\n") {
                let n = stream.read(&mut chunk).await.unwrap();
                assert!(n > 0 && header.len() < 8192);
                header.extend_from_slice(&chunk[..n]);
            }
            stream.write_all(&response).await.unwrap();
            stream.shutdown().await.unwrap();
        }));
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            client.fixture_get(Url::parse(&format!("http://{address}/fixture")).unwrap()),
        )
        .await
        .unwrap();
        match expected {
            Some(code) => assert_eq!(result.unwrap_err().code, code),
            None => assert_eq!(result.unwrap(), b"body"),
        }
        tokio::time::timeout(Duration::from_secs(3), &mut worker.0)
            .await
            .unwrap()
            .unwrap();
    }
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let mut worker = Owned(tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 2048];
        stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let body = vec![b'x'; 2 * 1024 * 1024 + 1];
        let _ = stream.write_all(&body).await;
    }));
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(3),
            client.fixture_get(Url::parse(&format!("http://{address}/large")).unwrap())
        )
        .await
        .unwrap()
        .unwrap_err()
        .code,
        ErrorCode::FilePolicyViolation
    );
    tokio::time::timeout(Duration::from_secs(3), &mut worker.0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        client
            .get(Url::parse("http://127.0.0.1/").unwrap())
            .await
            .unwrap_err()
            .code,
        ErrorCode::FilePolicyViolation
    );
    Settings::change(&store, true, |s| {
        s.mode = Mode::Blocked;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        client
            .fixture_get(Url::parse("http://127.0.0.1/").unwrap())
            .await
            .unwrap_err()
            .code,
        ErrorCode::RouteBlocked
    );
}

#[test]
fn selected_missing_or_invalid_proxy_cannot_build_a_route() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(
        crate::storage::Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    Settings::change(&store, true, |s| {
        s.mode = Mode::Proxy;
        Ok(())
    })
    .unwrap();
    assert!(RouteClient::new(store.clone()).is_err());
    Settings::change(&store, true, |s| {
        s.profiles = vec![profiles::Profile {
            id: "bad".into(),
            label: "Bad".into(),
            url: "file:///bad".into(),
        }];
        s.selected_profile_id = Some("bad".into());
        Ok(())
    })
    .unwrap();
    assert!(RouteClient::new(store).is_err());
}

struct Owned(tokio::task::JoinHandle<()>);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn socks5h_sends_destination_name_and_route_changes_cancel_live_read() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(
        crate::storage::Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let (sent, observed) = tokio::sync::oneshot::channel();
    let worker = Owned(tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut greeting = [0_u8; 2];
        stream.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting[0], 5);
        let mut methods = vec![0; greeting[1] as usize];
        stream.read_exact(&mut methods).await.unwrap();
        assert!(methods.contains(&0));
        stream.write_all(&[5, 0]).await.unwrap();
        let mut header = [0; 4];
        stream.read_exact(&mut header).await.unwrap();
        assert_eq!(header, [5, 1, 0, 3]);
        let length = stream.read_u8().await.unwrap();
        let mut host = vec![0; length as usize];
        stream.read_exact(&mut host).await.unwrap();
        let port = stream.read_u16().await.unwrap();
        sent.send((host, port)).unwrap();
        stream
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80])
            .await
            .unwrap();
        let mut pending = Vec::new();
        let mut chunk = [0; 512];
        loop {
            let count = stream.read(&mut chunk).await.unwrap();
            if count == 0 {
                break;
            }
            pending.extend_from_slice(&chunk[..count]);
            if pending.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        // Hold only until the client drops this exact route; no blind readiness sleep.
        let _ = stream.read(&mut chunk).await;
    }));
    Settings::change(&store, true, |s| {
        s.profiles = vec![profiles::Profile {
            id: "proxy1".into(),
            label: "Fixture".into(),
            url: format!("socks5h://{address}"),
        }];
        s.selected_profile_id = Some("proxy1".into());
        s.mode = Mode::Proxy;
        Ok(())
    })
    .unwrap();
    let client = RouteClient::new(store.clone()).unwrap();
    let request = tokio::spawn(async move {
        client
            .fixture_get(Url::parse("http://unresolvable-fixture.invalid:80/read").unwrap())
            .await
    });
    let (host, port) = tokio::time::timeout(Duration::from_secs(3), observed)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(host, b"unresolvable-fixture.invalid");
    assert_eq!(port, 80);
    Settings::change(&store, true, |s| {
        s.mode = Mode::Blocked;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), request)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::RouteBlocked
    );
    worker.0.abort();
}
#[tokio::test]
async fn dead_proxy_never_reaches_loopback_origin_directly() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(
        crate::storage::Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    let origin = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let dead = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let dead_address: SocketAddr = dead.local_addr().unwrap();
    drop(dead);
    Settings::change(&store, true, |s| {
        s.profiles = vec![profiles::Profile {
            id: "dead".into(),
            label: "Dead".into(),
            url: format!("http://{dead_address}"),
        }];
        s.selected_profile_id = Some("dead".into());
        s.mode = Mode::Proxy;
        Ok(())
    })
    .unwrap();
    let client = RouteClient::new(store).unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(3),
            client.fixture_get(
                Url::parse(&format!("http://{}/", origin.local_addr().unwrap())).unwrap()
            )
        )
        .await
        .unwrap()
        .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), origin.accept())
            .await
            .is_err()
    );
}
