//! Native desktop operations. Keep the clipboard owner alive while other
//! applications read it on X11/Wayland. File panels return the user's selection.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod desktop {
    pub use arboard::{Clipboard, Error as ClipboardError};
    pub use rfd::{
        AsyncFileDialog, AsyncMessageDialog, FileDialog, FileHandle, MessageButtons, MessageDialog,
        MessageDialogResult, MessageLevel,
    };
    use std::{io, path::PathBuf};

    /// Creates and returns an application's local data directory using the OS convention.
    /// Supply a reverse-domain qualifier, organization and application name.
    pub fn data_directory(
        qualifier: &str,
        organization: &str,
        application: &str,
    ) -> io::Result<PathBuf> {
        for part in [qualifier, organization, application] {
            if part.is_empty()
                || matches!(part, "." | "..")
                || part.contains(['/', '\\', ':'])
                || part.chars().any(char::is_control)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "expected a single application name component",
                ));
            }
        }
        let project = directories::ProjectDirs::from(qualifier, organization, application)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "local application data is unavailable",
                )
            })?;
        let path = project.data_local_dir().to_path_buf();
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }
}
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub use desktop::*;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::{
    AndroidSystem, BackgroundStatus, BackgroundTask, Credential, Fold, FoldState, Prompt,
    PromptResult, WidgetUpdate, WindowPosture,
};
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple_haptics;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple_notifications;
#[cfg(any(target_os = "android", target_os = "ios", target_os = "macos"))]
mod haptics;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use apple_notifications::AppleNotifications;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod audio;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
mod motion;
#[cfg(target_os = "ios")]
pub use ios::{
    BackgroundTask, IosDateTimeRequest, IosDateTimeSelection, IosDeepLinks, IosDocumentRequest,
    IosDocumentResult, IosPrompt, IosSystem, SelectedAction,
};
#[cfg(any(target_os = "android", target_os = "ios", target_os = "macos"))]
mod notification;
#[cfg(any(target_os = "android", target_os = "ios"))]
mod sample;
#[cfg(any(target_os = "android", target_os = "ios"))]
mod stylus;
/// Native credential store entry. Uses Keychain, Windows Credential Manager or
/// the user's Secret Service, depending on the platform.
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "windows"
))]
pub use keyring::{Entry as Credential, Error as CredentialError};
#[cfg(any(target_os = "android", target_os = "ios", target_os = "macos"))]
pub use notification::{NotificationAction, NotificationCategory, NotificationResponse};
/// Device input and haptics for supported native platforms.
pub mod hw {
    #[cfg(target_os = "android")]
    pub use crate::android::StylusPad;
    #[cfg(target_os = "ios")]
    pub use crate::ios::StylusPad;
    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub use crate::stylus::{StylusArea, StylusBatch, StylusPhase, StylusSample};
    /// Imports Android Camera2 buffers into a Vulkan device.
    #[cfg(all(target_os = "android", feature = "vulkan-camera"))]
    pub mod vulkan_camera;
    /// MIDI 1.0 ports, owned input/output connections and timestamped messages.
    #[cfg(all(
        feature = "midi",
        any(
            target_os = "android",
            target_os = "ios",
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        )
    ))]
    pub use midir as midi;
    /// GATT central events, service discovery, characteristic I/O and subscriptions.
    /// On Android, acquire adapters through AndroidSystem::bluetooth_adapters.
    #[cfg(all(
        feature = "bluetooth",
        any(
            target_os = "android",
            target_os = "ios",
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        )
    ))]
    pub mod ble {
        #[cfg(not(target_os = "android"))]
        pub use btleplug::platform::Manager;
        pub use btleplug::platform::{Adapter, Peripheral};
        pub use btleplug::{Error, Result, api};
    }
    #[cfg(target_os = "android")]
    pub use crate::android::{
        CameraBufferFrame, CameraBufferReader, CameraFrame, CameraTexture, Microphone, Sensor,
        SensorKind, SensorSample, SigningKey,
    };
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    pub use crate::apple::{Biometrics, EnclaveAccessibility, SecureEnclaveKey};
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    pub use crate::apple_haptics::AppleHaptics;
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    pub use crate::audio::Microphone;
    #[cfg(any(target_os = "android", target_os = "ios", target_os = "macos"))]
    pub use crate::haptics::HapticPulse;
    #[cfg(target_os = "ios")]
    pub use crate::{
        motion::Sensor,
        sample::{SensorKind, SensorSample},
    };
}
