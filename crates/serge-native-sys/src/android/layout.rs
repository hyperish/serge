use super::{AndroidSystem, Prompt, services::class};
use jni::{
    jni_sig, jni_str,
    objects::{JIntArray, JObject, JValue},
    refs::Global,
};
use std::io;

/// Physical state of a fold reported for the activity's own window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldState {
    /// The device is open flat.
    Flat,
    /// The two panels meet at a non-flat angle.
    HalfOpened,
}

/// A folding feature in physical pixels relative to the top-left of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    /// Left, top, right and bottom in the activity's window.
    pub bounds: [i32; 4],
    /// Current device posture.
    pub state: FoldState,
    /// Whether the hinge or crease runs vertically.
    pub vertical: bool,
    /// Whether the whole feature area hides content.
    pub occluded: bool,
    /// Whether the feature splits the usable window into two regions.
    pub separating: bool,
}

/// Jetpack WindowManager subscription. Dropping it unregisters the listener.
pub struct WindowPosture {
    system: AndroidSystem,
    listener: Global<JObject<'static>>,
}

impl WindowPosture {
    /// Latest complete snapshot, empty on a conventional display. Returns None before first delivery.
    pub fn latest(&self) -> io::Result<Option<Vec<Fold>>> {
        let data = self.system.access(|env, _| {
            let raw = env
                .call_method(&self.listener, jni_str!("snapshot"), jni_sig!("()[I"), &[])?
                .l()?;
            if raw.is_null() {
                return Ok(None);
            }
            let values = env.cast_local::<JIntArray>(raw)?;
            let mut result = vec![0; values.len(env)?];
            values.get_region(env, 0, &mut result)?;
            Ok(Some(result))
        })?;
        Ok(data.map(|data| {
            data.as_chunks::<8>()
                .0
                .iter()
                .map(|f| Fold {
                    bounds: [f[0], f[1], f[2], f[3]],
                    state: if f[4] == 1 {
                        FoldState::HalfOpened
                    } else {
                        FoldState::Flat
                    },
                    vertical: f[5] != 0,
                    occluded: f[6] != 0,
                    separating: f[7] != 0,
                })
                .collect()
        }))
    }
}

impl Drop for WindowPosture {
    fn drop(&mut self) {
        let _ = self.system.access(|env, _| {
            env.call_method(&self.listener, jni_str!("close"), jni_sig!("()V"), &[])?;
            Ok(())
        });
    }
}

impl AndroidSystem {
    /// Subscribes to window folding features. Build with android.window_layout enabled.
    pub fn window_posture(&self) -> io::Result<WindowPosture> {
        let listener = self.access(|env, activity| {
            let bridge = class(env, activity)?;
            let listener = env
                .call_static_method(
                    bridge,
                    jni_str!("posture"),
                    jni_sig!("(Landroid/app/Activity;)Ljava/lang/Object;"),
                    &[JValue::Object(activity)],
                )?
                .l()?;
            env.new_global_ref(listener)
        })?;
        Ok(WindowPosture {
            system: self.clone(),
            listener,
        })
    }

    /// Reports whether the activity currently occupies a picture-in-picture window.
    pub fn in_picture_in_picture(&self) -> io::Result<bool> {
        self.access(|env, activity| {
            env.call_method(
                activity,
                jni_str!("isInPictureInPictureMode"),
                jni_sig!("()Z"),
                &[],
            )?
            .z()
        })
    }

    /// Requests a PiP window on the UI thread. Declare supportsPictureInPicture on the activity.
    pub fn enter_picture_in_picture(&self, width: u32, height: u32) -> io::Result<Prompt> {
        if [width, height]
            .iter()
            .any(|&v| v == 0 || v > i32::MAX as u32)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a positive Android aspect ratio",
            ));
        }
        self.prompt("pip", "", "", &[&width.to_string(), &height.to_string()])
    }
}
