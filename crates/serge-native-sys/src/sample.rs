use hyve::bytemuck::{Pod, Zeroable};
/// A hardware sensor in the device's natural coordinate system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorKind {
    /// Acceleration including gravity in m/s².
    Accelerometer,
    /// Angular velocity in radians per second.
    Gyroscope,
    /// Ambient light level in lux in the first component.
    Light,
    /// Hinge angle in degrees on devices with an Android hinge sensor.
    HingeAngle,
}
/// One timestamped hardware sample.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, Pod, Zeroable)]
#[bytemuck(crate = "hyve::bytemuck")]
pub struct SensorSample {
    /// Nanoseconds since boot from the platform event.
    pub timestamp_ns: i64,
    /// Device axes, or a scalar followed by two zeroes for light and hinge angle.
    pub xyz: [f32; 3],
    pub(crate) padding: u32,
}
