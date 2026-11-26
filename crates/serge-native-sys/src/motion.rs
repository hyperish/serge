use crate::sample::{SensorKind, SensorSample};
use block2::RcBlock;
use hyve::SharedRingBuf;
use objc2::rc::Retained;
use objc2_core_motion::{CMAccelerometerData, CMGyroData, CMMotionManager};
use objc2_foundation::{NSError, NSOperationQueue};
use std::{io, time::Duration};

/// Core Motion subscription with the same physical units and sample layout as Android.
pub struct Sensor {
    manager: Retained<CMMotionManager>,
    queue: Retained<NSOperationQueue>,
    kind: SensorKind,
    ring: SharedRingBuf<SensorSample>,
}
impl Sensor {
    /// Starts a private serial delivery queue. Declare NSMotionUsageDescription in the app.
    pub fn open(kind: SensorKind, interval: Duration, capacity: usize) -> io::Result<Self> {
        if interval.is_zero()
            || interval.as_micros() > i32::MAX as u128
            || !(1..=1_048_576).contains(&capacity)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid sensor interval or capacity",
            ));
        }
        let queue = NSOperationQueue::new();
        queue.setMaxConcurrentOperationCount(1);
        let ring = SharedRingBuf::new(capacity);
        let writer = ring.clone();
        let save = move |time: f64, values: [f64; 3], scale: f64| {
            writer.push(SensorSample {
                timestamp_ns: (time * 1e9) as i64,
                xyz: values.map(|v| (v * scale) as f32),
                padding: 0,
            });
        };
        // SAFETY: the manager copies its callback and retains the queue. Core Motion
        // supplies borrowed data or an NSError; no borrowed pointer escapes the callback.
        #[allow(unsafe_code)]
        let manager = unsafe {
            let m = CMMotionManager::new();
            match kind {
                SensorKind::Accelerometer if m.isAccelerometerAvailable() => {
                    m.setAccelerometerUpdateInterval(interval.as_secs_f64());
                    let callback =
                        RcBlock::new(move |data: *mut CMAccelerometerData, err: *mut NSError| {
                            if let Some(data) = data.as_ref().filter(|_| err.is_null()) {
                                let a = data.acceleration();
                                save(data.timestamp(), [a.x, a.y, a.z], -9.80665);
                            }
                        });
                    m.startAccelerometerUpdatesToQueue_withHandler(
                        &queue,
                        RcBlock::as_ptr(&callback),
                    );
                }
                SensorKind::Gyroscope if m.isGyroAvailable() => {
                    m.setGyroUpdateInterval(interval.as_secs_f64());
                    let callback = RcBlock::new(move |data: *mut CMGyroData, err: *mut NSError| {
                        if let Some(data) = data.as_ref().filter(|_| err.is_null()) {
                            let r = data.rotationRate();
                            save(data.timestamp(), [r.x, r.y, r.z], 1.0);
                        }
                    });
                    m.startGyroUpdatesToQueue_withHandler(&queue, RcBlock::as_ptr(&callback));
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "requested sensor unavailable",
                    ));
                }
            }
            m
        };
        Ok(Self {
            manager,
            queue,
            kind,
            ring,
        })
    }
    /// Latest samples retained by the bounded ring.
    pub fn samples(&self) -> &SharedRingBuf<SensorSample> {
        &self.ring
    }
}
impl Drop for Sensor {
    fn drop(&mut self) {
        // SAFETY: this subscription owns the retained manager and its serial delivery queue.
        #[allow(unsafe_code)]
        unsafe {
            match self.kind {
                SensorKind::Accelerometer => self.manager.stopAccelerometerUpdates(),
                SensorKind::Gyroscope => self.manager.stopGyroUpdates(),
                SensorKind::Light | SensorKind::HingeAngle => {}
            }
        }
        self.queue.cancelAllOperations();
        self.queue.waitUntilAllOperationsAreFinished();
    }
}
