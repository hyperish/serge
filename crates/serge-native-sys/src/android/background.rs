use super::{AndroidSystem, services::app_class};
use jni::{
    jni_sig, jni_str,
    objects::{JObject, JValue},
    refs::Global,
};
use std::{io, time::Duration};

/// Android's response to a request to complete foreground work after leaving the app.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BackgroundStatus {
    /// A start request is in flight.
    Pending,
    /// The service promoted itself and posted its ongoing notification.
    Active,
    /// The duration elapsed, the system stopped the service, or promotion failed.
    Ended,
}

/// Dropping this guard releases the request and stops the service once all requests finish.
pub struct BackgroundTask {
    system: AndroidSystem,
    lease: Global<JObject<'static>>,
}

impl AndroidSystem {
    /// Requests time for user-visible work already in progress. Call from a foreground app.
    /// Enable `background_task = true` in Android Cargo metadata. Overlapping requests share
    /// the first notification and its 165-second maximum lifetime. Check status for OS expiry.
    pub fn background_task(
        &self,
        title: &str,
        message: &str,
        duration: Duration,
    ) -> io::Result<BackgroundTask> {
        let millis = duration.as_millis();
        if title.trim().is_empty() || !(1..=165_000).contains(&millis) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a notification title and 1..=165000 ms",
            ));
        }
        let lease = self.access(|env, activity| {
            let class = app_class(env, activity, "org.hyperish.serge.background.WorkService")?;
            let title = env.new_string(title)?;
            let message = env.new_string(message)?;
            let lease = env.call_static_method(class, jni_str!("begin"),
                jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;J)Lorg/hyperish/serge/background/WorkService$Lease;"),
                &[JValue::Object(activity), JValue::Object(&title), JValue::Object(&message), JValue::Long(millis as i64)])?.l()?;
            env.new_global_ref(lease)
        })?;
        Ok(BackgroundTask {
            system: self.clone(),
            lease,
        })
    }
}

impl BackgroundTask {
    /// Returns the current state without waiting on Android's service lifecycle.
    pub fn status(&self) -> io::Result<BackgroundStatus> {
        let code = self.system.access(|env, _| {
            env.call_method(&self.lease, jni_str!("status"), jni_sig!("()I"), &[])?
                .i()
        })?;
        Ok(match code {
            0 => BackgroundStatus::Pending,
            1 => BackgroundStatus::Active,
            _ => BackgroundStatus::Ended,
        })
    }
}

impl Drop for BackgroundTask {
    fn drop(&mut self) {
        let _ = self.system.access(|env, _| {
            env.call_method(&self.lease, jni_str!("close"), jni_sig!("()V"), &[])?;
            Ok(())
        });
    }
}
