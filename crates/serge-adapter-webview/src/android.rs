//! An optional system WebView child in a NativeActivity.
use crate::{Delivery, EventId, Host, LocalDocument, document::CSP, invalid};
use android_activity::AndroidApp;
use hyve::{HyveResult, Ulid, Value};
use jni::{
    Env, JavaVM,
    errors::Result as JniResult,
    jni_sig, jni_str,
    objects::{JClass, JObject, JString, JValue},
    refs::Global,
};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

/// Child bounds in physical pixels relative to the native activity's content area.
#[derive(Debug, Copy, Clone)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
}
impl Rect {
    fn args(&self) -> HyveResult<[JValue<'static>; 4]> {
        if self.width < 0 || self.height < 0 {
            return Err(invalid("negative webview size"));
        }
        Ok([self.x, self.y, self.width, self.height].map(JValue::Int))
    }
}

struct View {
    vm: JavaVM,
    object: Global<JObject<'static>>,
}
impl View {
    fn attach(app: &AndroidApp, doc: LocalDocument, bounds: Rect) -> HyveResult<Self> {
        let args = bounds.args()?;
        let salt = Ulid::new().to_string();
        let origin = format!(
            "https://slot-{}.invalid",
            Ulid::new().to_string().to_ascii_lowercase()
        );
        let bootstrap = include_str!("document.js")
            .replace("__SERGE_SLOT_KEY__", &salt)
            .replace(
                "window.ipc.postMessage",
                "window.__sergeAndroid.postMessage",
            );
        // SAFETY: android-activity owns the VM and its global Activity reference for this call.
        #[allow(unsafe_code)]
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        let object = vm.attach_current_thread(|env| {
            let raw = app.activity_as_ptr().cast();
            #[allow(unsafe_code)]
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            let loader = env.call_method(&activity, jni_str!("getClassLoader"), jni_sig!("()Ljava/lang/ClassLoader;"), &[])?.l()?;
            let name = env.new_string("org.hyperish.serge.webview.LocalWebView")?;
            let resolved = env.call_method(&loader, jni_str!("loadClass"), jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"), &[JValue::Object(&name)])?.l()?;
            let class = env.cast_local::<JClass>(resolved)?;
            let origin = env.new_string(origin)?;
            let bootstrap = env.new_string(bootstrap)?;
            let prefix = env.new_string(format!("{salt}/"))?;
            let csp = env.new_string(CSP)?;
            let view = env.new_object(class,
                jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V"),
                &[JValue::Object(&activity), JValue::Object(&origin), JValue::Object(&bootstrap), JValue::Object(&prefix), JValue::Object(&csp)])?;
            env.new_global_ref(view)
        }).map_err(|e| invalid(&e.to_string()))?;
        let view = Self { vm, object };
        view.asset("/index.html", "text/html", &doc.document)?;
        for (path, (mime, bytes)) in doc.assets {
            view.asset(&path, &mime, &bytes)?;
        }
        view.access(|env, obj| {
            env.call_method(obj, jni_str!("attach"), jni_sig!("(IIII)V"), &args)?;
            Ok(())
        })?;
        Ok(view)
    }
    fn access<T>(&self, action: impl FnOnce(&mut Env, &JObject) -> JniResult<T>) -> HyveResult<T> {
        self.vm
            .attach_current_thread(|env| action(env, &self.object))
            .map_err(|e| invalid(&e.to_string()))
    }
    fn asset(&self, path: &str, mime: &str, bytes: &[u8]) -> HyveResult<()> {
        self.access(|env, obj| {
            let path = env.new_string(path)?;
            let mime = env.new_string(mime.split(';').next().unwrap_or(mime))?;
            let bytes = env.byte_array_from_slice(bytes)?;
            env.call_method(
                obj,
                jni_str!("asset"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;[B)V"),
                &[
                    JValue::Object(&path),
                    JValue::Object(&mime),
                    JValue::Object(&bytes),
                ],
            )?;
            Ok(())
        })
    }
    fn evaluate(&self, script: &str) -> HyveResult<()> {
        self.access(|env, obj| {
            let js = env.new_string(script)?;
            env.call_method(
                obj,
                jni_str!("evaluate"),
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(&js)],
            )?;
            Ok(())
        })
    }
    fn next(&self) -> HyveResult<String> {
        self.access(|env, obj| {
            let event = env
                .call_method(obj, jni_str!("next"), jni_sig!("()Ljava/lang/String;"), &[])?
                .l()?;
            Ok(env.cast_local::<JString>(event)?.to_string())
        })
    }
    fn close(&self) {
        let _ = self.access(|env, obj| {
            env.call_method(obj, jni_str!("close"), jni_sig!("()V"), &[])?;
            Ok(())
        });
    }
}

type Guard = Box<dyn Fn(&Value) -> HyveResult<()> + Send + Sync>;
type Handler = Box<dyn Fn(EventId, Value, Slot) + Send + Sync>;
struct Inner {
    host: Mutex<Host>,
    view: Arc<View>,
    guard: Guard,
    handler: Handler,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.view.close();
    }
}

/// An Android WebView owned by its native host. Handlers run on the slot's worker thread.
/// Dropping the last slot closes its message listener and removes the child on the UI thread.
#[derive(Clone)]
pub struct Slot(Arc<Inner>);
impl Slot {
    /// Mounts one verified bundle in a child view. Enable `webview = true` in Android Cargo metadata.
    pub fn attach(
        app: &AndroidApp,
        bounds: Rect,
        doc: LocalDocument,
        fingerprint: u64,
        state: Value,
        authorize: impl Fn(&Value) -> HyveResult<()> + Send + Sync + 'static,
        handle: impl Fn(EventId, Value, Slot) + Send + Sync + 'static,
    ) -> HyveResult<Self> {
        let host = Mutex::new(Host::new(doc.build, fingerprint, state)?);
        let view = Arc::new(View::attach(app, doc, bounds)?);
        let inner = Arc::new(Inner {
            host,
            view: view.clone(),
            guard: Box::new(authorize),
            handler: Box::new(handle),
        });
        let owner = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("serge-webview".into())
            .spawn(move || run(view, owner))
            .map_err(|e| invalid(&e.to_string()))?;
        Ok(Self(inner))
    }

    fn host(&self) -> MutexGuard<'_, Host> {
        self.0.host.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn deliver(&self, delivery: Delivery) -> HyveResult<()> {
        match delivery {
            Delivery::None => Ok(()),
            Delivery::Execute(id, payload) => {
                (self.0.handler)(id, payload, self.clone());
                Ok(())
            }
            Delivery::Reopen => self.reload(),
            Delivery::Disconnected => self.0.view.evaluate("window.__sergeWebview.disconnect()"),
            Delivery::Send(message) => {
                let literal = serde_json::to_string(&message).expect("JSON string");
                self.0
                    .view
                    .evaluate(&format!("window.__sergeWebview.receive({literal})"))
            }
        }
    }

    /// Updates the size and position of the child on the UI thread.
    pub fn set_bounds(&self, bounds: Rect) -> HyveResult<()> {
        let args = bounds.args()?;
        self.0.view.access(|env, obj| {
            env.call_method(obj, jni_str!("resize"), jni_sig!("(IIII)V"), &args)?;
            Ok(())
        })
    }
    /// Publishes state, coalescing any unacknowledged change.
    pub fn update(&self, state: Value) -> HyveResult<()> {
        let d = self.host().update(state)?;
        self.deliver(d)
    }
    /// Records the outcome for this event even if the document is reloading.
    pub fn finish(&self, event: EventId, result: HyveResult<Value>) -> HyveResult<()> {
        let d = self.host().finish(event, result)?;
        self.deliver(d)
    }
    /// Reopens the pinned document in the same session.
    pub fn reload(&self) -> HyveResult<()> {
        self.host().suspend();
        self.0.view.access(|env, obj| {
            env.call_method(obj, jni_str!("reload"), jni_sig!("()V"), &[])?;
            Ok(())
        })
    }
    fn event(&self, event: &str) -> HyveResult<()> {
        let delivery = match event {
            "loading" => {
                self.host().suspend();
                return Ok(());
            }
            "open" => Delivery::Send(self.host().open()?),
            "message:reload" => return self.reload(),
            "message:tick" => self.host().tick()?,
            _ => {
                let Some(data) = event.strip_prefix("message:") else {
                    return Ok(());
                };
                self.host().receive(data, |p| (self.0.guard)(p))?
            }
        };
        self.deliver(delivery)
    }
}
fn run(view: Arc<View>, owner: Weak<Inner>) {
    while let Ok(event) = view.next() {
        let Some(inner) = owner.upgrade() else {
            break;
        };
        if event == "closed" {
            inner
                .host
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .suspend();
            break;
        }
        let _ = Slot(inner).event(&event);
    }
}
