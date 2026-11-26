//! The UI-thread executor of native targets. Tasks are polled on the UI
//! thread (they touch thread-local signals) inside the shared `tokio`
//! runtime's context, so in-process `#[server]` handlers can do I/O and use
//! timers; wake-ups from other threads ask the host for a turn.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use serge_core::{Executor, LocalFuture, TaskHandle};

struct TaskWaker {
    id: u64,
    ready: Arc<Mutex<Vec<u64>>>,
    notify: Arc<dyn Fn() + Send + Sync>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.lock().expect("ready queue").push(self.id);
        (self.notify)();
    }
}

struct Inner {
    _shared_wake: hyve::state::SharedWake,
    tasks: RefCell<HashMap<u64, LocalFuture>>,
    aborted: RefCell<Vec<u64>>,
    next: Cell<u64>,
    ready: Arc<Mutex<Vec<u64>>>,
    notify: Arc<dyn Fn() + Send + Sync>,
}

/// Runs UI tasks; the host calls [`run_ready`](Self::run_ready) when
/// notified.
#[derive(Clone)]
pub struct LocalExecutor(Rc<Inner>);

impl std::fmt::Debug for LocalExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalExecutor")
            .field("tasks", &self.0.tasks.borrow().len())
            .finish()
    }
}

impl LocalExecutor {
    /// An executor that calls `notify` (from any thread) when a task is
    /// ready to be polled.
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> LocalExecutor {
        let notify = Arc::new(notify);
        let wake = notify.clone();
        LocalExecutor(Rc::new(Inner {
            _shared_wake: hyve::state::on_shared_wake(move || wake()),
            tasks: RefCell::new(HashMap::new()),
            aborted: RefCell::new(Vec::new()),
            next: Cell::new(0),
            ready: Arc::new(Mutex::new(Vec::new())),
            notify,
        }))
    }

    /// Polls every ready task once; returns whether any was polled.
    pub fn run_ready(&self) -> bool {
        let _rt = serge_core::runtime().enter();
        let mut any = hyve::state::drain_shared_updates();
        loop {
            let ids: Vec<u64> = std::mem::take(&mut *self.0.ready.lock().expect("ready queue"));
            if ids.is_empty() {
                return any;
            }
            for id in ids {
                let Some(mut task) = self.0.tasks.borrow_mut().remove(&id) else {
                    continue;
                };
                any = true;
                let waker = Waker::from(Arc::new(TaskWaker {
                    id,
                    ready: self.0.ready.clone(),
                    notify: self.0.notify.clone(),
                }));
                let mut cx = Context::from_waker(&waker);
                if task.as_mut().poll(&mut cx).is_pending() {
                    let mut aborted = self.0.aborted.borrow_mut();
                    if let Some(p) = aborted.iter().position(|a| *a == id) {
                        aborted.swap_remove(p);
                    } else {
                        self.0.tasks.borrow_mut().insert(id, task);
                    }
                }
            }
        }
    }

    /// How many tasks are alive.
    pub fn len(&self) -> usize {
        self.0.tasks.borrow().len()
    }

    /// Whether no task is alive.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Executor for LocalExecutor {
    fn spawn(&self, task: LocalFuture) -> TaskHandle {
        let id = self.0.next.get();
        self.0.next.set(id + 1);
        self.0.tasks.borrow_mut().insert(id, task);
        self.0.ready.lock().expect("ready queue").push(id);
        (self.0.notify)();
        let weak: Weak<Inner> = Rc::downgrade(&self.0);
        TaskHandle::new(move || {
            if let Some(inner) = weak.upgrade() {
                // Dropping the future cancels it; a task being polled right
                // now is dropped when its poll returns.
                if inner.tasks.borrow_mut().remove(&id).is_none() {
                    inner.aborted.borrow_mut().push(id);
                }
            }
        })
    }
}

/// A future that is ready after `poll`ing returns `Pending` once (yields to
/// other tasks).
pub fn yield_now() -> impl std::future::Future<Output = ()> {
    let mut yielded = false;
    std::future::poll_fn(move |cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
}
