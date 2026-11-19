//! Web DOM template extraction, fine-grained surgical slot patching, and SSR handoff contracts.

use crate::core::{Element, HeadMetadata, PropValue, RouteOutcome, UiFeature, push_escaped_html};
use hyve::prelude::{CancellationToken, FromJson, HyveError, HyveResult, JsonValue, ToJson};
use std::collections::BTreeMap;
use std::rc::Rc;

type SlotGetter = Rc<dyn Fn() -> String>;

/// Static HTML template skeleton + fine-grained reactive slot closures (Leptos-inspired).
///
/// Static DOM markup is separated at compile/mount time so signal updates only evaluate and patch
/// changed slots rather than diffing or rebuilding virtual DOM trees.
#[derive(Clone)]
pub struct DomTemplate {
    /// Static HTML template containing `<!--slot:N-->` / `{{slot:N}}` markers for dynamic regions.
    pub template_html: String,
    slots: Vec<DynamicSlot>,
    head: HeadMetadata,
}

#[derive(Clone)]
struct DynamicSlot {
    id: usize,
    getter: SlotGetter,
}

/// Surgical DOM patch targeting a single slot offset whose text changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotPatch {
    /// Zero-based reactive slot index inside [`DomTemplate`].
    pub slot_id: usize,
    /// Updated text value for that slot.
    pub new_value: String,
}

/// Mounted client DOM instance tracking previous slot values for minimal surgical DOM mutations.
pub struct MountedDom {
    template: DomTemplate,
    cached_values: Vec<String>,
}

impl DomTemplate {
    /// Extracts static DOM markup and fine-grained reactive slot bindings from `element`.
    #[must_use]
    pub fn compile(element: &Element) -> Self {
        let mut html = String::new();
        let mut slots = Vec::new();
        compile_node(element, &mut html, &mut slots);
        let head = element.collect_head_metadata();
        Self {
            template_html: html,
            slots,
            head,
        }
    }

    /// Returns the number of dynamic reactive slots extracted from the template.
    #[must_use]
    pub fn dynamic_slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Renders full HTML with current slot values evaluated and escaped.
    #[must_use]
    pub fn render_html(&self) -> String {
        let mut out = self.template_html.clone();
        for slot in &self.slots {
            let marker = format!("{{{{slot:{}}}}}", slot.id);
            let mut escaped = String::new();
            push_escaped_html(&mut escaped, &(slot.getter)());
            out = out.replace(&marker, &escaped);
        }
        out
    }

    /// Returns collected SEO `<title>` and `<meta>` HTML string.
    #[must_use]
    pub fn head_html(&self) -> String {
        self.head.to_html()
    }

    /// Mounts this compiled template into a client DOM instance ready for surgical updates.
    #[must_use]
    pub fn mount(self) -> MountedDom {
        let cached_values = self.slots.iter().map(|s| (s.getter)()).collect();
        MountedDom {
            template: self,
            cached_values,
        }
    }
}

impl MountedDom {
    /// Evaluates only reactive slots and emits surgical patches for changed slots only.
    pub fn patch_dirty(&mut self) -> Vec<SlotPatch> {
        let mut patches = Vec::new();
        for (idx, slot) in self.template.slots.iter().enumerate() {
            let current = (slot.getter)();
            if self.cached_values[idx] != current {
                self.cached_values[idx] = current.clone();
                patches.push(SlotPatch {
                    slot_id: slot.id,
                    new_value: current,
                });
            }
        }
        patches
    }

    /// Renders the latest patched HTML snapshot.
    #[must_use]
    pub fn current_html(&self) -> String {
        self.template.render_html()
    }
}

fn compile_node(el: &Element, out: &mut String, slots: &mut Vec<DynamicSlot>) {
    match el.tag.as_str() {
        "Title" | "Meta" | "Empty" => return,
        "Fragment" => {
            for ch in el.resolved_children() {
                compile_node(&ch, out, slots);
            }
            return;
        }
        _ => {}
    }

    let html_tag = match el.tag.as_str() {
        "Card" | "Stack" | "Canvas" => "div",
        "Text" => "span",
        "Button" => "button",
        "Slider" => "input",
        "ProgressBar" => "progress",
        _ => "div",
    };

    out.push('<');
    out.push_str(html_tag);
    out.push_str(" data-serge=\"");
    out.push_str(&el.tag);
    out.push('"');

    if let Some(cls) = el.web_attrs.get("class") {
        out.push_str(" class=\"");
        push_escaped_html(out, &cls.to_display_string());
        out.push('"');
    }

    if el.tag == "Slider" {
        out.push_str(" type=\"range\"");
    }

    if el.resolved_value().is_some() {
        let slot_id = slots.len();
        let el_clone = el.clone();
        slots.push(DynamicSlot {
            id: slot_id,
            getter: Rc::new(move || {
                el_clone
                    .resolved_value()
                    .and_then(|v| v.as_f32())
                    .map(|f| format!("{f}"))
                    .unwrap_or_default()
            }),
        });
        out.push_str(&format!(" value=\"{{{{slot:{slot_id}}}}}\""));
    }

    out.push('>');

    if el.has_reactive_text() {
        let slot_id = slots.len();
        let el_clone = el.clone();
        slots.push(DynamicSlot {
            id: slot_id,
            getter: Rc::new(move || el_clone.resolved_text().unwrap_or_default()),
        });
        out.push_str(&format!("{{{{slot:{slot_id}}}}}"));
    } else if let Some(text) = el.resolved_text() {
        push_escaped_html(out, &text);
    }

    if let Some(PropValue::Range(min, max)) = el.props.get("range") {
        let _ = (min, max);
    }

    for child in el.resolved_children() {
        compile_node(&child, out, slots);
    }

    out.push_str("</");
    out.push_str(html_tag);
    out.push('>');
}

/// Route rendering strategy declared in [`SergeBundleManifest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteRenderMode {
    /// Server-side rendered with progressive hydration.
    Ssr,
    /// Client-side rendered SPA shell.
    Csr,
    /// Pre-rendered static site generation (SSG).
    Ssg,
}

/// Route entry exchanged between `serge` and `orbic::orbic-adapter-serge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestRoute {
    /// Route path pattern (e.g. `"/projects"`).
    pub path: String,
    /// Rendering mode (`ssr`, `csr`, `ssg`).
    pub mode: RouteRenderMode,
}

/// Production build manifest exchanged between `serge` (`hyve build`) and `orbic`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SergeBundleManifest {
    /// Schema compatibility version (`1`).
    pub schema_version: u32,
    /// Registered application routes.
    pub routes: Vec<ManifestRoute>,
    /// Hashed asset mapping (`logical_path -> hashed_filename`).
    pub hashed_assets: BTreeMap<String, String>,
}

impl SergeBundleManifest {
    /// Current manifest schema version (`1`).
    pub const CURRENT_SCHEMA_VERSION: u32 = 1;

    /// Builds a manifest from a [`UiFeature`] and asset map.
    #[must_use]
    pub fn from_feature(feature: &UiFeature, hashed_assets: BTreeMap<String, String>) -> Self {
        let routes = feature
            .route_paths()
            .into_iter()
            .map(|path| ManifestRoute {
                path,
                mode: RouteRenderMode::Ssr,
            })
            .collect();
        Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            routes,
            hashed_assets,
        }
    }
}

impl ToJson for SergeBundleManifest {
    fn to_json(&self) -> JsonValue {
        let routes_json = JsonValue::Array(
            self.routes
                .iter()
                .map(|r| {
                    let mode_str = match r.mode {
                        RouteRenderMode::Ssr => "ssr",
                        RouteRenderMode::Csr => "csr",
                        RouteRenderMode::Ssg => "ssg",
                    };
                    JsonValue::Object(vec![
                        ("path".to_owned(), JsonValue::Str(r.path.clone())),
                        ("mode".to_owned(), JsonValue::Str(mode_str.to_owned())),
                    ])
                })
                .collect(),
        );
        let assets_json = JsonValue::Object(
            self.hashed_assets
                .iter()
                .map(|(k, v)| (k.clone(), JsonValue::Str(v.clone())))
                .collect(),
        );
        JsonValue::Object(vec![
            (
                "schemaVersion".to_owned(),
                JsonValue::Number(f64::from(self.schema_version)),
            ),
            ("routes".to_owned(), routes_json),
            ("hashedAssets".to_owned(), assets_json),
        ])
    }
}

impl FromJson for SergeBundleManifest {
    fn from_json(val: &JsonValue) -> HyveResult<Self> {
        let schema_version = val
            .get("schemaVersion")
            .and_then(JsonValue::as_f64)
            .map(|v| v as u32)
            .ok_or_else(|| HyveError::domain("invalid_manifest", "missing schemaVersion"))?;
        let mut routes = Vec::new();
        if let Some(JsonValue::Array(arr)) = val.get("routes") {
            for item in arr {
                let path = item
                    .get("path")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| HyveError::domain("invalid_manifest", "missing route path"))?
                    .to_owned();
                let mode = match item.get("mode").and_then(JsonValue::as_str) {
                    Some("csr") => RouteRenderMode::Csr,
                    Some("ssg") => RouteRenderMode::Ssg,
                    _ => RouteRenderMode::Ssr,
                };
                routes.push(ManifestRoute { path, mode });
            }
        }
        let mut hashed_assets = BTreeMap::new();
        if let Some(JsonValue::Object(entries)) = val.get("hashedAssets") {
            for (k, v) in entries {
                if let Some(s) = v.as_str() {
                    hashed_assets.insert(k.clone(), s.to_owned());
                }
            }
        }
        Ok(Self {
            schema_version,
            routes,
            hashed_assets,
        })
    }
}

/// Incoming SSR render request handed off from `orbic` to `serge-web-ssr`.
#[derive(Debug, Clone)]
pub struct SsrRequest {
    /// Requested URL path.
    pub path: String,
    /// Authenticated caller indicator checked by route guards.
    pub authenticated: bool,
    /// Cooperative cancellation token triggered on client disconnect.
    pub cancel: CancellationToken,
}

/// Rendered SSR response envelope returned to `orbic`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsrResponse {
    /// HTTP status code (`200`, `302`, `404`).
    pub status: u16,
    /// Redirect location when `status == 302`.
    pub redirect_location: Option<String>,
    /// `<title>` and `<meta>` HTML string for document `<head>`.
    pub head_html: String,
    /// Rendered DOM HTML body.
    pub body_html: String,
}

/// Standard SSR hosting contract consumed by `orbic::orbic-adapter-serge`.
pub trait SergeSsrHandler {
    /// Renders an SSR document for `req`, aborting early if `req.cancel` is cancelled.
    fn render_ssr(&self, req: &SsrRequest) -> HyveResult<SsrResponse>;
}

/// Renders a single root [`UiFeature`] in-process for SSR testing and `orbic` integration.
pub struct FeatureSsrHandler {
    feature: UiFeature,
}

impl FeatureSsrHandler {
    /// Wraps a [`UiFeature`] as a [`SergeSsrHandler`].
    #[must_use]
    pub fn new(feature: UiFeature) -> Self {
        Self { feature }
    }
}

impl SergeSsrHandler for FeatureSsrHandler {
    fn render_ssr(&self, req: &SsrRequest) -> HyveResult<SsrResponse> {
        if req.cancel.is_cancelled() {
            return Err(HyveError::infra(
                "cancelled",
                "ssr render aborted on disconnect",
                false,
            ));
        }
        match self.feature.navigate(&req.path, req.authenticated) {
            RouteOutcome::Redirect(target) => Ok(SsrResponse {
                status: 302,
                redirect_location: Some(target),
                head_html: String::new(),
                body_html: String::new(),
            }),
            RouteOutcome::NotFound(_) => Ok(SsrResponse {
                status: 404,
                redirect_location: None,
                head_html: String::new(),
                body_html: "<div>Not Found</div>".to_owned(),
            }),
            RouteOutcome::Rendered { element, .. } => {
                let tmpl = DomTemplate::compile(&element);
                Ok(SsrResponse {
                    status: 200,
                    redirect_location: None,
                    head_html: tmpl.head_html(),
                    body_html: tmpl.render_html(),
                })
            }
        }
    }
}
