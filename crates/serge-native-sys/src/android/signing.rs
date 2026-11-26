//! AndroidKeyStore P-256 signing with an explicitly selectable hardware requirement.
use super::{AndroidSystem, services::class};
use jni::{
    jni_sig, jni_str,
    objects::{JByteArray, JObject, JValue},
    refs::Global,
};
use std::io;

/// A named P-256 key that remains in AndroidKeyStore when the handle is dropped.
pub struct SigningKey {
    system: AndroidSystem,
    key: Global<JObject<'static>>,
}

impl SigningKey {
    /// Reopens the same app-scoped identity, generating it on first use.
    /// A hardware requirement is checked for both new and existing keys.
    pub fn open(system: &AndroidSystem, name: &str, require_hardware: bool) -> io::Result<Self> {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected an identity name of 1 to 128 letters, digits, dots, underscores or hyphens",
            ));
        }
        let key = system.access(|env, activity| {
            let api = class(env, activity)?;
            let name = env.new_string(name)?;
            let handle = env.call_static_method(api,jni_str!("signingKey"),jni_sig!("(Landroid/content/Context;Ljava/lang/String;Z)Lorg/hyperish/serge/SigningKey;"),
                &[JValue::Object(activity),JValue::Object(&name),JValue::Bool(require_hardware)])?.l()?;
            env.new_global_ref(handle)
        })?;
        Ok(Self {
            system: system.clone(),
            key,
        })
    }
    /// Returns the OS-reported hardware protection of this specific private key.
    pub fn hardware_backed(&self) -> io::Result<bool> {
        self.system.access(|env, _| {
            env.call_method(&self.key, jni_str!("hardware"), jni_sig!("()Z"), &[])?
                .z()
        })
    }
    /// Exports only the X.509 SubjectPublicKeyInfo DER.
    pub fn public_key(&self) -> io::Result<Vec<u8>> {
        self.system.access(|env, _| {
            let result = env
                .call_method(&self.key, jni_str!("publicKey"), jni_sig!("()[B"), &[])?
                .l()?;
            let bytes = env.cast_local::<JByteArray>(result)?;
            env.convert_byte_array(bytes)
        })
    }
    /// ECDSA/SHA-256 over the supplied message. Returns a DER signature.
    pub fn sign(&self, message: &[u8]) -> io::Result<Vec<u8>> {
        self.system.access(|env, _| {
            let input = env.byte_array_from_slice(message)?;
            let result = env
                .call_method(
                    &self.key,
                    jni_str!("sign"),
                    jni_sig!("([B)[B"),
                    &[JValue::Object(&input)],
                )?
                .l()?;
            let bytes = env.cast_local::<JByteArray>(result)?;
            env.convert_byte_array(bytes)
        })
    }
    /// Verifies an ECDSA/SHA-256 signature using this identity's public key.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> io::Result<bool> {
        self.system.access(|env, _| {
            let input = env.byte_array_from_slice(message)?;
            let signature = env.byte_array_from_slice(signature)?;
            env.call_method(
                &self.key,
                jni_str!("verify"),
                jni_sig!("([B[B)Z"),
                &[JValue::Object(&input), JValue::Object(&signature)],
            )?
            .z()
        })
    }
    /// Deletes this identity. Refuses to delete a replacement generated under the same name.
    pub fn delete(self) -> io::Result<()> {
        self.system.access(|env, _| {
            env.call_method(&self.key, jni_str!("delete"), jni_sig!("()V"), &[])?;
            Ok(())
        })
    }
}
