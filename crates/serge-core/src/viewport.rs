//! Local frame producers mounted inside a Serge view.
use crate::{Element, FrameSource, Handler, Kind, Node, Point, Prop, Rect, Widget};
use serge_style::StyleProp;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
};

/// Physical pixel dimensions of a supplied surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extent {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}
impl Extent {
    /// Maximum byte size of a tightly packed RGBA8 backing store.
    pub const MAX_BYTES: u64 = 256 * 1024 * 1024;

    /// Physical backing size of a logical destination.
    pub fn pixels(rect: Rect, scale: f32) -> Self {
        Self {
            width: (rect.width * scale).round() as u32,
            height: (rect.height * scale).round() as u32,
        }
    }
    /// Valid byte count for tightly packed RGBA8, limited to 256 MiB per slot.
    pub fn byte_len(self) -> Option<usize> {
        let bytes = u64::from(self.width)
            .checked_mul(u64::from(self.height))?
            .checked_mul(4)?;
        (bytes > 0 && bytes <= Self::MAX_BYTES).then_some(bytes as usize)
    }
}
/// Pointer input routed to this viewport in local physical pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pointer {
    /// Primary pointer pressed.
    Down(Point),

    /// Pointer moved.
    Move(Point),

    /// Primary pointer released.
    Up(Point),
    /// Gesture cancelled by the host.
    Cancel,
}
type Cpu = Rc<dyn Fn(Extent) -> Option<Vec<u8>>>;
/// A reusable producer. Each mount has its own slot and resize generation.
#[derive(Clone)]
pub struct Viewport {
    label: String,
    cpu: Option<Cpu>,
    backend: Option<Rc<dyn Any>>,
    input: Option<Rc<dyn Fn(Pointer)>>,
    source: FrameSource,
    fallback: serge_style::Color,
}
impl Viewport {
    /// Creates a viewport with an accessibility label.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            cpu: None,
            backend: None,
            input: None,
            source: Default::default(),
            fallback: serge_style::Color::rgba(64, 64, 64, 255),
        }
    }
    /// Supplies tightly packed, premultiplied sRGB RGBA8 for the CPU renderer.
    pub fn cpu(mut self, draw: impl Fn(Extent) -> Option<Vec<u8>> + 'static) -> Self {
        self.cpu = Some(Rc::new(draw));
        self
    }
    /// Installs the producer for the current platform's native GPU or browser host.
    pub fn backend<T: 'static>(mut self, backend: T) -> Self {
        self.backend = Some(Rc::new(backend));
        self
    }
    /// Color displayed when this host cannot present a frame.
    pub fn fallback(mut self, color: serge_style::Color) -> Self {
        self.fallback = color;
        self
    }
    /// Receives local input forwarded by the host.
    pub fn input(mut self, f: impl Fn(Pointer) + 'static) -> Self {
        self.input = Some(Rc::new(f));
        self
    }
    /// A frame source the producer can bump when new content is ready.
    pub fn source(&self) -> FrameSource {
        self.source.clone()
    }
    /// The accessible name of this output.
    pub fn label(&self) -> &str {
        &self.label
    }
    /// Creates a canvas region which follows normal layout, clipping and z-order.
    pub fn element(&self, style: impl IntoIterator<Item = StyleProp>) -> Element {
        let mut w = Widget::new(Kind::Canvas);
        w.style.extend(style.into_iter().map(Prop::Static));
        w.handlers.push(Handler::Viewport(self.clone()));
        Element(Node::Widget(Box::new(w)))
    }
    /// Registers an instance for a local presentation host.
    pub fn mount(&self) -> Slot {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let inner = Rc::new(Instance {
            id,
            producer: self.clone(),
            extent: Cell::default(),
            generation: Cell::new(0),
            seen: Cell::new(0),
            available: Cell::new(None),
        });
        SLOTS.with(|slots| slots.borrow_mut().insert(id, Rc::downgrade(&inner)));
        Slot(inner)
    }
}
thread_local! { static SLOTS:RefCell<HashMap<u64,Weak<Instance>>>=RefCell::default(); }
struct Instance {
    id: u64,
    producer: Viewport,
    extent: Cell<Extent>,
    generation: Cell<u64>,
    seen: Cell<u64>,
    available: Cell<Option<bool>>,
}
impl Drop for Instance {
    fn drop(&mut self) {
        let _ = SLOTS.try_with(|s| s.borrow_mut().remove(&self.id));
    }
}
/// Local slot reference carried by a scene command.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TextureKey {
    id: u64,
    generation: u64,
}
/// Host-owned registration for the lifetime of a mounted viewport.
#[derive(Clone)]
pub struct Slot(Rc<Instance>);
impl Slot {
    /// Updates the extent and identifies this generation of the surface.
    pub fn frame(&self, size: Extent) -> TextureKey {
        if self.0.extent.replace(size) != size {
            self.invalidate();
        }
        self.0.seen.set(self.0.producer.source.version());
        TextureKey {
            id: self.0.id,
            generation: self.0.generation.get(),
        }
    }
    /// Invalidates pending frames when a platform surface is lost.
    pub fn invalidate(&self) {
        self.0
            .generation
            .set(self.0.generation.get().wrapping_add(1));
    }
    /// Resolves a still-mounted generation on its owning UI thread.
    pub fn resolve(key: TextureKey) -> Option<Self> {
        let instance = SLOTS.with(|slots| slots.borrow().get(&key.id).and_then(Weak::upgrade))?;
        (instance.generation.get() == key.generation).then_some(Self(instance))
    }
    /// Checks if the producer has a newer frame.
    pub fn dirty(&self) -> bool {
        self.0.seen.get() != self.0.producer.source.version()
    }
    /// Records whether the host has a presentable frame.
    pub fn presented(&self, available: bool) {
        if self.0.available.replace(Some(available)) != Some(available) {
            self.0.producer.source.bump();
        }
    }
    /// Whether the last attempted frame was unavailable.
    pub fn unavailable(&self) -> bool {
        self.0.available.get() == Some(false)
    }
    /// Color requested for unavailable frames.
    pub fn fallback(&self) -> serge_style::Color {
        self.0.producer.fallback
    }
    /// Current physical size.
    pub fn extent(&self) -> Extent {
        self.0.extent.get()
    }
    /// Requested CPU pixels after length validation.
    pub fn rgba(&self) -> Option<Vec<u8>> {
        let size = self.extent();
        let len = size.byte_len()?;
        self.0.producer.cpu.as_ref()?(size).filter(|buf| buf.len() == len)
    }
    /// Registered backend for the current host.
    pub fn backend<T: 'static>(&self) -> Option<&T> {
        self.0.producer.backend.as_ref()?.downcast_ref()
    }
    /// Accessible label of the mounted producer.
    pub fn label(&self) -> &str {
        self.0.producer.label()
    }
    /// Forwards an input event to this instance's producer.
    pub fn pointer(&self, p: Pointer) {
        if let Some(f) = &self.0.producer.input {
            f(p);
        }
    }
}
