//! `serge-router` — app assembly and typed routing (`docs/01-dx-and-dsl.md`
//! §1, 앱 루트): [`SergeApp`] is the root, [`feature()`] groups provided
//! values, guards and routes, and the router mounts one screen at a time.
//!
//! - A route's component takes only `Ctx<T>`, `Path<T>` and `Query<T>`;
//!   placeholders are `{name}` (`/projects/{id}`).
//! - A feature's scope is created the first time one of its routes is
//!   entered and lives until the app ends; `.provide(|| ..)` runs then,
//!   under the feature's own reactive owner, so its signals survive screen
//!   changes (`docs/04-open-questions.md` #3).
//! - Each screen has its own view: leaving it disposes the view, which
//!   cancels its tasks and in-flight `#[server]` calls.
//! - `RequireAuth` reads [`AuthState`] provided at the app root; it only
//!   redirects and never replaces the server's own checks.

use std::cell::{OnceCell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use hyve::api::Transport;
use hyve::state::{Owner, Signal, signal};
use hyve::{HyveResult, Scope};
use serge_core::builder::region;
use serge_core::{Element, RouteComponent, RouteParams};

/// Whether the user is signed in, as the UI knows it. Provide it at the app
/// root and update it on sign-in and sign-out; `RequireAuth` reads it.
#[derive(Clone, Copy, Debug)]
pub struct AuthState {
    signed_in: Signal<bool>,
}

impl AuthState {
    /// A state starting as `signed_in`.
    pub fn new(signed_in: bool) -> AuthState {
        AuthState {
            signed_in: signal(signed_in),
        }
    }

    /// Whether signed in (tracked).
    pub fn is_signed_in(&self) -> bool {
        self.signed_in.get()
    }

    /// Records a sign-in or sign-out.
    pub fn set(&self, signed_in: bool) {
        self.signed_in.set(signed_in);
    }
}

/// A screen guard on a feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Guard {
    /// Without a signed-in user, go to the path instead.
    RequireAuth {
        /// Where to send signed-out users.
        redirect: String,
    },
}

/// Screens that need a signed-in user.
#[derive(Debug)]
pub struct RequireAuth;

impl RequireAuth {
    /// Redirects signed-out users to `path` (`RequireAuth::redirect("/login")`).
    pub fn redirect(path: impl Into<String>) -> Guard {
        Guard::RequireAuth {
            redirect: path.into(),
        }
    }
}

type Provider = Rc<dyn Fn(&Scope)>;
type Render = Rc<dyn Fn() -> HyveResult<Element>>;

/// A UI feature: values it provides, guards, routes and sub-features.
#[derive(Default)]
pub struct UiFeature {
    provides: Vec<Provider>,
    guards: Vec<Guard>,
    routes: Vec<(String, Render)>,
    features: Vec<UiFeature>,
}

impl fmt::Debug for UiFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiFeature")
            .field("guards", &self.guards)
            .field(
                "routes",
                &self.routes.iter().map(|r| &r.0).collect::<Vec<_>>(),
            )
            .field("features", &self.features)
            .finish_non_exhaustive()
    }
}

/// A new feature (`serge::feature()`).
pub fn feature() -> UiFeature {
    UiFeature::default()
}

impl UiFeature {
    /// Provides the value `make()` returns to this feature's screens (and
    /// sub-features). It runs once, when the feature is first entered.
    pub fn provide<T: Send + Sync + 'static>(mut self, make: impl Fn() -> T + 'static) -> Self {
        self.provides.push(Rc::new(move |scope: &Scope| {
            scope.provide(make());
        }));
        self
    }

    /// Guards every route of this feature (and its sub-features).
    pub fn wrap(mut self, guard: Guard) -> Self {
        self.guards.push(guard);
        self
    }

    /// Mounts `component` at `path` (`/projects/{id}`).
    pub fn route<M>(mut self, path: &str, component: impl RouteComponent<M>) -> Self {
        let c = Rc::new(component);
        self.routes
            .push((path.to_owned(), Rc::new(move || c.render())));
        self
    }

    /// Adds a sub-feature.
    pub fn feature(mut self, sub: UiFeature) -> Self {
        self.features.push(sub);
        self
    }
}

/// Why an app could not be assembled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssemblyError(pub Vec<String>);

impl fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "the serge app is invalid:")?;
        for p in &self.0 {
            writeln!(f, "  - {p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AssemblyError {}

/// The app root (`SergeApp::new().provide(..).feature(..).run()`).
pub struct SergeApp {
    scope: Scope,
    features: Vec<UiFeature>,
    root: Option<Render>,
    layout: Option<Layout>,
    start: String,
    title: String,
}

type Layout = Rc<dyn Fn(Element) -> Element>;

impl fmt::Debug for SergeApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SergeApp")
            .field("title", &self.title)
            .field("features", &self.features)
            .finish_non_exhaustive()
    }
}

impl Default for SergeApp {
    fn default() -> Self {
        Self::new()
    }
}

impl SergeApp {
    /// An empty app.
    pub fn new() -> SergeApp {
        SergeApp {
            scope: Scope::root(),
            features: Vec::new(),
            root: None,
            layout: None,
            start: "/".into(),
            title: "serge".into(),
        }
    }

    /// Wraps every screen: `layout(screen)` is built once, at mount, with
    /// the current screen as the element to place (a navigation bar around
    /// it, say). Without one the screen is the whole app.
    pub fn layout(mut self, layout: impl Fn(Element) -> Element + 'static) -> Self {
        self.layout = Some(Rc::new(layout));
        self
    }

    /// Provides `value` to the whole app.
    pub fn provide<T: Send + Sync + 'static>(self, value: T) -> Self {
        self.scope.provide(value);
        self
    }

    /// The transport `#[server]` calls use (a remote transport, or an
    /// in-process one such as `orbic::InProcess`).
    pub fn transport(self, transport: Arc<dyn Transport>) -> Self {
        hyve::api::provide_transport(&self.scope, transport);
        self
    }

    /// Adds a feature.
    pub fn feature(mut self, f: UiFeature) -> Self {
        self.features.push(f);
        self
    }

    /// The first screen of an app without routes (or shown for `/` when no
    /// route matches it).
    pub fn root<M>(mut self, component: impl RouteComponent<M>) -> Self {
        let c = Rc::new(component);
        self.root = Some(Rc::new(move || c.render()));
        self
    }

    /// Where a native app starts (default `/`; the web uses the address).
    pub fn start_path(mut self, path: impl Into<String>) -> Self {
        self.start = path.into();
        self
    }

    /// The window or page title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Checks the app and builds its router.
    pub fn assemble(self) -> Result<Assembled, AssemblyError> {
        let mut problems = Vec::new();
        let mut flat = Vec::new();
        let root_feature = Rc::new(FeatureRt {
            parent: None,
            provides: Vec::new(),
            scope: OnceCell::new(),
        });
        fn walk(
            f: UiFeature,
            parent: &Rc<FeatureRt>,
            guards: &[Guard],
            flat: &mut Vec<FlatRoute>,
            problems: &mut Vec<String>,
        ) {
            let rt = Rc::new(FeatureRt {
                parent: Some(parent.clone()),
                provides: f.provides,
                scope: OnceCell::new(),
            });
            let mut g = guards.to_vec();
            g.extend(f.guards);
            for (path, render) in f.routes {
                match Pattern::parse(&path) {
                    Ok(pattern) => {
                        if flat
                            .iter()
                            .any(|r: &FlatRoute| r.pattern.source == pattern.source)
                        {
                            problems.push(format!("the route `{path}` is defined twice"));
                        }
                        flat.push(FlatRoute {
                            pattern,
                            render,
                            feature: rt.clone(),
                            guards: g.clone(),
                        });
                    }
                    Err(e) => problems.push(e),
                }
            }
            for sub in f.features {
                walk(sub, &rt, &g, flat, problems);
            }
        }
        for f in self.features {
            walk(f, &root_feature, &[], &mut flat, &mut problems);
        }
        let needs_auth = flat.iter().any(|r| {
            r.guards
                .iter()
                .any(|g| matches!(g, Guard::RequireAuth { .. }))
        });
        if needs_auth && self.scope.get::<AuthState>().is_none() {
            problems.push(
                "`RequireAuth` is used but no `AuthState` is provided; add `.provide(AuthState::new(false))` to the app".into(),
            );
        }
        if flat.is_empty() && self.root.is_none() {
            problems.push("the app has no routes and no `.root(..)` screen".into());
        }
        if !problems.is_empty() {
            return Err(AssemblyError(problems));
        }
        let _ = root_feature.scope.set(self.scope.clone());
        let router = Rc::new(Router {
            location: RefCell::new(None),
            start: self.start,
            routes: flat,
            root: self.root,
            app_scope: self.scope.clone(),
            owner: RefCell::new(None),
            history: RefCell::new(None),
        });
        Ok(Assembled {
            scope: self.scope,
            title: self.title,
            layout: self.layout,
            router,
        })
    }
}

struct FeatureRt {
    parent: Option<Rc<FeatureRt>>,
    provides: Vec<Provider>,
    scope: OnceCell<Scope>,
}

impl FeatureRt {
    /// The feature's scope, created (and its values provided) on first use.
    fn scope(&self, owner: Owner) -> Scope {
        self.scope
            .get_or_init(|| {
                let parent = self
                    .parent
                    .as_ref()
                    .map(|p| p.scope(owner))
                    .expect("the root feature's scope is set at assembly");
                let scope = parent.child();
                let feature_owner = owner.with(Owner::new);
                feature_owner.with(|| {
                    scope.enter(|| {
                        for p in &self.provides {
                            p(&scope);
                        }
                    })
                });
                scope
            })
            .clone()
    }
}

struct FlatRoute {
    pattern: Pattern,
    render: Render,
    feature: Rc<FeatureRt>,
    guards: Vec<Guard>,
}

#[derive(Debug)]
/// A parsed whole-segment UI route pattern.
#[derive(Clone)]
pub struct Pattern {
    source: String,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
enum Segment {
    Fixed(String),
    Param(String),
}

impl Pattern {
    /// Parses a route pattern such as `/projects/{id}`.
    pub fn parse(path: &str) -> Result<Pattern, String> {
        if !path.starts_with('/') {
            return Err(format!("the route `{path}` must start with `/`"));
        }
        let mut segments = Vec::new();
        for s in path.split('/').filter(|s| !s.is_empty()) {
            if let Some(name) = s.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Err(format!(
                        "`{{{name}}}` in `{path}` is not a placeholder name"
                    ));
                }
                segments.push(Segment::Param(name.to_owned()));
            } else if s.contains(['{', '}']) {
                return Err(format!(
                    "`{s}` in `{path}`: a placeholder takes a whole segment"
                ));
            } else {
                segments.push(Segment::Fixed(s.to_owned()));
            }
        }
        Ok(Pattern {
            source: path.to_owned(),
            segments,
        })
    }

    /// Matches a concrete path, returning its named parameters in declaration order.
    pub fn matches(&self, path: &str) -> Option<Vec<(String, String)>> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() != self.segments.len() {
            return None;
        }
        let mut params = Vec::new();
        for (seg, part) in self.segments.iter().zip(parts) {
            match seg {
                Segment::Fixed(f) if f == part => {}
                Segment::Fixed(_) => return None,
                Segment::Param(name) => {
                    params.push((name.clone(), hyve::api::params::percent_decode(part).ok()?))
                }
            }
        }
        Some(params)
    }
}

/// Where navigation is recorded: the browser history on the web, a stack on
/// native targets.
pub trait History {
    /// The address now.
    fn current(&self) -> String;
    /// A new entry.
    fn push(&self, path: &str);
    /// Replaces the current entry.
    fn replace(&self, path: &str);
    /// Goes back one entry; returns the new address, if any.
    fn back(&self) -> Option<String>;
}

/// The default history of native targets.
#[derive(Debug, Default)]
pub struct MemoryHistory(RefCell<Vec<String>>);

impl MemoryHistory {
    /// A history starting at `path`.
    pub fn new(path: &str) -> MemoryHistory {
        MemoryHistory(RefCell::new(vec![path.to_owned()]))
    }
}

impl History for MemoryHistory {
    fn current(&self) -> String {
        self.0
            .borrow()
            .last()
            .cloned()
            .unwrap_or_else(|| "/".into())
    }
    fn push(&self, path: &str) {
        self.0.borrow_mut().push(path.to_owned());
    }
    fn replace(&self, path: &str) {
        let mut v = self.0.borrow_mut();
        v.pop();
        v.push(path.to_owned());
    }
    fn back(&self) -> Option<String> {
        let mut v = self.0.borrow_mut();
        if v.len() > 1 {
            v.pop();
        }
        v.last().cloned()
    }
}

/// The router of a running app.
pub struct Router {
    location: RefCell<Option<Signal<String>>>,
    start: String,
    routes: Vec<FlatRoute>,
    root: Option<Render>,
    app_scope: Scope,
    owner: RefCell<Option<Owner>>,
    history: RefCell<Option<Rc<dyn History>>>,
}

impl fmt::Debug for Router {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Router").finish_non_exhaustive()
    }
}

impl Router {
    fn location(&self) -> Signal<String> {
        (*self.location.borrow()).expect("the router is mounted")
    }

    /// The current address (tracked).
    pub fn path(&self) -> String {
        self.location().get()
    }

    /// Opens `path` as a new history entry.
    pub fn navigate(&self, path: &str) {
        if let Some(h) = self.history.borrow().clone() {
            h.push(path);
        }
        self.location().set(path.to_owned());
    }

    /// Replaces the current entry with `path`.
    pub fn replace(&self, path: &str) {
        if let Some(h) = self.history.borrow().clone() {
            h.replace(path);
        }
        self.location().set(path.to_owned());
    }

    /// Goes back one entry.
    pub fn back(&self) {
        let h = self.history.borrow().clone();
        if let Some(p) = h.and_then(|h| h.back()) {
            self.location().set(p);
        }
    }

    /// The address changed outside the router (browser back/forward).
    pub fn set_location(&self, path: &str) {
        self.location().set(path.to_owned());
    }

    fn screen(self: &Rc<Self>, address: &str) -> Element {
        let (path, query) = address.split_once('?').unwrap_or((address, ""));
        let owner = (*self.owner.borrow()).expect("mounted");
        let found = self
            .routes
            .iter()
            .find_map(|r| r.pattern.matches(path).map(|p| (r, p)));
        let (render, scope, params): (Render, Scope, Vec<(String, String)>) = match found {
            Some((r, params)) => {
                for g in &r.guards {
                    match g {
                        Guard::RequireAuth { redirect } => {
                            let signed_in = self
                                .app_scope
                                .get::<AuthState>()
                                .is_some_and(|a| a.is_signed_in());
                            if !signed_in && path != redirect {
                                let me = self.clone();
                                let to = redirect.clone();
                                // Redirect after this evaluation finishes.
                                serge_core::spawn(async move { me.replace(&to) });
                                return Element::empty();
                            }
                        }
                    }
                }
                (r.render.clone(), r.feature.scope(owner), params)
            }
            None => match &self.root {
                Some(root) if path == "/" || self.routes.is_empty() => {
                    (root.clone(), self.app_scope.clone(), Vec::new())
                }
                _ => {
                    let msg = format!("No screen for {path}");
                    return message(msg);
                }
            },
        };
        let screen_scope = scope.child();
        screen_scope.provide(RouteParams {
            path: params,
            query: query.to_owned(),
        });
        Element::scoped(
            screen_scope,
            region(move || match render() {
                Ok(e) => e,
                Err(e) => message(format!("This address is not valid: {}", e.message())),
            }),
        )
    }
}

fn message(text: String) -> Element {
    use serge_core::builder::{Build, Lit};
    use serge_core::widgets::Text;
    Text::__new().__text(Lit(text)).build()
}

/// An assembled app: its scope, title and router.
pub struct Assembled {
    /// The app scope (provided values, the transport).
    pub scope: Scope,
    /// The window or page title.
    pub title: String,
    layout: Option<Layout>,
    router: Rc<Router>,
}

impl fmt::Debug for Assembled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Assembled")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl Assembled {
    /// UI routes in matching order, with whether they have client-side guards.
    pub fn routes(&self) -> impl Iterator<Item = (&str, bool)> {
        self.router
            .routes
            .iter()
            .map(|r| (r.pattern.source.as_str(), !r.guards.is_empty()))
    }
    /// Whether the app declares a root screen in addition to its named routes.
    pub fn has_root(&self) -> bool {
        self.router.root.is_some()
    }

    /// The router.
    pub fn router(&self) -> Rc<Router> {
        self.router.clone()
    }

    /// Uses `history` instead of the in-memory default (the web target
    /// passes the browser's).
    pub fn with_history(self, history: Rc<dyn History>) -> Self {
        *self.router.history.borrow_mut() = Some(history);
        self
    }

    /// The app's element: the current screen. Call it inside the root view
    /// (the target's mount does); it installs the navigation hook.
    pub fn element(&self) -> Element {
        let r = self.router.clone();
        *r.owner.borrow_mut() = Some(Owner::current());
        let existing = r.history.borrow().clone();
        let start = match existing {
            Some(h) => h.current(),
            None => {
                let h = Rc::new(MemoryHistory::new(&r.start));
                *r.history.borrow_mut() = Some(h);
                r.start.clone()
            }
        };
        *r.location.borrow_mut() = Some(signal(start));
        let nav = r.clone();
        serge_core::set_navigator(Rc::new(move |p| nav.navigate(p)));
        BACK.with(|b| {
            let r2 = r.clone();
            *b.borrow_mut() = Some(Rc::new(move || r2.back()));
        });
        let loc = r.location();
        LOCATION.with(|l| l.set(Some(loc)));
        let screens = region(move || {
            let address = r.path();
            r.screen(&address)
        });
        match &self.layout {
            Some(layout) => layout(screens),
            None => screens,
        }
    }
}

thread_local! {
    static BACK: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
    static LOCATION: std::cell::Cell<Option<Signal<String>>> = const { std::cell::Cell::new(None) };
}

/// The current address (`/projects/7?tab=a`), tracked: a navigation bar
/// that reads it updates on navigation. `"/"` before the app is mounted.
pub fn location() -> String {
    LOCATION
        .with(|l| l.get())
        .map_or_else(|| "/".to_owned(), |s| s.get())
}

/// Goes back one screen.
pub fn back() {
    let f = BACK.with(|b| b.borrow().clone());
    if let Some(f) = f {
        f();
    }
}

/// Opens `path` in the app's router.
pub fn navigate(path: &str) {
    serge_core::navigate(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_match_and_decode() {
        let p = Pattern::parse("/projects/{id}").unwrap();
        assert_eq!(
            p.matches("/projects/7"),
            Some(vec![("id".into(), "7".into())])
        );
        assert_eq!(
            p.matches("/projects/a%20b"),
            Some(vec![("id".into(), "a b".into())])
        );
        assert_eq!(p.matches("/projects"), None);
        assert!(Pattern::parse("projects").is_err());
        assert!(Pattern::parse("/p/{a b}").is_err());
        assert!(Pattern::parse("/p/x{id}").is_err());
    }
}
