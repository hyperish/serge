//! The native browser room client, driven by the playground's transport test.
#![cfg(target_arch = "wasm32")]
use hyve::Value;
use serge::realtime::{Carrier, RoomConnection, RoomCursor, RoomEndpoints};
use std::time::Duration;
use wasm_bindgen::prelude::*;

/// Applies two commands across a QUIC connection and a blocked-UDP fallback.
#[wasm_bindgen]
pub async fn exercise_room(
    udp: &str,
    blocked: &str,
    ws: &str,
    pin: Vec<u8>,
    tickets: js_sys::Array,
) -> Result<String, JsValue> {
    let mut cursor = RoomCursor::default();
    let mut observed = Vec::new();
    let mut issued = 0;
    for (address, expected) in [(udp, Carrier::WebTransport), (blocked, Carrier::WebSocket)] {
        let mut endpoints =
            RoomEndpoints::new(address, ws).map_err(|e| JsValue::from_str(e.message()))?;
        endpoints.certificate_hash = Some(
            pin.as_slice()
                .try_into()
                .map_err(|_| "invalid certificate pin")?,
        );
        endpoints.timeout = Duration::from_secs(2);
        let mut conn = RoomConnection::connect(&endpoints, || {
            let token = tickets.get(issued).as_string();
            issued += 1;
            async move {
                token.ok_or_else(|| hyve::HyveError::invalid_request("no remaining test ticket"))
            }
        })
        .await
        .map_err(|e| JsValue::from_str(e.message()))?;
        if conn.carrier() != expected {
            return Err("unexpected carrier".into());
        }
        conn.restore(&mut cursor, |e| observed.push(e.payload.clone()))
            .await
            .map_err(|e| JsValue::from_str(e.message()))?;
        let hint_id = conn
            .send_hint(Value::Int(37))
            .await
            .map_err(|e| JsValue::from_str(e.message()))?;
        let hint = conn
            .receive()
            .await
            .map_err(|e| JsValue::from_str(e.message()))?;
        if hint.kind != "room.hinted"
            || hint.request_id.as_deref() != Some(&hint_id)
            || hint.payload != Value::Int(37)
        {
            return Err("missing scoped transient reply".into());
        }
        let cmd = cursor
            .enqueue("counter.increment", Value::Null)
            .map_err(|e| JsValue::from_str(e.message()))?;
        conn.send_stream(cmd)
            .await
            .map_err(|e| JsValue::from_str(e.message()))?;
        let count = observed.len() + 1;
        while cursor.pending().count() != 0 || observed.len() < count {
            let e = conn
                .receive()
                .await
                .map_err(|e| JsValue::from_str(e.message()))?;
            if e.kind == "error" {
                return Err("room command failed".into());
            }
            if cursor
                .accept(&e)
                .map_err(|e| JsValue::from_str(e.message()))?
            {
                observed.push(e.payload);
            }
        }
    }
    if issued != 3 || observed != [Value::Int(1), Value::Int(2)] {
        return Err("room order or ticket count".into());
    }
    Ok("WebTransport, WebSocket, two commits".into())
}
