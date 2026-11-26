use crate::{invalid, object};
use base64::{Engine, engine::general_purpose::STANDARD};
use hyve::serde::__private::rkyv;
use hyve::{
    HyveResult, Ulid, Value,
    serde::{self, Binary, FORMAT_FINGERPRINT, Fingerprint, fnv1a, mix},
};

pub(crate) const MAX_FRAME: usize = 64 * 1024;
/// Fingerprint of the protocol, codec format and all three application models.
pub const fn fingerprint<S: Fingerprint, E: Fingerprint, R: Fingerprint>() -> u64 {
    mix(
        mix(fnv1a(b"serge.webview.v1"), FORMAT_FINGERPRINT),
        mix(S::FINGERPRINT, mix(E::FINGERPRINT, R::FINGERPRINT)),
    )
}

pub(crate) fn field<'a>(v: &'a Value, key: &str) -> HyveResult<&'a Value> {
    v.get(key).ok_or_else(|| invalid("missing webview field"))
}
pub(crate) fn text<'a>(v: &'a Value, key: &str) -> HyveResult<&'a str> {
    match field(v, key)? {
        Value::String(s) => Ok(s),
        _ => Err(invalid("expected webview string")),
    }
}
pub(crate) fn seq(v: &Value, k: &str) -> HyveResult<u64> {
    let s = text(v, k)?;
    let n = s
        .parse::<u64>()
        .map_err(|_| invalid("invalid webview sequence"))?;
    if n.to_string() != s {
        return Err(invalid("noncanonical webview sequence"));
    }
    Ok(n)
}
pub(crate) fn id(v: &Value, k: &str) -> HyveResult<Ulid> {
    let s = text(v, k)?;
    s.parse::<Ulid>()
        .ok()
        .filter(|id| id.to_string() == s)
        .ok_or_else(|| invalid("invalid webview identifier"))
}
/// The persistent identity of one user input within a host session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(crate=hyve::serde::__private::rkyv)]
pub struct EventId {
    /// Monotonic user-input sequence.
    pub seq: u64,
    /// Unique identifier retained across document reloads.
    pub id: Ulid,
}
impl EventId {
    fn parse(v: &Value) -> HyveResult<Self> {
        let seq = seq(v, "eventSeq")?;
        if seq == 0 {
            return Err(invalid("zero webview event sequence"));
        }
        Ok(Self {
            seq,
            id: id(v, "id")?,
        })
    }
    fn put(&self, v: &mut Value) {
        v.insert("eventSeq", self.seq.to_string());
        v.insert("id", self.id.to_string());
    }
}
/// Host-owned result. Errors crossing the bridge contain only public fields.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(crate=hyve::serde::__private::rkyv)]
pub enum Outcome {
    /// The host accepted the event and is still executing.
    Pending,
    /// The host committed the event.
    Success(Value),
    /// A final public error.
    Error {
        /// Stable public error code.
        code: String,
        /// User-visible description.
        message: String,
    },
}
impl Outcome {
    pub(crate) fn json(&self) -> Value {
        match self {
            Self::Pending => object([("kind", "pending".into())]),
            Self::Success(v) => object([("kind", "success".into()), ("value", v.clone())]),
            Self::Error { code, message } => object([
                ("kind", "error".into()),
                ("code", code.clone().into()),
                ("message", message.clone().into()),
            ]),
        }
    }
    fn parse(v: &Value) -> HyveResult<Self> {
        Ok(match text(v, "kind")? {
            "pending" => Self::Pending,
            "success" => Self::Success(field(v, "value")?.clone()),
            "error" => Self::Error {
                code: text(v, "code")?.into(),
                message: text(v, "message")?.into(),
            },
            _ => return Err(invalid("invalid webview outcome")),
        })
    }
}
/// The most recently accepted event and its recorded result.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(crate=hyve::serde::__private::rkyv)]
pub struct Receipt {
    /// Identity of the event.
    pub event: EventId,
    /// Recorded progress or final result.
    pub outcome: Outcome,
}
impl Receipt {
    fn json(&self) -> Value {
        let mut o = object([("outcome", self.outcome.json())]);
        self.event.put(&mut o);
        o
    }
    fn parse(v: &Value) -> HyveResult<Self> {
        Ok(Self {
            event: EventId::parse(v)?,
            outcome: Outcome::parse(field(v, "outcome")?)?,
        })
    }
}
/// Data messages in archive tag order. Sequences use decimal strings on JSON.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(crate=hyve::serde::__private::rkyv)]
#[allow(missing_docs)] // Fields follow the versioned public wire contract.
pub enum Message {
    Snapshot {
        state_seq: u64,
        state: Value,
        last_event: Option<Receipt>,
    },
    AckState(u64),
    Event {
        event: EventId,
        payload: Value,
    },
    EventResult(Receipt),
    Resync(u64),
}
/// A message addressed to one host session and document generation.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(crate=hyve::serde::__private::rkyv)]
pub struct Frame {
    /// Slot and principal-bound host session.
    pub session: Ulid,
    /// Changes on every document mount.
    pub generation: u64,
    /// Direction-specific message.
    pub message: Message,
}
impl Frame {
    /// Produces the JSON envelope without converting sequence numbers through JS Number.
    pub fn json(&self) -> Value {
        let mut v = object([
            ("session", self.session.to_string().into()),
            ("generation", self.generation.to_string().into()),
        ]);
        let kind = match &self.message {
            Message::Snapshot {
                state_seq,
                state,
                last_event,
            } => {
                v.insert("stateSeq", state_seq.to_string());
                v.insert("state", state.clone());
                v.insert(
                    "lastEvent",
                    last_event.as_ref().map_or(Value::Null, Receipt::json),
                );
                "snapshot"
            }
            Message::AckState(n) => {
                v.insert("stateSeq", n.to_string());
                "ackState"
            }
            Message::Event { event, payload } => {
                event.put(&mut v);
                v.insert("payload", payload.clone());
                "event"
            }
            Message::EventResult(r) => {
                r.event.put(&mut v);
                v.insert("outcome", r.outcome.json());
                "eventResult"
            }
            Message::Resync(n) => {
                v.insert("lastStateSeq", n.to_string());
                "resync"
            }
        };
        v.insert("kind", kind);
        v
    }
    /// Checks a standard envelope and its canonical addresses.
    pub fn parse(v: &Value) -> HyveResult<Self> {
        let message = match text(v, "kind")? {
            "snapshot" => Message::Snapshot {
                state_seq: seq(v, "stateSeq")?,
                state: field(v, "state")?.clone(),
                last_event: match field(v, "lastEvent")? {
                    Value::Null => None,
                    r => Some(Receipt::parse(r)?),
                },
            },
            "ackState" => Message::AckState(seq(v, "stateSeq")?),
            "event" => Message::Event {
                event: EventId::parse(v)?,
                payload: field(v, "payload")?.clone(),
            },
            "eventResult" => Message::EventResult(Receipt::parse(v)?),
            "resync" => Message::Resync(seq(v, "lastStateSeq")?),
            _ => return Err(invalid("unknown webview message")),
        };
        Ok(Self {
            session: id(v, "session")?,
            generation: seq(v, "generation")?,
            message,
        })
    }
}
/// A negotiated rail. JSON is available even when the model fingerprints differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rail {
    /// Byte-checked archive with magic, version and schema fingerprint.
    Archive(u64),
    /// Standard UTF-8 JSON.
    Json,
}
impl Rail {
    /// Encodes a bounded IPC string.
    pub fn encode(self, frame: &Frame) -> HyveResult<String> {
        match self {
            Self::Json => {
                let s = serde::to_json(&frame.json());
                if s.len() > MAX_FRAME {
                    return Err(invalid("webview JSON exceeds 64 KiB"));
                }
                Ok(s)
            }
            Self::Archive(fp) => {
                let body = frame.to_binary()?;
                if body.len() + 14 > MAX_FRAME {
                    return Err(invalid("webview archive exceeds 64 KiB"));
                }
                let mut raw = Vec::with_capacity(body.len() + 14);
                raw.extend_from_slice(b"SGWV");
                raw.extend(1u16.to_le_bytes());
                raw.extend(fp.to_le_bytes());
                raw.extend_from_slice(&body);
                Ok(STANDARD.encode(raw))
            }
        }
    }
    /// Bounds the input before decoding and validates every untrusted archive.
    pub fn decode(self, data: &str) -> HyveResult<Frame> {
        match self {
            Self::Json if data.len() <= MAX_FRAME => {
                Frame::parse(&serde::from_json(data).map_err(|e| e.into_request_error())?)
            }
            Self::Archive(fp) if data.len() <= MAX_FRAME.div_ceil(3) * 4 => {
                let raw = STANDARD
                    .decode(data)
                    .map_err(|_| invalid("invalid webview base64"))?;
                if raw.len() > MAX_FRAME
                    || raw.len() < 14
                    || &raw[..4] != b"SGWV"
                    || raw[4..6] != 1u16.to_le_bytes()
                    || raw[6..14] != fp.to_le_bytes()
                {
                    return Err(invalid("invalid webview archive header"));
                }
                Frame::from_binary(&raw[14..]).map_err(|e| e.into_request_error())
            }
            _ => Err(invalid("webview frame exceeds 64 KiB")),
        }
    }
}
