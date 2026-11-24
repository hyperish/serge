//! The in-process engine → UI ring (`docs/03-adapters-and-pipeline.md`
//! §2, Tier 1): `hyve`'s lock-free `SharedRingBuf` (writers never block,
//! readers get tear-free snapshots; one sequence lock per slot) plus a
//! [`FrameSource`], so a `Canvas` that reads it repaints on the next frame
//! after new samples arrive — no signal per sample.

use hyve::bytemuck::Pod;
use serge_core::FrameSource;

/// Keeps the latest `capacity` samples of a `#[repr(C)]` POD type. Clones
/// share the ring.
///
/// ```ignore
/// #[repr(C)]
/// #[derive(Clone, Copy, serge::bytemuck::Pod, serge::bytemuck::Zeroable)]
/// #[bytemuck(crate = "serge::bytemuck")]
/// pub struct FrameMetrics {
///     pub frame_ms: f32,
/// }
/// ```
pub struct SharedRingBuf<T: Pod> {
    ring: hyve::SharedRingBuf<T>,
    source: FrameSource,
}

impl<T: Pod> std::fmt::Debug for SharedRingBuf<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedRingBuf")
            .field("capacity", &self.capacity())
            .field("total_pushed", &self.total_pushed())
            .finish()
    }
}

impl<T: Pod> Clone for SharedRingBuf<T> {
    fn clone(&self) -> Self {
        SharedRingBuf {
            ring: self.ring.clone(),
            source: self.source.clone(),
        }
    }
}

impl<T: Pod> SharedRingBuf<T> {
    /// A ring keeping the latest `capacity` samples (at least one).
    pub fn new(capacity: usize) -> SharedRingBuf<T> {
        SharedRingBuf {
            ring: hyve::SharedRingBuf::new(capacity.max(1)),
            source: FrameSource::new(),
        }
    }

    /// Appends `sample`, overwriting the oldest when full. Any thread; never
    /// waits for readers.
    pub fn push(&self, sample: T) {
        self.ring.push(sample);
        self.source.bump();
    }

    /// Copies the newest samples into `out` (oldest of them first) and
    /// returns how many it copied: never a sample half-written by the
    /// engine. Inside a `Canvas` paint this also makes the canvas repaint
    /// when new samples arrive.
    pub fn read_latest_snapshot(&self, out: &mut [T]) -> usize {
        self.source.track();
        self.ring.read_latest_snapshot(out)
    }

    /// Every held sample, oldest first (tracked like
    /// [`read_latest_snapshot`](Self::read_latest_snapshot)).
    pub fn snapshot(&self) -> Vec<T> {
        let mut out = vec![T::zeroed(); self.len()];
        let n = self.read_latest_snapshot(&mut out);
        out.truncate(n);
        out
    }

    /// How many samples it keeps.
    pub fn capacity(&self) -> usize {
        self.ring.capacity()
    }

    /// How many samples were pushed so far.
    pub fn total_pushed(&self) -> u64 {
        self.ring.total_pushed()
    }

    /// How many samples are held.
    pub fn len(&self) -> usize {
        usize::try_from(self.total_pushed()).map_or(self.capacity(), |n| n.min(self.capacity()))
    }

    /// Whether nothing was pushed yet.
    pub fn is_empty(&self) -> bool {
        self.total_pushed() == 0
    }

    /// The source canvases watch.
    pub fn source(&self) -> &FrameSource {
        &self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyve::bytemuck::{Pod, Zeroable};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
    #[bytemuck(crate = "hyve::bytemuck")]
    struct Sample {
        seq: u64,
        check: u64,
    }

    #[test]
    fn keeps_the_latest_samples_in_order() {
        let r = SharedRingBuf::new(4);
        assert!(r.is_empty());
        for seq in 0..10 {
            r.push(Sample { seq, check: !seq });
        }
        assert_eq!(r.len(), 4);
        let seqs: Vec<u64> = r.snapshot().iter().map(|s| s.seq).collect();
        assert_eq!(seqs, [6, 7, 8, 9]);
        let mut two = [Sample::zeroed(); 2];
        assert_eq!(r.read_latest_snapshot(&mut two), 2);
        assert_eq!((two[0].seq, two[1].seq), (8, 9));
    }

    /// A writer thread never blocks on the reader, and the reader never sees
    /// a torn sample.
    #[test]
    fn concurrent_reads_are_never_torn() {
        let r = SharedRingBuf::new(64);
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let (r, stop) = (r.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut seq = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    r.push(Sample { seq, check: !seq });
                    seq += 1;
                }
                seq
            })
        };
        let mut out = [Sample::zeroed(); 32];
        let mut reads = 0;
        while reads < 20_000 {
            let n = r.read_latest_snapshot(&mut out);
            for s in &out[..n] {
                assert_eq!(s.check, !s.seq, "torn sample");
            }
            for w in out[..n].windows(2) {
                assert!(w[0].seq < w[1].seq, "out of order");
            }
            reads += 1;
        }
        stop.store(true, Ordering::Relaxed);
        assert!(writer.join().unwrap() > 0);
    }
}
