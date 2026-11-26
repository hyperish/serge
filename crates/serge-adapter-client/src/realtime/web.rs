use super::*;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/realtime/room.js")]
extern "C" {
    #[wasm_bindgen(js_name=roomSleep)]
    async fn room_sleep(milliseconds: u32);
    type BrowserRoom;
    #[wasm_bindgen(constructor, catch)]
    fn new(url: &str, quic: bool, pin: &[u8], timeout_ms: u32) -> Result<BrowserRoom, JsValue>;
    #[wasm_bindgen(method, catch)]
    async fn ready(this: &BrowserRoom) -> Result<Uint8Array, JsValue>;
    #[wasm_bindgen(method, catch)]
    async fn write(this: &BrowserRoom, bytes: Uint8Array) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name=writeStream)]
    async fn write_stream(this: &BrowserRoom, bytes: Uint8Array) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name=writeHint)]
    async fn write_hint(this: &BrowserRoom, bytes: Uint8Array) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn read(this: &BrowserRoom) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(method)]
    fn close(this: &BrowserRoom);
    #[wasm_bindgen(method)]
    fn extensions(this: &BrowserRoom, hints: bool, single: bool);
}

pub(super) async fn sleep(delay: Duration) {
    room_sleep(delay.as_millis().min(i32::MAX as u128) as u32).await;
}

pub(super) struct Socket(BrowserRoom, Option<js_sys::Promise>);
impl Drop for Socket {
    fn drop(&mut self) {
        self.0.close();
    }
}
impl Socket {
    pub async fn open(e: &RoomEndpoints, carrier: Carrier, url: &str) -> HyveResult<Self> {
        let pin = e
            .certificate_hash
            .as_ref()
            .map(<[_; 32]>::as_slice)
            .unwrap_or_default();
        let socket = Self(
            BrowserRoom::new(
                url,
                carrier == Carrier::WebTransport,
                pin,
                e.timeout.as_millis().clamp(1, i32::MAX as u128) as u32,
            )
            .map_err(|_| lost())?,
            None,
        );
        let hello = socket.0.ready().await.map_err(|_| lost())?;
        let hello = Event::parse(&Codec::Binary.decode(&hello.to_vec())?)?;
        if hello.kind != "hello" || hello.payload.get("version") != Some(&hyve::Value::Int(1)) {
            return Err(lost());
        }
        socket.0.extensions(
            hello.payload.get("hints") == Some(&hyve::Value::Bool(true)),
            hello.payload.get("singleStream") == Some(&hyve::Value::Bool(true)),
        );
        Ok(socket)
    }
    pub async fn send(&mut self, bytes: Vec<u8>) -> HyveResult<()> {
        self.0
            .write(Uint8Array::from(bytes.as_slice()))
            .await
            .map_err(|_| lost())
    }
    pub async fn send_stream(&mut self, bytes: Vec<u8>) -> HyveResult<()> {
        self.0
            .write_stream(Uint8Array::from(bytes.as_slice()))
            .await
            .map_err(|_| lost())
    }
    pub async fn send_hint(&mut self, bytes: Vec<u8>) -> HyveResult<()> {
        self.0
            .write_hint(Uint8Array::from(bytes.as_slice()))
            .await
            .map_err(|_| lost())
    }
    pub async fn receive(&mut self) -> HyveResult<Vec<u8>> {
        // An outgoing command or heartbeat may cancel this Rust future while
        // a browser read is already outstanding. Reuse that exact JS promise.
        let read = match &self.1 {
            Some(read) => read.clone(),
            None => {
                let read = self.0.read().map_err(|_| lost())?;
                self.1 = Some(read.clone());
                read
            }
        };
        let result = wasm_bindgen_futures::JsFuture::from(read).await;
        self.1 = None;
        Ok(Uint8Array::new(&result.map_err(|_| lost())?).to_vec())
    }
}
