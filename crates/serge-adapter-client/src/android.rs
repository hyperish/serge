//! Installs the Android verifier before making requests with platform trust.
use android_activity::AndroidApp;
use jni::{
    JavaVM,
    errors::Result,
    jni_sig, jni_str,
    objects::{JClassLoader, JObject, JValue},
    refs::Global,
};
use std::sync::atomic::{AtomicBool, Ordering};

static READY: AtomicBool = AtomicBool::new(false);
pub(crate) fn ready() -> bool {
    READY.load(Ordering::Acquire)
}

/// Loads the matching JVM verifier and validates server certificates against Android's system trust store.
/// NativeActivity launch calls this automatically. Custom Android hosts can call
/// it before their first HTTPS request. Only the application context is retained.
pub fn initialize_platform_verifier(app: &AndroidApp) -> Result<()> {
    if ready() {
        return Ok(());
    }
    // SAFETY: AndroidApp keeps the process JVM and activity global reference alive
    // through this synchronous attachment. The activity is borrowed, never deleted.
    #[allow(unsafe_code)]
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| -> Result<()> {
        let raw = app.activity_as_ptr().cast();
        #[allow(unsafe_code)]
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
        let context = env
            .call_method(
                activity,
                jni_str!("getApplicationContext"),
                jni_sig!("()Landroid/content/Context;"),
                &[],
            )?
            .l()?;
        let parent = env
            .call_method(
                &context,
                jni_str!("getClassLoader"),
                jni_sig!("()Ljava/lang/ClassLoader;"),
                &[],
            )?
            .l()?;
        // Java owns this buffer. There is no writable pointer into static Rust memory.
        let bytes =
            env.byte_array_from_slice(include_bytes!("../android/platform-verifier.dex"))?;
        let buffer = env
            .call_static_method(
                jni_str!("java/nio/ByteBuffer"),
                jni_str!("wrap"),
                jni_sig!("([B)Ljava/nio/ByteBuffer;"),
                &[JValue::Object(&bytes)],
            )?
            .l()?;
        let loader = env.new_object(
            jni_str!("dalvik/system/InMemoryDexClassLoader"),
            jni_sig!("(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V"),
            &[JValue::Object(&buffer), JValue::Object(&parent)],
        )?;
        let class = env.new_string("org.rustls.platformverifier.CertificateVerifier")?;
        env.call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[JValue::Object(&class)],
        )?;
        rustls_platform_verifier::android::init_with_refs(
            env.get_java_vm()?,
            env.new_global_ref(context)?,
            env.new_cast_global_ref::<JClassLoader>(loader)?,
        );
        READY.store(true, Ordering::Release);
        Ok(())
    })
}
