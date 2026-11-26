use crate::DesktopError;
use winit::window::Window;

/// Shows 0 through 100 percent, or clears the indicator with `None`.
/// Call from the view using `WindowHandle::with_native` after the window is displayed.
/// macOS shows a Dock badge for the app, Windows updates the window's taskbar button,
/// and compatible Linux launchers match the supplied installed `.desktop` filename.
pub fn set_task_progress(
    window: &Window,
    desktop_entry: &str,
    percent: Option<u8>,
) -> Result<(), DesktopError> {
    if percent.is_some_and(|n| n > 100) {
        return Err(DesktopError::Platform(
            "task progress must be 0 through 100".into(),
        ));
    }
    platform(window, desktop_entry, percent)
}

#[cfg(target_os = "macos")]
fn platform(_: &Window, _: &str, percent: Option<u8>) -> Result<(), DesktopError> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;
    let main = MainThreadMarker::new()
        .ok_or_else(|| DesktopError::Platform("Dock access requires the UI thread".into()))?;
    let tile = NSApplication::sharedApplication(main).dockTile();
    let value = percent.map(|n| NSString::from_str(&format!("{n}%")));
    tile.setBadgeLabel(value.as_deref());
    tile.display();
    Ok(())
}

#[cfg(target_os = "linux")]
fn platform(_: &Window, desktop_entry: &str, percent: Option<u8>) -> Result<(), DesktopError> {
    use std::{cell::RefCell, collections::HashMap};
    use zbus::{blocking::Connection, zvariant::Value};
    thread_local! { static BUS: RefCell<Option<Connection>> = const { RefCell::new(None) }; }
    let valid = desktop_entry.strip_suffix(".desktop").is_some_and(|stem| {
        !stem.is_empty()
            && stem
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    });
    if !valid {
        return Err(DesktopError::Platform(
            "expected a desktop entry filename".into(),
        ));
    }
    let uri = format!("application://{desktop_entry}");
    let properties = HashMap::from([
        ("progress-visible", Value::Bool(percent.is_some())),
        (
            "progress",
            Value::F64(f64::from(percent.unwrap_or(0)) / 100.),
        ),
    ]);
    BUS.with_borrow_mut(|saved| {
        if saved.is_none() {
            *saved =
                Some(Connection::session().map_err(|e| DesktopError::Platform(e.to_string()))?);
        }
        let conn = saved.as_ref().expect("session connection established");
        let result = conn.emit_signal(
            None::<&str>,
            "/org/hyperish/Serge/LauncherEntry",
            "com.canonical.Unity.LauncherEntry",
            "Update",
            &(uri, properties),
        );
        if result.is_err() {
            *saved = None;
        }
        result.map_err(|e| DesktopError::Platform(e.to_string()))
    })
}

#[cfg(target_os = "windows")]
fn platform(window: &Window, _: &str, percent: Option<u8>) -> Result<(), DesktopError> {
    use windows::Win32::{
        Foundation::{HWND, RPC_E_CHANGED_MODE},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::Shell::{ITaskbarList3, TBPF_NOPROGRESS, TaskbarList},
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window
        .window_handle()
        .map_err(|e| DesktopError::Platform(e.to_string()))?;
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return Err(DesktopError::Platform("expected a Win32 window".into()));
    };
    let hwnd = HWND(raw.hwnd.get() as *mut _);
    // SAFETY: the borrowed window keeps HWND valid. Create and release COM objects on this
    // thread. Balance each successful COM initialization, including S_FALSE.
    #[allow(unsafe_code)]
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
            return Err(DesktopError::Platform(initialized.message()));
        }
        let result = (|| -> windows::core::Result<()> {
            let list: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)?;
            list.HrInit()?;
            match percent {
                Some(n) => list.SetProgressValue(hwnd, u64::from(n), 100),
                None => list.SetProgressState(hwnd, TBPF_NOPROGRESS),
            }
        })();
        if initialized.is_ok() {
            CoUninitialize();
        }
        result.map_err(|e| DesktopError::Platform(e.to_string()))
    }
}
