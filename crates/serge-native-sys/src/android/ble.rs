//! Initialize btleplug in its own app-owned class loader, including NativeActivity-only APKs.
use super::{AndroidSystem, services::dex_class};
use btleplug::{
    api::Manager as _,
    platform::{Adapter, Manager},
};
use jni::{
    EnvUnowned, NativeMethod, errors::ThrowRuntimeExAndDefault, jni_sig, jni_str, objects::JClass,
    refs::Global,
};
use std::{io, sync::Mutex};

extern "system" fn initialize<'local>(mut env: EnvUnowned<'local>, _: JClass<'local>) {
    env.with_env(|env| {
        if let Err(error) = btleplug::platform::init(env)
            && !env.exception_check()
        {
            env.throw(error.to_string())?;
        }
        Ok::<_, jni::errors::Error>(())
    })
    .resolve::<ThrowRuntimeExAndDefault>();
}

impl AndroidSystem {
    /// Opens the Android BLE central. Scanning and connecting use the OS runtime permissions.
    pub async fn bluetooth_adapters(&self) -> io::Result<Vec<Adapter>> {
        static BOOTSTRAP: Mutex<(Option<Global<JClass<'static>>>, bool)> =
            Mutex::new((None, false));
        {
            let mut state = BOOTSTRAP
                .lock()
                .map_err(|_| io::Error::other("BLE initialization stopped"))?;
            if !state.1 {
                self.access(|env, activity| {
                    if state.0.is_none() {
                        let class = dex_class(
                            env,
                            activity,
                            include_bytes!("../../android/ble/adapter.dex"),
                            "org.hyperish.serge.BleBootstrap",
                        )?;
                        // SAFETY: the included Java class declares static native void initialize().
                        // The C system function above has that exact JNI ABI.
                        #[allow(unsafe_code)]
                        unsafe {
                            env.register_native_methods(
                                &class,
                                &[NativeMethod::from_raw_parts(
                                    jni_str!("initialize"),
                                    jni_str!("()V"),
                                    initialize as *mut std::ffi::c_void,
                                )],
                            )?;
                        }
                        state.0 = Some(class);
                    }
                    env.call_static_method(
                        state.0.as_ref().unwrap(),
                        jni_str!("initialize"),
                        jni_sig!("()V"),
                        &[],
                    )?;
                    Ok(())
                })?;
                state.1 = true;
            }
        }
        Manager::new()
            .await
            .map_err(io::Error::other)?
            .adapters()
            .await
            .map_err(io::Error::other)
    }
}
