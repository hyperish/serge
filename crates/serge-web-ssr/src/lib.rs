//! Native rendering and interchange contracts shared by the Serge builder and HTTP hosts.
mod manifest;
mod render;

pub use manifest::{
    SergeAsset, SergeBundleManifest, SergeRoute, VerifiedBundle, digest, media_type,
};
pub use render::{escape_html, render_element};
pub use serge_router::Pattern;

use bytes::Bytes;
use futures_core::Stream;
use hyve::{HyveResult, RequestId, Scope};
use std::{future::Future, pin::Pin};

/// A UI request after the host has applied its route policy.
pub struct SsrRequest {
    /// Matched concrete path without the query.
    pub path: String,
    /// Raw query string without a question mark.
    pub query: String,
    /// Correlation for the request and any work it starts.
    pub request_id: RequestId,
    /// Child of the route scope with the authorized principal and cancellation.
    pub scope: Scope,
}

/// Body chunks, produced under consumer backpressure.
pub type SsrBody = Pin<Box<dyn Stream<Item = HyveResult<Bytes>> + Send + 'static>>;
/// An HTML page or same-origin redirect.
pub enum SsrResponse {
    /// UTF-8 HTML.
    Html {
        /// 200 or 404.
        status: u16,
        /// Rendered bytes.
        body: SsrBody,
    },
    /// A root-relative redirect.
    Redirect {
        /// Same-origin destination.
        location: String,
        /// Use 308 for permanent redirects, 307 otherwise.
        permanent: bool,
    },
}
/// Asynchronous response headers, followed by the independently streamed body.
pub type SsrFuture = Pin<Box<dyn Future<Output = HyveResult<SsrResponse>> + Send + 'static>>;
/// The application-specific SSR endpoint that a host mounts with its bundle.
pub trait SergeSsrHandler: Send + Sync {
    /// Must equal the manifest's build ID.
    fn build_id(&self) -> &str;
    /// Renders a guarded request.
    fn render(&self, request: SsrRequest) -> SsrFuture;
}

/// One explicitly selected public model for the browser to restore. HTML is
/// never interpreted as script and the value is byte-checked on the client.
pub fn public_state<T: hyve::api::Wire>(
    build_id: &str,
    key: &str,
    value: &T,
) -> HyveResult<String> {
    use base64::Engine;
    let bytes = value.to_binary()?;
    if bytes.len() > 1024 * 1024 {
        return Err(hyve::HyveError::invalid_request(
            "public hydration state exceeds 1 MiB",
        ));
    }
    let data = base64::engine::general_purpose::STANDARD.encode(bytes.as_ref());
    Ok(format!(
        "<script type=\"application/octet-stream\" id=\"serge-state-{}\" data-build=\"{}\" data-schema=\"{:016x}\">{data}</script>",
        escape_html(key),
        escape_html(build_id),
        T::FINGERPRINT
    ))
}
