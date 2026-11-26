use std::io;

/// One step in a vibration waveform. Zero amplitude represents a pause.
#[derive(Clone, Copy, Debug)]
pub struct HapticPulse {
    /// Step duration in milliseconds.
    pub millis: u32,
    /// Intensity from 0 to 255. Devices without intensity control use on/off.
    pub amplitude: u8,
}
pub(crate) fn validate_waveform(steps: &[HapticPulse]) -> io::Result<()> {
    if steps.is_empty()
        || steps.len() > 256
        || steps.iter().any(|s| s.millis == 0)
        || steps.iter().map(|s| u64::from(s.millis)).sum::<u64>() > 30_000
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected 1 to 256 positive steps totaling at most 30 seconds",
        ));
    }
    Ok(())
}
