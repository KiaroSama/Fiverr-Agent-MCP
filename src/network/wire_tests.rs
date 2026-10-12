use super::*;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
