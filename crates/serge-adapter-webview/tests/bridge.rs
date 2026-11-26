//! Both rails must preserve event identity, state acknowledgement and current authorization.
use hyve::{Ulid, Value, serde};
use serge_adapter_webview::{Delivery, EventId, Frame, Host, Message, Outcome, Rail};
fn hello(h: &mut Host, rail: Rail) -> (Ulid, u64, String) {
    let mut m: Value = serde::from_json(&h.open().unwrap()).unwrap();
    let Value::String(s) = m.get("session").unwrap() else {
        panic!()
    };
    let session = s.parse().unwrap();
    let Value::String(s) = m.get("generation").unwrap() else {
        panic!()
    };
    let generation = s.parse().unwrap();
    m.insert("kind", "ready");
    match rail {
        Rail::Json => {
            m.insert("fingerprint", "0000000000000000");
            m.insert("rail", "json");
        }
        Rail::Archive(_) => m.insert("rail", "rkyv"),
    }
    (session, generation, serde::to_json(&m))
}
#[test]
fn pending_replay_revocation_and_exact_snapshot_retry_on_both_rails() {
    for rail in [Rail::Json, Rail::Archive(16)] {
        let mut h = Host::new(Ulid::new(), 16, "first".into()).unwrap();
        let (session, generation, ready) = hello(&mut h, rail);
        let initial = h.receive(&ready, |_| Ok(())).unwrap();
        let wire = |generation, message| {
            rail.encode(&Frame {
                session,
                generation,
                message,
            })
            .unwrap()
        };
        assert_eq!(h.update("second".into()).unwrap(), Delivery::None);
        assert_eq!(h.tick().unwrap(), initial);
        let ack = wire(generation, Message::AckState(1));
        let Delivery::Send(new) = h.receive(&ack, |_| Ok(())).unwrap() else {
            panic!()
        };
        assert!(matches!(
            rail.decode(&new).unwrap().message,
            Message::Snapshot { state_seq: 2, .. }
        ));
        let event = EventId {
            seq: 1,
            id: Ulid::new(),
        };
        let command = |generation, payload: &str| {
            wire(
                generation,
                Message::Event {
                    event,
                    payload: payload.into(),
                },
            )
        };
        let saved = command(generation, "save");
        assert_eq!(
            h.receive(&saved, |_| Ok(())).unwrap(),
            Delivery::Execute(event, "save".into())
        );
        let Delivery::Send(wait) = h.receive(&saved, |_| Ok(())).unwrap() else {
            panic!()
        };
        assert!(
            matches!(rail.decode(&wait).unwrap().message,Message::EventResult(r) if r.outcome==Outcome::Pending)
        );
        assert!(matches!(
            h.receive(&command(generation, "changed"), |_| Ok(()))
                .unwrap(),
            Delivery::Send(_)
        ));
        h.suspend();
        assert_eq!(h.receive(&ready, |_| Ok(())).unwrap(), Delivery::None);
        let (_, next, ready) = hello(&mut h, rail);
        h.receive(&ready, |_| Ok(())).unwrap();
        assert_eq!(
            h.receive(&saved, |_| panic!("old session authorized"))
                .unwrap(),
            Delivery::None
        );
        let done = h.finish(event, Ok("stored".into())).unwrap();
        assert_eq!(h.receive(&command(next, "save"), |_| Ok(())).unwrap(), done);
        let Delivery::Send(denied) = h
            .receive(&command(next, "save"), |_| {
                Err(hyve::HyveError::domain("forbidden", "revoked"))
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(
            matches!(rail.decode(&denied).unwrap().message,Message::EventResult(r) if matches!(r.outcome,Outcome::Error{..}))
        );
    }
}
#[test]
fn archive_format_bounds_and_canonical_json_sequences() {
    let f = Frame {
        session: Ulid::new(),
        generation: u64::MAX,
        message: Message::AckState(u64::MAX),
    };
    let json = Rail::Json.encode(&f).unwrap();
    assert!(json.contains("\"18446744073709551615\""));
    assert_eq!(Rail::Json.decode(&json).unwrap(), f);
    assert!(
        Rail::Json
            .decode(&json.replace("18446744073709551615", "001"))
            .is_err()
    );
    let archived = Rail::Archive(21).encode(&f).unwrap();
    assert_eq!(Rail::Archive(21).decode(&archived).unwrap(), f);
    assert!(Rail::Archive(42).decode(&archived).is_err());
    assert!(
        Rail::Archive(21)
            .decode(&archived[..archived.len() - 8])
            .is_err()
    );
    assert!(Rail::Json.decode(&"a".repeat(65537)).is_err());
}

#[test]
fn a_missing_acknowledgement_reopens_once_then_disconnects() {
    for rail in [Rail::Json, Rail::Archive(16)] {
        let mut host = Host::new(Ulid::new(), 16, 1.into()).unwrap();
        for expected in [Delivery::Reopen, Delivery::Disconnected] {
            let (_, _, ready) = hello(&mut host, rail);
            let baseline = host.receive(&ready, |_| Ok(())).unwrap();
            for _ in 0..3 {
                assert_eq!(host.tick().unwrap(), baseline);
            }
            assert_eq!(host.tick().unwrap(), expected);
            assert_eq!(host.tick().unwrap(), Delivery::None);
        }
        let (session, generation, ready) = hello(&mut host, rail);
        host.receive(&ready, |_| Ok(())).unwrap();
        let ack = rail
            .encode(&Frame {
                session,
                generation,
                message: Message::AckState(1),
            })
            .unwrap();
        host.receive(&ack, |_| Ok(())).unwrap();
        host.update(2.into()).unwrap();
        for _ in 0..3 {
            host.tick().unwrap();
        }
        assert_eq!(host.tick().unwrap(), Delivery::Reopen);
    }
}
