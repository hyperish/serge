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

use hyve::Scope;
use serge_core::{Cursor, Element};
use serge_native::{Backend, Key, LocalExecutor, Ui, choose_backend, requested_backend};
use serge_native_cpu::{CpuRenderer, Frame};
use serge_native_gpu::GpuSurface;
use serge_style::Theme;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

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
    Tasks,
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

struct Window_ {
    window: Arc<Window>,
    presenter: Presenter,
    ui: Ui,
}

struct App<F> {
    options: WindowOptions,
    scope: Scope,
    root: Option<F>,
    exec: LocalExecutor,
    win: Option<Window_>,
    requested: Option<String>,
    pointer: (f32, f32),
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

fn named(key: &WKey) -> Option<Key> {
    match key {
        WKey::Named(NamedKey::Backspace) => Some(Key::Backspace),
        WKey::Named(NamedKey::Enter) => Some(Key::Enter),
        WKey::Named(NamedKey::Tab) => Some(Key::Tab),
        WKey::Named(NamedKey::Escape) => Some(Key::Escape),
        _ => None,
    }
}

impl<F: FnOnce() -> Element> App<F> {
    fn scale(&self) -> f32 {
        self.win
            .as_ref()
            .map_or(1.0, |w| w.window.scale_factor() as f32)
    }

    fn redraw(&mut self) {
        let scale = self.scale();
        let Some(w) = self.win.as_mut() else { return };
        let size = w.window.inner_size();
        let (pw, ph) = (size.width.max(1), size.height.max(1));
        w.ui.resize(pw as f32 / scale, ph as f32 / scale);
        let scene = w.ui.frame();
        let debug = self.debug_frame.as_deref();
        match &mut w.presenter {
            Presenter::Gpu(surface) => match surface.render(&scene, pw, ph, scale, debug.is_some())
            {
                Ok(Some(frame)) => {
                    if let Some(path) = debug {
                        write_debug_frame(path, &frame);
                    }
                }
                Ok(None) => {}
                Err(e) => report_once(
                    &mut self.reported,
                    format_args!("the GPU frame failed: {e}"),
                ),
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
        if let Some(w) = &self.win {
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

impl<F: FnOnce() -> Element> ApplicationHandler<Wake> for App<F> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.options.title.clone())
            .with_inner_size(LogicalSize::new(self.options.width, self.options.height));
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.error = Some(DesktopError::Platform(e.to_string()));
                el.exit();
                return;
            }
        };
        window.set_ime_allowed(true);
        // The fallback policy, with the real surface and device.
        let size = window.inner_size();
        let (mut gpu, mut gpu_error) = (None, None);
        let backend = choose_backend(self.requested.as_deref(), false, || {
            match GpuSurface::new(window.clone(), size.width, size.height) {
                Ok(s) => {
                    gpu = Some(s);
                    Ok(())
                }
                Err(e) => {
                    gpu_error = Some(e.to_string());
                    Err(e.to_string())
                }
            }
        });
        let presenter = match backend {
            Ok(Backend::Gpu) => {
                let surface = gpu.take().expect("the GPU surface was created");
                eprintln!("serge: rendering on the GPU ({})", surface.adapter());
                Presenter::Gpu(Box::new(surface))
            }
            Ok(Backend::Cpu) => {
                let surface = softbuffer::Context::new(window.clone())
                    .and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone()));
                match surface {
                    Ok(surface) => {
                        match gpu_error {
                            Some(e) => eprintln!("serge: rendering on the CPU (no GPU: {e})"),
                            None => eprintln!("serge: rendering on the CPU"),
                        }
                        Presenter::Cpu(Box::new(CpuPresenter {
                            surface,
                            renderer: CpuRenderer::new(),
                            buffer: Vec::new(),
                        }))
                    }
                    Err(e) => {
                        self.error = Some(DesktopError::Platform(e.to_string()));
                        el.exit();
                        return;
                    }
                }
            }
            Err(e) => {
                self.error = Some(DesktopError::Backend(e));
                el.exit();
                return;
            }
        };
        let theme = match window.theme() {
            Some(winit::window::Theme::Dark) => Theme::DARK,
            _ => Theme::LIGHT,
        };
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
        self.win = Some(Window_ {
            window,
            presenter,
            ui,
        });
    }

    fn user_event(&mut self, _: &ActiveEventLoop, _: Wake) {
        self.exec.run_ready();
        self.after_input();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let scale = self.scale();
        let Some(w) = self.win.as_ref() else { return };
        match event {
            WindowEvent::CloseRequested => {
                w.ui.unmount();
                el.exit();
                return;
            }
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
                if let Some(k) = named(&logical_key) {
                    w.ui.key(k);
                } else if let Some(t) = text {
                    w.ui.text_input(&t);
                }
            }
            WindowEvent::Ime(Ime::Commit(t)) => w.ui.text_input(&t),
            _ => return,
        }
        self.exec.run_ready();
        self.after_input();
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        match &self.win {
            Some(w) if w.ui.animating() => {
                el.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(16),
                ));
                if w.ui.needs_frame() {
                    w.window.request_redraw();
                }
            }
            _ => el.set_control_flow(ControlFlow::Wait),
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
    // A misspelled value fails before a window opens; the GPU is tried
    // once the window exists.
    let requested = std::env::var("HYPERISH_RENDER_BACKEND").ok();
    requested_backend(requested.as_deref()).map_err(DesktopError::Backend)?;
    let el = EventLoop::<Wake>::with_user_event()
        .build()
        .map_err(|e| DesktopError::Platform(e.to_string()))?;
    let proxy: Arc<Mutex<EventLoopProxy<Wake>>> = Arc::new(Mutex::new(el.create_proxy()));
    let exec = LocalExecutor::new(move || {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(Wake::Tasks);
        }
    });
    serge_core::install_executor(Rc::new(exec.clone()));
    let mut app = App {
        options,
        scope,
        root: Some(root),
        exec,
        win: None,
        requested,
        pointer: (0.0, 0.0),
        error: None,
        reported: false,
        debug_frame: std::env::var_os("SERGE_DEBUG_FRAME").map(Into::into),
    };
    el.run_app(&mut app)
        .map_err(|e| DesktopError::Platform(e.to_string()))?;
    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
