//! The web UI executor: tasks run on the browser's microtask queue
//! (`wasm-bindgen-futures`); aborting drops the future (an in-flight
//! `fetch` is aborted by its transport when its future drops).

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use serge_core::{Executor, LocalFuture, TaskHandle};

/// Spawns UI tasks with `wasm_bindgen_futures::spawn_local`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WebExecutor;

struct Slot {
    fut: RefCell<Option<LocalFuture>>,
    waker: RefCell<Option<Waker>>,
    aborted: Cell<bool>,
}

struct Task(Rc<Slot>);

impl Future for Task {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let slot = &self.0;
        if slot.aborted.get() {
            slot.fut.borrow_mut().take();
            return Poll::Ready(());
        }
        *slot.waker.borrow_mut() = Some(cx.waker().clone());
        let mut fut = slot.fut.borrow_mut();
        let Some(f) = fut.as_mut() else {
            return Poll::Ready(());
        };
        let done = f.as_mut().poll(cx).is_ready();
        if done || slot.aborted.get() {
            fut.take();
            return Poll::Ready(());
        }
        Poll::Pending
    }
}

impl Executor for WebExecutor {
    fn spawn(&self, task: LocalFuture) -> TaskHandle {
        let slot = Rc::new(Slot {
            fut: RefCell::new(Some(task)),
            waker: RefCell::new(None),
            aborted: Cell::new(false),
        });
        wasm_bindgen_futures::spawn_local(Task(slot.clone()));
        TaskHandle::new(move || {
            slot.aborted.set(true);
            // Drop the future now unless it is being polled (then the poll
            // drops it when it returns).
            if let Ok(mut f) = slot.fut.try_borrow_mut() {
                let dropped = f.take();
                drop(f);
                drop(dropped);
            }
            if let Some(w) = slot.waker.borrow_mut().take() {
                w.wake();
            }
        })
    }
}
