//! `serge-platform-desktop` — a desktop window for a serge UI: `winit`
//! (external dependency, `docs/02-architecture.md` §3) for the window and
//! input, the native runtime for layout and events, and a renderer chosen by
//! the fallback policy (`docs/03-adapters-and-pipeline.md` §1.2). Phase 1
//! presents CPU frames with `softbuffer`; the GPU renderer is not in this
//! build yet, so automatic selection uses the CPU and an explicit
//! `HYPERISH_RENDER_BACKEND=gpu` is an error.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hyve::Scope;
use serge_core::{Cursor, Element};
use serge_native::{Backend, Key, LocalExecutor, Ui, choose_backend};
use serge_native_cpu::CpuRenderer;
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

struct Window_ {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    ui: Ui,
}

struct App<F> {
    options: WindowOptions,
    scope: Scope,
    root: Option<F>,
    exec: LocalExecutor,
    win: Option<Window_>,
    renderer: CpuRenderer,
    pointer: (f32, f32),
    buffer: Vec<u32>,
    error: Option<DesktopError>,
    reported: bool,
    debug_frame: Option<std::path::PathBuf>,
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
        let frame = self.renderer.render(
            &scene,
            pw.min(u32::from(u16::MAX)) as u16,
            ph.min(u32::from(u16::MAX)) as u16,
            scale,
        );
        frame.to_xrgb(&mut self.buffer);
        let (Some(nw), Some(nh)) = (
            NonZeroU32::new(u32::from(frame.width)),
            NonZeroU32::new(u32::from(frame.height)),
        ) else {
            return;
        };
        if w.surface.resize(nw, nh).is_err() {
            return;
        }
        match w.surface.buffer_mut() {
            Ok(mut buf) => {
                let n = buf.len().min(self.buffer.len());
                buf[..n].copy_from_slice(&self.buffer[..n]);
                if let Err(e) = buf.present()
                    && !self.reported
                {
                    eprintln!("serge: presenting the frame failed: {e}");
                    self.reported = true;
                }
            }
            Err(e) if !self.reported => {
                eprintln!("serge: no frame buffer for the window: {e}");
                self.reported = true;
            }
            Err(_) => {}
        }
        // `SERGE_DEBUG_FRAME=<file.ppm>`: the frame just presented, as
        // pixels (to inspect a window where screenshots are not allowed).
        if let Some(path) = &self.debug_frame {
            let mut out = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
            for p in frame.rgba.as_chunks::<4>().0 {
                out.extend_from_slice(&p[..3]);
            }
            let _ = std::fs::write(path, out);
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
        let surface = softbuffer::Context::new(window.clone())
            .and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone()));
        let surface = match surface {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(DesktopError::Platform(e.to_string()));
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
            surface,
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
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
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
    let requested = std::env::var("HYPERISH_RENDER_BACKEND").ok();
    match choose_backend(requested.as_deref(), false, || {
        Err("the GPU renderer is not in this build yet".to_string())
    }) {
        Ok(Backend::Cpu) => {}
        Ok(Backend::Gpu) => unreachable!("the GPU path reports unavailable"),
        Err(e) => return Err(DesktopError::Backend(e)),
    }
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
        renderer: CpuRenderer::new(),
        pointer: (0.0, 0.0),
        buffer: Vec::new(),
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
