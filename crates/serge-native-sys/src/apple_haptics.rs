//! Play application-supplied vibration steps with Core Haptics.
#![allow(unsafe_code)]
use crate::haptics::{HapticPulse, validate_waveform};
use objc2::{AnyThread, rc::Retained};
use objc2_core_haptics::{CHHapticDeviceCapability, CHHapticEngine};
use objc2_foundation::NSData;
use serde_json::{Value, json};
use std::io;

fn pattern(steps: &[HapticPulse]) -> io::Result<Option<Value>> {
    validate_waveform(steps)?;
    let mut events = Vec::new();
    let mut elapsed = 0u32;
    for step in steps {
        if step.amplitude > 0 {
            events.push(json!({"Event": {
                "Time": f64::from(elapsed) / 1000.,
                "EventType": "HapticContinuous",
                "EventDuration": f64::from(step.millis) / 1000.,
                "EventParameters": [{"ParameterID": "HapticIntensity", "ParameterValue": f64::from(step.amplitude)/255.}]
            }}));
        }
        elapsed += step.millis;
    }
    Ok((!events.is_empty()).then(|| json!({"Version":1.0,"Pattern":events})))
}
/// A live Core Haptics engine. Keep it alive through the end of the waveform.
pub struct AppleHaptics(Retained<CHHapticEngine>);
impl AppleHaptics {
    /// Returns None when the current device has no supported haptic actuator.
    pub fn open() -> io::Result<Option<Self>> {
        // SAFETY: the framework returns its own capabilities object and a retained engine.
        unsafe {
            if !CHHapticEngine::capabilitiesForHardware().supportsHaptics() {
                return Ok(None);
            }
            let engine = CHHapticEngine::initAndReturnError(CHHapticEngine::alloc())
                .map_err(|e| io::Error::other(e.to_string()))?;
            engine.setPlaysHapticsOnly(true);
            engine.setAutoShutdownEnabled(true);
            Ok(Some(Self(engine)))
        }
    }
    /// Plays up to 256 steps over at most 30 seconds. The OS starts the engine as needed.
    pub fn vibrate_waveform(&self, steps: &[HapticPulse]) -> io::Result<()> {
        if let Some(pattern) = pattern(steps)? {
            let data = NSData::with_bytes(&serde_json::to_vec(&pattern)?);
            // SAFETY: NSData owns the complete AHAP pattern during the synchronous submission.
            unsafe { self.0.playPatternFromData_error(&data) }
                .map_err(|e| io::Error::other(e.to_string()))?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_haptics::CHHapticPattern;
    use objc2_foundation::{NSString, NSURL};
    #[test]
    fn core_haptics_loads_the_waveform_with_its_rest_and_intensities() {
        let pulse = |millis, amplitude| HapticPulse { millis, amplitude };
        let ahap = pattern(&[pulse(120, 128), pulse(200, 0), pulse(80, 255)])
            .unwrap()
            .unwrap();
        assert_eq!(ahap["Pattern"][1]["Event"]["Time"], 0.32);
        assert_eq!(
            ahap["Pattern"][0]["Event"]["EventParameters"][0]["ParameterValue"],
            128. / 255.
        );
        let path = std::env::temp_dir().join(format!("serge-{}.ahap", hyve::Ulid::new()));
        std::fs::write(&path, serde_json::to_vec(&ahap).unwrap()).unwrap();
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
        // SAFETY: the URL names the complete, closed temporary file above.
        let decoded =
            unsafe { CHHapticPattern::initWithContentsOfURL_error(CHHapticPattern::alloc(), &url) };
        std::fs::remove_file(path).unwrap();
        assert!((unsafe { decoded.unwrap().duration() } - 0.4).abs() < 0.00001);
        assert!(pattern(&[pulse(120, 0)]).unwrap().is_none());
        assert!(pattern(&[pulse(0, 255)]).is_err());
        assert!(pattern(&[pulse(30_001, 1)]).is_err());
    }
}
