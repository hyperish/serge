//! Shared UI element tree, custom paint primitives, feature DI, and type-safe router.

use crate::style::{Color, LayoutDirection, Length};
use hyve::prelude::{
    Ctx, FromJson, HyveResult, Signal, ToJson, from_json_str, signal, to_json_string,
};
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ops::RangeInclusive;
use std::rc::Rc;

type TextGetter = Rc<dyn Fn() -> String>;
type ValueGetter = Rc<dyn Fn() -> PropValue>;
type CondFn = Rc<dyn Fn() -> bool>;
type BranchFn = Rc<dyn Fn() -> Element>;
type ClickHandler = Rc<RefCell<dyn FnMut()>>;
type ChangeF32Handler = Rc<RefCell<dyn FnMut(f32)>>;
type ChangeTextHandler = Rc<RefCell<dyn FnMut(String)>>;
type PaintHandler = Rc<RefCell<dyn FnMut(Rect, &mut PaintCtx)>>;
type CustomEventHandler = Rc<RefCell<dyn FnMut()>>;
type GuardFn = Rc<dyn Fn(&str, bool) -> Option<String>>;
type RouteFactory = Rc<dyn Fn(&FeatureCtx) -> Element>;

/// Logical pixel rectangle emitted by `serge` layout and consumed by `PaintCtx` / renderers.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left X coordinate in logical pixels.
    pub x: f32,
    /// Top Y coordinate in logical pixels.
    pub y: f32,
    /// Rectangle width in logical pixels.
    pub width: f32,
    /// Rectangle height in logical pixels.
    pub height: f32,
}

impl Rect {
    /// Creates a new rectangle.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Real-time telemetry sample stored in lock-free [`hyve::prelude::SharedRingBuf`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FrameMetrics {
    /// Total frame duration in milliseconds.
    pub frame_time_ms: f32,
    /// GPU command execution duration in milliseconds.
    pub gpu_time_ms: f32,
}

impl FrameMetrics {
    /// Zeroed frame sample constant (`[FrameMetrics::ZERO; 128]`).
    pub const ZERO: Self = Self {
        frame_time_ms: 0.0,
        gpu_time_ms: 0.0,
    };

    /// Constructs a new telemetry sample.
    #[must_use]
    pub const fn new(frame_time_ms: f32, gpu_time_ms: f32) -> Self {
        Self {
            frame_time_ms,
            gpu_time_ms,
        }
    }
}

/// Trait implemented by data points rendered via [`PaintCtx::draw_sparkline`].
pub trait SparklineSample {
    /// Returns the scalar Y value for this sample.
    fn sample_value(&self) -> f32;
}

impl SparklineSample for FrameMetrics {
    fn sample_value(&self) -> f32 {
        self.frame_time_ms
    }
}

impl SparklineSample for f32 {
    fn sample_value(&self) -> f32 {
        *self
    }
}

impl SparklineSample for f64 {
    fn sample_value(&self) -> f32 {
        *self as f32
    }
}

/// Backend-neutral draw instruction produced by layout and `Canvas::paint(...)`.
///
/// Consumed symmetrically by both `GpuBatchRenderer` and `CpuFallbackRenderer`.
#[derive(Debug, Clone, PartialEq)]
pub enum ScenePrimitive {
    /// Solid rounded rectangle with optional border stroke.
    Rect {
        /// Destination bounds in logical pixels.
        bounds: Rect,
        /// Fill color.
        fill: Color,
        /// Optional border stroke color.
        border: Option<Color>,
        /// Border thickness in pixels.
        border_width: f32,
        /// Corner radius in pixels.
        radius: f32,
    },
    /// Shaped text baseline run.
    TextRun {
        /// Bounding box for the text run.
        bounds: Rect,
        /// UTF-8 rendered content.
        text: String,
        /// Foreground text color.
        color: Color,
        /// Font size in logical pixels.
        font_size: f32,
    },
    /// Bar within a `Canvas` histogram plot.
    HistogramBar {
        /// Bar bounds.
        bounds: Rect,
        /// Normalized bar value (`0.0..=1.0`).
        value: f32,
        /// Bar fill color.
        color: Color,
    },
    /// Polyline sample point or segment inside a telemetry sparkline.
    SparklineSegment {
        /// Start X coordinate.
        x0: f32,
        /// Start Y coordinate.
        y0: f32,
        /// End X coordinate.
        x1: f32,
        /// End Y coordinate.
        y1: f32,
        /// Stroke color.
        color: Color,
    },
    /// Horizontal progress bar track + active fill.
    ProgressBarFill {
        /// Bar bounds.
        bounds: Rect,
        /// Progress fraction clamped to `0.0..=1.0`.
        progress: f32,
        /// Track color.
        track: Color,
        /// Fill color.
        fill: Color,
    },
    /// Zero-copy handle referencing an external GPU / camera texture.
    ExternalTexture {
        /// Target quad bounds.
        bounds: Rect,
        /// Opaque GPU texture identifier.
        texture_id: u64,
    },
}

/// Immediate-mode custom vector/GPU paint context passed into `Canvas { paint(bounds, ctx) => ... }`.
#[derive(Debug, Default, Clone)]
pub struct PaintCtx {
    primitives: Vec<ScenePrimitive>,
}

impl PaintCtx {
    /// Creates an empty paint context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Emits a filled rectangle primitive.
    pub fn draw_rect(&mut self, bounds: Rect, fill: Color) {
        self.primitives.push(ScenePrimitive::Rect {
            bounds,
            fill,
            border: None,
            border_width: 0.0,
            radius: 0.0,
        });
    }

    /// Draws an exposure-shifted RGB luminance histogram inside `bounds`.
    pub fn draw_histogram(&mut self, bounds: Rect, exposure: f32) {
        const BARS: usize = 16;
        let bar_w = (bounds.width / BARS as f32).max(1.0);
        let shift = (exposure / 5.0).clamp(-1.0, 1.0);
        for i in 0..BARS {
            let t = (i as f32 / (BARS - 1) as f32) * 2.0 - 1.0 - shift;
            let height_ratio = (-t * t * 2.4).exp().clamp(0.08, 1.0);
            let h = bounds.height * height_ratio;
            let bar = Rect::new(
                bounds.x + i as f32 * bar_w,
                bounds.y + (bounds.height - h),
                (bar_w - 1.0).max(1.0),
                h,
            );
            self.primitives.push(ScenePrimitive::HistogramBar {
                bounds: bar,
                value: height_ratio,
                color: Color::INDIGO_500,
            });
        }
    }

    /// Draws a continuous telemetry sparkline polyline across `samples` within `bounds`.
    pub fn draw_sparkline<S: SparklineSample>(&mut self, bounds: Rect, samples: &[S]) {
        if samples.is_empty() {
            return;
        }
        let mut max_v = 1.0_f32;
        for s in samples {
            let v = s.sample_value();
            if v > max_v {
                max_v = v;
            }
        }
        if samples.len() == 1 {
            let y = bounds.y + bounds.height * 0.5;
            self.primitives.push(ScenePrimitive::SparklineSegment {
                x0: bounds.x,
                y0: y,
                x1: bounds.x + bounds.width,
                y1: y,
                color: Color::EMERALD_500,
            });
            return;
        }
        let step_x = bounds.width / (samples.len() - 1) as f32;
        for i in 0..(samples.len() - 1) {
            let v0 = (samples[i].sample_value() / max_v).clamp(0.0, 1.0);
            let v1 = (samples[i + 1].sample_value() / max_v).clamp(0.0, 1.0);
            let x0 = bounds.x + i as f32 * step_x;
            let x1 = bounds.x + (i + 1) as f32 * step_x;
            let y0 = bounds.y + bounds.height * (1.0 - v0);
            let y1 = bounds.y + bounds.height * (1.0 - v1);
            self.primitives.push(ScenePrimitive::SparklineSegment {
                x0,
                y0,
                x1,
                y1,
                color: Color::EMERALD_500,
            });
        }
    }

    /// Consumes and returns all recorded primitives.
    #[must_use]
    pub fn into_primitives(self) -> Vec<ScenePrimitive> {
        self.primitives
    }

    /// Borrows all recorded primitives.
    #[must_use]
    pub fn primitives(&self) -> &[ScenePrimitive] {
        &self.primitives
    }
}

/// Typed property value accepted inside `ui! { ... }`.
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    /// CSS/Logical pixel or percentage length.
    Length(Length),
    /// Stack layout direction (`v_stack`, `h_stack`, `z_stack`).
    Layout(LayoutDirection),
    /// Single-precision floating-point value (slider value, progress, etc.).
    Float(f32),
    /// Integer property value.
    Int(i32),
    /// Boolean flag.
    Bool(bool),
    /// Inclusive numeric range (`-5.0..=5.0`).
    Range(f32, f32),
    /// UTF-8 string property.
    Str(String),
}

impl PropValue {
    /// Converts this property into a logical pixel length if possible.
    #[must_use]
    pub fn as_px(&self, default_px: f32) -> f32 {
        match self {
            Self::Length(len) => len.to_px(default_px),
            Self::Float(v) => *v,
            Self::Int(v) => *v as f32,
            _ => default_px,
        }
    }

    /// Converts numeric properties into `f32` if possible.
    #[must_use]
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Int(v) => Some(*v as f32),
            Self::Length(Length::Px(v)) => Some(*v),
            _ => None,
        }
    }

    /// Converts string properties into `&str` if possible.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
}

impl From<Length> for PropValue {
    fn from(value: Length) -> Self {
        Self::Length(value)
    }
}

impl From<LayoutDirection> for PropValue {
    fn from(value: LayoutDirection) -> Self {
        Self::Layout(value)
    }
}

impl From<f32> for PropValue {
    fn from(value: f32) -> Self {
        Self::Float(value)
    }
}

impl From<i32> for PropValue {
    fn from(value: i32) -> Self {
        Self::Int(value)
    }
}

impl From<bool> for PropValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<RangeInclusive<f32>> for PropValue {
    fn from(value: RangeInclusive<f32>) -> Self {
        Self::Range(*value.start(), *value.end())
    }
}

impl From<String> for PropValue {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

impl From<&str> for PropValue {
    fn from(value: &str) -> Self {
        Self::Str(value.to_owned())
    }
}

/// Target-specific attribute value (`web::class: "..."`, `native::elevation: 4`).
#[derive(Debug, Clone, PartialEq)]
pub enum AttrVal {
    /// String attribute value.
    Str(String),
    /// Integer attribute value.
    Int(i32),
    /// Float attribute value.
    Float(f32),
    /// Boolean attribute value.
    Bool(bool),
}

impl AttrVal {
    /// Renders the attribute value into a display string.
    #[must_use]
    pub fn to_display_string(&self) -> String {
        match self {
            Self::Str(s) => s.clone(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Bool(b) => b.to_string(),
        }
    }
}

impl From<&str> for AttrVal {
    fn from(value: &str) -> Self {
        Self::Str(value.to_owned())
    }
}

impl From<String> for AttrVal {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

impl From<i32> for AttrVal {
    fn from(value: i32) -> Self {
        Self::Int(value)
    }
}

impl From<f32> for AttrVal {
    fn from(value: f32) -> Self {
        Self::Float(value)
    }
}

impl From<bool> for AttrVal {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

#[derive(Clone)]
enum TextSource {
    Static(String),
    Reactive(TextGetter),
}

#[derive(Clone)]
struct ConditionalBranch {
    cond: CondFn,
    then_fn: BranchFn,
    else_fn: BranchFn,
}

/// Platform-neutral reactive UI node built by `ui! { ... }` and `#[component]` functions.
#[derive(Clone)]
pub struct Element {
    /// Element tag name (`"Card"`, `"Text"`, `"Button"`, `"Slider"`, `"Canvas"`, etc.).
    pub tag: String,
    /// Named layout and widget properties.
    pub props: BTreeMap<String, PropValue>,
    /// Target-specific web attributes (`web::*`).
    pub web_attrs: BTreeMap<String, AttrVal>,
    /// Target-specific native attributes (`native::*`).
    pub native_attrs: BTreeMap<String, AttrVal>,
    text_source: Option<TextSource>,
    value_reactive: Option<ValueGetter>,
    conditional: Option<ConditionalBranch>,
    on_click: Option<ClickHandler>,
    on_change_f32: Option<ChangeF32Handler>,
    on_change_text: Option<ChangeTextHandler>,
    on_paint: Option<PaintHandler>,
    custom_events: BTreeMap<String, CustomEventHandler>,
    children: Vec<Element>,
}

impl Element {
    /// Creates a new element with the given tag name.
    #[must_use]
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            props: BTreeMap::new(),
            web_attrs: BTreeMap::new(),
            native_attrs: BTreeMap::new(),
            text_source: None,
            value_reactive: None,
            conditional: None,
            on_click: None,
            on_change_f32: None,
            on_change_text: None,
            on_paint: None,
            custom_events: BTreeMap::new(),
            children: Vec::new(),
        }
    }

    /// Creates an empty invisible placeholder element.
    #[must_use]
    pub fn empty() -> Self {
        Self::new("Empty")
    }

    /// Creates a transparent fragment container holding sibling elements.
    #[must_use]
    pub fn fragment(children: Vec<Self>) -> Self {
        let mut el = Self::new("Fragment");
        el.children = children;
        el
    }

    /// Creates a reactive conditional UI element node (`if cond { then } else { else }`).
    #[must_use]
    pub fn conditional(
        cond: impl Fn() -> bool + 'static,
        then_fn: impl Fn() -> Self + 'static,
        else_fn: impl Fn() -> Self + 'static,
    ) -> Self {
        let mut el = Self::new("Conditional");
        el.conditional = Some(ConditionalBranch {
            cond: Rc::new(cond),
            then_fn: Rc::new(then_fn),
            else_fn: Rc::new(else_fn),
        });
        el
    }

    /// Sets a static text content slot (allows zero-diffing static template extraction).
    pub fn set_text_static(&mut self, text: impl Into<String>) {
        self.text_source = Some(TextSource::Static(text.into()));
    }

    /// Sets an eagerly evaluated text value (used by static snapshot macros).
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text_source = Some(TextSource::Static(text.into()));
    }

    /// Sets a fine-grained reactive text slot evaluated lazily against `hyve-state` signals.
    pub fn set_text_reactive(&mut self, getter: impl Fn() -> String + 'static) {
        self.text_source = Some(TextSource::Reactive(Rc::new(getter)));
    }

    /// Returns `true` when the text slot is dynamically computed via signals/closures.
    #[must_use]
    pub fn has_reactive_text(&self) -> bool {
        matches!(self.text_source, Some(TextSource::Reactive(_)))
    }

    /// Evaluates the current text content of this element.
    #[must_use]
    pub fn resolved_text(&self) -> Option<String> {
        match &self.text_source {
            Some(TextSource::Static(s)) => Some(s.clone()),
            Some(TextSource::Reactive(f)) => Some(f()),
            None => None,
        }
    }

    /// Sets a typed property on this element.
    pub fn set_prop(&mut self, key: impl Into<String>, value: PropValue) {
        self.props.insert(key.into(), value);
    }

    /// Sets a fine-grained reactive `value` getter.
    pub fn set_value_reactive(&mut self, getter: impl Fn() -> PropValue + 'static) {
        let initial = getter();
        self.props.insert("value".to_owned(), initial);
        self.value_reactive = Some(Rc::new(getter));
    }

    /// Evaluates the current `value` property (preferring reactive closure if present).
    #[must_use]
    pub fn resolved_value(&self) -> Option<PropValue> {
        if let Some(getter) = &self.value_reactive {
            Some(getter())
        } else {
            self.props.get("value").cloned()
        }
    }

    /// Sets a platform-namespaced escape-hatch attribute (`web::*` or `native::*`).
    pub fn set_platform_attr(&mut self, ns: &str, key: impl Into<String>, val: AttrVal) {
        match ns {
            "web" => {
                self.web_attrs.insert(key.into(), val);
            }
            "native" => {
                self.native_attrs.insert(key.into(), val);
            }
            _ => {
                self.props.insert(
                    format!("{ns}::{}", key.into()),
                    PropValue::Str(val.to_display_string()),
                );
            }
        }
    }

    /// Registers a `clicked => ...` handler.
    pub fn on_click(&mut self, handler: impl FnMut() + 'static) {
        self.on_click = Some(Rc::new(RefCell::new(handler)));
    }

    /// Registers a numeric `changed(v) => ...` handler (e.g. `Slider`).
    pub fn on_change_f32(&mut self, handler: impl FnMut(f32) + 'static) {
        self.on_change_f32 = Some(Rc::new(RefCell::new(handler)));
    }

    /// Registers a text `changed(s) => ...` handler (e.g. `Input`).
    pub fn on_change_text(&mut self, handler: impl FnMut(String) + 'static) {
        self.on_change_text = Some(Rc::new(RefCell::new(handler)));
    }

    /// Registers a `Canvas` `paint(bounds, ctx) => ...` callback.
    pub fn set_paint(&mut self, handler: impl FnMut(Rect, &mut PaintCtx) + 'static) {
        self.on_paint = Some(Rc::new(RefCell::new(handler)));
    }

    /// Registers a custom event callback by name.
    pub fn on_custom_event(&mut self, name: impl Into<String>, handler: impl FnMut() + 'static) {
        self.custom_events
            .insert(name.into(), Rc::new(RefCell::new(handler)));
    }

    /// Appends a child node.
    pub fn push_child(&mut self, child: Self) {
        self.children.push(child);
    }

    /// Returns static or dynamically expanded children (evaluating any `Conditional` branch).
    #[must_use]
    pub fn resolved_children(&self) -> Vec<Self> {
        if let Some(cond) = &self.conditional {
            let branch = if (cond.cond)() {
                (cond.then_fn)()
            } else {
                (cond.else_fn)()
            };
            if branch.tag == "Empty" {
                return Vec::new();
            }
            return vec![branch];
        }
        self.children.clone()
    }

    /// Returns whether this node has a registered click handler.
    #[must_use]
    pub fn is_clickable(&self) -> bool {
        self.on_click.is_some()
    }

    /// Invokes this element's click handler directly if present.
    pub fn trigger_click(&self) -> bool {
        if let Some(cb) = &self.on_click {
            (cb.borrow_mut())();
            true
        } else {
            false
        }
    }

    /// Collects all resolved non-empty text strings across the element subtree.
    #[must_use]
    pub fn collect_texts(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(t) = self.resolved_text() {
            out.push(t);
        }
        for child in self.resolved_children() {
            out.extend(child.collect_texts());
        }
        out
    }

    /// Finds the first `Button` whose resolved label matches `label` and triggers its click handler.
    pub fn click_first_button(&self, label: &str) -> bool {
        if self.tag == "Button"
            && self.resolved_text().as_deref() == Some(label)
            && self.trigger_click()
        {
            return true;
        }
        for child in self.resolved_children() {
            if child.click_first_button(label) {
                return true;
            }
        }
        false
    }

    /// Finds the first `Slider` in the tree and triggers its `changed(val)` callback.
    pub fn set_slider_value(&self, val: f32) -> bool {
        if self.tag == "Slider"
            && let Some(cb) = &self.on_change_f32
        {
            (cb.borrow_mut())(val);
            return true;
        }
        for child in self.resolved_children() {
            if child.set_slider_value(val) {
                return true;
            }
        }
        false
    }

    /// Executes `paint(bounds, ctx)` on this element if it is a `Canvas`.
    pub fn paint_into(&self, bounds: Rect, ctx: &mut PaintCtx) -> bool {
        if let Some(cb) = &self.on_paint {
            (cb.borrow_mut())(bounds, ctx);
            true
        } else {
            false
        }
    }

    /// Collects SEO `<Title>` and `<Meta>` elements from the subtree.
    #[must_use]
    pub fn collect_head_metadata(&self) -> HeadMetadata {
        let mut head = HeadMetadata::default();
        self.gather_head(&mut head);
        head
    }

    fn gather_head(&self, head: &mut HeadMetadata) {
        match self.tag.as_str() {
            "Title" => {
                if let Some(t) = self.resolved_text() {
                    head.title = Some(t);
                }
            }
            "Meta" => {
                let name = self
                    .props
                    .get("name")
                    .or_else(|| self.props.get("property"))
                    .and_then(PropValue::as_str)
                    .unwrap_or("description")
                    .to_owned();
                let content = self
                    .props
                    .get("content")
                    .and_then(PropValue::as_str)
                    .map(str::to_owned)
                    .or_else(|| self.resolved_text())
                    .unwrap_or_default();
                head.metas.push((name, content));
            }
            _ => {}
        }
        for child in self.resolved_children() {
            child.gather_head(head);
        }
    }
}

/// SEO `<head>` tags collected from `<Title>` and `<Meta>` nodes in a view tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeadMetadata {
    /// Document `<title>` string.
    pub title: Option<String>,
    /// `(name/property, content)` meta tag pairs.
    pub metas: Vec<(String, String)>,
}

impl HeadMetadata {
    /// Renders collected head metadata as escaped HTML tags.
    #[must_use]
    pub fn to_html(&self) -> String {
        let mut out = String::new();
        if let Some(title) = &self.title {
            out.push_str("<title>");
            push_escaped_html(&mut out, title);
            out.push_str("</title>");
        }
        for (name, content) in &self.metas {
            out.push_str("<meta name=\"");
            push_escaped_html(&mut out, name);
            out.push_str("\" content=\"");
            push_escaped_html(&mut out, content);
            out.push_str("\">");
        }
        out
    }
}

pub(crate) fn push_escaped_html(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}

/// Route guard authenticating or redirecting UI navigation.
#[derive(Clone)]
pub struct RequireAuth {
    redirect_to: String,
}

impl RequireAuth {
    /// Redirects unauthenticated navigation attempts to `login_path` (e.g. `"/login"`).
    #[must_use]
    pub fn redirect(login_path: impl Into<String>) -> Self {
        Self {
            redirect_to: login_path.into(),
        }
    }

    fn into_guard_fn(self) -> GuardFn {
        let target = self.redirect_to;
        Rc::new(move |_path, authenticated| {
            if authenticated {
                None
            } else {
                Some(target.clone())
            }
        })
    }
}

/// Feature dependency injection container resolving `Ctx<T>` and `Signal<T>` arguments.
#[derive(Default)]
pub struct FeatureCtx {
    values: RefCell<HashMap<TypeId, Box<dyn Any>>>,
}

impl FeatureCtx {
    fn provide<T: 'static>(&self, value: T) {
        self.values
            .borrow_mut()
            .insert(TypeId::of::<T>(), Box::new(Ctx::new(value)));
    }

    fn resolve_ctx<T: 'static>(&self) -> Ctx<T> {
        self.values
            .borrow()
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<Ctx<T>>().copied())
            .unwrap_or_else(|| {
                panic!(
                    "missing feature context provider for {}",
                    std::any::type_name::<T>()
                )
            })
    }

    fn resolve_signal<T: Default + Clone + 'static>(&self) -> Signal<T> {
        let mut map = self.values.borrow_mut();
        if let Some(existing) = map
            .get(&TypeId::of::<Signal<T>>())
            .and_then(|b| b.downcast_ref::<Signal<T>>().copied())
        {
            return existing;
        }
        let sig = signal(T::default());
        map.insert(TypeId::of::<Signal<T>>(), Box::new(sig));
        sig
    }
}

/// Extractor trait implemented by route component parameter types (`Ctx<T>`, `Signal<T>`).
pub trait FromFeatureCtx {
    /// Extracts this parameter from `ctx`.
    fn from_feature_ctx(ctx: &FeatureCtx) -> Self;
}

impl<T: 'static> FromFeatureCtx for Ctx<T> {
    fn from_feature_ctx(ctx: &FeatureCtx) -> Self {
        ctx.resolve_ctx::<T>()
    }
}

impl<T: Default + Clone + 'static> FromFeatureCtx for Signal<T> {
    fn from_feature_ctx(ctx: &FeatureCtx) -> Self {
        ctx.resolve_signal::<T>()
    }
}

/// Converts typed `#[component]` functions into feature-mounted route factories.
pub trait IntoRouteView<Marker> {
    /// Converts `self` into a boxed route view factory.
    fn into_route_factory(self) -> RouteFactory;
}

impl<F> IntoRouteView<()> for F
where
    F: Fn() -> Element + 'static,
{
    fn into_route_factory(self) -> RouteFactory {
        Rc::new(move |_| self())
    }
}

impl<F, A> IntoRouteView<(A,)> for F
where
    F: Fn(A) -> Element + 'static,
    A: FromFeatureCtx,
{
    fn into_route_factory(self) -> RouteFactory {
        Rc::new(move |ctx| self(A::from_feature_ctx(ctx)))
    }
}

impl<F, A, B> IntoRouteView<(A, B)> for F
where
    F: Fn(A, B) -> Element + 'static,
    A: FromFeatureCtx,
    B: FromFeatureCtx,
{
    fn into_route_factory(self) -> RouteFactory {
        Rc::new(move |ctx| self(A::from_feature_ctx(ctx), B::from_feature_ctx(ctx)))
    }
}

impl<F, A, B, C> IntoRouteView<(A, B, C)> for F
where
    F: Fn(A, B, C) -> Element + 'static,
    A: FromFeatureCtx,
    B: FromFeatureCtx,
    C: FromFeatureCtx,
{
    fn into_route_factory(self) -> RouteFactory {
        Rc::new(move |ctx| {
            self(
                A::from_feature_ctx(ctx),
                B::from_feature_ctx(ctx),
                C::from_feature_ctx(ctx),
            )
        })
    }
}

/// Result of resolving a route inside [`UiFeature::navigate`].
pub enum RouteOutcome {
    /// Rendered view tree at `path`.
    Rendered {
        /// Matched route path.
        path: String,
        /// Constructed UI element tree.
        element: Box<Element>,
    },
    /// Redirected by a guard (e.g. `RequireAuth::redirect("/login")`).
    Redirect(String),
    /// No route registered for the requested path.
    NotFound(String),
}

/// Composable feature module binding DI stores, auth guards, and routes.
#[derive(Default)]
pub struct UiFeature {
    ctx: FeatureCtx,
    guards: Vec<GuardFn>,
    routes: BTreeMap<String, RouteFactory>,
}

/// Starts building a composable UI feature module (`project_feature()`).
#[must_use]
pub fn feature() -> UiFeature {
    UiFeature::new()
}

impl UiFeature {
    /// Creates an empty UI feature module.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a shared feature context provider (accessible as `Ctx<T>` in routes).
    #[must_use]
    pub fn provide<T: 'static>(self, provider: impl FnOnce() -> T) -> Self {
        self.ctx.provide(provider());
        self
    }

    /// Attaches a route guard (such as [`RequireAuth`]) to all routes in this feature.
    #[must_use]
    pub fn wrap(mut self, auth: RequireAuth) -> Self {
        self.guards.push(auth.into_guard_fn());
        self
    }

    /// Mounts a component function onto `path`, injecting `Ctx<T>` and `Signal<T>` arguments automatically.
    #[must_use]
    pub fn route<F, M>(mut self, path: impl Into<String>, component: F) -> Self
    where
        F: IntoRouteView<M>,
    {
        self.routes
            .insert(path.into(), component.into_route_factory());
        self
    }

    /// Returns all registered route path patterns.
    #[must_use]
    pub fn route_paths(&self) -> Vec<String> {
        self.routes.keys().cloned().collect()
    }

    /// Resolves `Ctx<T>` directly from this feature's provider container.
    #[must_use]
    pub fn context<T: 'static>(&self) -> Ctx<T> {
        self.ctx.resolve_ctx::<T>()
    }

    /// Resolves `path` under the given authentication state.
    #[must_use]
    pub fn navigate(&self, path: &str, authenticated: bool) -> RouteOutcome {
        for guard in &self.guards {
            if let Some(redirect) = guard(path, authenticated) {
                return RouteOutcome::Redirect(redirect);
            }
        }
        match self.routes.get(path) {
            Some(factory) => RouteOutcome::Rendered {
                path: path.to_owned(),
                element: Box::new(factory(&self.ctx)),
            },
            None => RouteOutcome::NotFound(path.to_owned()),
        }
    }
}

/// Browser/sandbox persistent reactive signal backed by `hyve-serde` JSON serialization.
pub struct PersistentSignal<T: Clone + ToJson + FromJson + 'static> {
    key: String,
    inner: Signal<T>,
    storage: Rc<RefCell<BTreeMap<String, String>>>,
}

impl<T: Clone + ToJson + FromJson + 'static> PersistentSignal<T> {
    /// Creates or restores a persistent signal keyed by `key`.
    pub fn new(key: impl Into<String>, default_value: T) -> Self {
        let storage: Rc<RefCell<BTreeMap<String, String>>> = Rc::new(RefCell::new(BTreeMap::new()));
        let key = key.into();
        let initial = storage
            .borrow()
            .get(&key)
            .and_then(|raw| from_json_str::<T>(raw).ok())
            .unwrap_or(default_value);
        storage
            .borrow_mut()
            .insert(key.clone(), to_json_string(&initial));
        Self {
            key,
            inner: signal(initial),
            storage,
        }
    }

    /// Reads the current persisted value.
    #[must_use]
    pub fn get(&self) -> T {
        self.inner.get()
    }

    /// Updates both the live signal and serialized storage representation.
    pub fn set(&self, value: T) {
        self.storage
            .borrow_mut()
            .insert(self.key.clone(), to_json_string(&value));
        self.inner.set(value);
    }

    /// Returns the raw serialized JSON snapshot stored under `key`.
    pub fn snapshot_json(&self) -> HyveResult<String> {
        Ok(self
            .storage
            .borrow()
            .get(&self.key)
            .cloned()
            .unwrap_or_else(|| "null".to_owned()))
    }
}
