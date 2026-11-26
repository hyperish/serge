use super::AndroidSystem;
pub use crate::sample::{SensorKind, SensorSample};
use hyve::SharedRingBuf;
use ndk::audio::{
    AudioCallbackResult, AudioDirection, AudioFormat, AudioStream, AudioStreamBuilder,
};
use ndk::looper::ThreadLooper;
use ndk_sys as ffi;
use std::{
    ffi::CString,
    io,
    ptr::{self, NonNull},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

impl SensorKind {
    fn id(self) -> i32 {
        match self {
            Self::Accelerometer => ffi::ASENSOR_TYPE_ACCELEROMETER,
            Self::Gyroscope => ffi::ASENSOR_TYPE_GYROSCOPE,
            Self::Light => ffi::ASENSOR_TYPE_LIGHT,
            Self::HingeAngle => ffi::ASENSOR_TYPE_HINGE_ANGLE,
        }
    }
}
struct Worker {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Worker {
    fn start<T: Send + 'static>(
        init: impl FnOnce(mpsc::SyncSender<io::Result<T>>, Arc<AtomicBool>) + Send + 'static,
    ) -> io::Result<(Self, T)> {
        let (tx, rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = thread::Builder::new()
            .name("serge-device-input".into())
            .spawn(move || init(tx, flag))?;
        let worker = Self {
            stop,
            thread: Some(thread),
        };
        let output = rx
            .recv()
            .map_err(|_| io::Error::other("device input initialization stopped"))??;
        Ok((worker, output))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn capacity(n: usize) -> io::Result<()> {
    if n == 0 || n > 1_048_576 {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid device ring capacity",
        ))
    } else {
        Ok(())
    }
}

struct Queue {
    manager: NonNull<ffi::ASensorManager>,
    sensor: *const ffi::ASensor,
    queue: NonNull<ffi::ASensorEventQueue>,
}
impl Queue {
    // The manager is owned by the OS. The sensor and queue are accessed only on
    // the creating looper thread. Drop disables and destroys before the looper exits.
    #[allow(unsafe_code)]
    fn open(
        package: &CString,
        kind: SensorKind,
        interval: Duration,
        looper: &ThreadLooper,
    ) -> io::Result<Self> {
        unsafe {
            let manager = NonNull::new(ffi::ASensorManager_getInstanceForPackage(package.as_ptr()))
                .ok_or_else(|| io::Error::other("no Android sensor manager"))?;
            let sensor = ffi::ASensorManager_getDefaultSensor(manager.as_ptr(), kind.id());
            if sensor.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "requested sensor unavailable",
                ));
            }
            let queue = NonNull::new(ffi::ASensorManager_createEventQueue(
                manager.as_ptr(),
                looper.as_foreign().ptr().as_ptr(),
                1,
                None,
                ptr::null_mut(),
            ))
            .ok_or_else(|| io::Error::other("could not create Android sensor queue"))?;
            let q = Self {
                manager,
                sensor,
                queue,
            };
            if ffi::ASensorEventQueue_enableSensor(queue.as_ptr(), sensor) < 0 {
                return Err(io::Error::other("could not enable Android sensor"));
            }
            let min = ffi::ASensor_getMinDelay(sensor);
            if min > 0
                && ffi::ASensorEventQueue_setEventRate(
                    queue.as_ptr(),
                    sensor,
                    (interval.as_micros() as i32).max(min),
                ) < 0
            {
                return Err(io::Error::other("could not set Android sensor rate"));
            }
            Ok(q)
        }
    }
    #[allow(unsafe_code)]
    fn drain(&self, kind: SensorKind, ring: &SharedRingBuf<SensorSample>) {
        // All slots are initialized; process only the count written by the NDK.
        let mut events = unsafe { std::mem::zeroed::<[ffi::ASensorEvent; 32]>() };
        let count = unsafe {
            ffi::ASensorEventQueue_getEvents(self.queue.as_ptr(), events.as_mut_ptr(), events.len())
        };
        for e in events
            .iter()
            .take(count.max(0) as usize)
            .filter(|e| e.type_ == kind.id())
        {
            // These NDK sensor types use the float data union.
            let data = unsafe { e.__bindgen_anon_1.__bindgen_anon_1.data };
            let xyz = if matches!(kind, SensorKind::Light | SensorKind::HingeAngle) {
                [data[0], 0., 0.]
            } else {
                [data[0], data[1], data[2]]
            };
            ring.push(SensorSample {
                timestamp_ns: e.timestamp,
                xyz,
                padding: 0,
            });
        }
    }
}
impl Drop for Queue {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        unsafe {
            ffi::ASensorEventQueue_disableSensor(self.queue.as_ptr(), self.sensor);
            ffi::ASensorManager_destroyEventQueue(self.manager.as_ptr(), self.queue.as_ptr());
        }
    }
}
/// Active hardware subscription. Dropping it disables the OS sensor.
pub struct Sensor {
    _worker: Worker,
    ring: SharedRingBuf<SensorSample>,
}
impl Sensor {
    /// Starts a bounded sensor ring. The platform enforces its supported sampling rate.
    pub fn open(
        system: &AndroidSystem,
        kind: SensorKind,
        interval: Duration,
        length: usize,
    ) -> io::Result<Self> {
        capacity(length)?;
        if interval.is_zero() || interval.as_micros() > i32::MAX as u128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid sensor interval",
            ));
        }
        let package = CString::new(system.package_name()?).map_err(io::Error::other)?;
        let ring = SharedRingBuf::new(length);
        let producer = ring.clone();
        let (worker, ()) = Worker::start(move |started, stop| {
            let looper = ThreadLooper::prepare();
            let queue = match Queue::open(&package, kind, interval, &looper) {
                Ok(q) => q,
                Err(e) => {
                    let _ = started.send(Err(e));
                    return;
                }
            };
            if started.send(Ok(())).is_err() {
                return;
            }
            while !stop.load(Ordering::Acquire) {
                if looper.poll_once_timeout(Duration::from_millis(50)).is_err() {
                    break;
                }
                queue.drain(kind, &producer);
            }
        })?;
        Ok(Self {
            _worker: worker,
            ring,
        })
    }
    /// Latest tear-free samples, shared with the producer thread.
    pub fn samples(&self) -> &SharedRingBuf<SensorSample> {
        &self.ring
    }
}
/// A bounded mono i16 PCM ring from the Android AAudio input stream.
pub struct Microphone {
    stream: AudioStream,
    ring: SharedRingBuf<i16>,
}
impl Microphone {
    /// Opens a mono input stream after the activity has RECORD_AUDIO permission.
    pub fn open(system: &AndroidSystem, length: usize) -> io::Result<Self> {
        capacity(length)?;
        if !system.is_granted("android.permission.RECORD_AUDIO")? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "RECORD_AUDIO is required",
            ));
        }
        let ring = SharedRingBuf::new(length);
        let producer = ring.clone();
        let builder = AudioStreamBuilder::new().map_err(io::Error::other)?;
        let stream = builder
            .direction(AudioDirection::Input)
            .format(AudioFormat::PCM_I16)
            .channel_count(1)
            .data_callback(Box::new(move |_, data, n| {
                if data.is_null() || n < 0 {
                    return AudioCallbackResult::Stop;
                }
                // SAFETY: before starting, we check that one input frame is one i16.
                // AAudio owns this buffer for precisely the duration of this callback.
                #[allow(unsafe_code)]
                let samples = unsafe { std::slice::from_raw_parts(data.cast::<i16>(), n as usize) };
                for &sample in samples {
                    producer.push(sample)
                }
                AudioCallbackResult::Continue
            }))
            .open_stream()
            .map_err(io::Error::other)?;
        if stream.channel_count() != 1 || stream.format() != AudioFormat::PCM_I16 {
            return Err(io::Error::other("AAudio did not open mono i16 input"));
        }
        stream.request_start().map_err(io::Error::other)?;
        Ok(Self { stream, ring })
    }
    /// Actual OS sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.stream.sample_rate() as u32
    }
    /// Latest signed 16-bit PCM samples.
    pub fn samples(&self) -> &SharedRingBuf<i16> {
        &self.ring
    }
}
impl Drop for Microphone {
    fn drop(&mut self) {
        let _ = self.stream.request_stop();
    }
}
