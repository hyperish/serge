//! Android application services owned by a NativeActivity.
mod background;
pub use background::{BackgroundStatus, BackgroundTask};
#[cfg(feature = "bluetooth")]
mod ble;
mod camera;
mod camera_buffer;
pub use camera_buffer::{CameraBufferFrame, CameraBufferReader};
mod layout;
mod notifications;
pub use camera::{CameraFrame, CameraTexture};
pub use layout::{Fold, FoldState, WindowPosture};
mod sensors;
mod services;
mod signing;
mod stylus;
pub use stylus::StylusPad;
mod widgets;
pub use sensors::{Microphone, Sensor, SensorKind, SensorSample};
pub use services::{Credential, Prompt, PromptResult};
pub use signing::SigningKey;
pub use widgets::WidgetUpdate;

use android_activity::AndroidApp;
use jni::{
    Env, JavaVM,
    errors::Result as JniResult,
    jni_sig, jni_str,
    objects::{JObject, JString, JValue},
    refs::Global,
};
use std::{io, path::PathBuf};

use crate::haptics::{HapticPulse, validate_waveform};

/// Typed access to the current activity. Create a new handle on activity recreation.
#[derive(Clone)]
pub struct AndroidSystem(pub(crate) AndroidApp);
impl AndroidSystem {
    /// Binds system services to the activity passed to android_main.
    pub fn new(app: AndroidApp) -> Self {
        Self(app)
    }
    fn access<T>(&self, f: impl FnOnce(&mut Env<'_>, &JObject) -> JniResult<T>) -> io::Result<T> {
        // SAFETY: AndroidApp owns the unmodified VM and global activity reference
        // throughout this synchronous attachment. Neither is retained or deleted here.
        #[allow(unsafe_code)]
        let vm = unsafe { JavaVM::from_raw(self.0.vm_as_ptr().cast()) };
        vm.attach_current_thread(|env| {
            let raw = self.0.activity_as_ptr().cast();
            #[allow(unsafe_code)]
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            f(env, activity.as_ref())
        })
        .map_err(io::Error::other)
    }
    /// The activity's application-private data directory.
    pub fn data_directory(&self) -> io::Result<PathBuf> {
        self.0
            .internal_data_path()
            .ok_or_else(|| io::Error::other("Android app directory unavailable"))
    }
    /// Application package reported by Android.
    pub fn package_name(&self) -> io::Result<String> {
        self.access(|env, a| {
            let package = env
                .call_method(
                    a,
                    jni_str!("getPackageName"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let name = env.cast_local::<JString>(package)?;
            Ok(name.to_string())
        })
    }
    /// Returns the data URI delivered by the activity's current launch intent.
    /// A regular launcher intent has no URI. An app registers accepted links in its manifest.
    pub fn launch_uri(&self) -> io::Result<Option<String>> {
        self.access(|env, activity| {
            let intent = env
                .call_method(
                    activity,
                    jni_str!("getIntent"),
                    jni_sig!("()Landroid/content/Intent;"),
                    &[],
                )?
                .l()?;
            if intent.is_null() {
                return Ok(None);
            }
            let uri = env
                .call_method(
                    &intent,
                    jni_str!("getDataString"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            if uri.is_null() {
                return Ok(None);
            }
            Ok(Some(env.cast_local::<JString>(uri)?.to_string()))
        })
    }
    /// Returns the current Android grant for a manifest permission.
    pub fn is_granted(&self, permission: &str) -> io::Result<bool> {
        self.access(|env, a| {
            let perm = env.new_string(permission)?;
            Ok(env
                .call_method(
                    a,
                    jni_str!("checkSelfPermission"),
                    jni_sig!("(Ljava/lang/String;)I"),
                    &[JValue::Object(&perm)],
                )?
                .i()?
                == 0)
        })
    }
    /// Pulses this device's vibrator using the default amplitude, when hardware is present.
    /// The app declares android.permission.VIBRATE.
    pub fn vibrate(&self, millis: u32) -> io::Result<bool> {
        if !(1..=30_000).contains(&millis) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "vibration duration must be 1..=30000 ms",
            ));
        }
        self.play_vibration(|env| {
            env.call_static_method(
                jni_str!("android/os/VibrationEffect"),
                jni_str!("createOneShot"),
                jni_sig!("(JI)Landroid/os/VibrationEffect;"),
                &[JValue::Long(i64::from(millis)), JValue::Int(-1)],
            )?
            .l()
        })
    }
    /// Plays a finite pattern, subject to the operating system's vibration settings.
    pub fn vibrate_waveform(&self, steps: &[HapticPulse]) -> io::Result<bool> {
        validate_waveform(steps)?;
        self.play_vibration(|env| {
            let times = env.new_long_array(steps.len())?;
            let levels = env.new_int_array(steps.len())?;
            times.set_region(
                env,
                0,
                &steps
                    .iter()
                    .map(|s| i64::from(s.millis))
                    .collect::<Vec<_>>(),
            )?;
            levels.set_region(
                env,
                0,
                &steps
                    .iter()
                    .map(|s| i32::from(s.amplitude))
                    .collect::<Vec<_>>(),
            )?;
            env.call_static_method(
                jni_str!("android/os/VibrationEffect"),
                jni_str!("createWaveform"),
                jni_sig!("([J[II)Landroid/os/VibrationEffect;"),
                &[
                    JValue::Object(&times),
                    JValue::Object(&levels),
                    JValue::Int(-1),
                ],
            )?
            .l()
        })
    }

    fn play_vibration(
        &self,
        build: impl for<'local> FnOnce(&mut Env<'local>) -> JniResult<JObject<'local>>,
    ) -> io::Result<bool> {
        self.access(|env, a| {
            let svc = env.new_string("vibrator")?;
            let vibrator = env
                .call_method(
                    a,
                    jni_str!("getSystemService"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[JValue::Object(&svc)],
                )?
                .l()?;
            if vibrator.is_null()
                || !env
                    .call_method(&vibrator, jni_str!("hasVibrator"), jni_sig!("()Z"), &[])?
                    .z()?
            {
                return Ok(false);
            }
            let effect = build(env)?;
            env.call_method(
                &vibrator,
                jni_str!("vibrate"),
                jni_sig!("(Landroid/os/VibrationEffect;)V"),
                &[JValue::Object(&effect)],
            )?;
            Ok(true)
        })
    }
    /// Writes a text clip to the OS clipboard.
    pub fn set_clipboard_text(&self, label: &str, text: &str) -> io::Result<()> {
        self.access(|env,a|{
            let name=env.new_string("clipboard")?;
            let clipboard=env.call_method(a,jni_str!("getSystemService"),jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),&[JValue::Object(&name)])?.l()?;
            let label=env.new_string(label)?;let text=env.new_string(text)?;
            let clip=env.call_static_method(jni_str!("android/content/ClipData"),jni_str!("newPlainText"),jni_sig!("(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"),&[JValue::Object(&label),JValue::Object(&text)])?.l()?;
            env.call_method(&clipboard,jni_str!("setPrimaryClip"),jni_sig!("(Landroid/content/ClipData;)V"),&[JValue::Object(&clip)])?;
            Ok(())
        })
    }
    /// Reads the current text clip when the OS grants this foreground activity access.
    pub fn clipboard_text(&self) -> io::Result<Option<String>> {
        self.access(|env, a| {
            let name = env.new_string("clipboard")?;
            let clipboard = env
                .call_method(
                    a,
                    jni_str!("getSystemService"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[JValue::Object(&name)],
                )?
                .l()?;
            let clip = env
                .call_method(
                    &clipboard,
                    jni_str!("getPrimaryClip"),
                    jni_sig!("()Landroid/content/ClipData;"),
                    &[],
                )?
                .l()?;
            if clip.is_null()
                || env
                    .call_method(&clip, jni_str!("getItemCount"), jni_sig!("()I"), &[])?
                    .i()?
                    == 0
            {
                return Ok(None);
            }
            let item = env
                .call_method(
                    &clip,
                    jni_str!("getItemAt"),
                    jni_sig!("(I)Landroid/content/ClipData$Item;"),
                    &[JValue::Int(0)],
                )?
                .l()?;
            let chars = env
                .call_method(
                    &item,
                    jni_str!("getText"),
                    jni_sig!("()Ljava/lang/CharSequence;"),
                    &[],
                )?
                .l()?;
            if chars.is_null() {
                return Ok(None);
            }
            let text = env
                .call_method(
                    &chars,
                    jni_str!("toString"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            Ok(Some(env.cast_local::<JString>(text)?.to_string()))
        })
    }
    /// Presents the Android chooser for sharing plain text.
    pub fn share_text(&self, title: &str, text: &str) -> io::Result<()> {
        self.access(|env, a| {
            let action = env.new_string("android.intent.action.SEND")?;
            let intent = env.new_object(
                jni_str!("android/content/Intent"),
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(&action)],
            )?;
            let mime = env.new_string("text/plain")?;
            env.call_method(
                &intent,
                jni_str!("setType"),
                jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
                &[JValue::Object(&mime)],
            )?;
            let extra = env.new_string("android.intent.extra.TEXT")?;
            let text = env.new_string(text)?;
            env.call_method(
                &intent,
                jni_str!("putExtra"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;"),
                &[JValue::Object(&extra), JValue::Object(&text)],
            )?;
            let title = env.new_string(title)?;
            let chooser = env
                .call_static_method(
                    jni_str!("android/content/Intent"),
                    jni_str!("createChooser"),
                    jni_sig!(
                        "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;"
                    ),
                    &[JValue::Object(&intent), JValue::Object(&title)],
                )?
                .l()?;
            env.call_method(
                a,
                jni_str!("startActivity"),
                jni_sig!("(Landroid/content/Intent;)V"),
                &[JValue::Object(&chooser)],
            )?;
            Ok(())
        })
    }
}
