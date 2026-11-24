//! `serge-adapter-client` — the client side of `#[server]` calls and of
//! engine-to-UI streaming:
//!
//! - [`RemoteTransport`] sends calls over HTTP with the shared translation
//!   (`hyve::api::http`): the fast (binary) rail first, falling back once to
//!   the standard (JSON) rail on `415`; errors come back as the same
//!   `HyveError`s the server returned; when the calling view unmounts (or
//!   the call's token is cancelled) the request is aborted. Native targets
//!   use `hyper` on serge's `tokio` runtime, the web uses `fetch` with an
//!   `AbortController`.
//! - [`SharedRingBuf`] is a lock-free ring of POD samples (`hyve`'s) an
//!   engine thread pushes into and a `Canvas` paint reads; reading it in
//!   `paint` repaints on new data.

mod ring;

use std::future::{Future, poll_fn};
use std::pin::pin;
use std::sync::Arc;
use std::task::Poll;

use hyve::api::http::{HttpAnswer, HttpRequest, decode, encode, wants_standard_rail};
use hyve::api::{BoxFuture, Call, Rail, Reply, Transport, errors};
use hyve::{CancellationToken, HyveResult};

pub use ring::SharedRingBuf;

/// `#[server]` calls over HTTP to a server whose functions are mounted at
/// `base` (`http://127.0.0.1:3000/v1`; on the web also a same-origin path
/// such as `/v1`).
#[derive(Clone)]
pub struct RemoteTransport {
    origin: String,
    prefix: String,
    rail: Rail,
    headers: Vec<(String, String)>,
    fingerprint: Option<u64>,
    #[cfg(not(target_arch = "wasm32"))]
    client: hyper_util::client::legacy::Client<
        hyper_util::client::legacy::connect::HttpConnector,
        http_body_util::Full<bytes::Bytes>,
    >,
}

impl std::fmt::Debug for RemoteTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteTransport")
            .field("origin", &self.origin)
            .field("prefix", &self.prefix)
            .field("rail", &self.rail)
            .finish_non_exhaustive()
    }
}

/// Splits `http://host:port/prefix` into origin and path prefix.
fn split_base(base: &str) -> (String, String) {
    let base = base.trim_end_matches('/');
    match base.find("://") {
        Some(i) => match base[i + 3..].find('/') {
            Some(j) => (base[..i + 3 + j].to_owned(), base[i + 3 + j..].to_owned()),
            None => (base.to_owned(), String::new()),
        },
        None => (String::new(), base.to_owned()),
    }
}

impl RemoteTransport {
    /// Calls functions mounted at `base`.
    ///
    /// # Panics
    ///
    /// On native targets, panics unless `base` is an absolute `http://` URL
    /// (TLS is not in the Phase 1 prototype).
    pub fn new(base: &str) -> RemoteTransport {
        let (origin, prefix) = split_base(base);
        #[cfg(not(target_arch = "wasm32"))]
        assert!(
            origin.starts_with("http://"),
            "serge: RemoteTransport needs an absolute http:// URL on native targets, got {base:?}"
        );
        RemoteTransport {
            origin,
            prefix,
            rail: Rail::Fast,
            headers: Vec::new(),
            fingerprint: None,
            #[cfg(not(target_arch = "wasm32"))]
            client: hyper_util::client::legacy::Client::builder(
                hyper_util::rt::TokioExecutor::new(),
            )
            .build_http(),
        }
    }

    /// Sends every call on `rail` (default: fast, falling back on `415`).
    pub fn rail(mut self, rail: Rail) -> Self {
        self.rail = rail;
        self
    }

    /// Sends `Authorization: Bearer <token>` (native clients' sessions).
    pub fn bearer(self, token: &str) -> Self {
        self.header("authorization", &format!("Bearer {token}"))
    }

    /// Sends the cookie session's CSRF token (`x-csrf-token`).
    pub fn csrf_token(self, token: &str) -> Self {
        self.header("x-csrf-token", token)
    }

    /// Adds a header to every call.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .push((name.to_ascii_lowercase(), value.to_owned()));
        self
    }

    /// Claims `fingerprint` on fast-rail calls instead of the compiled one
    /// (tests of the schema-mismatch fallback).
    #[doc(hidden)]
    pub fn claim_fingerprint(mut self, fingerprint: u64) -> Self {
        self.fingerprint = Some(fingerprint);
        self
    }

    /// As a transport to provide (`SergeApp::transport(..)`).
    pub fn into_arc(self) -> Arc<dyn Transport> {
        Arc::new(self)
    }

    async fn run(&self, call: Call) -> HyveResult<Reply> {
        let pattern = format!("{}{}", self.prefix, call.descriptor().path);
        let cancel = call.cancellation().clone();
        if self.rail == Rail::Fast {
            let answer = self
                .send(
                    encode(&call, Rail::Fast, &pattern, self.fingerprint)?,
                    &cancel,
                )
                .await?;
            if !wants_standard_rail(answer.status) {
                return decode(answer.view());
            }
        }
        let answer = self
            .send(encode(&call, Rail::Standard, &pattern, None)?, &cancel)
            .await?;
        decode(answer.view())
    }
}

/// A response, owned.
struct Answer {
    status: u16,
    content_type: Option<String>,
    retry_after: bool,
    body: Vec<u8>,
}

impl Answer {
    fn view(&self) -> HttpAnswer<'_> {
        HttpAnswer {
            status: self.status,
            content_type: self.content_type.as_deref(),
            retry_after: self.retry_after,
            body: &self.body,
        }
    }
}

/// `f`, or `None` once `cancel` fires (dropping `f` aborts its request).
async fn or_cancelled<F: Future>(f: F, cancel: &CancellationToken) -> Option<F::Output> {
    let mut f = pin!(f);
    let mut c = pin!(cancel.cancelled());
    poll_fn(|cx| {
        if let Poll::Ready(v) = f.as_mut().poll(cx) {
            return Poll::Ready(Some(v));
        }
        if c.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

impl Transport for RemoteTransport {
    fn call(&self, call: Call) -> BoxFuture<'_, HyveResult<Reply>> {
        Box::pin(self.run(call))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl RemoteTransport {
    async fn send(&self, req: HttpRequest, cancel: &CancellationToken) -> HyveResult<Answer> {
        use http_body_util::BodyExt;

        let mut b = http::Request::builder()
            .method(req.method.as_str())
            .uri(format!("{}{}", self.origin, req.path));
        for (k, v) in &req.headers {
            b = b.header(*k, v);
        }
        for (k, v) in &self.headers {
            b = b.header(k.as_str(), v);
        }
        let request = b
            .body(http_body_util::Full::new(bytes::Bytes::from(req.body)))
            .map_err(|e| errors::transport_unavailable(format!("invalid request: {e}")))?;
        let client = self.client.clone();
        // The I/O runs on serge's runtime; the guard aborts it when this
        // future is dropped (the view unmounted) or cancelled.
        let task = serge_core::runtime().spawn(async move {
            let r = client.request(request).await.map_err(|e| e.to_string())?;
            let status = r.status().as_u16();
            let content_type = r
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let retry_after = r.headers().contains_key(http::header::RETRY_AFTER);
            let body = r
                .into_body()
                .collect()
                .await
                .map_err(|e| e.to_string())?
                .to_bytes()
                .to_vec();
            Ok::<_, String>(Answer {
                status,
                content_type,
                retry_after,
                body,
            })
        });
        struct Abort(tokio::task::AbortHandle);
        impl Drop for Abort {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _guard = Abort(task.abort_handle());
        match or_cancelled(task, cancel).await {
            None => Err(errors::cancelled("the caller cancelled the call")),
            Some(Ok(Ok(a))) => Ok(a),
            Some(Ok(Err(e))) => Err(errors::transport_unavailable(format!(
                "{}: {e}",
                self.origin
            ))),
            Some(Err(e)) => Err(errors::transport_unavailable(e.to_string())),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl RemoteTransport {
    async fn send(&self, req: HttpRequest, cancel: &CancellationToken) -> HyveResult<Answer> {
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;

        let fail = |e: wasm_bindgen::JsValue| errors::transport_unavailable(format!("{e:?}"));
        let ctrl = web_sys::AbortController::new().map_err(fail)?;
        struct Abort(web_sys::AbortController, bool);
        impl Drop for Abort {
            fn drop(&mut self) {
                if !self.1 {
                    self.0.abort();
                }
            }
        }
        let mut guard = Abort(ctrl, false);
        let init = web_sys::RequestInit::new();
        init.set_method(req.method.as_str());
        let headers = web_sys::Headers::new().map_err(fail)?;
        for (k, v) in &req.headers {
            headers.set(k, v).map_err(fail)?;
        }
        for (k, v) in &self.headers {
            headers.set(k, v).map_err(fail)?;
        }
        init.set_headers(&headers);
        if !req.body.is_empty() {
            init.set_body(&js_sys::Uint8Array::from(&req.body[..]).into());
        }
        init.set_signal(Some(&guard.0.signal()));
        // The session cookie goes along (also to an API on another origin
        // that allows credentials).
        init.set_credentials(web_sys::RequestCredentials::Include);
        let url = format!("{}{}", self.origin, req.path);
        let request = web_sys::Request::new_with_str_and_init(&url, &init).map_err(fail)?;
        let window = web_sys::window().ok_or_else(|| errors::transport_unavailable("no window"))?;
        let exchange = async {
            let resp: web_sys::Response = JsFuture::from(window.fetch_with_request(&request))
                .await?
                .dyn_into()?;
            let buf = JsFuture::from(resp.array_buffer()?).await?;
            Ok::<_, wasm_bindgen::JsValue>(Answer {
                status: resp.status(),
                content_type: resp.headers().get("content-type").ok().flatten(),
                retry_after: resp.headers().has("retry-after").unwrap_or(false),
                body: js_sys::Uint8Array::new(&buf).to_vec(),
            })
        };
        match or_cancelled(exchange, cancel).await {
            None => Err(errors::cancelled("the caller cancelled the call")),
            Some(r) => {
                guard.1 = true;
                r.map_err(fail)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_base;

    #[test]
    fn bases_split_into_origin_and_prefix() {
        assert_eq!(
            split_base("http://127.0.0.1:3000/v1/"),
            ("http://127.0.0.1:3000".into(), "/v1".into())
        );
        assert_eq!(
            split_base("http://h:1"),
            ("http://h:1".into(), String::new())
        );
        assert_eq!(split_base("/v1"), (String::new(), "/v1".into()));
        assert_eq!(split_base(""), (String::new(), String::new()));
    }
}
