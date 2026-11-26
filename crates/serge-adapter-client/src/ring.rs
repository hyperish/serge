//! Tear-free POD samples from an engine thread or an attached local process.
//! Reading a snapshot during paint tracks the producer’s revision.

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
    ring: Backend<T>,
    source: FrameSource,
}

#[derive(Clone)]
enum Backend<T: Pod> {
    Local(hyve::SharedRingBuf<T>),
    #[cfg(all(
        feature = "shm",
        target_has_atomic = "64",
        any(
            target_os = "macos",
            target_os = "ios",
            target_os = "linux",
            target_os = "android",
            windows
        )
    ))]
    Mapped(hyve::MappedRing<T>),
}

#[cfg(all(
    feature = "shm",
    target_has_atomic = "64",
    any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android",
        windows
    )
))]
impl<T: Pod> SharedRingBuf<T> {
    fn from_mapped(ring: hyve::MappedRing<T>) -> Self {
        let producer = ring.clone();
        Self {
            ring: Backend::Mapped(ring),
            source: FrameSource::with_revision(move || producer.revision()),
        }
    }

    /// Allocates shared memory for a versioned POD layout.
    pub fn new_shared(capacity: usize, layout: u64) -> std::io::Result<Self> {
        hyve::MappedRing::new(capacity, layout).map(Self::from_mapped)
    }

    /// Attaches a shared Windows section received over authorized local IPC.
    #[cfg(windows)]
    pub fn from_token(token: &[u8; 24], layout: u64) -> std::io::Result<Self> {
        hyve::MappedRing::from_token(token, layout).map(Self::from_mapped)
    }
    /// Returns a Windows section token for the authorized peer.
    #[cfg(windows)]
    pub fn share_token(&self) -> std::io::Result<[u8; 24]> {
        match &self.ring {
            Backend::Mapped(ring) => Ok(ring.share_token()),
            Backend::Local(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "this ring is in-process",
            )),
        }
    }

    /// Attaches a shared ring sent over a private Unix socket.
    #[cfg(unix)]
    pub fn receive(peer: &std::os::unix::net::UnixStream, layout: u64) -> std::io::Result<Self> {
        hyve::MappedRing::receive(peer, layout).map(Self::from_mapped)
    }

    /// Sends a shared ring to a peer. The in-process constructor has no descriptor.
    #[cfg(unix)]
    pub fn send(&self, peer: &std::os::unix::net::UnixStream) -> std::io::Result<()> {
        match &self.ring {
            Backend::Mapped(ring) => ring.send(peer),
            Backend::Local(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "this ring is in-process",
            )),
        }
    }
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
            ring: Backend::Local(hyve::SharedRingBuf::new(capacity.max(1))),
            source: FrameSource::new(),
        }
    }

    /// Appends `sample`, overwriting the oldest when full. Any thread; never
    /// waits for readers.
    pub fn push(&self, sample: T) {
        self.try_push(sample)
            .expect("shared ring has exhausted its generations");
    }

    /// Fallible variant for long-lived shared-memory producers.
    pub fn try_push(&self, sample: T) -> std::io::Result<()> {
        match &self.ring {
            Backend::Local(ring) => {
                ring.push(sample);
                self.source.bump();
                Ok(())
            }
            #[cfg(all(
                feature = "shm",
                target_has_atomic = "64",
                any(
                    target_os = "macos",
                    target_os = "ios",
                    target_os = "linux",
                    target_os = "android",
                    windows
                )
            ))]
            Backend::Mapped(ring) => ring.push(sample),
        }
    }

    /// Copies the newest samples into `out` (oldest of them first) and
    /// returns how many it copied: never a sample half-written by the
    /// engine. Inside a `Canvas` paint this also makes the canvas repaint
    /// when new samples arrive.
    pub fn read_latest_snapshot(&self, out: &mut [T]) -> usize {
        self.source.track();
        match &self.ring {
            Backend::Local(ring) => ring.read_latest_snapshot(out),
            #[cfg(all(
                feature = "shm",
                target_has_atomic = "64",
                any(
                    target_os = "macos",
                    target_os = "ios",
                    target_os = "linux",
                    target_os = "android",
                    windows
                )
            ))]
            Backend::Mapped(ring) => ring.read_latest_snapshot(out),
        }
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
        match &self.ring {
            Backend::Local(ring) => ring.capacity(),
            #[cfg(all(
                feature = "shm",
                target_has_atomic = "64",
                any(
                    target_os = "macos",
                    target_os = "ios",
                    target_os = "linux",
                    target_os = "android",
                    windows
                )
            ))]
            Backend::Mapped(ring) => ring.capacity(),
        }
    }

    /// How many samples were pushed so far.
    pub fn total_pushed(&self) -> u64 {
        match &self.ring {
            Backend::Local(ring) => ring.total_pushed(),
            #[cfg(all(
                feature = "shm",
                target_has_atomic = "64",
                any(
                    target_os = "macos",
                    target_os = "ios",
                    target_os = "linux",
                    target_os = "android",
                    windows
                )
            ))]
            Backend::Mapped(ring) => ring.total_pushed(),
        }
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

    #[cfg(all(
        feature = "shm",
        target_has_atomic = "64",
        any(
            target_os = "macos",
            target_os = "ios",
            target_os = "linux",
            target_os = "android",
            windows
        )
    ))]
    #[test]
    fn independently_attached_producer_invalidates_the_canvas() {
        use serge_core::paint::{sources_changed, track_frame_sources};
        let ring = SharedRingBuf::<u64>::new_shared(2, 17).unwrap();
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        ring.send(&a).unwrap();
        let writer = SharedRingBuf::<u64>::receive(&b, 17).unwrap();
        let (values, tracked) = track_frame_sources(|| ring.snapshot());
        assert!(values.is_empty());
        assert!(!sources_changed(&tracked));
        writer.try_push(42).unwrap();
        assert!(sources_changed(&tracked));
        assert_eq!(ring.snapshot(), [42]);
    }

    /// A writer thread never blocks on the reader, and the reader never sees
    /// a torn sample.
    #[test]
    fn concurrent_reads_are_never_torn() {
        let r = SharedRingBuf::new(64);
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, started) = std::sync::mpsc::channel();
        let writer = {
            let (r, stop) = (r.clone(), stop.clone());
            std::thread::spawn(move || {
                r.push(Sample { seq: 0, check: !0 });
                ready.send(()).unwrap();
                let mut seq = 1u64;
                while !stop.load(Ordering::Relaxed) {
                    r.push(Sample { seq, check: !seq });
                    seq += 1;
                }
                seq
            })
        };
        started.recv().unwrap();
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
