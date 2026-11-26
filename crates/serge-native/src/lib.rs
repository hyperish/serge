//! `serge-native` — the native runtime shared by every native platform and
//! by headless tests: it mounts `Element`s into a retained tree, keeps it in
//! sync with signals, lays it out (`serge-native-layout`), shapes text
//! (`serge-native-text`), collects [`ScenePrimitive`]s for the GPU/CPU
//! renderers, routes input, and runs UI tasks ([`LocalExecutor`]).
//!
//! [`Headless`] drives the same runtime without a window (tests, CI).

mod accessibility;
mod exec;
mod scene;
mod tree;
pub use accesskit;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use hyve::Scope;
use hyve::state::untrack;
use serge_core::{
    Attr, AttrValue, Cursor, Element, Handler, Kind, Point, Rect, ScenePrimitive, View,
};
use serge_native_text::TextSystem;
use serge_style::Theme;

pub use exec::{LocalExecutor, yield_now};
pub use serge_native_text::Font;

use tree::{NKind, Shared, Tree};

/// A key that is not text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// Deletes the character before the caret.
    Backspace,
    /// Submits an input or activates a focused control.
    Enter,
    /// Inserts a space or activates a focused button or checkbox.
    Space,
    /// Moves focus to the next enabled control.
    Tab,
    /// Moves focus to the previous enabled control.
    BackTab,
    /// Increases or decreases a focused slider by one step.
    Step(bool),
    /// Drops focus.
    Escape,
}

/// The renderer a window uses (`docs/03-adapters-and-pipeline.md` §1.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    /// GPU (`wgpu`).
    Gpu,
    /// CPU (`vello_cpu`).
    Cpu,
}

/// Why no renderer could be chosen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BackendError {
    /// `HYPERISH_RENDER_BACKEND` names neither `cpu` nor `gpu`.
    Unknown(String),
    /// `gpu` was requested explicitly and the GPU path failed.
    GpuUnavailable(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::Unknown(v) => write!(
                f,
                "HYPERISH_RENDER_BACKEND={v:?} is not a backend; use `cpu` or `gpu`"
            ),
            BackendError::GpuUnavailable(why) => write!(
                f,
                "HYPERISH_RENDER_BACKEND=gpu, but the GPU renderer is unavailable: {why}"
            ),
        }
    }
}

impl std::error::Error for BackendError {}

/// The backend `requested` (`HYPERISH_RENDER_BACKEND`) names: `None` when
/// unset or empty, an error when it names neither `cpu` nor `gpu`.
pub fn requested_backend(requested: Option<&str>) -> Result<Option<Backend>, BackendError> {
    match requested.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) if v.eq_ignore_ascii_case("cpu") => Ok(Some(Backend::Cpu)),
        Some(v) if v.eq_ignore_ascii_case("gpu") => Ok(Some(Backend::Gpu)),
        Some(v) => Err(BackendError::Unknown(v.to_owned())),
    }
}

/// The fallback policy as a pure function: an explicit `requested` value
/// (`HYPERISH_RENDER_BACKEND`) always wins and an explicit `gpu` that fails
/// is an error; without one, headless runs use the CPU and windows try the
/// GPU, falling back to the CPU when `gpu()` fails.
pub fn choose_backend(
    requested: Option<&str>,
    headless: bool,
    gpu: impl FnOnce() -> Result<(), String>,
) -> Result<Backend, BackendError> {
    match requested_backend(requested)? {
        Some(Backend::Cpu) => Ok(Backend::Cpu),
        Some(Backend::Gpu) => gpu()
            .map(|()| Backend::Gpu)
            .map_err(BackendError::GpuUnavailable),
        None if headless => Ok(Backend::Cpu),
        None => Ok(gpu().map_or(Backend::Cpu, |()| Backend::Gpu)),
    }
}

/// A mounted UI: its tree, text system and input state. One per window.
pub struct Ui {
    shared: Rc<Shared>,
    text: RefCell<TextSystem>,
    view: View,
}

impl std::fmt::Debug for Ui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ui").finish_non_exhaustive()
    }
}

impl Ui {
    /// Mounts `root()` in a view under `scope`. `request_frame` is called
    /// whenever something changed that needs a new frame.
    pub fn mount(
        scope: Scope,
        theme: Theme,
        request_frame: Rc<dyn Fn()>,
        root: impl FnOnce() -> Element,
    ) -> Ui {
        let view = View::root(scope);
        let shared = Rc::new(Shared {
            tree: RefCell::new(Tree::new(view.clone())),
            theme,
            request_frame: RefCell::new(request_frame),
        });
        let root_id = shared.tree.borrow().root;
        view.run(|| {
            let el = untrack(root);
            tree::mount(&shared, el.0, root_id, &view);
        });
        Ui {
            shared,
            text: RefCell::new(TextSystem::new()),
            view,
        }
    }

    /// The root view.
    pub fn view(&self) -> &View {
        &self.view
    }

    /// Unmounts everything (cancels tasks and in-flight calls).
    pub fn unmount(&self) {
        self.view.dispose();
    }

    /// Sets the window size in logical pixels.
    pub fn resize(&self, width: f32, height: f32) {
        self.set_viewport(Rect::new(0.0, 0.0, width, height));
    }

    /// Lays out controls within the platform's usable rectangle, in window coordinates.
    pub fn set_viewport(&self, bounds: Rect) {
        let mut tree = self.shared.tree.borrow_mut();
        let origin = (bounds.x, bounds.y);
        let size = (bounds.width.max(0.0), bounds.height.max(0.0));
        if tree.origin != origin || tree.size != size {
            tree.origin = origin;
            tree.size = size;
            tree.dirty = true;
        }
    }

    /// Whether something changed since the last [`frame`](Self::frame).
    pub fn needs_frame(&self) -> bool {
        let t = self.shared.tree.borrow();
        t.dirty || self.frame_sources_changed(&t)
    }

    fn frame_sources_changed(&self, t: &Tree) -> bool {
        let mut changed = false;
        t.walk(t.root, &mut |_, n| {
            changed |= n.viewport.as_ref().is_some_and(|v| v.dirty());
            if let Some(c) = &n.canvas
                && serge_core::paint::sources_changed(&c.sources)
            {
                changed = true;
            }
        });
        changed
    }

    /// Whether a canvas reads a frame source (the host then keeps
    /// producing frames).
    pub fn animating(&self) -> bool {
        let t = self.shared.tree.borrow();
        let mut any = false;
        t.walk(t.root, &mut |_, n| {
            if n.viewport.is_some() || n.canvas.as_ref().is_some_and(|c| !c.sources.is_empty()) {
                any = true;
            }
        });
        any
    }

    /// Lays out and collects the scene.
    pub fn frame(&self) -> Vec<ScenePrimitive> {
        self.frame_at(1.0)
    }

    /// Collects a frame for a specific physical pixel scale.
    pub fn frame_at(&self, scale: f32) -> Vec<ScenePrimitive> {
        self.shared.tree.borrow_mut().scale = scale;
        // Canvases whose frame sources changed repaint first.
        let stale: Vec<_> = {
            let t = self.shared.tree.borrow();
            let mut v = Vec::new();
            t.walk(t.root, &mut |_, n| {
                if let Some(c) = &n.canvas
                    && serge_core::paint::sources_changed(&c.sources)
                {
                    v.push(c.tick);
                }
            });
            v
        };
        for tick in stale {
            tick.update(|t| *t += 1);
        }
        // Layout may resize canvases, whose paint then runs; one more pass
        // picks up anything that changed size again.
        for _ in 0..3 {
            let resized = {
                let mut t = self.shared.tree.borrow_mut();
                let mut text = self.text.borrow_mut();
                scene::layout(&mut t, &mut text)
            };
            if resized.is_empty() {
                break;
            }
            let mut t = self.shared.tree.borrow_mut();
            let signals: Vec<_> = resized
                .into_iter()
                .filter_map(|(id, r)| {
                    t.node_mut(id)
                        .and_then(|n| n.canvas.as_ref())
                        .map(|c| (c.bounds, r))
                })
                .collect();
            drop(t);
            for (s, r) in signals {
                s.set(r);
            }
        }
        let mut t = self.shared.tree.borrow_mut();
        t.dirty = false;
        let mut text = self.text.borrow_mut();
        scene::collect(&t, &mut text, &self.shared.theme)
    }

    fn handler(&self, id: usize, pick: impl Fn(&Handler) -> bool) -> Option<(Handler, View)> {
        let t = self.shared.tree.borrow();
        let n = t.node(id)?;
        n.handlers
            .iter()
            .find(|h| pick(h))
            .map(|h| (h.clone(), n.view.clone()))
    }

    /// Invalidates viewport resources after the native surface has been released.
    pub fn invalidate_surfaces(&self) {
        let mut tree = self.shared.tree.borrow_mut();
        tree.walk(tree.root, &mut |_, n| {
            if let Some(slot) = &n.viewport {
                slot.invalidate()
            }
        });
        tree.dirty = true;
    }

    fn viewport_pointer(
        &self,
        x: f32,
        y: f32,
        event: impl FnOnce(Point) -> serge_core::viewport::Pointer,
        captured: bool,
    ) {
        let hit = {
            let tree = self.shared.tree.borrow();
            let id = if captured { tree.pressed } else { None }.or_else(|| scene::hit(&tree, x, y));
            id.and_then(|id| tree.node(id)).and_then(|n| {
                Some((
                    n.viewport.clone()?,
                    Point::new(
                        (x - n.rect.x) * tree.scale,
                        (y + tree.scroll_y - n.rect.y) * tree.scale,
                    ),
                ))
            })
        };
        if let Some((slot, point)) = hit {
            slot.pointer(event(point))
        }
    }

    /// The pointer moved; returns whether a frame is needed.
    pub fn pointer_move(&self, x: f32, y: f32) -> bool {
        self.viewport_pointer(x, y, serge_core::viewport::Pointer::Move, true);
        let drag = self.shared.tree.borrow().drag;
        if let Some(id) = drag {
            self.slide(id, x);
            return true;
        }
        let mut t = self.shared.tree.borrow_mut();
        let h = scene::hit(&t, x, y);
        if h != t.hover {
            t.hover = h;
            t.dirty = true;
            return true;
        }
        false
    }

    /// The primary button went down.
    pub fn pointer_down(&self, x: f32, y: f32) {
        self.viewport_pointer(x, y, serge_core::viewport::Pointer::Down, false);
        let target = {
            let mut t = self.shared.tree.borrow_mut();
            let h = scene::hit(&t, x, y);
            t.hover = h;
            t.pressed = h;
            t.focus = h;
            t.dirty = true;
            h
        };
        if let Some(id) = target
            && self.kind(id) == Some(Kind::Slider)
        {
            self.shared.tree.borrow_mut().drag = Some(id);
            self.slide(id, x);
        }
    }

    /// Stops an interrupted pointer gesture without activating the pressed control.
    pub fn pointer_cancel(&self) {
        let slot = {
            let tree = self.shared.tree.borrow();
            tree.pressed
                .and_then(|id| tree.node(id))
                .and_then(|n| n.viewport.clone())
        };
        if let Some(slot) = slot {
            slot.pointer(serge_core::viewport::Pointer::Cancel);
        }

        let mut tree = self.shared.tree.borrow_mut();
        tree.pressed = None;
        tree.drag = None;
        tree.hover = None;
        tree.dirty = true;
    }

    /// Whether the focused control accepts input from a platform keyboard.
    pub fn wants_text_input(&self) -> bool {
        let tree = self.shared.tree.borrow();
        tree.focus.and_then(|id| tree.node(id)).is_some_and(|node| {
            node.kind == NKind::Widget(Kind::Input) && !node.flag(Attr::Disabled)
        })
    }

    /// The primary button went up: clicks, toggles.
    pub fn pointer_up(&self, x: f32, y: f32) {
        self.viewport_pointer(x, y, serge_core::viewport::Pointer::Up, true);
        let (pressed, over) = {
            let mut t = self.shared.tree.borrow_mut();
            let p = t.pressed.take();
            t.drag = None;
            t.dirty = true;
            (p, scene::hit(&t, x, y))
        };
        let Some(id) = pressed.filter(|p| Some(*p) == over) else {
            return;
        };
        self.activate(id);
    }

    fn activate(&self, id: usize) {
        let (kind, disabled, checked, href) = {
            let t = self.shared.tree.borrow();
            let Some(n) = t.node(id) else { return };
            (
                self.kind(id),
                n.flag(Attr::Disabled),
                n.flag(Attr::Checked),
                n.attr(Attr::Href).map(|v| v.as_str().to_owned()),
            )
        };
        match kind {
            Some(Kind::Button) if !disabled => {
                if let Some((Handler::Clicked(f), v)) =
                    self.handler(id, |h| matches!(h, Handler::Clicked(_)))
                {
                    v.run(|| f());
                }
            }
            Some(Kind::Checkbox) if !disabled => {
                if let Some((Handler::ChangedBool(f), v)) =
                    self.handler(id, |h| matches!(h, Handler::ChangedBool(_)))
                {
                    v.run(|| f(!checked));
                }
            }
            Some(Kind::Link) if !disabled => {
                if let Some(to) = href {
                    let v = self.shared.tree.borrow().node(id).map(|n| n.view.clone());
                    if let Some(v) = v {
                        v.run(|| serge_core::navigate(&to));
                    }
                }
            }
            _ => {}
        }
        self.shared.changed();
    }

    /// Scrolls the window content by `dy` logical pixels.
    pub fn scroll(&self, dy: f32) {
        let mut t = self.shared.tree.borrow_mut();
        let max = (t.content_height - t.size.1).max(0.0);
        t.scroll_y = (t.scroll_y + dy).clamp(0.0, max);
        t.dirty = true;
    }

    /// Text typed (or committed by an input method) into the focused input.
    pub fn text_input(&self, s: &str) {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        if s.is_empty() {
            return;
        }
        self.edit(|text| text.push_str(&s));
    }

    /// A non-text key.
    pub fn key(&self, key: Key) {
        match key {
            Key::Backspace => self.edit(|t| {
                t.pop();
            }),
            Key::Enter | Key::Space | Key::Step(_) => {
                let Some(id) = self.shared.tree.borrow().focus else {
                    return;
                };
                match (self.kind(id), key) {
                    (Some(Kind::Input), Key::Enter) => {
                        if let Some((Handler::Submitted(f), v)) =
                            self.handler(id, |h| matches!(h, Handler::Submitted(_)))
                        {
                            v.run(|| f());
                        }
                    }
                    (Some(Kind::Input), Key::Space) => self.text_input(" "),
                    (Some(Kind::Button | Kind::Checkbox), Key::Space | Key::Enter)
                    | (Some(Kind::Link), Key::Enter) => self.activate(id),
                    (Some(Kind::Slider), Key::Step(up)) => self.set_slider(id, |n, step| {
                        n.num(Attr::Value, n.num(Attr::Min, 0.0)) + if up { step } else { -step }
                    }),
                    _ => {}
                }
            }
            Key::Escape => {
                self.shared.tree.borrow_mut().focus = None;
            }
            Key::Tab | Key::BackTab => {
                let mut t = self.shared.tree.borrow_mut();
                let mut controls = Vec::new();
                t.walk(t.root, &mut |id, n| {
                    if n.focusable() {
                        controls.push(id);
                    }
                });
                if key == Key::BackTab {
                    controls.reverse();
                }
                let position = controls.iter().position(|id| Some(*id) == t.focus);
                t.focus = position
                    .and_then(|p| controls.get(p + 1))
                    .or(controls.first())
                    .copied();
            }
        }
        self.shared.changed();
    }

    fn edit(&self, f: impl FnOnce(&mut String)) {
        let (id, text) = {
            let mut t = self.shared.tree.borrow_mut();
            let Some(id) = t.focus else { return };
            let Some(n) = t.node_mut(id) else { return };
            if n.kind != NKind::Widget(Kind::Input) || n.flag(Attr::Disabled) {
                return;
            }
            f(&mut n.input);
            (id, n.input.clone())
        };
        if let Some((Handler::ChangedText(f), v)) =
            self.handler(id, |h| matches!(h, Handler::ChangedText(_)))
        {
            v.run(|| f(text));
        }
        self.shared.changed();
    }

    fn kind(&self, id: usize) -> Option<Kind> {
        match self.shared.tree.borrow().node(id)?.kind {
            NKind::Widget(k) => Some(k),
            _ => None,
        }
    }

    fn slide(&self, id: usize, x: f32) {
        self.set_slider(id, |n, _| {
            let track = scene::slider_track(n, 0.0);
            let (lo, hi) = (n.num(Attr::Min, 0.0), n.num(Attr::Max, 1.0));
            lo + ((x - track.x) / track.width).clamp(0.0, 1.0) * (hi - lo)
        });
    }

    fn set_slider(&self, id: usize, change: impl FnOnce(&tree::NNode, f32) -> f32) {
        let value = {
            let t = self.shared.tree.borrow();
            let Some(n) = t.node(id) else { return };
            let (lo, hi) = (n.num(Attr::Min, 0.0), n.num(Attr::Max, 1.0));
            if !n.focusable() || !lo.is_finite() || !hi.is_finite() || lo >= hi {
                return;
            }
            let step = n.num(Attr::Step, 0.0);
            let mut v = change(n, if step > 0.0 { step } else { (hi - lo) / 100.0 });
            if !v.is_finite() {
                return;
            }
            if step > 0.0 {
                v = lo + ((v - lo) / step).round() * step;
            }
            v.clamp(lo, hi)
        };
        if let Some((Handler::ChangedNum(f), v)) =
            self.handler(id, |h| matches!(h, Handler::ChangedNum(_)))
        {
            v.run(|| f(value));
        }
        self.shared.changed();
    }

    /// The cursor to show at `(x, y)`.
    pub fn cursor_at(&self, x: f32, y: f32) -> Cursor {
        let t = self.shared.tree.borrow();
        match scene::hit(&t, x, y).and_then(|id| t.node(id)) {
            Some(n) => n.cursor.unwrap_or(match n.kind {
                NKind::Widget(Kind::Input) => Cursor::Text,
                _ => Cursor::Pointer,
            }),
            None => Cursor::Default,
        }
    }

    /// Visible widgets with text, in paint order: `(kind, text, rect in
    /// window coordinates)`. For tests and accessibility.
    pub fn texts(&self) -> Vec<(Kind, String, Rect)> {
        let t = self.shared.tree.borrow();
        let mut out = Vec::new();
        let dy = -t.scroll_y;
        t.walk(t.root, &mut |_, n| {
            if let NKind::Widget(k) = n.kind {
                let text = match k {
                    Kind::Input => n.input.clone(),
                    _ => n.text().to_owned(),
                };
                if !text.is_empty() || matches!(k, Kind::Input) {
                    out.push((k, text, n.rect.offset(0.0, dy)));
                }
            }
        });
        out
    }

    /// Widgets of `kind` in paint order with their rects and attributes.
    pub fn widgets(&self, kind: Kind) -> Vec<(Rect, Vec<(Attr, AttrValue)>)> {
        let t = self.shared.tree.borrow();
        let mut out = Vec::new();
        let dy = -t.scroll_y;
        t.walk(t.root, &mut |_, n| {
            if n.kind == NKind::Widget(kind) {
                out.push((n.rect.offset(0.0, dy), n.attrs.clone()));
            }
        });
        out
    }
}

/// The UI without a window: the native runtime, the CPU-side scene and
/// simulated input, for tests (`docs/01-dx-and-dsl.md` §2, 헤드리스 통합
/// 테스트).
pub struct Headless {
    ui: Ui,
    exec: LocalExecutor,
}

impl std::fmt::Debug for Headless {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Headless").finish_non_exhaustive()
    }
}

impl Headless {
    /// Mounts `root()` in a `width` × `height` window under a new root scope.
    pub fn new(width: f32, height: f32, root: impl FnOnce() -> Element) -> Headless {
        Headless::with_scope(Scope::root(), width, height, root)
    }

    /// Like [`new`](Self::new) under `scope` (with values provided).
    pub fn with_scope(
        scope: Scope,
        width: f32,
        height: f32,
        root: impl FnOnce() -> Element,
    ) -> Headless {
        let exec = LocalExecutor::new(|| {});
        serge_core::install_executor(Rc::new(exec.clone()));
        let ui = Ui::mount(scope, Theme::LIGHT, Rc::new(|| {}), root);
        ui.resize(width, height);
        let h = Headless { ui, exec };
        h.settle();
        h
    }

    /// The runtime.
    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    /// Polls ready tasks and produces a frame (layout and paint).
    pub fn settle(&self) -> Vec<ScenePrimitive> {
        self.exec.run_ready();
        self.ui.frame()
    }

    /// Settles until `cond` holds or `timeout` passes (tasks waiting on I/O
    /// on other threads); returns whether it held.
    pub fn wait_until(&self, timeout: Duration, cond: impl Fn(&Headless) -> bool) -> bool {
        let end = Instant::now() + timeout;
        loop {
            self.settle();
            if cond(self) {
                return true;
            }
            if Instant::now() > end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The visible texts in paint order.
    pub fn texts(&self) -> Vec<String> {
        self.ui.texts().into_iter().map(|(_, t, _)| t).collect()
    }

    /// Whether some widget shows exactly `text`.
    pub fn shows(&self, text: &str) -> bool {
        self.texts().iter().any(|t| t == text)
    }

    /// The rect of the first widget showing `text`.
    pub fn rect_of(&self, text: &str) -> Option<Rect> {
        self.ui
            .texts()
            .into_iter()
            .find(|(_, t, _)| t == text)
            .map(|(_, _, r)| r)
    }

    /// Clicks at `(x, y)` and settles.
    pub fn click(&self, x: f32, y: f32) {
        self.ui.pointer_move(x, y);
        self.ui.pointer_down(x, y);
        self.ui.pointer_up(x, y);
        self.settle();
    }

    /// Clicks the center of the first widget showing `text`.
    ///
    /// # Panics
    ///
    /// Panics if no widget shows `text`.
    pub fn click_text(&self, text: &str) {
        let r = self
            .rect_of(text)
            .unwrap_or_else(|| panic!("no widget shows {text:?}; visible: {:?}", self.texts()));
        let c = r.center();
        self.click(c.x, c.y);
    }

    /// Drags the `nth` slider to `fraction` of its track.
    pub fn slide(&self, nth: usize, fraction: f32) {
        let (r, _) = self
            .ui
            .widgets(Kind::Slider)
            .into_iter()
            .nth(nth)
            .expect("slider");
        let track = Rect::new(
            r.x + scene::THUMB,
            r.y,
            r.width - 2.0 * scene::THUMB,
            r.height,
        );
        let y = r.center().y;
        self.ui.pointer_down(r.center().x, y);
        self.ui.pointer_move(track.x + track.width * fraction, y);
        self.ui.pointer_up(track.x + track.width * fraction, y);
        self.settle();
    }

    /// Focuses the `nth` input and types `text`.
    pub fn type_into(&self, nth: usize, text: &str) {
        let (r, _) = self
            .ui
            .widgets(Kind::Input)
            .into_iter()
            .nth(nth)
            .expect("input");
        let c = r.center();
        self.ui.pointer_down(c.x, c.y);
        self.ui.pointer_up(c.x, c.y);
        self.ui.text_input(text);
        self.settle();
    }

    /// Presses a key and settles.
    pub fn press(&self, key: Key) {
        self.ui.key(key);
        self.settle();
    }

    /// How many UI tasks are alive.
    pub fn tasks(&self) -> usize {
        self.exec.len()
    }
}

impl Drop for Headless {
    fn drop(&mut self) {
        self.ui.unmount();
    }
}
