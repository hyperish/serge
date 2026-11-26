//! Native menus and status items attached to the application's event loop.
#[cfg(target_os = "macos")]
mod popover;
mod progress;
#[cfg(target_os = "linux")]
mod x11;
use super::{DesktopError, Proxy, Wake, WindowSpec};
pub use global_hotkey::hotkey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use menu::{Menu, MenuEvent, MenuEventHandler, MenuItemKind};
#[cfg(target_os = "macos")]
pub use popover::MenuBarPopover;
pub use progress::set_task_progress;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
pub use tray_icon::{Icon, menu};
use tray_icon::{TrayIcon, TrayIconBuilder};
#[cfg(target_os = "linux")]
pub(super) use x11::WindowMenus;
#[cfg(target_os = "windows")]
thread_local! {
    static WINDOW_MENUS: std::cell::RefCell<HashMap<isize, Menu>> = std::cell::RefCell::default();
}
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub(super) fn translate_accelerator(message: *const std::ffi::c_void) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, HACCEL, MSG, TranslateAcceleratorW, WM_KEYDOWN, WM_SYSKEYDOWN,
    };
    // SAFETY: winit supplies a live MSG from the event loop on this thread.
    let Some(msg) = (unsafe { message.cast::<MSG>().as_ref() }) else {
        return false;
    };
    if !matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN) {
        return false;
    }
    // SAFETY: only the top-level HWND registered by attach can supply an accelerator table.
    let root = unsafe { GetAncestor(msg.hwnd, GA_ROOT) };
    let table = WINDOW_MENUS.with_borrow(|menus| menus.get(&(root.0 as isize)).map(Menu::haccel));
    table
        .is_some_and(|h| h != 0 && unsafe { TranslateAcceleratorW(root, HACCEL(h as _), msg) } != 0)
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

pub(super) type Factory<'a> = Box<dyn FnOnce() -> Result<DesktopSystem<'a>, DesktopError> + 'a>;

/// Application status item with a native command menu.
pub struct Tray {
    /// Icon pixels and dimensions checked by [`Icon::from_rgba`].
    pub icon: Icon,
    /// Accessible text for the status item.
    pub tooltip: String,
    /// Commands available from the status item.
    pub menu: Menu,
}
/// One system configuration for all windows in the application.
pub struct DesktopSystem<'a> {
    /// Native menu bar on macOS, Windows and Linux X11.
    pub menu_bar: Option<Menu>,
    /// Right-click menu on macOS, Windows and Linux X11.
    pub context_menu: Option<Menu>,
    /// Status item on macOS, Windows and compatible Linux D-Bus desktops.
    pub tray: Option<Tray>,
    /// A status-bar button that opens an AppKit view controller in a transient popover.
    #[cfg(target_os = "macos")]
    pub menu_bar_popover: Option<MenuBarPopover>,
    /// System-wide keyboard shortcuts and their command IDs.
    pub shortcuts: Vec<(hotkey::HotKey, String)>,
    action: Box<dyn FnMut(&str) + 'a>,
}
impl<'a> DesktopSystem<'a> {
    /// Receives native menu IDs on the UI thread in the focused window's view.
    pub fn new(action: impl FnMut(&str) + 'a) -> Self {
        Self {
            menu_bar: None,
            context_menu: None,
            tray: None,
            #[cfg(target_os = "macos")]
            menu_bar_popover: None,
            shortcuts: Vec::new(),
            action: Box::new(action),
        }
    }
    pub(super) fn install(self, proxy: &Proxy) -> Result<Installed<'a>, DesktopError> {
        #[cfg(target_os = "linux")]
        if self.menu_bar.is_some() || self.context_menu.is_some() {
            gtk::init().map_err(|error| DesktopError::Platform(error.to_string()))?;
        }
        let shortcuts = Shortcuts::install(&self.shortcuts, proxy)?;
        let mut ids = HashSet::new();
        for m in [
            self.menu_bar.as_ref(),
            self.context_menu.as_ref(),
            self.tray.as_ref().map(|t| &t.menu),
        ]
        .into_iter()
        .flatten()
        {
            collect(m.items(), &mut ids);
        }
        let tray = self
            .tray
            .map(|t| {
                TrayIconBuilder::new()
                    .with_icon(t.icon)
                    .with_menu(Box::new(t.menu))
                    .with_tooltip(t.tooltip)
                    .build()
            })
            .transpose()
            .map_err(|e| DesktopError::Platform(e.to_string()))?;
        #[cfg(target_os = "macos")]
        if let Some(menu) = &self.menu_bar {
            menu.init_for_nsapp();
        }
        let previous = MenuEvent::set_event_handler(None);
        let forward = previous.clone();
        let proxy = proxy.clone();
        MenuEvent::set_event_handler(Some(Arc::new(move |event| {
            if ids.contains(event.id.as_ref()) {
                if let Ok(p) = proxy.lock() {
                    let _ = p.send_event(Wake::Menu(event.id.0));
                }
            } else if let Some(f) = &forward {
                f(event);
            }
        })));
        Ok(Installed {
            bar: self.menu_bar,
            context: self.context_menu,
            _tray: tray,
            #[cfg(target_os = "macos")]
            _popover: self.menu_bar_popover,
            _shortcuts: shortcuts,
            previous,
            action: self.action,
        })
    }
}

struct Shortcuts {
    _manager: GlobalHotKeyManager,
    stop: crossbeam_channel::Sender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Shortcuts {
    fn install(
        bindings: &[(hotkey::HotKey, String)],
        proxy: &Proxy,
    ) -> Result<Option<Self>, DesktopError> {
        if bindings.is_empty() {
            return Ok(None);
        }
        let manager =
            GlobalHotKeyManager::new().map_err(|e| DesktopError::Platform(e.to_string()))?;
        let mut commands = HashMap::new();
        for (key, command) in bindings {
            if command.is_empty() || commands.insert(key.id(), command.clone()).is_some() {
                return Err(DesktopError::Platform(
                    "duplicate global shortcut or empty command".into(),
                ));
            }
            manager
                .register(*key)
                .map_err(|e| DesktopError::Platform(e.to_string()))?;
        }
        let (stop, done) = crossbeam_channel::bounded(1);
        let proxy = proxy.clone();
        let worker = std::thread::Builder::new().name("serge-shortcuts".into()).spawn(move || {
            let events = GlobalHotKeyEvent::receiver();
            loop {
                crossbeam_channel::select! {
                    recv(done) -> _ => break,
                    recv(events) -> event => {
                        let Ok(e) = event else { break };
                        if e.state == HotKeyState::Pressed && let Some(command) = commands.get(&e.id)
                            && let Ok(p) = proxy.lock() {
                            let _ = p.send_event(Wake::Menu(command.clone()));
                        }
                    }
                }
            }
        }).map_err(|e| DesktopError::Platform(e.to_string()))?;
        Ok(Some(Self {
            _manager: manager,
            stop,
            worker: Some(worker),
        }))
    }
}
impl Drop for Shortcuts {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn collect(items: Vec<MenuItemKind>, ids: &mut HashSet<String>) {
    for item in items {
        ids.insert(item.id().0.clone());
        if let MenuItemKind::Submenu(submenu) = item {
            collect(submenu.items(), ids);
        }
    }
}

/// Opens native windows and builds their system UI on the event-loop thread.
pub fn run<'a>(
    windows: Vec<WindowSpec<'a>>,
    build: impl FnOnce() -> Result<DesktopSystem<'a>, DesktopError> + 'a,
) -> Result<(), DesktopError> {
    super::run_configured(windows, Some(Box::new(build)))
}
pub(super) struct Installed<'a> {
    bar: Option<Menu>,
    context: Option<Menu>,
    _tray: Option<TrayIcon>,
    #[cfg(target_os = "macos")]
    _popover: Option<MenuBarPopover>,
    _shortcuts: Option<Shortcuts>,
    previous: Option<MenuEventHandler>,
    action: Box<dyn FnMut(&str) + 'a>,
}
impl Installed<'_> {
    pub(super) fn dispatch(&mut self, id: &str) {
        (self.action)(id);
    }
    #[cfg(target_os = "windows")]
    #[allow(unsafe_code)]
    pub(super) fn attach(&self, window: &Window) -> Result<(), DesktopError> {
        if let Some(menu) = &self.bar {
            let handle = window
                .window_handle()
                .map_err(|e| DesktopError::Platform(e.to_string()))?;
            if let RawWindowHandle::Win32(win) = handle.as_raw() {
                // SAFETY: winit owns this live HWND on the same event-loop thread.
                unsafe { menu.init_for_hwnd(win.hwnd.get()) }
                    .map_err(|e| DesktopError::Platform(e.to_string()))?;
                WINDOW_MENUS.with_borrow_mut(|menus| menus.insert(win.hwnd.get(), menu.clone()));
            }
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    #[allow(unsafe_code)]
    pub(super) fn detach(&self, window: &Window) {
        if let (Some(menu), Ok(handle)) = (&self.bar, window.window_handle())
            && let RawWindowHandle::Win32(win) = handle.as_raw()
        {
            WINDOW_MENUS.with_borrow_mut(|menus| menus.remove(&win.hwnd.get()));
            // SAFETY: Desktop calls detach before dropping the corresponding winit window.
            let _ = unsafe { menu.remove_for_hwnd(win.hwnd.get()) };
        }
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[allow(unsafe_code)]
    pub(super) fn context(&self, window: &Window) {
        use menu::ContextMenu;
        let (Some(menu), Ok(handle)) = (&self.context, window.window_handle()) else {
            return;
        };
        // SAFETY: the matching winit native window/view is live and we are on
        // the window's creation thread for the entire modal menu invocation.
        unsafe {
            match handle.as_raw() {
                #[cfg(target_os = "macos")]
                RawWindowHandle::AppKit(w) => {
                    menu.show_context_menu_for_nsview(w.ns_view.as_ptr(), None);
                }
                #[cfg(target_os = "windows")]
                RawWindowHandle::Win32(w) => {
                    menu.show_context_menu_for_hwnd(w.hwnd.get(), None);
                }
                _ => {}
            }
        }
    }
}
impl Drop for Installed<'_> {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        WINDOW_MENUS.with_borrow_mut(HashMap::clear);
        #[cfg(target_os = "macos")]
        if let Some(menu) = &self.bar {
            menu.remove_for_nsapp();
        }
        MenuEvent::set_event_handler(self.previous.take());
    }
}
