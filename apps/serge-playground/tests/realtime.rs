//! The native dual-carrier client against a guarded Orbic room.
#![cfg(all(feature = "realtime", not(target_arch = "wasm32")))]
use hyve::{HyveError, HyveResult, Value, model};
use orbic::{
    AuthGuard, Cache, Db, OrbicApp, RoomExt, SessionUser, TestClient, component,
    realtime::{Change, RoomServer, Rooms},
};
use serge::realtime::{Carrier, Command, Event, RoomConnection, RoomCursor, RoomEndpoints};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot, time::timeout};
use wtransport::{Identity, ServerConfig};

#[model]
#[derive(Clone)]
struct User {
    name: String,
}
impl SessionUser for User {
    fn principal(&self) -> String {
        self.name.clone()
    }
}

type Server = (
    RoomEndpoints,
    TestClient,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<std::io::Result<()>>,
);
async fn server(db: Db, cache: Cache) -> Server {
    server_on(db, cache, "127.0.0.1:0", 2).await
}
async fn server_on(db: Db, cache: Cache, ws_addr: &str, retention: u32) -> Server {
    let rooms = Rooms::new(db, cache.clone(), retention).await.unwrap();
    let counter = rooms
        .define("counter", |state, _, c| {
            if c.kind != "counter.increment" {
                return Err(HyveError::invalid_request("unknown counter operation"));
            }
            let old = match state {
                Value::Null => 0,
                Value::Int(n) => *n,
                _ => return Err(HyveError::internal("invalid counter state")),
            };
            Ok(Change {
                state: (old + 1).into(),
                kind: "counter.changed".into(),
                payload: (old + 1).into(),
            })
        })
        .unwrap()
        .on_hint(|who, path, payload| {
            if path != PATH || who.is_empty() {
                return Err(HyveError::invalid_request("unexpected hint scope"));
            }
            Ok(payload.clone())
        });
    let api = TestClient::new(
        OrbicApp::new().provide(cache).mount(
            "/v1",
            component()
                .wrap(AuthGuard::<User>::require_user())
                .room("/projects/{id}/live", counter)
                .post("/logout", orbic_auth::sign_out),
        ),
    );
    let identity = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
    let pin = *identity.certificate_chain().as_slice()[0].hash().as_ref();
    let config = ServerConfig::builder()
        .with_bind_address("127.0.0.1:0".parse().unwrap())
        .with_identity(identity)
        .build();
    let server = RoomServer::new(rooms, config, TcpListener::bind(ws_addr).await.unwrap()).unwrap();
    let (udp, tcp) = server.addresses().unwrap();
    let mut endpoints = RoomEndpoints::new(
        &format!("https://{udp}/realtime"),
        &format!("ws://{tcp}/realtime"),
    )
    .unwrap();
    endpoints.certificate_hash = Some(pin);
    let (stop, done) = oneshot::channel();
    (
        endpoints,
        api,
        stop,
        tokio::spawn(server.serve(async {
            let _ = done.await;
        })),
    )
}
const PATH: &str = "/v1/projects/17/live";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_view_subscription_recovers_and_replays_its_offline_outbox() {
    use serge::{
        prelude::*,
        realtime::{RoomUpdate, subscribe},
        testing::Headless,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let db = Db::in_memory().await;
    let cache = Cache::in_memory();
    let (endpoints, api, stop, task) = server(db.clone(), cache.clone()).await;
    let ws_addr = endpoints
        .websocket_url()
        .strip_prefix("ws://")
        .unwrap()
        .strip_suffix("/realtime")
        .unwrap()
        .to_owned();
    let alice = api.as_user(User {
        name: "subscriber".into(),
    });
    let handle = Rc::new(RefCell::new(None));
    let store = handle.clone();
    let lost = Rc::new(Cell::new(false));
    let gone = lost.clone();
    let view = Headless::new(320.0, 240.0, move || {
        let seq = hyve::state::signal(0_i64);
        let sub = subscribe(
            endpoints,
            move || {
                let api = alice.clone();
                async move { issue(&api).await }
            },
            move |update| match update {
                RoomUpdate::State(e) => seq.set(e.seq),
                RoomUpdate::Reconnecting(_) => gone.set(true),
                _ => {}
            },
        );
        sub.send("counter.increment", Value::Null).unwrap();
        *store.borrow_mut() = Some(sub);
        ui! {Text{"committed {seq()}"}}
    });
    assert!(view.wait_until(Duration::from_secs(6), |v| v.shows("committed 1")));
    let _ = stop.send(());
    task.await.unwrap().unwrap();
    assert!(view.wait_until(Duration::from_secs(6), |_| lost.get()));
    let handle = handle.borrow().as_ref().unwrap().clone();
    handle.send("counter.increment", Value::Null).unwrap();
    handle.send("counter.increment", Value::Null).unwrap();
    let (_, _, stop, task) = server_on(db, cache, &ws_addr, 2).await;
    assert!(view.wait_until(Duration::from_secs(12), |v| v.shows("committed 3")));
    view.ui().unmount();
    assert_eq!(
        handle
            .send("counter.increment", Value::Null)
            .unwrap_err()
            .code(),
        "room_closed"
    );
    let _ = stop.send(());
    task.await.unwrap().unwrap();
}

#[tokio::test]
#[ignore = "run python3 apps/serge-playground/tests/browser/realtime.py"]
async fn browser_room_transports() {
    use std::{
        io::Write,
        process::{Command as Process, Stdio},
    };
    let (endpoints, api, stop, task) = server(Db::in_memory().await, Cache::in_memory()).await;
    let alice = api.as_user(User {
        name: "browser".into(),
    });
    let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut data = Value::map();
    data.insert("udp", endpoints.webtransport_url());
    data.insert("ws", endpoints.websocket_url());
    data.insert(
        "blocked",
        format!("https://{}/realtime", silent.local_addr().unwrap()),
    );
    data.insert(
        "pin",
        endpoints
            .certificate_hash
            .unwrap()
            .into_iter()
            .map(Value::from)
            .collect::<Vec<_>>(),
    );
    let mut tickets = Vec::new();
    for _ in 0..4 {
        tickets.push(issue(&alice).await.unwrap());
    }
    data.insert("tickets", tickets);
    let input = hyve::serde::to_json(&data);
    let result = tokio::task::spawn_blocking(move || {
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/browser/realtime.py");
        let mut proc = Process::new("python3")
            .arg(script)
            .arg("--drive")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        proc.stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        proc.wait().unwrap()
    })
    .await
    .unwrap();
    let _ = stop.send(());
    task.await.unwrap().unwrap();
    assert!(result.success());
}
async fn issue(api: &TestClient) -> HyveResult<String> {
    let response = api.post(PATH).send().await;
    if response.status() != 200 {
        return Err(response.error());
    }
    Ok(response.json())
}
async fn ack(c: &mut RoomConnection) -> Event {
    timeout(Duration::from_secs(5), async {
        loop {
            let event = c.receive().await.unwrap();
            assert_ne!(event.kind, "error", "{:?}", event.payload);
            if event.kind == "room.ack" {
                return event;
            }
        }
    })
    .await
    .expect("command ack")
}

#[tokio::test]
async fn restore_applies_every_page_before_resending_the_outbox() {
    let (endpoints, api, stop, task) = server_on(
        Db::in_memory().await,
        Cache::in_memory(),
        "127.0.0.1:0",
        256,
    )
    .await;
    let alice = api.as_user(User {
        name: "alice".into(),
    });
    let mut producer = RoomConnection::connect(&endpoints, || issue(&alice))
        .await
        .unwrap();
    producer
        .restore(&mut RoomCursor::default(), |_| {})
        .await
        .unwrap();
    for seq in 1..=135 {
        producer
            .send(&Command::new("counter.increment", Value::Null))
            .await
            .unwrap();
        assert_eq!(ack(&mut producer).await.seq, seq);
    }
    let mut reader = RoomConnection::connect(&endpoints, || issue(&alice))
        .await
        .unwrap();
    let mut cursor = RoomCursor::default();
    cursor.enqueue("counter.increment", Value::Null).unwrap();
    let mut seen = Vec::new();
    reader
        .restore(&mut cursor, |e| seen.push(e.seq))
        .await
        .unwrap();
    assert_eq!(seen, (1..=135).collect::<Vec<_>>());
    assert_eq!(ack(&mut reader).await.seq, 136);
    drop((reader, producer));
    let _ = stop.send(());
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn quic_to_ws_replays_committed_commands_and_survives_a_coordinator_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}?mode=rwc", tmp.path().join("rooms.db").display());
    let db = Db::connect(&url).await.unwrap();
    let cache = Cache::in_memory();
    let (endpoints, api, stop, task) = server(db.clone(), cache.clone()).await;
    assert_eq!(api.post(PATH).send().await.status(), 401);
    let alice = api.as_user(User {
        name: "alice".into(),
    });
    let used = issue(&alice).await.unwrap();
    let mut live = RoomConnection::connect(&endpoints, || async { Ok(used.clone()) })
        .await
        .unwrap();
    assert_eq!(live.carrier(), Carrier::WebTransport);
    assert!(
        RoomConnection::connect(&endpoints, || async { Ok(used.clone()) })
            .await
            .is_err(),
        "ticket can be used only once"
    );
    let mut cursor = RoomCursor::default();
    let original = cursor
        .enqueue("counter.increment", Value::Null)
        .unwrap()
        .clone();
    live.restore(&mut cursor, |_| {}).await.unwrap();
    let first = ack(&mut live).await;
    assert_eq!(first.seq, 1);
    drop(live); // Client lost the ack before recording it.
    let _ = stop.send(());
    task.await.unwrap().unwrap();
    let (next, api, stop, task) = server(Db::connect(&url).await.unwrap(), cache).await;
    let bob = api.as_user(User { name: "bob".into() });
    let mut other = RoomConnection::connect(&next, || issue(&bob))
        .await
        .unwrap();
    other
        .restore(&mut RoomCursor::default(), |_| {})
        .await
        .unwrap();
    other.send(&original).await.unwrap();
    assert_eq!(
        ack(&mut other).await.seq,
        2,
        "same request ID for another principal is independent"
    );
    // Hold an unused loopback UDP port, forcing timeout of the first carrier.
    let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut fallback = RoomEndpoints::new(
        &format!("https://{}/realtime", silent.local_addr().unwrap()),
        next.websocket_url(),
    )
    .unwrap();
    fallback.timeout = Duration::from_millis(400);
    let tickets = AtomicUsize::new(0);
    let mut ws = RoomConnection::connect(&fallback, || {
        tickets.fetch_add(1, Ordering::Relaxed);
        issue(&alice)
    })
    .await
    .unwrap();
    assert_eq!(ws.carrier(), Carrier::WebSocket);
    assert_eq!(tickets.load(Ordering::Relaxed), 2);
    let mut seen = Vec::new();
    ws.restore(&mut cursor, |e| seen.push(e.payload.clone()))
        .await
        .unwrap();
    assert_eq!(seen, [Value::Int(1), Value::Int(2)]);
    let replay = ack(&mut ws).await;
    assert_eq!(replay, first);
    cursor.accept(&replay).unwrap();
    assert_eq!(cursor.pending().count(), 0);
    for expected in 3..=5 {
        let cmd = Command::new("counter.increment", Value::Null);
        other.send(&cmd).await.unwrap();
        assert_eq!(ack(&mut other).await.seq, expected);
    }
    let mut fresh = RoomCursor::default();
    let mut snapshot = Vec::new();
    let mut reconnect = RoomConnection::connect(&next, || issue(&alice))
        .await
        .unwrap();
    reconnect
        .restore(&mut fresh, |e| {
            snapshot.push((e.kind.clone(), e.seq, e.payload.clone()))
        })
        .await
        .unwrap();
    assert_eq!(snapshot, [("room.snapshot".into(), 5, Value::Int(5))]);
    assert_eq!(alice.post("/v1/logout").send().await.status(), 204);
    let error = timeout(Duration::from_secs(3), reconnect.receive())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        error.payload.get("code"),
        Some(&Value::String("unauthenticated".into()))
    );
    drop((ws, other, reconnect));
    let _ = stop.send(());
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn datagrams_are_ephemeral_and_single_stream_commands_commit_once() {
    let db = Db::in_memory().await;
    let (endpoints, api, stop, task) = server(db, Cache::in_memory()).await;
    let alice = api.as_user(User {
        name: "alice".into(),
    });
    let mut room = RoomConnection::connect(&endpoints, || issue(&alice))
        .await
        .unwrap();
    assert_eq!(room.carrier(), Carrier::WebTransport);
    let mut cursor = RoomCursor::default();
    room.restore(&mut cursor, |_| {}).await.unwrap();
    let id = room.send_hint(75.into()).await.unwrap();
    let hint = timeout(Duration::from_secs(3), room.receive())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            hint.kind.as_str(),
            hint.request_id.as_deref(),
            &hint.payload
        ),
        ("room.hinted", Some(id.as_str()), &Value::Int(75))
    );
    assert!(!cursor.accept(&hint).unwrap());
    assert_eq!(cursor.sequence(), 0);
    assert!(room.send_hint("x".repeat(1024).into()).await.is_err());
    let operation = Command::new("counter.increment", Value::Null);
    room.send_stream(&operation).await.unwrap();
    let first = ack(&mut room).await;
    assert_eq!(first.seq, 1);
    room.send(&operation).await.unwrap();
    assert_eq!(ack(&mut room).await, first);
    drop(room);
    let _ = stop.send(());
    task.await.unwrap().unwrap();
}
