use super::{AndroidSystem, services::class};
use crate::stylus::{StylusArea, StylusBatch, StylusPhase, StylusSample};
use jni::{
    jni_sig, jni_str,
    objects::{JLongArray, JObject, JValue},
    refs::Global,
};
use std::{f32::consts::FRAC_PI_2, io};

/// App-owned native drawing surface. It receives touches in its area until dropped.
pub struct StylusPad {
    system: AndroidSystem,
    pad: Global<JObject<'static>>,
}
impl AndroidSystem {
    /// Attaches a transparent pen input region above the activity's content.
    pub fn stylus_pad(&self, area: StylusArea) -> io::Result<StylusPad> {
        area.validate()?;
        let pad = self.access(|env, activity| {
            let bridge = class(env, activity)?;
            let pad = env
                .call_static_method(
                    bridge,
                    jni_str!("stylusPad"),
                    jni_sig!("(Landroid/app/Activity;IIII)Lorg/hyperish/serge/StylusPad;"),
                    &[
                        JValue::Object(activity),
                        JValue::Int(area.x as i32),
                        JValue::Int(area.y as i32),
                        JValue::Int(area.width as i32),
                        JValue::Int(area.height as i32),
                    ],
                )?
                .l()?;
            env.new_global_ref(pad)
        })?;
        Ok(StylusPad {
            system: self.clone(),
            pad,
        })
    }
}
impl StylusPad {
    /// Returns true once the platform has attached the surface, or an error on dismissal.
    pub fn is_ready(&self) -> io::Result<bool> {
        match self.system.access(|env, _| {
            env.call_method(&self.pad, jni_str!("status"), jni_sig!("()I"), &[])?
                .i()
        })? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(io::Error::other("Android stylus surface unavailable")),
        }
    }
    /// Takes all queued stylus and eraser samples, including OS-batched motion.
    pub fn drain(&self) -> io::Result<StylusBatch> {
        let words = self.system.access(|env, _| {
            let array = env
                .call_method(&self.pad, jni_str!("drain"), jni_sig!("()[J"), &[])?
                .l()?;
            let array = env.cast_local::<JLongArray>(array)?;
            let mut words = vec![0; array.len(env)?];
            array.get_region(env, 0, &mut words)?;
            Ok(words)
        })?;
        let (dropped, data) = words
            .split_first()
            .ok_or_else(|| io::Error::other("missing stylus batch"))?;
        let (samples, rest) = data.as_chunks::<8>();
        if !rest.is_empty() {
            return Err(io::Error::other("incomplete stylus sample"));
        }
        let mut batch = StylusBatch {
            dropped: *dropped as u64,
            samples: Vec::with_capacity(samples.len()),
        };
        for &[time, id, phase, eraser, x, y, pressure, tilt] in samples {
            let float = |bits| f32::from_bits(bits as u32);
            batch.samples.push(StylusSample {
                timestamp_ns: time as u64,
                pointer_id: id as u64,
                phase: match phase {
                    0 => StylusPhase::Down,
                    1 => StylusPhase::Move,
                    2 => StylusPhase::Up,
                    3 => StylusPhase::Cancel,
                    _ => return Err(io::Error::other("invalid stylus phase")),
                },
                eraser: eraser != 0,
                position: [float(x), float(y)],
                pressure: float(pressure).clamp(0., 1.),
                tilt: float(tilt).clamp(0., FRAC_PI_2),
            });
        }
        Ok(batch)
    }
}
impl Drop for StylusPad {
    fn drop(&mut self) {
        let _ = self.system.access(|env, _| {
            env.call_method(&self.pad, jni_str!("close"), jni_sig!("()V"), &[])?;
            Ok(())
        });
    }
}
