use block2::RcBlock;
use objc2::{rc::Retained, runtime::Bool};
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};
use std::{io, sync::mpsc};

/// One device-owner biometric evaluation, cancelled when its owner is dropped.
pub struct Biometrics {
    context: Retained<LAContext>,
    result: mpsc::Receiver<io::Result<()>>,
}
impl Biometrics {
    /// Checks hardware availability and enrollment without showing a system prompt.
    pub fn available() -> bool {
        // SAFETY: LAContext and this policy are available on supported Apple targets.
        #[allow(unsafe_code)]
        unsafe {
            LAContext::new()
                .canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthenticationWithBiometrics)
                .is_ok()
        }
    }
    /// Asks for device-owner biometrics. On Face ID devices the app supplies NSFaceIDUsageDescription.
    pub fn evaluate(reason: &str) -> io::Result<Self> {
        if reason.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "supply a reason for biometric evaluation",
            ));
        }
        let (tx, result) = mpsc::channel();
        // SAFETY: LAContext retains the reply block and owns the private evaluation context.
        // NSError is used only during this callback. Results can be read on any UI tick.
        #[allow(unsafe_code)]
        let context = unsafe {
            let c = LAContext::new();
            c.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthenticationWithBiometrics)
                .map_err(|e| io::Error::new(io::ErrorKind::Unsupported, e.to_string()))?;
            let reply = RcBlock::new(move |success: Bool, error: *mut NSError| {
                let result = if success.as_bool() {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        error
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "authentication failed".into()),
                    ))
                };
                let _ = tx.send(result);
            });
            c.evaluatePolicy_localizedReason_reply(
                LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
                &NSString::from_str(reason),
                &reply,
            );
            c
        };
        Ok(Self { context, result })
    }
    /// Polls once without blocking the UI thread.
    pub fn try_result(&self) -> Option<io::Result<()>> {
        self.result.try_recv().ok()
    }
}
impl Drop for Biometrics {
    fn drop(&mut self) {
        // SAFETY: this context belongs to the operation being cancelled.
        #[allow(unsafe_code)]
        unsafe {
            self.context.invalidate()
        }
    }
}

/// A transient P-256 signing key in the Secure Enclave. Only its public key leaves the device.
pub struct SecureEnclaveKey(security_framework::key::SecKey);

/// Determines when this device's non-exportable private key can sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnclaveAccessibility {
    /// Only while the operating system is unlocked.
    WhileUnlocked,
    /// Allows background use after the first device unlock following boot.
    AfterFirstUnlock,
}

impl SecureEnclaveKey {
    /// Requests an enclave key for this process. Propagates the OS error on unsupported hosts.
    pub fn generate() -> io::Result<Self> {
        Self::generate_with_access(EnclaveAccessibility::WhileUnlocked)
    }
    /// Creates an enclave key under the developer's explicit device-lock policy.
    pub fn generate_with_access(policy: EnclaveAccessibility) -> io::Result<Self> {
        use core_foundation::{
            base::ToVoid, boolean::CFBoolean, dictionary::CFMutableDictionary, number::CFNumber,
        };
        use security_framework::{
            access_control::{ProtectionMode, SecAccessControl},
            key::SecKey,
            passwords_options::AccessControlOptions,
        };
        use security_framework_sys::item::{
            kSecAttrAccessControl, kSecAttrIsPermanent, kSecAttrKeySizeInBits, kSecAttrKeyType,
            kSecAttrKeyTypeECSECPrimeRandom, kSecAttrTokenID, kSecAttrTokenIDSecureEnclave,
            kSecPrivateKeyAttrs,
        };
        let protection = match policy {
            EnclaveAccessibility::WhileUnlocked => {
                ProtectionMode::AccessibleWhenUnlockedThisDeviceOnly
            }
            EnclaveAccessibility::AfterFirstUnlock => {
                ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly
            }
        };
        let access = SecAccessControl::create_with_protection(
            Some(protection),
            AccessControlOptions::PRIVATE_KEY_USAGE.bits(),
        )
        .map_err(|err| io::Error::other(err.to_string()))?;
        let size = CFNumber::from(256);
        // SAFETY: these names are immortal Security framework CFString constants.
        // The dictionaries retain the values. Supply private key attributes explicitly on both Apple targets.
        #[allow(unsafe_code)]
        let attributes = unsafe {
            let private = CFMutableDictionary::from_CFType_pairs(&[
                (kSecAttrAccessControl.to_void(), access.to_void()),
                (
                    kSecAttrIsPermanent.to_void(),
                    CFBoolean::false_value().to_void(),
                ),
            ]);
            CFMutableDictionary::from_CFType_pairs(&[
                (
                    kSecAttrKeyType.to_void(),
                    kSecAttrKeyTypeECSECPrimeRandom.to_void(),
                ),
                (kSecAttrKeySizeInBits.to_void(), size.to_void()),
                (
                    kSecAttrTokenID.to_void(),
                    kSecAttrTokenIDSecureEnclave.to_void(),
                ),
                (kSecPrivateKeyAttrs.to_void(), private.to_void()),
            ])
            .to_immutable()
        };
        #[allow(deprecated)]
        SecKey::generate(attributes)
            .map(Self)
            .map_err(|err| io::Error::other(err.to_string()))
    }
    /// Exports the uncompressed SEC1 public point (65 bytes).
    pub fn public_key(&self) -> io::Result<Vec<u8>> {
        self.0
            .public_key()
            .and_then(|key| key.external_representation())
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| io::Error::other("public key is unavailable"))
    }
    /// Signs the message using ECDSA with SHA-256. The returned signature is DER encoded.
    pub fn sign(&self, message: &[u8]) -> io::Result<Vec<u8>> {
        self.0
            .create_signature(
                security_framework::key::Algorithm::ECDSASignatureMessageX962SHA256,
                message,
            )
            .map_err(|err| io::Error::other(err.to_string()))
    }
}

#[cfg(test)]
mod signing_tests {
    use super::*;
    #[test]
    #[ignore = "requires an available Secure Enclave"]
    fn secure_enclave_signs_and_rejects_a_changed_message() {
        use security_framework::key::Algorithm;
        let key =
            SecureEnclaveKey::generate_with_access(EnclaveAccessibility::AfterFirstUnlock).unwrap();
        let public = key.0.public_key().unwrap();
        let point = key.public_key().unwrap();
        assert_eq!((point.len(), point[0]), (65, 4));
        let signature = key.sign(b"Serge hardware key").unwrap();
        let algorithm = Algorithm::ECDSASignatureMessageX962SHA256;
        assert!(
            public
                .verify_signature(algorithm, b"Serge hardware key", &signature)
                .unwrap()
        );
        assert!(
            !public
                .verify_signature(algorithm, b"changed", &signature)
                .unwrap_or(false)
        );
        assert!(key.0.external_representation().is_none());
    }
}
