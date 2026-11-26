//! Mounts the verified page on a single child WebView. Each slot receives a
//! unique origin. The webview has no general file handler or external navigation.
use crate::{Delivery, EventId, Host, LocalDocument, invalid, wire::MAX_FRAME};
use hyve::{HyveResult, Ulid, Value};
use std::{
    borrow::Cow,
    cell::RefCell,
    rc::{Rc, Weak},
};
use wry::{
    NewWindowResponse, PageLoadEvent, WebView, WebViewBuilder,
    http::{Request, Response},
    raw_window_handle::HasWindowHandle,
};
pub use wry::{Rect, dpi};

#[cfg(target_os = "linux")]
thread_local! { static LINUX_SLOTS:std::cell::Cell<usize>=const{std::cell::Cell::new(0)}; }

/// Advances the GTK event source of live X11 child webviews. The Serge native
/// event loop calls this before waiting. Embedders should pump on the GTK thread.
#[cfg(target_os = "linux")]
pub fn pump_gtk() -> bool {
    if LINUX_SLOTS.with(|n| n.get()) == 0 {
        return false;
    }
    for _ in 0..32 {
        if !gtk::events_pending() {
            break;
        }
        gtk::main_iteration_do(false);
    }
    true
}

impl LocalDocument {
    fn serve(&self, req: &Request<Vec<u8>>, authority: &str) -> Response<Cow<'static, [u8]>> {
        let body = if req.method() == "GET"
            && req
                .uri()
                .authority()
                .is_some_and(|a| a.as_str() == authority)
            && req.uri().query().is_none()
        {
            match req.uri().path() {
                "/index.html" => Some(("text/html; charset=utf-8", self.document.as_slice())),
                path => self
                    .assets
                    .get(path)
                    .map(|(kind, bytes)| (kind.as_str(), bytes.as_slice())),
            }
        } else {
            None
        };
        let (status, kind, bytes) = match body {
            Some((kind, bytes)) => (200, kind, bytes),
            None => (404, "text/plain", &[][..]),
        };
        Response::builder()
            .status(status)
            .header("Content-Type", kind)
            .header("Content-Security-Policy", crate::document::CSP)
            .header("X-Content-Type-Options", "nosniff")
            .header("Cache-Control", "no-store")
            .body(Cow::Owned(bytes.to_vec()))
            .expect("static response headers")
    }
}
type Guard = Box<dyn Fn(&Value) -> HyveResult<()>>;
type Handler = Box<dyn Fn(EventId, Value, Slot)>;
struct Inner {
    #[cfg(target_os = "linux")]
    counted: std::cell::Cell<bool>,
    host: RefCell<Host>,
    view: RefCell<Weak<WebView>>,
    guard: Guard,
    handler: Handler,
}

impl Inner {
    fn deliver(self: &Rc<Self>, delivery: Delivery) -> HyveResult<()> {
        let Some(view) = self.view.borrow().upgrade() else {
            return Ok(());
        };
        match delivery {
            Delivery::None => (),
            Delivery::Reopen => view.reload().map_err(|e| invalid(&e.to_string()))?,
            Delivery::Disconnected => view
                .evaluate_script("window.__sergeWebview.disconnect()")
                .map_err(|e| invalid(&e.to_string()))?,
            Delivery::Execute(id, payload) => (self.handler)(
                id,
                payload,
                Slot {
                    inner: self.clone(),
                    view,
                },
            ),
            Delivery::Send(s) => evaluate(&view, &s)?,
        }
        Ok(())
    }
}
fn evaluate(view: &WebView, data: &str) -> HyveResult<()> {
    let literal = serde_json::to_string(data).expect("JSON string");
    view.evaluate_script(&format!("window.__sergeWebview.receive({literal})"))
        .map_err(|e| invalid(&e.to_string()))
}
/// A child webview bound to this native window and its current principal.
/// Retain the slot for the lifetime of its view. Drop it on a principal change.
#[derive(Clone)]
pub struct Slot {
    inner: Rc<Inner>,
    view: Rc<WebView>,
}
impl Slot {
    /// Opens an isolated slot. Call finish exactly once for a completed event. The handler
    /// may hold its slot while awaiting a local operation. Bounds are in native window coordinates.
    pub fn attach(
        window: &impl HasWindowHandle,
        bounds: wry::Rect,
        doc: LocalDocument,
        fingerprint: u64,
        state: Value,
        authorize: impl Fn(&Value) -> HyveResult<()> + 'static,
        handle: impl Fn(EventId, Value, Slot) + 'static,
    ) -> HyveResult<Self> {
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::*;
            use wry::raw_window_handle::RawWindowHandle;
            if !matches!(
                window
                    .window_handle()
                    .map_err(|e| invalid(&e.to_string()))?
                    .as_raw(),
                RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)
            ) {
                return Err(invalid("the Linux webview requires an X11 parent"));
            }
            gtk::init().map_err(|e| invalid(&e.to_string()))?;
            if gtk::gdk::Display::default().is_none_or(|d| d.type_().name() != "GdkX11Display") {
                return Err(invalid("the Linux webview requires a GDK X11 display"));
            }
        }
        let origin = format!("slot-{}", Ulid::new().to_string().to_ascii_lowercase());
        let logical = format!("sergeview://{origin}/index.html");
        #[cfg(target_os = "windows")]
        let url = format!("http://sergeview.{origin}/index.html");
        #[cfg(not(target_os = "windows"))]
        let url = logical.clone();
        let host = Host::new(doc.build, fingerprint, state)?;
        let inner = Rc::new(Inner {
            #[cfg(target_os = "linux")]
            counted: std::cell::Cell::new(false),
            host: RefCell::new(host),
            guard: Box::new(authorize),
            handler: Box::new(handle),
            view: RefCell::new(Weak::new()),
        });
        let ipc = inner.clone();
        let loading = inner.clone();
        let nav = inner.clone();
        let page = url.clone();
        let location = url.clone();
        let expected = url.clone();
        let gate_url = logical.clone();
        let salt = Ulid::new().to_string();
        let prefix = format!("{salt}/");
        let bootstrap = include_str!("document.js").replace("__SERGE_SLOT_KEY__", &salt);
        let builder = WebViewBuilder::new()
            .with_url(logical)
            .with_bounds(bounds)
            .with_devtools(false)
            .with_clipboard(false)
            .with_initialization_script_for_main_only(bootstrap, true)
            .with_custom_protocol("sergeview".into(), move |_, req| doc.serve(&req, &origin))
            .with_navigation_handler(move |target| {
                let allowed = target == location || target == gate_url;
                if allowed {
                    nav.host.borrow_mut().suspend()
                }
                allowed
            })
            .with_permission_handler(|_| wry::PermissionResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
            .with_on_page_load_handler(move |event, target| {
                if target != expected {
                    return;
                }
                match event {
                    PageLoadEvent::Started => loading.host.borrow_mut().suspend(),
                    PageLoadEvent::Finished => {
                        let hello = loading.host.borrow_mut().open();
                        if let (Ok(s), Some(view)) = (hello, loading.view.borrow().upgrade()) {
                            let _ = evaluate(&view, &s);
                        }
                    }
                }
            })
            .with_ipc_handler(move |req| {
                if req.uri().to_string() != page
                    || req.body().len() > MAX_FRAME.div_ceil(3) * 4 + prefix.len()
                {
                    return;
                }
                let Some(data) = req.body().strip_prefix(&prefix) else {
                    return;
                };
                if data == "reload" {
                    if let Some(view) = ipc.view.borrow().upgrade() {
                        ipc.host.borrow_mut().suspend();
                        let _ = view.load_url(&page);
                    }
                    return;
                }
                if data == "tick" {
                    let retry = ipc.host.borrow_mut().tick();
                    if let Ok(d) = retry {
                        let _ = ipc.deliver(d);
                    }
                    return;
                }
                let result = ipc.host.borrow_mut().receive(data, |p| (ipc.guard)(p));
                if let Ok(delivery) = result {
                    let _ = ipc.deliver(delivery);
                }
            });
        #[cfg(target_os = "ios")]
        let view = builder.build(window);
        #[cfg(not(target_os = "ios"))]
        let view = builder.build_as_child(window);
        let view = view.map_err(|e| invalid(&e.to_string()))?;
        #[cfg(target_os = "ios")]
        set_uikit_bounds(&view, bounds);
        let view = Rc::new(view);
        inner.view.replace(Rc::downgrade(&view));
        #[cfg(target_os = "linux")]
        {
            LINUX_SLOTS.with(|n| n.set(n.get() + 1));
            inner.counted.set(true);
        }
        Ok(Self { inner, view })
    }
    /// Update child bounds after layout.
    pub fn set_bounds(&self, bounds: wry::Rect) -> HyveResult<()> {
        #[cfg(target_os = "ios")]
        {
            set_uikit_bounds(&self.view, bounds);
            Ok(())
        }
        #[cfg(not(target_os = "ios"))]
        self.view
            .set_bounds(bounds)
            .map_err(|e| invalid(&e.to_string()))
    }
    /// Publish current state, coalescing unacknowledged changes.
    pub fn update(&self, state: Value) -> HyveResult<()> {
        let d = self.inner.host.borrow_mut().update(state)?;
        self.inner.deliver(d)
    }
    /// Completes an event, including when the page has been reopened.
    pub fn finish(&self, event: EventId, result: HyveResult<Value>) -> HyveResult<()> {
        let d = self.inner.host.borrow_mut().finish(event, result)?;
        self.inner.deliver(d)
    }
    /// Reload the local page. Session and last event outcome survive the new generation.
    pub fn reload(&self) -> HyveResult<()> {
        self.inner.host.borrow_mut().suspend();
        self.view.reload().map_err(|e| invalid(&e.to_string()))
    }
}

// Wry attaches its WKWebView to the provided UIView, but its set_bounds only resizes
// macOS child views. UIKit positions the slot in its parent's logical coordinate space.
#[cfg(target_os = "ios")]
fn set_uikit_bounds(view: &WebView, bounds: Rect) {
    use objc2::rc::Retained;
    use objc2_ui_kit::{
        UIScrollView, UIScrollViewContentInsetAdjustmentBehavior, UIViewAutoresizing,
    };
    use wry::WebViewExtIOS;

    let native = view.webview();
    // The embedding layout supplies the slot's complete content rectangle. UIKit's
    // automatic safe-area inset would subtract the status bar from it a second time.
    // SAFETY: WKWebView.scrollView is a public, nonnull UIScrollView getter on the UI thread.
    #[allow(unsafe_code)]
    let scroll: Retained<UIScrollView> = unsafe { objc2::msg_send![&*native, scrollView] };
    scroll.setContentInsetAdjustmentBehavior(UIScrollViewContentInsetAdjustmentBehavior::Never);
    let scale = native.contentScaleFactor();
    let position = bounds.position.to_logical::<f64>(scale);
    let size = bounds.size.to_logical::<f64>(scale);
    let mut frame = native.frame();
    frame.origin.x = position.x;
    frame.origin.y = position.y;
    frame.size.width = size.width;
    frame.size.height = size.height;
    native.setAutoresizingMask(UIViewAutoresizing::None);
    native.setFrame(frame);
}

#[cfg(target_os = "linux")]
impl Drop for Inner {
    fn drop(&mut self) {
        if self.counted.get() {
            LINUX_SLOTS.with(|n| n.set(n.get() - 1));
        }
    }
}
