//! View scopes: a `hyve` [`Scope`] (provided values, cancellation) paired with
//! a reactive [`Owner`] (signals, effects). Every mounted component and
//! reactive region has one; unmounting disposes both, which cancels the
//! `#[server]` calls and tasks started under it.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use hyve::state::{Owner, on_cleanup};
use hyve::{Ctx, Scope};

/// A view scope. Cheap to clone.
#[derive(Clone, Debug)]
pub struct View {
    scope: Scope,
    owner: Owner,
}

thread_local! {
    static CURRENT: RefCell<Vec<View>> = const { RefCell::new(Vec::new()) };
    static EXECUTOR: RefCell<Option<Rc<dyn Executor>>> = const { RefCell::new(None) };
}

impl View {
    /// A root view over `scope` (owned by the current reactive owner).
    pub fn root(scope: Scope) -> View {
        let owner = Owner::new();
        let s = scope.clone();
        owner.with(|| on_cleanup(move || s.dispose()));
        View { scope, owner }
    }

    /// A child view whose owner is a child of the *current* reactive owner
    /// (inside an effect: disposed before the effect reruns) and whose scope is
    /// a child of this view's scope.
    pub fn child(&self) -> View {
        self.child_in(&self.scope)
    }

    /// Like [`child`](Self::child) with a scope under `parent` instead (a
    /// screen under its feature's scope).
    pub fn child_in(&self, parent: &Scope) -> View {
        let scope = parent.child();
        let owner = Owner::new();
        let s = scope.clone();
        owner.with(|| on_cleanup(move || s.dispose()));
        View { scope, owner }
    }

    /// Runs `f` inside this view: its scope is current (for `Ctx<T>` and
    /// `#[server]` stubs) and its owner owns what `f` creates.
    pub fn run<R>(&self, f: impl FnOnce() -> R) -> R {
        let _g = Push::new(self.clone());
        self.scope.enter(|| self.owner.with(f))
    }

    /// Runs `f` with this view's scope current but the reactive owner
    /// unchanged (effect bodies, which own their own nodes).
    pub fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _g = Push::new(self.clone());
        self.scope.enter(f)
    }

    /// The innermost view being run on this thread.
    pub fn current() -> Option<View> {
        CURRENT.with(|c| c.borrow().last().cloned())
    }

    /// The view's `hyve` scope.
    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    /// The view's reactive owner.
    pub fn owner(&self) -> Owner {
        self.owner
    }

    /// Disposes the owner (and with it the scope): effects stop, cleanups run,
    /// the scope's cancellation token fires.
    pub fn dispose(&self) {
        self.owner.dispose();
    }

    /// Whether the view is still mounted.
    pub fn is_alive(&self) -> bool {
        self.owner.is_alive() && !self.scope.is_disposed()
    }
}

struct Push;

impl Push {
    fn new(v: View) -> Push {
        CURRENT.with(|c| c.borrow_mut().push(v));
        Push
    }
}

impl Drop for Push {
    fn drop(&mut self) {
        CURRENT.with(|c| {
            c.borrow_mut().pop();
        });
    }
}

/// The value of type `T` provided to the current view (by `.provide()` on
/// the app or a feature).
///
/// # Panics
///
/// Panics with the type's name when nothing provides it — a component that
/// takes `Ctx<T>` is mounted where `T` was not provided.
pub fn inject<T: Send + Sync + 'static>() -> Ctx<T> {
    let scope = Scope::current().expect("serge: components run inside a view scope");
    scope.get::<T>().unwrap_or_else(|| {
        panic!(
            "serge: no `{}` is provided here; provide it with `.provide(..)` on the app or a feature that encloses this view",
            std::any::type_name::<T>()
        )
    })
}

/// A boxed local future (UI tasks touch thread-local signals, so they are
/// not `Send`).
pub type LocalFuture = Pin<Box<dyn Future<Output = ()>>>;

/// Runs UI tasks on the UI thread. Each target installs one
/// ([`install_executor`]).
pub trait Executor {
    /// Starts `task`; the returned handle aborts it.
    fn spawn(&self, task: LocalFuture) -> TaskHandle;
}

/// Stops a spawned task (drops its future).
pub struct TaskHandle(Box<dyn FnOnce()>);

impl TaskHandle {
    /// A handle that runs `abort` when aborted.
    pub fn new(abort: impl FnOnce() + 'static) -> Self {
        TaskHandle(Box::new(abort))
    }

    /// Aborts the task.
    pub fn abort(self) {
        (self.0)()
    }
}

/// Installs the UI-thread executor (the target does this at startup).
pub fn install_executor(executor: Rc<dyn Executor>) {
    EXECUTOR.with(|e| *e.borrow_mut() = Some(executor));
}

/// Runs `future` as a task of the current view: its scope stays current
/// across `.await`s (so `#[server]` calls find the transport and its
/// cancellation), and the task is dropped when the view unmounts. This is
/// what `event => async move { .. }` handlers use.
///
/// # Panics
///
/// Panics when no executor is installed (outside a running app or test
/// harness).
pub fn spawn(future: impl Future<Output = ()> + 'static) {
    let view = View::current();
    let task: LocalFuture = match &view {
        Some(v) => Box::pin(v.scope().clone().scoped(future)),
        None => Box::pin(future),
    };
    let handle = EXECUTOR.with(|e| {
        e.borrow()
            .clone()
            .expect("serge: no executor is installed; run the app (or a test harness) first")
            .spawn(task)
    });
    if let Some(v) = view {
        let handle = RefCell::new(Some(handle));
        v.owner().with(|| {
            on_cleanup(move || {
                if let Some(h) = handle.borrow_mut().take() {
                    h.abort();
                }
            })
        });
    }
}

/// The process-wide `tokio` runtime of native targets: the UI executor polls
/// tasks inside its context (so in-process `#[server]` handlers can use I/O
/// and timers), and remote transports run their I/O on it.
#[cfg(not(target_arch = "wasm32"))]
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("serge-io")
            .enable_all()
            .build()
            .expect("serge: failed to start the tokio runtime")
    })
}

/// Waits `duration` without blocking the UI thread (a timer on serge's
/// runtime natively, `setTimeout` on the web). For UI tasks: polling,
/// animations, demo producers.
pub async fn sleep(duration: std::time::Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let timer = {
            let _rt = runtime().enter();
            tokio::time::sleep(duration)
        };
        timer.await;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let ms = i32::try_from(duration.as_millis()).unwrap_or(i32::MAX);
        let p = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(w) = web_sys::window() {
                let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(p).await;
    }
}
