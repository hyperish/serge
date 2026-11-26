//! Typed access to the app's generated Kotlin widget bridge.
use super::{AndroidSystem, services::class};
use jni::{
    Env,
    errors::Result,
    jni_sig, jni_str,
    objects::{JClass, JObject, JValue},
    refs::Global,
};
use std::io;

fn widget_class<'local>(env: &mut Env<'local>, activity: &JObject) -> Result<JClass<'local>> {
    let services = class(env, activity)?;
    let class = env
        .call_static_method(
            services,
            jni_str!("widgetBridge"),
            jni_sig!("(Landroid/app/Activity;)Ljava/lang/Class;"),
            &[JValue::Object(activity)],
        )?
        .l()?;
    env.cast_local::<JClass>(class)
}

/// A pending Glance update. The result counts the updated OS widget instances.
pub struct WidgetUpdate {
    system: AndroidSystem,
    future: Global<JObject<'static>>,
}
impl WidgetUpdate {
    /// Returns None until the OS update finishes, or an error if it failed.
    pub fn try_result(&self) -> io::Result<Option<i32>> {
        self.system.access(|env, _| {
            if !env
                .call_method(&self.future, jni_str!("isDone"), jni_sig!("()Z"), &[])?
                .z()?
            {
                return Ok(None);
            }
            let result = env
                .call_method(
                    &self.future,
                    jni_str!("get"),
                    jni_sig!("()Ljava/lang/Object;"),
                    &[],
                )?
                .l()?;
            Ok(Some(
                env.call_method(&result, jni_str!("intValue"), jni_sig!("()I"), &[])?
                    .i()?,
            ))
        })
    }
}
impl Drop for WidgetUpdate {
    fn drop(&mut self) {
        let _ = self.system.access(|env, _| {
            env.call_method(
                &self.future,
                jni_str!("cancel"),
                jni_sig!("(Z)Z"),
                &[JValue::Bool(true)],
            )?;
            Ok(())
        });
    }
}

impl AndroidSystem {
    /// Publish projected JSON to one home-screen widget, or all instances of its kind.
    pub fn update_widget(
        &self,
        kind: &str,
        json: &str,
        id: Option<i32>,
    ) -> io::Result<WidgetUpdate> {
        if id.is_some_and(|n| n <= 0) || json.len() > 65536 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid widget id or state size",
            ));
        }
        let future=self.access(|env, a| {
            let class=widget_class(env,a)?;
            let kind=env.new_string(kind)?;
            let json=env.new_string(json)?;
            let future=env.call_static_method(&class,jni_str!("update"),jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;I)Ljava/util/concurrent/Future;"),
                &[JValue::Object(a),JValue::Object(&kind),JValue::Object(&json),JValue::Int(id.unwrap_or(-1))])?.l()?;
            env.new_global_ref(future)
        })?;
        Ok(WidgetUpdate {
            system: self.clone(),
            future,
        })
    }
    /// Post or update an ongoing notification. False means OS notifications are disabled.
    pub fn ongoing_notification(
        &self,
        kind: &str,
        json: &str,
        id: i32,
        channel: &str,
        label: &str,
        small_icon: i32,
    ) -> io::Result<bool> {
        self.access(|env, a| {
            let class=widget_class(env,a)?;
            let kind=env.new_string(kind)?;
            let json=env.new_string(json)?;
            let channel=env.new_string(channel)?;
            let label=env.new_string(label)?;
            env.call_static_method(&class,jni_str!("notify"),jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;ILjava/lang/String;Ljava/lang/String;I)Z"),
                &[JValue::Object(a), JValue::Object(&kind),JValue::Object(&json),JValue::Int(id),JValue::Object(&channel),JValue::Object(&label),JValue::Int(small_icon)])?.z()
        })
    }
    /// Remove the identified notification of this kind.
    pub fn cancel_notification(&self, kind: &str, id: i32) -> io::Result<()> {
        self.access(|env, a| {
            let class = widget_class(env, a)?;
            let kind = env.new_string(kind)?;
            env.call_static_method(
                &class,
                jni_str!("cancel"),
                jni_sig!("(Landroid/content/Context;Ljava/lang/String;I)V"),
                &[JValue::Object(a), JValue::Object(&kind), JValue::Int(id)],
            )?;
            Ok(())
        })
    }
}
