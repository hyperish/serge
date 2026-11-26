use super::*;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use hyve::api::realtime::{BINARY_PROTOCOL, MAX_FRAME};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};
use wtransport::{
    ClientConfig, Connection, Endpoint, RecvStream, SendStream, endpoint::endpoint_side,
    tls::Sha256Digest,
};

pub(super) async fn sleep(delay: Duration) {
    tokio::time::sleep(delay).await;
}

pub(super) struct QuicSocket {
    send: FramedWrite<SendStream, LengthDelimitedCodec>,
    recv: FramedRead<RecvStream, LengthDelimitedCodec>,
    connection: Connection,
    single: bool,
    hints: bool,
    _endpoint: Endpoint<endpoint_side::Client>,
}
pub(super) enum Socket {
    Quic(Box<QuicSocket>),
    Ws(Box<WebSocketStream<MaybeTlsStream<TcpStream>>>),
}

impl Socket {
    pub async fn open(e: &RoomEndpoints, carrier: Carrier, url: &str) -> HyveResult<Self> {
        tokio::time::timeout(e.timeout, async {
            let mut socket = match carrier {
                Carrier::WebTransport => {
                    let config = ClientConfig::builder().with_bind_default();
                    let config = match e.certificate_hash {
                        Some(hash) => {
                            config.with_server_certificate_hashes([Sha256Digest::new(hash)])
                        }
                        None => config.with_native_certs(),
                    };
                    let endpoint = Endpoint::client(config.build()).map_err(|_| lost())?;
                    let connection = endpoint.connect(url).await.map_err(|_| lost())?;
                    let (mut send, recv) = connection
                        .open_bi()
                        .await
                        .map_err(|_| lost())?
                        .await
                        .map_err(|_| lost())?;
                    send.write_all(b"HYV1").await.map_err(|_| lost())?;
                    let framing = || {
                        LengthDelimitedCodec::builder()
                            .max_frame_length(MAX_FRAME)
                            .new_codec()
                    };
                    Self::Quic(Box::new(QuicSocket {
                        send: FramedWrite::new(send, framing()),
                        recv: FramedRead::new(recv, framing()),
                        connection,
                        single: false,
                        hints: false,
                        _endpoint: endpoint,
                    }))
                }
                Carrier::WebSocket => {
                    let mut req = url.into_client_request().map_err(|_| lost())?;
                    req.headers_mut().insert(
                        "sec-websocket-protocol",
                        BINARY_PROTOCOL.parse().expect("static protocol"),
                    );
                    let mut config = WebSocketConfig::default();
                    config.max_frame_size = Some(MAX_FRAME);
                    config.max_message_size = Some(MAX_FRAME);
                    let (ws, response) =
                        tokio_tungstenite::connect_async_with_config(req, Some(config), false)
                            .await
                            .map_err(|_| lost())?;
                    if !response
                        .headers()
                        .get("sec-websocket-protocol")
                        .is_some_and(|h| h == BINARY_PROTOCOL)
                    {
                        return Err(lost());
                    }
                    Self::Ws(Box::new(ws))
                }
            };
            let hello = Event::parse(&Codec::Binary.decode(&socket.receive().await?)?)?;
            if hello.kind != "hello" || hello.payload.get("version") != Some(&hyve::Value::Int(1)) {
                return Err(lost());
            }
            if let Self::Quic(q) = &mut socket {
                q.single = hello.payload.get("singleStream") == Some(&hyve::Value::Bool(true));
                q.hints = hello.payload.get("hints") == Some(&hyve::Value::Bool(true));
            }
            Ok(socket)
        })
        .await
        .map_err(|_| lost())?
    }
    pub async fn send(&mut self, bytes: Vec<u8>) -> HyveResult<()> {
        match self {
            Self::Quic(socket) => socket
                .send
                .send(Bytes::from(bytes))
                .await
                .map_err(|_| lost()),
            Self::Ws(ws) => ws
                .send(Message::Binary(bytes.into()))
                .await
                .map_err(|_| lost()),
        }
    }
    pub async fn send_stream(&mut self, bytes: Vec<u8>) -> HyveResult<()> {
        let Self::Quic(socket) = self else {
            return self.send(bytes).await;
        };
        if !socket.single {
            return self.send(bytes).await;
        }
        let mut tx = socket
            .connection
            .open_uni()
            .await
            .map_err(|_| lost())?
            .await
            .map_err(|_| lost())?;
        tx.write_all(b"HYU1").await.map_err(|_| lost())?;
        tx.write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .map_err(|_| lost())?;
        tx.write_all(&bytes).await.map_err(|_| lost())?;
        tx.finish().await.map_err(|_| lost())
    }
    pub async fn send_hint(&mut self, bytes: Vec<u8>) -> HyveResult<()> {
        let Self::Quic(socket) = self else {
            return self.send(bytes).await;
        };
        if socket.hints
            && socket
                .connection
                .max_datagram_size()
                .is_some_and(|limit| bytes.len() <= limit)
        {
            socket.connection.send_datagram(bytes).map_err(|_| lost())
        } else {
            self.send(bytes).await
        }
    }
    pub async fn receive(&mut self) -> HyveResult<Vec<u8>> {
        match self {
            Self::Quic(socket) => tokio::select! {
                value = socket.recv.next() => value.ok_or_else(lost)?.map(|b| b.to_vec()).map_err(|_| lost()),
                value = socket.connection.receive_datagram(), if socket.hints => {
                    let bytes = value.map_err(|_| lost())?.payload();
                    if bytes.len() > hyve::api::realtime::MAX_HINT { return Err(HyveError::invalid_request("room hint too large")); }
                    Ok(bytes.to_vec())
                }
            },
            Self::Ws(ws) => loop {
                match ws.next().await.ok_or_else(lost)?.map_err(|_| lost())? {
                    Message::Binary(b) => return Ok(b.to_vec()),
                    Message::Ping(_) => ws.flush().await.map_err(|_| lost())?,
                    Message::Pong(_) => {}
                    _ => return Err(lost()),
                }
            },
        }
    }
}
