//! `serge-platform-desktop` — a desktop window for a serge UI: `winit`
//! (external dependency, `docs/02-architecture.md` §3) for the window and
//! input, the native runtime for layout and events, and a renderer chosen by
//! the fallback policy (`docs/03-adapters-and-pipeline.md` §1.2): the GPU
//! (`serge-native-gpu`) when a surface and device can be created, else CPU
//! frames (`serge-native-cpu`) presented with `softbuffer`.
//! `HYPERISH_RENDER_BACKEND=cpu|gpu` picks one explicitly; an explicit `gpu`
//! that fails is an error.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hyve::{Scope, Ulid};
use serge_core::{Cursor, Element};
use serge_native::{Backend, Key, LocalExecutor, Ui, choose_backend, requested_backend};
use serge_native_cpu::{CpuRenderer, Frame};
use serge_native_gpu::GpuSurface;
pub use serge_native_gpu::viewport;
use serge_style::Theme;
use winit::application::ApplicationHandler;
#[cfg(not(target_os = "ios"))]
use winit::dpi::LogicalSize;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta};
pub use winit::event::{Force, Touch, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};
mod placement;
pub use placement::WindowPlacement;
pub use winit::dpi;

#[cfg(all(
    feature = "system",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
pub mod system;

#[cfg(target_os = "android")]
mod android_access;
#[cfg(not(target_os = "android"))]
use accesskit_winit::Adapter as AccessAdapter;
#[cfg(target_os = "android")]
use android_access::Adapter as AccessAdapter;

/// Window options.
#[derive(Clone, Debug)]
pub struct WindowOptions {
    /// The title.
    pub title: String,
    /// Initial width in logical pixels.
    pub width: f32,
    /// Initial height in logical pixels.
    pub height: f32,
}

impl Default for WindowOptions {
    fn default() -> Self {
        WindowOptions {
            title: "serge".into(),
            width: 960.0,
            height: 720.0,
        }
    }
}

/// Why the window could not run.
#[derive(Debug)]
pub enum DesktopError {
    /// The renderer policy failed (`HYPERISH_RENDER_BACKEND`).
    Backend(serge_native::BackendError),
    /// The platform refused (no display, …).
    Platform(String),
}

impl std::fmt::Display for DesktopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DesktopError::Backend(e) => e.fmt(f),
            DesktopError::Platform(e) => write!(f, "the window could not be created: {e}"),
        }
    }
}

impl std::error::Error for DesktopError {}

enum Wake {
    #[cfg(all(
        feature = "system",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    Menu(String),
    Tasks,
    Access(accesskit_winit::Event),
    Open(
        Ulid,
        WindowOptions,
        WindowPlacement,
        Scope,
        Box<dyn FnOnce() -> Element + Send>,
    ),
    Close(Ulid),
    Focus(Ulid),
}

impl From<accesskit_winit::Event> for Wake {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Access(event)
    }
}

type Proxy = Arc<Mutex<EventLoopProxy<Wake>>>;

/// A window’s controller, available from the view scope on any application thread.
#[derive(Clone)]
pub struct WindowHandle {
    id: Ulid,
    proxy: Proxy,
}
struct NativeWindow {
    window: std::sync::Weak<Window>,
    thread: std::thread::ThreadId,
}
impl WindowHandle {
    /// Borrows the native window of the current view on its event-loop thread.
    /// Native controls attached here should be dropped when the view unmounts.
    pub fn with_native<R>(use_window: impl FnOnce(&Window) -> R) -> Option<R> {
        let window = Scope::current()?.get::<NativeWindow>()?;
        if window.thread != std::thread::current().id() {
            return None;
        }
        Some(use_window(window.window.upgrade()?.as_ref()))
    }

    /// Controller of the window containing the current view.
    pub fn current() -> Option<Self> {
        Some((*Scope::current()?.get::<Self>()?).clone())
    }
    fn send(&self, event: Wake) -> Result<(), DesktopError> {
        self.proxy
            .lock()
            .map_err(|_| DesktopError::Platform("window dispatcher unavailable".into()))?
            .send_event(event)
            .map_err(|_| DesktopError::Platform("desktop event loop is closed".into()))
    }
    /// Opens another native window. Its factory runs on the main thread.
    pub fn open(
        &self,
        options: WindowOptions,
        scope: Scope,
        root: impl FnOnce() -> Element + Send + 'static,
    ) -> Result<Self, DesktopError> {
        self.open_placed(options, WindowPlacement::default(), scope, root)
    }
    /// Opens a window with its own floating level and optional persisted bounds.
    pub fn open_placed(
        &self,
        options: WindowOptions,
        placement: WindowPlacement,
        scope: Scope,
        root: impl FnOnce() -> Element + Send + 'static,
    ) -> Result<Self, DesktopError> {
        let id = Ulid::new();
        self.send(Wake::Open(id, options, placement, scope, Box::new(root)))?;
        Ok(Self {
            id,
            proxy: self.proxy.clone(),
        })
    }
    /// Requests that the OS focus this window.
    pub fn focus(&self) -> Result<(), DesktopError> {
        self.send(Wake::Focus(self.id))
    }
    /// Unmounts this window and cancels its view work.
    pub fn close(&self) -> Result<(), DesktopError> {
        self.send(Wake::Close(self.id))
    }
}

/// Window-local native input and application lifecycle. File drops carry the OS path;
/// multi-touch input preserves touch IDs and calibrated stylus force/altitude when available.
pub enum NativeEvent<'a> {
    /// The surface is available for presentation.
    Resumed,
    /// Release device input and other foreground-only resources.
    Suspended,
    /// Borrowed OS window event before normal UI dispatch.
    Window(&'a WindowEvent),
}
type Observer = Box<dyn FnMut(NativeEvent<'_>)>;

/// Initial window, its application scope, and its main-thread view constructor.
pub struct WindowSpec<'a> {
    options: WindowOptions,
    placement: WindowPlacement,
    scope: Scope,
    root: Box<dyn FnOnce() -> Element + 'a>,
    observer: Option<Observer>,
}
impl<'a> WindowSpec<'a> {
    /// Creates a native window description without opening it.
    pub fn new(options: WindowOptions, scope: Scope, root: impl FnOnce() -> Element + 'a) -> Self {
        Self {
            options,
            placement: WindowPlacement::default(),
            scope,
            root: Box::new(root),
            observer: None,
        }
    }
    /// Configures floating level and restoration for this window.
    pub fn placement(mut self, placement: WindowPlacement) -> Self {
        self.placement = placement;
        self
    }
    /// Receives native file drops, stylus and lifecycle changes on the window's UI thread.
    pub fn on_event(mut self, observer: impl FnMut(NativeEvent<'_>) + 'static) -> Self {
        self.observer = Some(Box::new(observer));
        self
    }
}

/// How a window's frames reach the screen: one presenter per window.
enum Presenter {
    Gpu(Box<GpuSurface>),
    Cpu(Box<CpuPresenter>),
}

/// CPU frames copied into a `softbuffer` surface.
struct CpuPresenter {
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    renderer: CpuRenderer,
    buffer: Vec<u32>,
}

fn surface(window: &Arc<Window>, requested: Option<&str>) -> Result<Presenter, DesktopError> {
    let size = drawable_size(window);
    let mut gpu = None;
    let backend = choose_backend(
        requested,
        false,
        || match GpuSurface::with_software_adapter(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            !cfg!(target_os = "android") || requested == Some("gpu"),
        ) {
            Ok(surface) => {
                gpu = Some(surface);
                Ok(())
            }
            Err(e) => {
                eprintln!("serge: GPU surface unavailable: {e}");
                Err(e.to_string())
            }
        },
    )
    .map_err(DesktopError::Backend)?;
    match backend {
        Backend::Gpu => {
            let surface = gpu.expect("created GPU surface");
            eprintln!("serge: rendering on the GPU ({})", surface.adapter());
            Ok(Presenter::Gpu(Box::new(surface)))
        }
        Backend::Cpu => {
            let context = softbuffer::Context::new(window.clone())
                .map_err(|e| DesktopError::Platform(e.to_string()))?;
            let surface = softbuffer::Surface::new(&context, window.clone())
                .map_err(|e| DesktopError::Platform(e.to_string()))?;
            eprintln!("serge: rendering on the CPU");
            Ok(Presenter::Cpu(Box::new(CpuPresenter {
                surface,
                renderer: CpuRenderer::new(),
                buffer: Vec::new(),
            })))
        }
    }
}

#[cfg(target_os = "ios")]
mod ios;

fn drawable_size(window: &Window) -> winit::dpi::PhysicalSize<u32> {
    #[cfg(target_os = "ios")]
    {
        window.outer_size()
    }
    #[cfg(not(target_os = "ios"))]
    {
        window.inner_size()
    }
}

#[cfg(target_os = "android")]
fn mobile_area(window: &Window) -> Option<serge_core::Rect> {
    use winit::platform::android::WindowExtAndroid;
    let rect = window.content_rect();
    let scale = window.scale_factor() as f32;
    (rect.right > rect.left && rect.bottom > rect.top).then(|| {
        serge_core::Rect::new(
            rect.left as f32 / scale,
            rect.top as f32 / scale,
            (rect.right - rect.left) as f32 / scale,
            (rect.bottom - rect.top) as f32 / scale,
        )
    })
}
#[cfg(target_os = "ios")]
use ios::content as mobile_area;

struct Window_ {
    #[cfg(all(feature = "system", target_os = "linux"))]
    menus: Option<system::WindowMenus>,
    access: AccessAdapter,
    window: Arc<Window>,
    presenter: Option<Presenter>,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    ime: bool,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    content_bounds: Option<serge_core::Rect>,
    ui: Ui,
}

struct App<F> {
    observer: Option<Observer>,
    options: WindowOptions,
    placement: placement::State,
    scope: Scope,
    root: Option<F>,
    exec: LocalExecutor,
    win: Option<Window_>,
    requested: Option<String>,
    pointer: (f32, f32),
    touch: Option<u64>,
    shift: bool,
    error: Option<DesktopError>,
    reported: bool,
    debug_frame: Option<std::path::PathBuf>,
}

/// Reports a presentation problem once per window.
fn report_once(reported: &mut bool, what: std::fmt::Arguments<'_>) {
    if !*reported {
        eprintln!("serge: {what}");
        *reported = true;
    }
}

/// `SERGE_DEBUG_FRAME=<file.ppm>`: the frame just presented, as pixels (to
/// inspect a window where screenshots are not allowed).
fn write_debug_frame(path: &std::path::Path, frame: &Frame) {
    let mut out = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
    for p in frame.rgba.as_chunks::<4>().0 {
        out.extend_from_slice(&p[..3]);
    }
    let _ = std::fs::write(path, out);
}

fn named(key: &WKey, shift: bool) -> Option<Key> {
    match key {
        WKey::Named(NamedKey::Backspace) => Some(Key::Backspace),
        WKey::Named(NamedKey::Enter) => Some(Key::Enter),
        WKey::Named(NamedKey::Tab) => Some(if shift { Key::BackTab } else { Key::Tab }),
        WKey::Named(NamedKey::Space) => Some(Key::Space),
        WKey::Character(s) if s == " " => Some(Key::Space),
        WKey::Named(NamedKey::ArrowUp | NamedKey::ArrowRight) => Some(Key::Step(true)),
        WKey::Named(NamedKey::ArrowDown | NamedKey::ArrowLeft) => Some(Key::Step(false)),
        WKey::Named(NamedKey::Escape) => Some(Key::Escape),
        _ => None,
    }
}

impl<F: FnOnce() -> Element> App<F> {
    fn notify(&mut self, event: NativeEvent<'_>) {
        if let Some(observer) = &mut self.observer {
            self.scope.enter(|| observer(event));
        }
    }

    fn scale(&self) -> f32 {
        self.win
            .as_ref()
            .map_or(1.0, |w| w.window.scale_factor() as f32)
    }

    fn redraw(&mut self) {
        let scale = self.scale();
        let Some(w) = self.win.as_mut() else { return };
        let size = drawable_size(&w.window);
        let (pw, ph) = (size.width.max(1), size.height.max(1));
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let area = mobile_area(&w.window);
        #[cfg(all(feature = "system", target_os = "linux"))]
        let area = w.menus.as_ref().and_then(|m| m.viewport(&w.window));
        #[cfg(not(any(
            target_os = "android",
            target_os = "ios",
            all(feature = "system", target_os = "linux")
        )))]
        let area: Option<serge_core::Rect> = None;
        match area {
            Some(area) => w.ui.set_viewport(area),
            None => w.ui.resize(pw as f32 / scale, ph as f32 / scale),
        }
        let scene = w.ui.frame_at(scale);
        w.access.update_if_active(|| {
            w.ui.accessibility_tree(&self.options.title, f64::from(scale))
        });
        let debug = self.debug_frame.as_deref();
        let Some(presenter) = w.presenter.as_mut() else {
            return;
        };
        match presenter {
            Presenter::Gpu(surface) => match surface.render(&scene, pw, ph, scale, debug.is_some())
            {
                Ok(Some(frame)) => {
                    if let Some(path) = debug {
                        write_debug_frame(path, &frame);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    w.ui.invalidate_surfaces();
                    if self.requested.as_deref() == Some("gpu") {
                        self.error = Some(DesktopError::Platform(e.to_string()));
                    } else {
                        report_once(
                            &mut self.reported,
                            format_args!("GPU presentation failed, switching to CPU: {e}"),
                        );
                        w.presenter.take();
                        match self::surface(&w.window, Some("cpu")) {
                            Ok(p) => {
                                w.presenter = Some(p);
                                w.window.request_redraw();
                            }
                            Err(e) => {
                                self.error = Some(e);
                            }
                        }
                    }
                }
            },
            Presenter::Cpu(cpu) => {
                let CpuPresenter {
                    surface,
                    renderer,
                    buffer,
                } = &mut **cpu;
                let frame = renderer.render(
                    &scene,
                    pw.min(u32::from(u16::MAX)) as u16,
                    ph.min(u32::from(u16::MAX)) as u16,
                    scale,
                );
                frame.to_xrgb(buffer);
                let (Some(nw), Some(nh)) = (
                    NonZeroU32::new(u32::from(frame.width)),
                    NonZeroU32::new(u32::from(frame.height)),
                ) else {
                    return;
                };
                if surface.resize(nw, nh).is_err() {
                    return;
                }
                match surface.buffer_mut() {
                    Ok(mut buf) => {
                        let n = buf.len().min(buffer.len());
                        buf[..n].copy_from_slice(&buffer[..n]);
                        if let Err(e) = buf.present() {
                            report_once(
                                &mut self.reported,
                                format_args!("presenting the frame failed: {e}"),
                            );
                        }
                    }
                    Err(e) => report_once(
                        &mut self.reported,
                        format_args!("no frame buffer for the window: {e}"),
                    ),
                }
                if let Some(path) = debug {
                    write_debug_frame(path, &frame);
                }
            }
        }
    }

    fn after_input(&mut self) {
        if let Some(w) = &mut self.win {
            #[cfg(any(target_os = "android", target_os = "ios"))]
            {
                let wants = w.ui.wants_text_input();
                if w.ime != wants {
                    w.window.set_ime_allowed(wants);
                    w.ime = wants;
                }
            }
            let (x, y) = self.pointer;
            w.window.set_cursor(match w.ui.cursor_at(x, y) {
                Cursor::Pointer => CursorIcon::Pointer,
                Cursor::Text => CursorIcon::Text,
                Cursor::Default => CursorIcon::Default,
            });
            if w.ui.needs_frame() {
                w.window.request_redraw();
            }
        }
    }
}

impl<F: FnOnce() -> Element> App<F> {
    fn mount(&mut self, el: &ActiveEventLoop, proxy: EventLoopProxy<Wake>) {
        if self.win.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_visible(false)
            .with_title(self.options.title.clone());
        // UIKit chooses the window's screen bounds, including rotation and multitasking.
        #[cfg(not(target_os = "ios"))]
        let attrs =
            attrs.with_inner_size(LogicalSize::new(self.options.width, self.options.height));
        let attrs = self.placement.attributes(el, attrs);
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.error = Some(DesktopError::Platform(e.to_string()));
                el.exit();
                return;
            }
        };
        self.placement.place(&window);
        #[cfg(not(target_os = "android"))]
        let access = AccessAdapter::with_event_loop_proxy(el, &window, proxy);
        #[cfg(target_os = "android")]
        let access = match AccessAdapter::new(el, window.id(), proxy) {
            Ok(access) => access,
            Err(e) => {
                self.error = Some(DesktopError::Platform(e.to_string()));
                el.exit();
                return;
            }
        };
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        window.set_ime_allowed(true);
        let presenter = match surface(&window, self.requested.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(e);
                el.exit();
                return;
            }
        };
        let theme = match window.theme() {
            Some(winit::window::Theme::Dark) => Theme::DARK,
            _ => Theme::LIGHT,
        };
        self.scope.provide(NativeWindow {
            window: Arc::downgrade(&window),
            thread: std::thread::current().id(),
        });
        let w2 = window.clone();
        let root = self.root.take().expect("mounted once");
        let ui = Ui::mount(
            self.scope.clone(),
            theme,
            Rc::new(move || w2.request_redraw()),
            root,
        );
        self.exec.run_ready();
        window.request_redraw();
        window.set_visible(true);
        self.win = Some(Window_ {
            #[cfg(all(feature = "system", target_os = "linux"))]
            menus: None,
            access,
            window,
            presenter: Some(presenter),
            #[cfg(any(target_os = "android", target_os = "ios"))]
            ime: false,
            #[cfg(any(target_os = "android", target_os = "ios"))]
            content_bounds: None,
            ui,
        });
    }

    fn input(&mut self, event: WindowEvent) {
        let scale = self.scale();
        let Some(w) = self.win.as_ref() else { return };
        match event {
            WindowEvent::RedrawRequested => {
                self.redraw();
                return;
            }
            WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. }
            | WindowEvent::Occluded(false) => {
                w.window.request_redraw();
                return;
            }
            WindowEvent::CursorMoved {
                position: PhysicalPosition { x, y },
                ..
            } => {
                self.pointer = (x as f32 / scale, y as f32 / scale);
                w.ui.pointer_move(self.pointer.0, self.pointer.1);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = self.pointer;
                match state {
                    ElementState::Pressed => w.ui.pointer_down(x, y),
                    ElementState::Released => w.ui.pointer_up(x, y),
                }
            }
            WindowEvent::Touch(touch) => {
                let x = touch.location.x as f32 / scale;
                let y = touch.location.y as f32 / scale;
                match touch.phase {
                    TouchPhase::Started if self.touch.is_none() => {
                        self.touch = Some(touch.id);
                        w.ui.pointer_down(x, y);
                    }
                    TouchPhase::Moved if self.touch == Some(touch.id) => {
                        w.ui.pointer_move(x, y);
                    }
                    TouchPhase::Ended if self.touch == Some(touch.id) => {
                        self.touch = None;
                        w.ui.pointer_up(x, y);
                    }
                    TouchPhase::Cancelled if self.touch == Some(touch.id) => {
                        self.touch = None;
                        w.ui.pointer_cancel();
                    }
                    _ => return,
                }
                self.pointer = (x, y);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 40.0,
                    MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / scale,
                };
                w.ui.scroll(dy);
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        text,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                if let Some(k) = named(&logical_key, self.shift) {
                    w.ui.key(k);
                } else if let Some(t) = text {
                    w.ui.text_input(&t);
                }
            }
            WindowEvent::Ime(Ime::Commit(t)) => w.ui.text_input(&t),
            WindowEvent::ModifiersChanged(mods) => self.shift = mods.state().shift_key(),
            _ => return,
        }
        self.exec.run_ready();
        self.after_input();
    }
}

type Mounted<'a> = App<Box<dyn FnOnce() -> Element + 'a>>;

struct Desktop<'a> {
    #[cfg(all(
        feature = "system",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    install: Option<system::Factory<'a>>,
    #[cfg(all(
        feature = "system",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    system: Option<system::Installed<'a>>,
    initial: Vec<WindowSpec<'a>>,
    windows: Vec<(Ulid, Mounted<'a>)>,
    exec: LocalExecutor,
    proxy: Proxy,
    requested: Option<String>,
    error: Option<DesktopError>,
}
impl<'a> Desktop<'a> {
    fn open(&mut self, el: &ActiveEventLoop, id: Ulid, spec: WindowSpec<'a>) {
        let scope = spec.scope.child();
        scope.provide(WindowHandle {
            id,
            proxy: self.proxy.clone(),
        });
        let mut app = App {
            observer: spec.observer,
            options: spec.options,
            placement: placement::State::new(spec.placement),
            scope,
            root: Some(spec.root),
            exec: self.exec.clone(),
            win: None,
            requested: self.requested.clone(),
            pointer: (0.0, 0.0),
            touch: None,
            shift: false,
            error: None,
            reported: false,
            debug_frame: std::env::var_os("SERGE_DEBUG_FRAME").map(Into::into),
        };
        let proxy = self.proxy.lock().expect("window dispatcher").clone();
        app.mount(el, proxy);
        #[cfg(all(feature = "system", target_os = "linux"))]
        if let (Some(system), Some(w)) = (&self.system, &mut app.win) {
            match system.x11_menus(&w.window) {
                Ok(menus) => w.menus = menus,
                Err(error) => {
                    app.error = Some(error);
                    el.exit();
                }
            }
        }
        #[cfg(all(feature = "system", target_os = "windows"))]
        if let (Some(system), Some(w)) = (&self.system, &app.win)
            && let Err(e) = system.attach(&w.window)
        {
            app.error = Some(e);
            el.exit();
        }
        if let Some(error) = app.error.take() {
            app.scope.dispose();
            self.error = Some(error);
        } else {
            app.notify(NativeEvent::Resumed);
            self.windows.push((id, app));
        }
    }
    fn close(&mut self, el: &ActiveEventLoop, id: Ulid) {
        if let Some(index) = self.windows.iter().position(|item| item.0 == id) {
            let (_, mut app) = self.windows.swap_remove(index);
            if let Some(w) = app.win {
                if let Err(e) = app.placement.save(&w.window) {
                    self.error = Some(DesktopError::Platform(e.to_string()));
                }
                #[cfg(all(feature = "system", target_os = "windows"))]
                if let Some(system) = &self.system {
                    system.detach(&w.window);
                }
                w.ui.unmount();
            }
            app.scope.dispose();
        }
        if self.windows.is_empty() {
            el.exit();
        }
    }
}
impl ApplicationHandler<Wake> for Desktop<'_> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        for (_, app) in &mut self.windows {
            if let Some(w) = &mut app.win
                && w.presenter.is_none()
            {
                match surface(&w.window, app.requested.as_deref()) {
                    Ok(p) => {
                        w.presenter = Some(p);
                        w.window.request_redraw();
                        app.notify(NativeEvent::Resumed);
                    }
                    Err(error) => {
                        self.error = Some(error);
                        el.exit();
                        return;
                    }
                }
            }
        }

        #[cfg(all(
            feature = "system",
            any(target_os = "macos", target_os = "linux", target_os = "windows")
        ))]
        if let Some(factory) = self.install.take() {
            match factory().and_then(|config| config.install(&self.proxy)) {
                Ok(system) => self.system = Some(system),
                Err(e) => {
                    self.error = Some(e);
                    el.exit();
                    return;
                }
            }
        }
        for spec in std::mem::take(&mut self.initial) {
            self.open(el, Ulid::new(), spec);
            if self.error.is_some() {
                break;
            }
        }
    }
    fn suspended(&mut self, _: &ActiveEventLoop) {
        for (_, app) in &mut self.windows {
            app.notify(NativeEvent::Suspended);
            if let Some(w) = &mut app.win {
                app.touch = None;
                w.ui.pointer_cancel();
                w.presenter.take();
                w.ui.invalidate_surfaces();
            }
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: Wake) {
        match event {
            #[cfg(all(
                feature = "system",
                any(target_os = "macos", target_os = "linux", target_os = "windows")
            ))]
            Wake::Menu(id) => {
                if let Some(system) = &mut self.system {
                    let mut windows = self.windows.iter().filter_map(|(_, app)| app.win.as_ref());
                    let w = windows
                        .clone()
                        .find(|w| w.window.has_focus())
                        .or_else(|| windows.next());
                    if let Some(w) = w {
                        w.ui.view().run(|| system.dispatch(&id));
                    }
                }
            }
            Wake::Access(event) => {
                if let Some(window) = self
                    .windows
                    .iter_mut()
                    .filter_map(|(_, app)| app.win.as_mut())
                    .find(|w| w.window.id() == event.window_id)
                {
                    match event.window_event {
                        accesskit_winit::WindowEvent::InitialTreeRequested => {
                            window.window.request_redraw()
                        }
                        accesskit_winit::WindowEvent::ActionRequested(action) => {
                            window.ui.accessibility_action(action);
                            window.window.request_redraw();
                        }
                        accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
                    }
                }
            }
            Wake::Tasks => {
                self.exec.run_ready();
                for (_, w) in &mut self.windows {
                    w.after_input();
                }
            }
            Wake::Open(id, options, placement, scope, root) => self.open(
                el,
                id,
                WindowSpec::new(options, scope, root).placement(placement),
            ),
            Wake::Close(id) => self.close(el, id),
            Wake::Focus(id) => {
                if let Some(window) = self
                    .windows
                    .iter()
                    .find(|(key, _)| *key == id)
                    .and_then(|(_, app)| app.win.as_ref())
                {
                    window.window.focus_window();
                }
            }
        }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some((key, app)) = self
            .windows
            .iter_mut()
            .find(|(_, app)| app.win.as_ref().is_some_and(|w| w.window.id() == id))
        else {
            return;
        };
        #[cfg(all(feature = "system", target_os = "linux"))]
        if app
            .win
            .as_ref()
            .and_then(|w| w.menus.as_ref())
            .is_some_and(|m| m.activate_shortcut(&event))
        {
            return;
        }
        app.notify(NativeEvent::Window(&event));
        if matches!(
            event,
            WindowEvent::Moved(_)
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
        ) && let Some(w) = &app.win
        {
            app.placement.observe(&w.window);
        }
        #[cfg(not(target_os = "android"))]
        if let Some(w) = app.win.as_mut() {
            #[cfg(all(feature = "system", target_os = "linux"))]
            if matches!(
                event,
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Right,
                    ..
                }
            ) && let Some(menus) = &w.menus
            {
                menus.context(app.pointer, w.window.scale_factor());
            }
            #[cfg(all(feature = "system", any(target_os = "macos", target_os = "windows")))]
            if matches!(
                event,
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Right,
                    ..
                }
            ) && let Some(system) = &self.system
            {
                system.context(&w.window);
            }
            #[cfg(not(target_os = "android"))]
            w.access.process_event(&w.window, &event);
        }
        if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
            let key = *key;
            self.close(el, key);
        } else {
            app.input(event);
            if let Some(error) = app.error.take() {
                self.error = Some(error);
                el.exit();
            }
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        for w in self
            .windows
            .iter_mut()
            .filter_map(|(_, app)| app.win.as_mut())
        {
            if w.presenter.is_some() {
                let bounds = mobile_area(&w.window);
                if w.content_bounds != bounds {
                    w.content_bounds = bounds;
                    w.window.request_redraw();
                }
            }
        }
        #[cfg(all(
            feature = "hotpatch",
            debug_assertions,
            target_os = "macos",
            target_arch = "aarch64"
        ))]
        let polling = serge_native_hotpatch::between_frames();
        #[cfg(not(all(
            feature = "hotpatch",
            debug_assertions,
            target_os = "macos",
            target_arch = "aarch64"
        )))]
        let polling = false;
        let mut animating = false;
        for window in self.windows.iter().filter_map(|(_, app)| app.win.as_ref()) {
            if window.ui.animating() {
                animating = true;
                if window.ui.needs_frame() {
                    window.window.request_redraw();
                }
            }
        }
        #[cfg(all(feature = "webview", target_os = "linux"))]
        let animating = serge_adapter_webview::pump_gtk() || animating;
        #[cfg(all(feature = "system", target_os = "linux"))]
        let animating = self
            .system
            .as_ref()
            .is_some_and(system::Installed::pump_gtk)
            || animating;
        if animating {
            el.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
            ));
        } else if polling {
            el.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(100),
            ));
        } else {
            el.set_control_flow(ControlFlow::Wait);
        }
    }
    fn exiting(&mut self, _: &ActiveEventLoop) {
        for (_, mut app) in self.windows.drain(..) {
            if let Some(w) = app.win {
                if let Err(e) = app.placement.save(&w.window) {
                    self.error = Some(DesktopError::Platform(e.to_string()));
                }
                w.ui.unmount();
            }
            app.scope.dispose();
        }
    }
}

/// Opens a window showing `root()` under `scope` and runs until it closes.
/// Must be called on the main thread.
pub fn run(
    options: WindowOptions,
    scope: Scope,
    root: impl FnOnce() -> Element,
) -> Result<(), DesktopError> {
    run_windows(vec![WindowSpec::new(options, scope, root)])
}

/// Opens the initial native windows on a single main-thread event loop.
/// Each has a separate renderer, input focus, view tree, and child scope.
pub fn run_windows(initial: Vec<WindowSpec<'_>>) -> Result<(), DesktopError> {
    #[cfg(all(
        feature = "hotpatch",
        debug_assertions,
        target_os = "macos",
        target_arch = "aarch64"
    ))]
    serge_native_hotpatch::initialize();
    run_configured(
        initial,
        #[cfg(target_os = "android")]
        None,
        #[cfg(all(
            feature = "system",
            any(target_os = "macos", target_os = "linux", target_os = "windows")
        ))]
        None,
    )
}

fn run_configured<'a>(
    initial: Vec<WindowSpec<'a>>,
    #[cfg(target_os = "android")] android: Option<AndroidApp>,
    #[cfg(all(
        feature = "system",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    install: Option<system::Factory<'a>>,
) -> Result<(), DesktopError> {
    if initial.is_empty() {
        return Err(DesktopError::Platform(
            "expected at least one window".into(),
        ));
    }
    // A misspelled value fails before a window opens; the GPU is tried
    // once the window exists.
    let requested = std::env::var("HYPERISH_RENDER_BACKEND").ok();
    requested_backend(requested.as_deref()).map_err(DesktopError::Backend)?;
    let mut builder = EventLoop::<Wake>::with_user_event();
    #[cfg(all(target_os = "linux", any(feature = "system", feature = "webview")))]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        let gtk_host = cfg!(feature = "webview");
        #[cfg(feature = "system")]
        let gtk_host = gtk_host || install.is_some();
        // GTK child windows and winit must use the same window system. A Wayland
        // desktop with XWayland advertises both displays; winit otherwise picks Wayland.
        let x11_allowed = std::env::var("GDK_BACKEND").map_or(true, |backends| {
            backends
                .split(',')
                .any(|name| matches!(name.trim(), "*" | "x11"))
        });
        if gtk_host && x11_allowed && std::env::var_os("DISPLAY").is_some_and(|s| !s.is_empty()) {
            if !gtk::is_initialized() {
                gtk::gdk::set_allowed_backends("x11");
            }
            builder.with_x11();
        }
    }
    #[cfg(all(target_os = "windows", feature = "system"))]
    if install.is_some() {
        use winit::platform::windows::EventLoopBuilderExtWindows;
        builder.with_msg_hook(system::translate_accelerator);
    }
    #[cfg(target_os = "android")]
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        builder.with_android_app(
            android.ok_or_else(|| {
                DesktopError::Platform("launch from serge::android::launch".into())
            })?,
        );
    }
    let el = builder
        .build()
        .map_err(|e| DesktopError::Platform(e.to_string()))?;
    let proxy: Arc<Mutex<EventLoopProxy<Wake>>> = Arc::new(Mutex::new(el.create_proxy()));
    let tasks = proxy.clone();
    let exec = LocalExecutor::new(move || {
        if let Ok(p) = tasks.lock() {
            let _ = p.send_event(Wake::Tasks);
        }
    });
    serge_core::install_executor(Rc::new(exec.clone()));
    let mut app = Desktop {
        #[cfg(all(
            feature = "system",
            any(target_os = "macos", target_os = "linux", target_os = "windows")
        ))]
        install,
        #[cfg(all(
            feature = "system",
            any(target_os = "macos", target_os = "linux", target_os = "windows")
        ))]
        system: None,
        initial,
        windows: Vec::new(),
        exec,
        proxy,
        requested,
        error: None,
    };
    el.run_app(&mut app)
        .map_err(|e| DesktopError::Platform(e.to_string()))?;
    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Android activity supplied to the application's native entry point.
#[cfg(target_os = "android")]
pub use winit::platform::android::activity::AndroidApp;

/// Runs a single Android activity, rebuilding its surface on resume.
#[cfg(target_os = "android")]
pub fn run_android(host: AndroidApp, window: WindowSpec<'_>) -> Result<(), DesktopError> {
    run_configured(vec![window], Some(host))
}
