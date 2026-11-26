use crate::{
    invalid, object,
    wire::{EventId, Frame, MAX_FRAME, Message, Outcome, Rail, Receipt, field, id, seq, text},
};
use hyve::{
    HyveResult, Ulid, Value,
    serde::{self, Binary},
};

/// What the host should do after a checked message.
#[derive(Debug, PartialEq)]
pub enum Delivery {
    /// Nothing to send, including stale documents and duplicate state acknowledgements.
    None,
    /// Send the returned string to the current document.
    Send(String),
    /// The event is recorded as pending. Run the application handler once, then call Host::finish.
    Execute(EventId, Value),
    /// Reopen the document once and negotiate a new generation.
    Reopen,
    /// Stop automatic recovery and surface the disconnected state.
    Disconnected,
}

struct Last {
    receipt: Receipt,
    payload: Value,
}

/// State for one authenticated principal and one slot. Drop the host and construct a
/// new session when either changes. Authorization is checked at each event, including retries.
pub struct Host {
    build: Ulid,
    session: Ulid,
    generation: u64,
    fp: u64,
    rail: Option<Rail>,
    open: bool,
    state: Value,
    state_seq: u64,
    unacked: Option<Frame>,
    retries: u8,
    recovering: bool,
    dirty: bool,
    last: Option<Last>,
}

fn bounded(value: &Value) -> HyveResult<()> {
    // A snapshot carries both a state and a result, plus addresses and base64 framing.
    if serde::to_json(value).len() > ((MAX_FRAME - 1024) / 2)
        || value.to_binary()?.len() > ((MAX_FRAME - 1024) / 2)
    {
        Err(invalid("webview value too large for a snapshot"))
    } else {
        Ok(())
    }
}
impl Host {
    /// Binds one build, model fingerprint and initial state to a fresh session.
    pub fn new(build: Ulid, fp: u64, state: Value) -> HyveResult<Self> {
        bounded(&state)?;
        Ok(Self {
            build,
            session: Ulid::new(),
            generation: 0,
            fp,
            rail: None,
            open: false,
            state,
            state_seq: 1,
            unacked: None,
            retries: 0,
            recovering: false,
            dirty: false,
            last: None,
        })
    }
    /// Invalidates the previous document immediately at navigation start.
    pub fn suspend(&mut self) {
        self.open = false;
        self.rail = None;
        self.unacked = None;
    }
    /// Opens a new document generation and emits its JSON Hello.
    pub fn open(&mut self) -> HyveResult<String> {
        self.suspend();
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("webview session exhausted"))?;
        self.open = true;
        Ok(serde::to_json(&object([
            ("version", 1.into()),
            ("kind", "hello".into()),
            ("buildId", self.build.to_string().into()),
            ("session", self.session.to_string().into()),
            ("generation", self.generation.to_string().into()),
            ("fingerprint", format!("{:016x}", self.fp).into()),
        ])))
    }
    fn snapshot(&mut self) -> HyveResult<Delivery> {
        if self.dirty {
            self.state_seq = self
                .state_seq
                .checked_add(1)
                .ok_or_else(|| invalid("webview state sequence exhausted"))?;
            self.dirty = false;
        }
        let frame = Frame {
            session: self.session,
            generation: self.generation,
            message: Message::Snapshot {
                state_seq: self.state_seq,
                state: self.state.clone(),
                last_event: self.last.as_ref().map(|l| l.receipt.clone()),
            },
        };
        let Some(rail) = self.rail else {
            return Ok(Delivery::None);
        };
        let encoded = rail.encode(&frame)?;
        self.unacked = Some(frame);
        self.retries = 0;
        Ok(Delivery::Send(encoded))
    }
    fn send(&self, message: Message) -> HyveResult<Delivery> {
        match self.rail {
            Some(rail) => Ok(Delivery::Send(rail.encode(&Frame {
                session: self.session,
                generation: self.generation,
                message,
            })?)),
            None => Ok(Delivery::None),
        }
    }
    /// Retransmit an unacknowledged snapshot at the one-second document heartbeat.
    /// Reopen once after three retries. Acknowledging the new baseline restores the retry budget.
    pub fn tick(&mut self) -> HyveResult<Delivery> {
        let (Some(frame), Some(rail)) = (&self.unacked, self.rail) else {
            return Ok(Delivery::None);
        };
        if self.retries >= 3 {
            self.suspend();
            return Ok(if std::mem::replace(&mut self.recovering, true) {
                Delivery::Disconnected
            } else {
                Delivery::Reopen
            });
        }
        self.retries += 1;
        Ok(Delivery::Send(rail.encode(frame)?))
    }
    /// Changes the authoritative state. Coalesces updates while one snapshot is in flight.
    pub fn update(&mut self, state: Value) -> HyveResult<Delivery> {
        bounded(&state)?;
        if self.state == state {
            return Ok(Delivery::None);
        }
        self.state = state;
        self.dirty = true;
        if self.unacked.is_some() || self.rail.is_none() {
            Ok(Delivery::None)
        } else {
            self.snapshot()
        }
    }
    fn ready(&mut self, ready: &Value) -> HyveResult<Delivery> {
        if self.generation == 0
            || id(ready, "session")? != self.session
            || seq(ready, "generation")? != self.generation
        {
            return Ok(Delivery::None);
        }
        if field(ready, "version")? != &Value::Int(1) || id(ready, "buildId")? != self.build {
            return Err(invalid("incompatible webview document version or build"));
        }
        let fp = text(ready, "fingerprint")?;
        let fp = u64::from_str_radix(fp, 16)
            .ok()
            .filter(|v| format!("{v:016x}") == fp)
            .ok_or_else(|| invalid("invalid webview fingerprint"))?;
        self.rail = Some(match (fp == self.fp, text(ready, "rail")?) {
            (true, "rkyv") => Rail::Archive(self.fp),
            (false, "json") => Rail::Json,
            _ => return Err(invalid("inconsistent webview rail selection")),
        });
        self.snapshot()
    }
    /// Receives a document frame. The authorization closure must consult current host permissions.
    pub fn receive(
        &mut self,
        data: &str,
        authorize: impl FnOnce(&Value) -> HyveResult<()>,
    ) -> HyveResult<Delivery> {
        if !self.open {
            return Ok(Delivery::None);
        }
        if data.len() <= 1024
            && data.starts_with('{')
            && let Ok(ready) = serde::from_json::<Value>(data)
            && text(&ready, "kind").ok() == Some("ready")
        {
            return self.ready(&ready);
        }
        let Some(rail) = self.rail else {
            return Ok(Delivery::None);
        };
        let frame = rail.decode(data)?;
        if frame.session != self.session || frame.generation != self.generation {
            return Ok(Delivery::None);
        }
        match frame.message {
            Message::AckState(n) if self.unacked.is_some() && n == self.state_seq => {
                self.unacked = None;
                self.recovering = false;
                if self.dirty {
                    self.snapshot()
                } else {
                    Ok(Delivery::None)
                }
            }
            Message::AckState(_) => Ok(Delivery::None),
            Message::Resync(_) => self.snapshot(),
            Message::Event { event, payload } => {
                let permission = authorize(&payload);
                if let Some(last) = &self.last {
                    if event == last.receipt.event && payload == last.payload {
                        let mut r = last.receipt.clone();
                        if let Err(e) = permission {
                            r.outcome = public_error(e)
                        }
                        return self.send(Message::EventResult(r));
                    }
                    if last.receipt.outcome == Outcome::Pending
                        || event.id == last.receipt.event.id
                        || last.receipt.event.seq.checked_add(1) != Some(event.seq)
                    {
                        return self.snapshot();
                    }
                } else if event.seq != 1 {
                    return self.snapshot();
                }
                self.last = Some(Last {
                    receipt: Receipt {
                        event,
                        outcome: Outcome::Pending,
                    },
                    payload: payload.clone(),
                });
                if let Err(e) = permission {
                    return self.finish(event, Err(e));
                }
                Ok(Delivery::Execute(event, payload))
            }
            _ => Err(invalid("wrong webview message direction")),
        }
    }
    /// Records the final result even if a document was reloaded during execution.
    pub fn finish(&mut self, event: EventId, result: HyveResult<Value>) -> HyveResult<Delivery> {
        let Some(last) = &mut self.last else {
            return Ok(Delivery::None);
        };
        if last.receipt.event != event || last.receipt.outcome != Outcome::Pending {
            return Ok(Delivery::None);
        }
        let mut outcome = match result {
            Ok(v) => Outcome::Success(v),
            Err(e) => public_error(e),
        };
        if bounded(&outcome.json()).is_err() {
            outcome = public_error(invalid("webview result too large"))
        }
        last.receipt.outcome = outcome;
        let receipt = last.receipt.clone();
        self.send(Message::EventResult(receipt))
    }
}
fn public_error(e: hyve::HyveError) -> Outcome {
    let e = e.to_public();
    Outcome::Error {
        code: e.code().into(),
        message: e.message().into(),
    }
}
