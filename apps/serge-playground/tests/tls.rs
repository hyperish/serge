//! `https://` remote calls. A TLS front on loopback (a test CA, a leaf with
//! an IP SAN) forwards to the orbic backend: with the CA pinned, calls give
//! the same results as in-process calls on both rails (and the `415`
//! fallback), and are cancelled with their scope. A CA the operating system
//! does not trust and a certificate for another name are refused before
//! anything reaches the backend, and the refusal is not retried on the
//! standard rail.
#![cfg(feature = "server")]
#![allow(missing_docs)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use hyve::api::{Rail, Transport, provide_transport};
use hyve::prelude::*;
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use serge::RemoteTransport;
use serge_playground::*;

struct Ca {
    pem: String,
    params: CertificateParams,
    key: KeyPair,
}

fn ca() -> Ca {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "serge test CA");
    let key = KeyPair::generate().unwrap();
    let pem = params.self_signed(&key).unwrap().pem();
    Ca { pem, params, key }
}

/// The backend behind a TLS front whose certificate names `name` and is
/// signed by `ca`; returns the front's `/v1` URL and how many connections
/// got through the handshake to the backend.
fn serve_tls(ca: &Ca, name: &str) -> (String, Arc<AtomicUsize>) {
    let rt = serge::runtime();
    let backend_listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let backend_addr = backend_listener.local_addr().unwrap();
    rt.spawn(orbic::serve(
        backend(),
        backend_listener,
        std::future::pending(),
    ));

    let leaf_key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(vec![name.to_owned()])
        .unwrap()
        .signed_by(&leaf_key, &Issuer::from_params(&ca.params, &ca.key))
        .unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![leaf.der().clone()],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
    )
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let front = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let addr = front.local_addr().unwrap();
    let through = Arc::new(AtomicUsize::new(0));
    let counter = through.clone();
    rt.spawn(async move {
        while let Ok((tcp, _)) = front.accept().await {
            let (acceptor, counter) = (acceptor.clone(), counter.clone());
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                counter.fetch_add(1, Ordering::SeqCst);
                if let Ok(mut plain) = tokio::net::TcpStream::connect(backend_addr).await {
                    let _ = tokio::io::copy_bidirectional(&mut tls, &mut plain).await;
                }
            });
        }
    });
    (format!("https://{addr}/v1"), through)
}

fn scope_with(t: Arc<dyn Transport>) -> Scope {
    let s = Scope::root();
    provide_transport(&s, t);
    s
}

fn run<F: std::future::IntoFuture>(scope: &Scope, f: impl FnOnce() -> F) -> F::Output {
    let call = scope.enter(|| f().into_future());
    serge::runtime().block_on(scope.scoped(call))
}

fn in_process() -> Arc<dyn Transport> {
    Arc::new(orbic::InProcess::new(backend().build().unwrap()))
}

#[test]
fn pinned_roots_carry_calls_over_tls() {
    let ca = ca();
    let (base, through) = serve_tls(&ca, "127.0.0.1");
    let local = scope_with(in_process());
    for remote in [
        RemoteTransport::new(&base).root_certificates(&ca.pem),
        RemoteTransport::new(&base)
            .root_certificates(&ca.pem)
            .rail(Rail::Standard),
        // A stale schema: 415, then once more on the standard rail.
        RemoteTransport::new(&base)
            .root_certificates(&ca.pem)
            .claim_fingerprint(0xdead_beef),
    ] {
        let r = scope_with(remote.into_arc());
        let a = run(&r, || {
            create_project_rpc(CreateProject {
                title: "over tls".into(),
            })
        });
        assert_eq!(a.unwrap().title, "over tls");
        let a = run(&r, || {
            create_project_rpc(CreateProject {
                title: String::new(),
            })
        })
        .unwrap_err();
        let b = run(&local, || {
            create_project_rpc(CreateProject {
                title: String::new(),
            })
        })
        .unwrap_err();
        assert_eq!((a.code(), a.message()), (b.code(), b.message()));
        assert_eq!(a.details(), b.details());
        let list = run(&r, list_projects).unwrap();
        assert!(list.iter().any(|p| p.title == "over tls"), "{list:?}");
    }
    assert!(through.load(Ordering::SeqCst) > 0);

    // Cancelled with the scope, over TLS too.
    let scope = scope_with(
        RemoteTransport::new(&base)
            .root_certificates(&ca.pem)
            .into_arc(),
    );
    let s2 = scope.clone();
    serge::runtime().spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        s2.dispose();
    });
    let start = Instant::now();
    let e = run(&scope, || build_report(ReportRequest { millis: 5000 })).unwrap_err();
    assert_eq!(e.code(), "cancelled", "{e:?}");
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn untrusted_or_misnamed_certificates_are_refused() {
    let ca = ca();
    // The system's trust store does not know the test CA.
    let (base, through) = serve_tls(&ca, "127.0.0.1");
    // A certificate from the pinned CA, but for another name.
    let (misnamed, misnamed_through) = serve_tls(&ca, "example.invalid");
    for (transport, what) in [
        (RemoteTransport::new(&base), "unknown CA"),
        (
            RemoteTransport::new(&base).rail(Rail::Standard),
            "unknown CA, standard rail",
        ),
        (
            RemoteTransport::new(&base).claim_fingerprint(0xdead_beef),
            "unknown CA, stale schema",
        ),
        (
            RemoteTransport::new(&misnamed).root_certificates(&ca.pem),
            "another name",
        ),
    ] {
        let r = scope_with(transport.into_arc());
        let e = run(&r, list_projects).unwrap_err();
        eprintln!("{what}: {}", e.message());
        assert_eq!(e.code(), "transport_unavailable", "{what}: {e:?}");
        assert!(
            e.message().contains("certificate"),
            "{what}: {}",
            e.message()
        );
    }
    assert_eq!(
        through.load(Ordering::SeqCst),
        0,
        "nothing reached the backend"
    );
    assert_eq!(misnamed_through.load(Ordering::SeqCst), 0);
}

#[test]
fn bad_root_certificates_fail_every_call_with_the_reason() {
    let (base, _) = serve_tls(&ca(), "127.0.0.1");
    for (pem, says) in [
        ("not a certificate", "no certificate"),
        (
            "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
            "root certificate",
        ),
    ] {
        let r = scope_with(
            RemoteTransport::new(&base)
                .root_certificates(pem)
                .into_arc(),
        );
        let e = run(&r, list_projects).unwrap_err();
        assert_eq!(e.code(), "transport_unavailable");
        assert!(e.message().contains(says), "{}", e.message());
    }
}
