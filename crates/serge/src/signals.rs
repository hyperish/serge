use hyve::serde::{Binary, Fingerprint};
use hyve::state::{Signal, signal};
use hyve::validate::Validate;

/// A signal that can resume across successful `hyve dev` rebuilds.
/// Supply stable instance and field keys and opt in only public UI state.
/// Ordinary and release launches initialize from `initial`.
pub fn dev_signal<T>(instance: &str, key: &str, initial: impl FnOnce() -> T) -> Signal<T>
where
    T: Binary + Fingerprint + Validate + Send + Sync + 'static,
{
    #[cfg(debug_assertions)]
    if let Some(slot) = checkpoint::Slot::development(instance, key) {
        return checkpoint::tracked(slot, initial, true);
    }
    let _ = (instance, key);
    signal(initial())
}

/// Browser signal backed by localStorage. The app supplies a stable namespace
/// and a field key. Values are decoded and validated again on mount.
#[cfg(target_arch = "wasm32")]
pub fn persistent_signal<T>(namespace: &str, key: &str, initial: impl FnOnce() -> T) -> Signal<T>
where
    T: Binary + Fingerprint + Validate + Send + Sync + 'static,
{
    let storage = web_sys::window().and_then(|window| window.local_storage().ok().flatten());
    match storage {
        Some(storage) => checkpoint::tracked(
            checkpoint::Slot::browser(storage, "persistent", namespace, key),
            initial,
            false,
        ),
        None => signal(initial()),
    }
}

#[cfg(any(debug_assertions, target_arch = "wasm32"))]
mod checkpoint {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use hyve::serde::FORMAT_FINGERPRINT;
    use hyve::state::{effect, on_cleanup};
    use std::rc::Rc;

    const MAX_BYTES: usize = 1024 * 1024;

    pub(super) struct Slot {
        key: String,
        #[cfg(target_arch = "wasm32")]
        store: web_sys::Storage,
        #[cfg(not(target_arch = "wasm32"))]
        dir: std::path::PathBuf,
    }

    impl Slot {
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        pub(super) fn development(instance: &str, key: &str) -> Option<Self> {
            use sha2::{Digest, Sha256};
            let dir = std::env::var_os("HYPERISH_SERGE_DEV_STATE")?.into();
            let hash = Sha256::digest(format!("{}:{instance}{key}", instance.len()).as_bytes());
            let key = hash.iter().map(|b| format!("{b:02x}")).collect();
            Some(Self { dir, key })
        }

        #[cfg(all(debug_assertions, target_arch = "wasm32"))]
        pub(super) fn development(instance: &str, key: &str) -> Option<Self> {
            let window = web_sys::window()?;
            let session = window
                .document()?
                .query_selector("meta[name=serge-dev-session]")
                .ok()??
                .get_attribute("content")?;
            Some(Self::browser(
                window.session_storage().ok()??,
                &session,
                instance,
                key,
            ))
        }

        #[cfg(target_arch = "wasm32")]
        pub(super) fn browser(
            store: web_sys::Storage,
            namespace: &str,
            instance: &str,
            field: &str,
        ) -> Self {
            Self {
                store,
                key: format!(
                    "serge/1/{}:{namespace}{}:{instance}{field}",
                    namespace.len(),
                    instance.len()
                ),
            }
        }

        #[cfg(target_arch = "wasm32")]
        fn read(&self) -> Result<Option<String>, String> {
            self.store
                .get_item(&self.key)
                .map_err(|_| "cannot read browser storage".into())
        }

        #[cfg(target_arch = "wasm32")]
        fn write(&self, value: Option<&str>) -> Result<(), String> {
            match value {
                Some(data) => self.store.set_item(&self.key, data),
                None => self.store.remove_item(&self.key),
            }
            .map_err(|_| "cannot write browser storage".into())
        }

        #[cfg(not(target_arch = "wasm32"))]
        fn read(&self) -> Result<Option<String>, String> {
            use std::io::{ErrorKind, Read};
            match std::fs::File::open(self.dir.join(&self.key)) {
                Ok(file) => {
                    let mut text = String::new();
                    file.take(MAX_BYTES as u64 * 2 + 1)
                        .read_to_string(&mut text)
                        .map_err(|e| e.to_string())?;
                    Ok(Some(text))
                }
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        fn write(&self, value: Option<&str>) -> Result<(), String> {
            use std::io::{ErrorKind, Write};
            let dest = self.dir.join(&self.key);
            if let Some(data) = value {
                let mut tmp =
                    tempfile::NamedTempFile::new_in(&self.dir).map_err(|e| e.to_string())?;
                tmp.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
                tmp.persist(dest).map_err(|e| e.to_string())?;
            } else if let Err(e) = std::fs::remove_file(dest)
                && e.kind() != ErrorKind::NotFound
            {
                return Err(e.to_string());
            }
            Ok(())
        }
    }

    fn log(key: &str, error: &str) {
        let msg = format!("serge: state {key}: {error}");
        #[cfg(target_arch = "wasm32")]
        web_sys::console::warn_1(&msg.into());
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("{msg}");
    }

    fn restore<T: Binary + Validate>(data: &str, header: &str) -> Result<T, String> {
        let payload = data
            .strip_prefix(header)
            .ok_or("model or codec fingerprint changed")?;
        if payload.len() > MAX_BYTES.div_ceil(3) * 4 {
            return Err("checkpoint exceeds the size limit".into());
        }
        let bytes = STANDARD
            .decode(payload)
            .map_err(|_| "invalid checkpoint encoding")?;
        if bytes.len() > MAX_BYTES {
            return Err("checkpoint exceeds the size limit".into());
        }
        let value = T::from_binary(&bytes).map_err(|_| "invalid checkpoint bytes")?;
        value
            .validate()
            .map_err(|_| "restored value fails model validation")?;
        Ok(value)
    }

    pub(super) fn tracked<T>(
        slot: Slot,
        initial: impl FnOnce() -> T,
        clear_on_unmount: bool,
    ) -> Signal<T>
    where
        T: Binary + Fingerprint + Validate + Send + Sync + 'static,
    {
        let header = format!("1/{FORMAT_FINGERPRINT:016x}/{:016x}/", T::FINGERPRINT);
        let saved = slot
            .read()
            .and_then(|data| data.map(|s| restore::<T>(&s, &header)).transpose());
        let value = match saved {
            Ok(Some(value)) => value,
            Ok(None) => initial(),
            Err(reason) => {
                log(&slot.key, &reason);
                initial()
            }
        };
        let sig = signal(value);
        let slot = Rc::new(slot);
        if clear_on_unmount {
            let slot = slot.clone();
            on_cleanup(move || {
                if let Err(err) = slot.write(None) {
                    log(&slot.key, &err);
                }
            });
        }
        effect(move || {
            let result = sig.with(|v| {
                if v.validate().is_err() {
                    return slot.write(None);
                }
                let bytes = v.to_binary().map_err(|e| e.to_string())?;
                if bytes.as_ref().len() > MAX_BYTES {
                    slot.write(None)?;
                    return Err("value exceeds the checkpoint size limit".into());
                }
                slot.write(Some(&format!(
                    "{header}{}",
                    STANDARD.encode(bytes.as_ref())
                )))
            });
            if let Err(err) = result {
                log(&slot.key, &err);
            }
        });
        sig
    }
}
