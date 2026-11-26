use std::io;

/// Drawing area in physical pixels from the top-left of the app's content.
#[derive(Clone, Copy, Debug)]
pub struct StylusArea {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}
impl StylusArea {
    pub(crate) fn validate(self) -> io::Result<()> {
        if self.width == 0
            || self.height == 0
            || u64::from(self.x) + u64::from(self.width) > i32::MAX as u64
            || u64::from(self.y) + u64::from(self.height) > i32::MAX as u64
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid stylus area",
            ));
        }
        Ok(())
    }
}

/// Contact milestones for one stylus stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StylusPhase {
    /// Pen made contact with the drawing area.
    Down,
    /// Pen moved while touching the drawing area.
    Move,
    /// Pen lifted.
    Up,
    /// The OS interrupted the stroke.
    Cancel,
}

/// One measured pen contact. Coalesced motion is delivered in sample order for each stroke.
#[derive(Clone, Copy, Debug)]
pub struct StylusSample {
    /// OS uptime in nanoseconds at acquisition.
    pub timestamp_ns: u64,
    /// Identity for this contact from down through up or cancellation.
    pub pointer_id: u64,
    /// Current stage of the contact.
    pub phase: StylusPhase,
    /// Physical pixels relative to the drawing area's top-left, even when moving outside it.
    pub position: [f32; 2],
    /// Tip pressure relative to the nominal device maximum, clamped to 0 through 1.
    pub pressure: f32,
    /// Radians from the surface normal. A perpendicular pen has zero tilt.
    pub tilt: f32,
    /// The OS identified the eraser end of the tool.
    pub eraser: bool,
}

/// Samples retained since the last read. A slow consumer can detect interrupted strokes.
#[derive(Debug, Default)]
pub struct StylusBatch {
    /// Count of older samples overwritten since the preceding read.
    pub dropped: u64,
    /// At most 512 most recent samples.
    pub samples: Vec<StylusSample>,
}
