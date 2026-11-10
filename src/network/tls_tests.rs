//! Loopback-only positive CONNECT/TLS trust and credential separation.
use super::*;
use std::{net::Ipv4Addr, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{self, pki_types::PrivatePkcs8KeyDer},
};

struct Owned(tokio::task::JoinHandle<()>);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn header(stream: &mut (impl tokio::io::AsyncRead + Unpin)) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        assert!(bytes.len() < 8192);
        bytes.push(stream.read_u8().await.unwrap());
    }
    String::from_utf8(bytes).unwrap().to_ascii_lowercase()
}

#[tokio::test]
async fn trusted_connect_tunnel_keeps_proxy_auth_outside_tls_and_cookie_inside() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(
        crate::storage::Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    let certified = rcgen::generate_simple_self_signed(vec!["fixture.invalid".into()]).unwrap();
    let certificate = certified.cert.der().clone();
    let key = PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.clone()], key.into())
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    for trusted in [true, false] {
        let origin = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let origin_address = origin.local_addr().unwrap();
        let proxy = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = proxy.local_addr().unwrap();
        let acceptor = acceptor.clone();
        let mut worker = Owned(tokio::spawn(async move {
            let (mut stream, _) = proxy.accept().await.unwrap();
            let connect = header(&mut stream).await;
            assert!(connect.starts_with(&format!(
                "connect fixture.invalid:{} http/1.1\r\n",
                origin_address.port()
            )));
            assert!(connect.contains("\r\nproxy-authorization: basic zml4dhvyztp0zxn0\r\n"));
            assert!(!connect.contains("cookie:") && !connect.contains("synthetic-session"));
            stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
            let tls = acceptor.accept(stream).await;
            if !trusted {
                assert!(
                    tls.is_err(),
                    "untrusted certificate must abort the handshake"
                );
                return;
            }
            let mut tls = tls.unwrap();
            let request = header(&mut tls).await;
            assert!(request.starts_with("get /fixture http/1.1\r\n"));
            assert!(request.contains("\r\ncookie: session=synthetic-session\r\n"));
            assert!(!request.contains("proxy-authorization:") && !request.contains("fixture:test"));
            tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody")
                .await
                .unwrap();
            tls.shutdown().await.unwrap();
        }));
        Settings::change(&store, true, |s| {
            s.mode = Mode::Proxy;
            s.profiles = vec![profiles::Profile {
                id: "fixture".into(),
                label: "Synthetic TLS".into(),
                url: format!("http://fixture:test@{address}"),
            }];
            s.selected_profile_id = Some("fixture".into());
            Ok(())
        })
        .unwrap();
        let mut route = RouteClient::new(store.clone()).unwrap();
        // Only fixture trust changes; the production request/cancellation/body path stays.
        // No invalid-certificate bypass exists, nor any host trust-store mutation.
        let certificates = if trusted {
            vec![reqwest::Certificate::from_der(certificate.as_ref()).unwrap()]
        } else {
            vec![]
        };
        route.client = reqwest::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(format!("http://fixture:test@{address}")).unwrap())
            .tls_certs_only(certificates)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            route.fixture_session_page(
                Url::parse(&format!(
                    "https://fixture.invalid:{}/fixture",
                    origin_address.port()
                ))
                .unwrap(),
                "session=synthetic-session",
            ),
        )
        .await
        .unwrap();
        if trusted {
            assert_eq!(result.unwrap(), b"body");
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::ProxyUnreachable);
        }
        tokio::time::timeout(Duration::from_secs(3), &mut worker.0)
            .await
            .unwrap()
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), origin.accept())
                .await
                .is_err(),
            "CONNECT must not fall back to the origin"
        );
    }
}
