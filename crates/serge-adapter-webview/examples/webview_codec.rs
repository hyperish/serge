//! Minimal client bundle for a document that negotiates the archived rail.
#[cfg(target_arch = "wasm32")]
pub use serge_adapter_webview::browser::{decode, encode};
