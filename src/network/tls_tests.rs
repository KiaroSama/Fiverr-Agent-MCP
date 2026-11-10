//! Loopback-only CONNECT, independent proxy/origin TLS trust and DNS separation.
use super::*;
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
};
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
async fn proxy_bootstrap_dns_failure_never_resolves_or_connects_to_destination() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(
        crate::storage::Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    for scheme in ["http", "https"] {
        let origin = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let proxy = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let proxy_url = format!(
            "{scheme}://proxy.invalid:{}",
            proxy.local_addr().unwrap().port()
        );
        Settings::change(&store, true, |s| {
            s.mode = Mode::Proxy;
            s.profiles = vec![profiles::Profile {
                id: "fixture".into(),
                label: "DNS refusal".into(),
                url: proxy_url.clone(),
            }];
            s.selected_profile_id = Some("fixture".into());
            Ok(())
        })
        .unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut route = RouteClient::new(store.clone()).unwrap();
        route.client = reqwest::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(&proxy_url).unwrap())
            .dns_resolver(Arc::new(Resolver {
                address: None,
                calls: calls.clone(),
            }))
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            route.fixture_get(
                Url::parse(&format!(
                    "https://127.0.0.1:{}/fixture",
                    origin.local_addr().unwrap().port()
                ))
                .unwrap(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap_err().code, ErrorCode::ProxyUnreachable);
        assert_eq!(*calls.lock().unwrap(), ["proxy.invalid"]);
        for listener in [origin, proxy] {
            assert!(
                tokio::time::timeout(Duration::from_millis(20), listener.accept())
                    .await
                    .is_err(),
                "DNS refusal must make no connection"
            );
        }
    }
}

struct Resolver {
    address: Option<SocketAddr>,
    calls: Arc<Mutex<Vec<String>>>,
}
impl reqwest::dns::Resolve for Resolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        self.calls.lock().unwrap().push(name.as_str().to_owned());
        let address = self.address;
        let proxy = name.as_str() == "proxy.invalid";
        Box::pin(async move {
            match address.filter(|_| proxy) {
                Some(address) => Ok(Box::new(std::iter::once(address)) as reqwest::dns::Addrs),
                None => Err(std::io::Error::other("Synthetic resolver refusal").into()),
            }
        })
    }
}
trait FixtureIo: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> FixtureIo for T {}

fn certificate(name: &str) -> (TlsAcceptor, reqwest::Certificate) {
    let certified = rcgen::generate_simple_self_signed(vec![name.into()]).unwrap();
    let der = certified.cert.der().clone();
    let key = PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![der.clone()], key.into())
        .unwrap();
    (
        TlsAcceptor::from(Arc::new(config)),
        reqwest::Certificate::from_der(der.as_ref()).unwrap(),
    )
}

#[tokio::test]
async fn trusted_connect_tunnel_keeps_proxy_auth_outside_tls_and_cookie_inside() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(
        crate::storage::Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    // Trust and name checks are independent at the proxy and destination TLS layers.
    for (secure_proxy, trust_proxy, trust_origin, proxy_name, origin_name) in [
        (false, false, true, "proxy.invalid", "fixture.invalid"),
        (false, false, false, "proxy.invalid", "fixture.invalid"),
        (true, true, true, "proxy.invalid", "fixture.invalid"),
        (true, false, true, "proxy.invalid", "fixture.invalid"),
        (true, true, false, "proxy.invalid", "fixture.invalid"),
        (true, true, true, "wrong.invalid", "fixture.invalid"),
        (true, true, true, "proxy.invalid", "wrong.invalid"),
    ] {
        let (proxy_acceptor, proxy_certificate) = certificate(proxy_name);
        let (acceptor, origin_certificate) = certificate(origin_name);
        let proxy_valid = trust_proxy && proxy_name == "proxy.invalid";
        let origin_valid = trust_origin && origin_name == "fixture.invalid";
        let trusted = (!secure_proxy || proxy_valid) && origin_valid;
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
            let (stream, _) = proxy.accept().await.unwrap();
            let mut stream: Box<dyn FixtureIo> = if secure_proxy {
                let tls = proxy_acceptor.accept(stream).await;
                if !proxy_valid {
                    assert!(
                        tls.is_err(),
                        "invalid proxy certificate must stop before CONNECT"
                    );
                    return;
                }
                Box::new(tls.unwrap())
            } else {
                Box::new(stream)
            };
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
        let scheme = if secure_proxy { "https" } else { "http" };
        let proxy_url = format!("{scheme}://fixture:test@proxy.invalid:{}", address.port());
        Settings::change(&store, true, |s| {
            s.mode = Mode::Proxy;
            s.profiles = vec![profiles::Profile {
                id: "fixture".into(),
                label: "Synthetic TLS".into(),
                url: proxy_url.clone(),
            }];
            s.selected_profile_id = Some("fixture".into());
            Ok(())
        })
        .unwrap();
        let mut route = RouteClient::new(store.clone()).unwrap();
        // Only fixture trust changes; the production request/cancellation/body path stays.
        // No invalid-certificate bypass exists, nor any host trust-store mutation.
        let mut certificates = Vec::new();
        if trust_proxy {
            certificates.push(proxy_certificate);
        }
        if trust_origin {
            certificates.push(origin_certificate);
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        route.client = reqwest::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(&proxy_url).unwrap())
            .dns_resolver(Arc::new(Resolver {
                address: Some(address),
                calls: calls.clone(),
            }))
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
        assert_eq!(
            *calls.lock().unwrap(),
            ["proxy.invalid"],
            "destination DNS must stay in CONNECT"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), origin.accept())
                .await
                .is_err(),
            "CONNECT must not fall back to the origin"
        );
    }
}
