use super::{AndroidSystem, services::app_class};
use crate::notification::{NotificationCategory, NotificationResponse, valid_id, validate_message};
use jni::{
    jni_sig, jni_str,
    objects::{JObjectArray, JString, JValue},
};
use std::io;

const CLASS: &str = "org.hyperish.serge.notifications.Actions";
impl AndroidSystem {
    /// Posts or replaces a local notification and its command buttons. Enable Android Cargo
    /// metadata `notifications = true`. Returns false when OS permission or the channel is disabled.
    pub fn post_local_notification(
        &self,
        id: &str,
        title: &str,
        body: &str,
        category: NotificationCategory<'_>,
    ) -> io::Result<bool> {
        category.validate()?;
        validate_message(id, title, body)?;
        self.access(|env, a| {
            let bridge = app_class(env,a,CLASS)?;
            let id = env.new_string(id)?;
            let title = env.new_string(title)?;
            let body = env.new_string(body)?;
            let keys = JObjectArray::<JString>::new(env, category.actions.len(), &JString::null())?;
            let labels = JObjectArray::<JString>::new(env, category.actions.len(), &JString::null())?;
            for (i, action) in category.actions.iter().enumerate() {
                let key = env.new_string(action.id)?;
                let label = env.new_string(action.title)?;
                keys.set_element(env,i,&key)?;
                labels.set_element(env,i,&label)?;
            }
            env.call_static_method(bridge,jni_str!("post"),
                jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;)Z"),
                &[JValue::Object(a),JValue::Object(&id),JValue::Object(&title),JValue::Object(&body),JValue::Object(&keys),JValue::Object(&labels)])?.z()
        })
    }
    /// Cancels the active request and invalidates any previous action token for it.
    pub fn cancel_local_notification(&self, id: &str) -> io::Result<()> {
        if !valid_id(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid notification ID",
            ));
        }
        self.access(|env, a| {
            let bridge = app_class(env, a, CLASS)?;
            let id = env.new_string(id)?;
            env.call_static_method(
                bridge,
                jni_str!("cancel"),
                jni_sig!("(Landroid/content/Context;Ljava/lang/String;)V"),
                &[JValue::Object(a), JValue::Object(&id)],
            )?;
            Ok(())
        })
    }
    /// Takes the oldest unhandled action from the app's private SQLite inbox.
    pub fn take_notification_action(&self) -> io::Result<Option<NotificationResponse>> {
        self.access(|env, a| {
            let bridge = app_class(env, a, CLASS)?;
            let reply = env
                .call_static_method(
                    bridge,
                    jni_str!("take"),
                    jni_sig!("(Landroid/content/Context;)[Ljava/lang/String;"),
                    &[JValue::Object(a)],
                )?
                .l()?;
            if reply.is_null() {
                return Ok(None);
            }
            let pair = env.cast_local::<JObjectArray<JString>>(reply)?;
            Ok(Some(NotificationResponse {
                notification: pair.get_element(env, 0)?.to_string(),
                action: pair.get_element(env, 1)?.to_string(),
            }))
        })
    }
}
