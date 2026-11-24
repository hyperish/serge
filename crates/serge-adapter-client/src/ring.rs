//! A bounded buffer shared between an engine thread (writer) and the UI
//! (reader). Phase 1 uses a mutex; a lock-free ring is planned.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serge_core::FrameSource;

/// Keeps the last `capacity` values; pushing marks new data for canvases
/// that read it.
#[derive(Debug)]
pub struct SharedRingBuf<T> {
    inner: Arc<Mutex<VecDeque<T>>>,
    capacity: usize,
    source: FrameSource,
}

impl<T> Clone for SharedRingBuf<T> {
    fn clone(&self) -> Self {
        SharedRingBuf {
            inner: self.inner.clone(),
            capacity: self.capacity,
            source: self.source.clone(),
        }
    }
}

impl<T> SharedRingBuf<T> {
    /// A buffer keeping the last `capacity` values.
    pub fn new(capacity: usize) -> SharedRingBuf<T> {
        SharedRingBuf {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity: capacity.max(1),
            source: FrameSource::new(),
        }
    }

    /// Appends `value`, dropping the oldest when full (any thread).
    pub fn push(&self, value: T) {
        let mut q = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() == self.capacity {
            q.pop_front();
        }
        q.push_back(value);
        drop(q);
        self.source.bump();
    }

    /// Reads the values oldest first. Inside a `Canvas` paint this also
    /// makes the canvas repaint when new values arrive.
    pub fn with<R>(&self, f: impl FnOnce(&VecDeque<T>) -> R) -> R {
        self.source.track();
        let q = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&q)
    }

    /// How many values are held.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The source canvases watch.
    pub fn source(&self) -> &FrameSource {
        &self.source
    }
}

impl<T: Copy> SharedRingBuf<T> {
    /// Copies the newest values into `out` (oldest of them first) and
    /// returns how many it copied: a consistent snapshot, never a value
    /// half-written by the engine. Tracked like [`with`](Self::with).
    pub fn read_latest_snapshot(&self, out: &mut [T]) -> usize {
        self.with(|q| {
            let n = q.len().min(out.len());
            for (slot, v) in out.iter_mut().zip(q.iter().skip(q.len() - n)) {
                *slot = *v;
            }
            n
        })
    }
}

impl<T: Clone> SharedRingBuf<T> {
    /// A copy of the values, oldest first (tracked like [`with`](Self::with)).
    pub fn snapshot(&self) -> Vec<T> {
        self.with(|q| q.iter().cloned().collect())
    }
}
