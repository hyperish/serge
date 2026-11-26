//! Authenticated room streams on desktop and web.
pub use hyve::api::realtime::{Codec, Command, Event, RoomCursor, reconnect_delay};
use hyve::{HyveError, HyveResult};
use std::{future::Future, time::Duration};
use url::{Host, Url};
#[cfg_attr(target_arch = "wasm32", path = "web.rs")]
#[cfg_attr(not(target_arch = "wasm32"), path = "native.rs")]
mod platform;
mod subscription;
pub use subscription::{RoomHandle, RoomUpdate, subscribe};

fn lost() -> HyveError {
    HyveError::infra("connection_lost", "room connection unavailable").retryable(true)
}

/// Transport addresses without credentials. Each connection attempt obtains
/// its own ticket, including when UDP is unavailable.
#[derive(Clone)]
pub struct RoomEndpoints {
    quic: Url,
    ws: Url,
    /// Optional SHA-256 of a WebTransport development certificate.
    pub certificate_hash: Option<[u8; 32]>,
    /// Time budget per carrier handshake.
    pub timeout: Duration,
}
impl RoomEndpoints {
    /// Preferred HTTP/3 endpoint without credentials.
    pub fn webtransport_url(&self) -> &str {
        self.quic.as_str()
    }
    /// Fallback endpoint without credentials.
    pub fn websocket_url(&self) -> &str {
        self.ws.as_str()
    }
    /// QUIC uses HTTPS. WS uses WSS or a loopback WS origin.
    pub fn new(quic: &str, websocket: &str) -> HyveResult<Self> {
        let parse = |url: &str| {
            Url::parse(url).map_err(|_| HyveError::invalid_request("invalid room endpoint"))
        };
        let quic = parse(quic)?;
        let ws = parse(websocket)?;
        let loopback = matches!(ws.host(), Some(Host::Domain("localhost")))
            || match ws.host() {
                Some(Host::Ipv4(ip)) => ip.is_loopback(),
                Some(Host::Ipv6(ip)) => ip.is_loopback(),
                _ => false,
            };
        if quic.scheme() != "https"
            || !(ws.scheme() == "wss" || ws.scheme() == "ws" && loopback)
            || [&quic, &ws].iter().any(|u| {
                u.host().is_none()
                    || !u.username().is_empty()
                    || u.password().is_some()
                    || u.query().is_some()
                    || u.fragment().is_some()
            })
        {
            return Err(HyveError::invalid_request(
                "room endpoints require TLS or loopback WebSocket and a credential-free URL",
            ));
        }
        Ok(Self {
            quic,
            ws,
            certificate_hash: None,
            timeout: Duration::from_secs(3),
        })
    }
}

/// The carrier selected by a successful handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carrier {
    /// HTTP/3 bidirectional stream.
    WebTransport,
    /// Binary Hyve envelope in a WebSocket frame.
    WebSocket,
}

/// A live authenticated connection. Dropping it closes its transport.
pub struct RoomConnection {
    socket: platform::Socket,
    carrier: Carrier,
}
impl RoomConnection {
    /// Tries WebTransport and falls back to WebSocket. `issue_ticket` should
    /// call the authorized component route over the existing HTTP transport.
    pub async fn connect<F, Fut>(endpoints: &RoomEndpoints, mut issue_ticket: F) -> HyveResult<Self>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = HyveResult<String>>,
    {
        for carrier in [Carrier::WebTransport, Carrier::WebSocket] {
            let ticket = issue_ticket().await?;
            let mut url = match carrier {
                Carrier::WebTransport => endpoints.quic.clone(),
                Carrier::WebSocket => endpoints.ws.clone(),
            };
            url.query_pairs_mut().append_pair("ticket", &ticket);
            if let Ok(socket) = platform::Socket::open(endpoints, carrier, url.as_str()).await {
                return Ok(Self { socket, carrier });
            }
        }
        Err(lost())
    }
    /// Negotiated carrier.
    pub fn carrier(&self) -> Carrier {
        self.carrier
    }
    /// Sends an operation with its original ID.
    pub async fn send(&mut self, cmd: &Command) -> HyveResult<()> {
        self.socket
            .send(Codec::Binary.encode(&cmd.envelope())?)
            .await
    }
    /// Dispatches an independent application command on its own unidirectional
    /// QUIC stream. Its durable result arrives through `receive`. WebSocket uses
    /// the same command ID and envelope on the existing connection.
    pub async fn send_stream(&mut self, cmd: &Command) -> HyveResult<()> {
        if cmd.kind.starts_with("room.") {
            return Err(HyveError::invalid_request(
                "send room controls on the primary stream",
            ));
        }
        self.socket
            .send_stream(Codec::Binary.encode(&cmd.envelope())?)
            .await
    }
    /// Sends a transient input after resume. A reply, if delivered, has type
    /// `room.hinted` and this request ID. Hints are never queued or replayed.
    pub async fn send_hint(&mut self, value: hyve::Value) -> HyveResult<String> {
        let cmd = Command::new("room.hint", value);
        let bytes = Codec::Binary.encode(&cmd.envelope())?;
        if bytes.len() > hyve::api::realtime::MAX_HINT {
            return Err(HyveError::invalid_request("room hint too large"));
        }
        self.socket.send_hint(bytes).await?;
        Ok(cmd.id)
    }
    /// Receives the next validated server event.
    pub async fn receive(&mut self) -> HyveResult<Event> {
        Event::parse(&Codec::Binary.decode(&self.socket.receive().await?)?)
    }
    /// Resumes at the last applied sequence, delivers missing state, and
    /// resends queued operations with their original IDs in submission order.
    pub async fn restore(
        &mut self,
        cursor: &mut RoomCursor,
        mut on_state: impl FnMut(&Event),
    ) -> HyveResult<()> {
        self.send(&cursor.resume()).await?;
        loop {
            let event = self.receive().await?;
            if event.kind == "error" {
                return Err(lost());
            }
            if cursor.accept(&event)? {
                on_state(&event);
            }
            if event.kind == "room.ready" {
                break;
            }
        }
        for command in cursor.pending() {
            self.send(command).await?;
        }
        Ok(())
    }
}
