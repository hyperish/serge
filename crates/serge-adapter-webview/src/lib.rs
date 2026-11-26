//! A host-owned slot for a verified local page. The host owns the session,
//! state and event results. A new document always negotiates before submitting.
mod host;
mod wire;
pub use host::{Delivery, Host};
pub use wire::{EventId, Frame, Message, Outcome, Rail, Receipt, fingerprint};

#[cfg(all(
    feature = "native-host",
    any(
        target_os = "android",
        target_os = "ios",
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
    )
))]
mod document;
#[cfg(all(
    feature = "native-host",
    any(
        target_os = "android",
        target_os = "ios",
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
    )
))]
pub use document::LocalDocument;
#[cfg(all(feature = "native-host", target_os = "android"))]
mod android;
#[cfg(all(feature = "native-host", target_os = "android"))]
pub use android::{Rect, Slot};
#[cfg(all(
    feature = "native-host",
    any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux"
    )
))]
mod native;
#[cfg(all(
    feature = "native-host",
    any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux"
    )
))]
pub use native::{Rect, Slot, dpi};

pub(crate) fn invalid(message: &str) -> hyve::HyveError {
    hyve::HyveError::invalid_request(message)
}

pub(crate) fn object<const N: usize>(fields: [(&str, hyve::Value); N]) -> hyve::Value {
    hyve::Value::Map(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

/// JS-callable archive codec for a local WASM document. Connect the exported
/// encode and decode functions as the document's codec with its own fingerprint.
#[cfg(target_arch = "wasm32")]
pub mod browser {
    use crate::{Frame, Rail, wire::MAX_FRAME};
    use hyve::serde;
    use wasm_bindgen::prelude::*;

    fn rail(fp: &str) -> Result<Rail, JsValue> {
        let n =
            u64::from_str_radix(fp, 16).map_err(|_| JsValue::from_str("invalid fingerprint"))?;
        if format!("{n:016x}") != fp {
            return Err(JsValue::from_str("invalid fingerprint"));
        }
        Ok(Rail::Archive(n))
    }
    /// Encodes a canonical document message with this build's schema identity.
    #[wasm_bindgen(js_name = sergeWebviewEncode)]
    pub fn encode(json: &str, fp: &str) -> Result<String, JsValue> {
        if json.len() > MAX_FRAME {
            return Err(JsValue::from_str("webview JSON exceeds 64 KiB"));
        }
        let v = serde::from_json(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
        rail(fp)?
            .encode(&Frame::parse(&v).map_err(|e| JsValue::from_str(&e.to_string()))?)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
    /// Validates and decodes a host archive into a canonical lossless JSON string.
    #[wasm_bindgen(js_name = sergeWebviewDecode)]
    pub fn decode(data: &str, fp: &str) -> Result<String, JsValue> {
        let frame = rail(fp)?
            .decode(data)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(serde::to_json(&frame.json()))
    }
}

#[cfg(all(feature = "native-host", target_os = "linux"))]
pub use native::pump_gtk;
