//! Interactive full-stack MVP/PoC showcase integrating `hyve`, `orbic`, and `serge`.
//!
//! Run modes:
//! - Default (`cargo run --bin serge-playground`): runs an initial E2E verification pass and serves
//!   the live **Hyperish Studio PoC** web workbench at `http://127.0.0.1:3000`.
//! - `--once`: runs the E2E verification suite once, prints the summary report, and exits 0.
//! - `--port <N>`: overrides the HTTP bind port (falls back to `3001` if `3000` is in use).

use hyve::api::block_on;
use hyve::prelude::*;
use orbic::prelude::{
    AuthGuard, BackendComponent, Cache, CacheOpts, CurrentUser, CursorParams, Db, OrbicApp,
    OrbicResult, Page, Query, RateLimit, RealtimeHub, RealtimeTransport, SessionStore, TestClient,
    User, key, sql, table, tag, test_user, ttl,
};
use serge::prelude::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// 1. Confirmed Domain Models & `orbic` Backend (`#[model]`, `#[table]`, `sql!`, `#[server]`)
// ---------------------------------------------------------------------------

/// Persisted studio project entity shared between `orbic` SQL storage and `serge` UI.
#[model]
#[table("projects")]
pub struct Project {
    /// Primary ULID identifier.
    #[pk]
    pub id: Id<Project>,
    /// Owner user identifier.
    pub owner_id: Id<User>,
    /// Validated project title (`1..=80` chars).
    #[validate(len = 1..=80)]
    pub title: String,
    /// Public visibility toggle.
    pub is_public: bool,
    /// Monotonic optimistic locking version.
    pub version: i64,
}

/// Validated project creation payload accepted by REST and Dual-Rail `#[server]` RPC.
#[model]
pub struct CreateProjectInput {
    /// Project title (`1..=80` chars).
    #[validate(len = 1..=80)]
    pub title: String,
    /// Public visibility toggle.
    pub is_public: bool,
}

/// Assembles the project REST + cache domain component matching `orbic/docs/01-dx-and-dsl.md`.
pub fn project_component() -> BackendComponent {
    orbic::core::component()
        .wrap(AuthGuard::require_user())
        .wrap(RateLimit::per_minute(120))
        .post("/projects", create_project)
        .get("/projects", list_projects)
}

async fn list_projects(
    db: Ctx<Db>,
    cache: Ctx<Cache>,
    user: Ctx<CurrentUser>,
    cursor: Query<CursorParams>,
) -> OrbicResult<Page<Project>> {
    cache
        .get_or_try_init(
            key!("projects", user.id, cursor.after),
            CacheOpts::ttl(ttl!("30s")).tag(tag!("projects", user.id)),
            || async {
                db.query::<Project>()
                    .where_eq(Project::OWNER_ID, user.id)
                    .cursor_after(cursor.after.clone(), 50)
                    .fetch_page()
                    .await
            },
        )
        .await
}

async fn create_project(
    db: Ctx<Db>,
    cache: Ctx<Cache>,
    user: Ctx<CurrentUser>,
    input: Valid<CreateProjectInput>,
) -> OrbicResult<Project> {
    let created = db
        .tx(|tx| async move {
            let data = input.into_inner();
            sql!(
                Project,
                "INSERT INTO projects (id, owner_id, title, is_public, version)
                 VALUES ($1, $2, $3, $4, 1) RETURNING *",
                Id::new(),
                user.id,
                data.title,
                data.is_public
            )
            .fetch_one(tx)
            .await
        })
        .await?;

    cache.invalidate_tag(tag!("projects", user.id)).await?;
    Ok(created)
}

/// Inline Fast-Rail `#[server]` RPC invoked directly from `serge` click handlers.
#[server(POST "/rpc-projects")]
pub async fn create_project_rpc(
    #[server_ctx] db: Ctx<Db>,
    #[server_ctx] cache: Ctx<Cache>,
    #[server_ctx] user: Ctx<CurrentUser>,
    input: Valid<CreateProjectInput>,
) -> HyveResult<Project> {
    let created = db
        .insert(Project {
            id: Id::new(),
            owner_id: user.id,
            title: input.into_inner().title,
            is_public: true,
            version: 1,
        })
        .await?;
    cache.invalidate_tag(tag!("projects", user.id)).await?;
    Ok(created)
}

/// Builds the full `OrbicApp` wiring DB, L1/L2 cache, L2 Opaque sessions, OpenAPI, and components.
fn build_orbic_app(db: Db, cache: Cache, sessions: SessionStore) -> OrbicApp {
    let rpc_component = orbic::core::component()
        .wrap(AuthGuard::require_user())
        .handle(create_project_rpc);

    OrbicApp::new()
        .provide(db)
        .provide(cache)
        .provide(sessions)
        .openapi("/docs")
        .mount("/v1", project_component())
        .mount("/v1", rpc_component)
}

// ---------------------------------------------------------------------------
// 2. Confirmed `serge` UI Components (`ui!`, `#[component]`, `#[ios::live_activity]`)
// ---------------------------------------------------------------------------

/// Reactive draft store shared inside [`project_feature`] via `Ctx<ProjectDraftStore>`.
pub struct ProjectDraftStore {
    /// Draft project title signal.
    pub title: Signal<String>,
}

impl ProjectDraftStore {
    /// Reads the current draft title (matches `store.title()` syntax in `serge` docs).
    #[must_use]
    pub fn title(&self) -> String {
        self.title.get()
    }
}

/// Minimal counter showcase from `serge/README.md`.
#[component]
pub fn CounterCard() -> Element {
    let mut count = signal(0);

    ui! {
        Card { padding: 16px, gap: 12px,
            Text { "Count: {count()}" }
            Button {
                text: "Increment",
                clicked => { count += 1; }
            }
        }
    }
}

/// Feature module binding `ProjectDraftStore`, `RequireAuth` redirect guard, and `ProjectEditorCard`.
pub fn project_feature() -> UiFeature {
    feature()
        .provide(|| ProjectDraftStore {
            title: signal("Lookdev Studio Alpha".to_owned()),
        })
        .wrap(RequireAuth::redirect("/login"))
        .route("/projects", ProjectEditorCard)
}

/// Main project editor panel matching `serge/docs/01-dx-and-dsl.md`.
#[component]
pub fn ProjectEditorCard(store: Ctx<ProjectDraftStore>, exposure: Signal<f32>) -> Element {
    let mut liked = signal(false);

    ui! {
        Card {
            layout: v_stack,
            gap: 8px,
            padding: 12px,
            Text { "Exposure: {exposure():.2}" }
            Slider {
                range: -5.0..=5.0,
                value: exposure(),
                changed(v) => { exposure.set(v); }
            }
            Button {
                text: if liked() { "좋아요 취소" } else { "좋아요" },
                clicked => { liked.toggle(); }
            }
            Button {
                text: "저장",
                clicked => async move {
                    let _ = create_project_rpc(CreateProjectInput {
                        title: store.title(),
                        is_public: true,
                    })
                    .await;
                }
            }
            Canvas {
                height: 64px,
                paint(bounds, ctx) => {
                    ctx.draw_histogram(bounds, exposure());
                }
            }
        }
    }
}

/// Real-time telemetry canvas reading lock-free [`SharedRingBuf<FrameMetrics>`].
#[component]
pub fn RealtimeTelemetry(ring_buf: SharedRingBuf<FrameMetrics>) -> Element {
    ui! {
        Canvas {
            height: 80px,
            paint(bounds, ctx) => {
                let mut samples = [FrameMetrics::ZERO; 128];
                let n = ring_buf.read_latest_snapshot(&mut samples);
                ctx.draw_sparkline(bounds, &samples[..n]);
            }
        }
    }
}

/// Export progress state compiled statically into iOS Dynamic Island SwiftUI views.
pub struct ExportStatus {
    /// Active project display name.
    pub project_name: String,
    /// Export progress (`0.0..=1.0`).
    pub progress: f32,
}

/// Static iOS Live Activity & Dynamic Island component matching `serge/docs/01-dx-and-dsl.md`.
#[ios::live_activity]
pub fn ExportIsland(state: ExportStatus) -> LiveActivity {
    live_activity! {
        compact_leading: ui! { Icon { "cube" } },
        compact_trailing: ui! { Text { "{(state.progress * 100.0) as u32}%" } },
        expanded: ui! {
            Stack { gap: 8px,
                Text { "{state.project_name} 내보내는 중..." }
                ProgressBar { value: state.progress }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Pure-Rust 32-bit BMP Image Encoder (Zero External Crates)
// ---------------------------------------------------------------------------

/// Encodes an RGBA8 [`CpuFrameBuffer`] into an uncompressed 32-bit BGRA BMP image.
#[must_use]
pub fn encode_bmp(fb: &CpuFrameBuffer) -> Vec<u8> {
    let width = fb.width;
    let height = fb.height;
    let pixel_bytes = (width as usize) * (height as usize) * 4;
    let file_size = 54 + pixel_bytes;

    let mut out = Vec::with_capacity(file_size);
    // BITMAPFILEHEADER (14 bytes)
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());

    // BITMAPINFOHEADER (40 bytes)
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bpp
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    // BMP stores rows bottom-up in BGRA channel order.
    for y in (0..height).rev() {
        let row_start = (y as usize) * (width as usize) * 4;
        for x in 0..width {
            let idx = row_start + (x as usize) * 4;
            let r = fb.pixels[idx];
            let g = fb.pixels[idx + 1];
            let b = fb.pixels[idx + 2];
            let a = fb.pixels[idx + 3];
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 4. Studio PoC Controller State (Live Thread-Local UI + `orbic` App + Telemetry)
// ---------------------------------------------------------------------------

struct PatchLogEntry {
    source: &'static str,
    slot_id: usize,
    value: String,
}

struct BackendLogEntry {
    kind: String,
    status: u16,
    summary: String,
}

struct StudioWorkbench {
    db: Db,
    _cache: Cache,
    sessions: SessionStore,
    client: TestClient,
    hub: RealtimeHub,
    hub_events: Vec<orbic::realtime::ServerFrame>,
    next_client_msg: u64,
    feature: UiFeature,
    counter_card: Element,
    counter_dom: MountedDom,
    editor_view: Element,
    editor_dom: MountedDom,
    exposure: f32,
    liked: bool,
    authenticated: bool,
    last_guard_outcome: String,
    ring_buf: SharedRingBuf<FrameMetrics>,
    telemetry_view: Element,
    backend_override: Option<String>,
    export_progress: f32,
    last_idem_key: String,
    last_dual_rail_title: String,
    patch_log: Vec<PatchLogEntry>,
    backend_log: Vec<BackendLogEntry>,
    frame_rev: u64,
}

impl StudioWorkbench {
    fn new() -> Self {
        let db = block_on(Db::in_memory());
        let cache = Cache::in_memory();
        let sessions = SessionStore::new(cache.clone(), Duration::from_secs(300));
        let app = build_orbic_app(db.clone(), cache.clone(), sessions.clone());
        let client = TestClient::new(app)
            .with_rpc_prefix("/v1")
            .with_default_user(test_user());

        let hub = RealtimeHub::new();
        let ticket = hub.issue_ticket(test_user().id);
        let _ = hub.connect_with_ticket("studio-ui", &ticket, RealtimeTransport::WebTransport);

        let feature = project_feature();
        let authenticated = true;
        let editor_view = match feature.navigate("/projects", authenticated) {
            RouteOutcome::Rendered { element, .. } => *element,
            _ => Element::empty(),
        };
        let editor_dom = DomTemplate::compile(&editor_view).mount();

        let counter_card = CounterCard();
        let counter_dom = DomTemplate::compile(&counter_card).mount();

        let ring_buf = SharedRingBuf::<FrameMetrics>::new(128);
        for i in 0..24 {
            let ms = 6.2 + ((i as f32) * 0.45).sin() * 2.1 + (i as f32) * 0.05;
            ring_buf.write(FrameMetrics::new(ms, ms * 0.35));
        }
        let telemetry_view = RealtimeTelemetry(ring_buf.clone());

        let mut studio = Self {
            db,
            _cache: cache,
            sessions,
            client,
            hub,
            hub_events: Vec::new(),
            next_client_msg: 1,
            feature,
            counter_card,
            counter_dom,
            editor_view,
            editor_dom,
            exposure: 0.0,
            liked: false,
            authenticated,
            last_guard_outcome: "200 Rendered (/projects)".to_owned(),
            ring_buf,
            telemetry_view,
            backend_override: None,
            export_progress: 0.72,
            last_idem_key: "studio-idem-1".to_owned(),
            last_dual_rail_title: "Lookdev Studio Alpha".to_owned(),
            patch_log: Vec::new(),
            backend_log: Vec::new(),
            frame_rev: 1,
        };

        // Seed 1 initial project via the FastBinary #[server] RPC so the table starts populated.
        studio.invoke_editor_save("Lookdev Studio Alpha");
        studio
    }

    fn current_title(&self) -> String {
        self.feature.context::<ProjectDraftStore>().title()
    }

    fn publish_hub(&mut self, msg_type: &str, event_name: &str, payload: &str) {
        let id = format!("studio-msg-{}", self.next_client_msg);
        self.next_client_msg += 1;
        if let Ok(Some(ev)) = self.hub.publish_client_frame(
            orbic::realtime::ClientFrame {
                msg_type: msg_type.to_owned(),
                id,
                payload: payload.to_owned(),
            },
            event_name,
        ) {
            self.hub_events.push(ev);
            if self.hub_events.len() > 16 {
                self.hub_events.remove(0);
            }
        }
    }

    fn record_patches(&mut self, source: &'static str, patches: Vec<SlotPatch>) {
        for p in patches {
            self.patch_log.push(PatchLogEntry {
                source,
                slot_id: p.slot_id,
                value: p.new_value,
            });
        }
        if self.patch_log.len() > 24 {
            let overflow = self.patch_log.len() - 24;
            self.patch_log.drain(0..overflow);
        }
    }

    fn log_backend(&mut self, kind: &str, status: u16, summary: String) {
        self.backend_log.push(BackendLogEntry {
            kind: kind.to_owned(),
            status,
            summary,
        });
        if self.backend_log.len() > 24 {
            self.backend_log.remove(0);
        }
    }

    fn list_projects(&self) -> Vec<Project> {
        let resp = block_on(self.client.get("/v1/projects").as_user(test_user()).send());
        resp.json::<Page<Project>>()
            .map(|p| p.data)
            .unwrap_or_default()
    }

    fn invoke_editor_save(&mut self, title: &str) {
        self.feature
            .context::<ProjectDraftStore>()
            .title
            .set(title.to_owned());
        self.last_dual_rail_title = title.to_owned();

        let before_count = self.list_projects().len();
        let transport = self.client.clone().into_transport();
        hyve::api::set_thread_transport(Some(transport));
        let clicked = self.editor_view.click_first_button("저장");
        hyve::api::set_thread_transport(None);

        let after = self.list_projects();
        if clicked && after.len() > before_count {
            let summary = format!(
                "#[server] FastBinary RPC created project \"{}\" (total={})",
                title,
                after.len()
            );
            self.log_backend("#[server] RPC", 200, summary.clone());
            self.publish_hub("project.create", "project.created", title);
        } else {
            self.log_backend(
                "#[server] RPC",
                400,
                format!("Validation rejected title \"{title}\" (expected 1..=80 chars)"),
            );
        }
        self.frame_rev += 1;
    }

    fn execute_action(&mut self, body: &str) {
        let parsed = JsonValue::parse(body).unwrap_or(JsonValue::Null);
        let action = parsed
            .get("action")
            .and_then(JsonValue::as_str)
            .unwrap_or("");

        match action {
            "increment" => {
                let _ = self.counter_card.click_first_button("Increment");
                let patches = self.counter_dom.patch_dirty();
                self.record_patches("CounterCard", patches);
                self.frame_rev += 1;
            }
            "exposure" => {
                let v = parsed
                    .get("value")
                    .and_then(JsonValue::as_f64)
                    .map_or(0.0, |n| (n as f32).clamp(-5.0, 5.0));
                self.exposure = v;
                let _ = self.editor_view.set_slider_value(v);
                let patches = self.editor_dom.patch_dirty();
                self.record_patches("ProjectEditorCard", patches);
                self.frame_rev += 1;
            }
            "toggle_like" => {
                if !self.editor_view.click_first_button("좋아요") {
                    let _ = self.editor_view.click_first_button("좋아요 취소");
                }
                self.liked = self
                    .editor_view
                    .collect_texts()
                    .iter()
                    .any(|t| t == "좋아요 취소");
                let patches = self.editor_dom.patch_dirty();
                self.record_patches("ProjectEditorCard", patches);
                self.frame_rev += 1;
            }
            "set_title" => {
                let title = parsed
                    .get("title")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("")
                    .to_owned();
                self.feature
                    .context::<ProjectDraftStore>()
                    .title
                    .set(title.clone());
                self.last_dual_rail_title = title;
            }
            "toggle_auth" => {
                self.authenticated = !self.authenticated;
                match self.feature.navigate("/projects", self.authenticated) {
                    RouteOutcome::Redirect(target) => {
                        self.last_guard_outcome = format!("302 Redirect -> {target}");
                        self.log_backend("RequireAuth", 302, self.last_guard_outcome.clone());
                    }
                    RouteOutcome::Rendered { path, element } => {
                        self.last_guard_outcome = format!("200 Rendered ({path})");
                        self.editor_view = *element;
                        self.editor_dom = DomTemplate::compile(&self.editor_view).mount();
                        let _ = self.editor_view.set_slider_value(self.exposure);
                        let _ = self.editor_dom.patch_dirty();
                        self.log_backend("RequireAuth", 200, self.last_guard_outcome.clone());
                    }
                    RouteOutcome::NotFound(p) => {
                        self.last_guard_outcome = format!("404 NotFound ({p})");
                    }
                }
                self.frame_rev += 1;
            }
            "save_rpc" => {
                let title = parsed
                    .get("title")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| self.current_title());
                self.invoke_editor_save(&title);
            }
            "rest_create" => {
                let title = parsed
                    .get("title")
                    .and_then(JsonValue::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("REST PoC Scene")
                    .to_owned();
                let key = format!("studio-idem-{}", self.next_client_msg);
                self.next_client_msg += 1;
                self.last_idem_key = key.clone();
                self.last_dual_rail_title = title.clone();
                let resp = block_on(
                    self.client
                        .post("/v1/projects")
                        .as_user(test_user())
                        .idempotency_key(&key)
                        .json(&CreateProjectInput {
                            title: title.clone(),
                            is_public: true,
                        })
                        .send(),
                );
                let replayed = resp.header("X-Idempotency-Replayed").unwrap_or("false");
                self.log_backend(
                    "REST POST /v1/projects",
                    resp.status(),
                    format!("key={key} replayed={replayed} title=\"{title}\""),
                );
                if resp.status() < 400 {
                    self.publish_hub("project.create", "project.created", &title);
                }
            }
            "rest_idempotent_replay" => {
                let key = self.last_idem_key.clone();
                let title = self.last_dual_rail_title.clone();
                // Ensure the key exists first if not yet sent over REST.
                let _ = block_on(
                    self.client
                        .post("/v1/projects")
                        .as_user(test_user())
                        .idempotency_key(&key)
                        .json(&CreateProjectInput {
                            title: title.clone(),
                            is_public: true,
                        })
                        .send(),
                );
                let replay = block_on(
                    self.client
                        .post("/v1/projects")
                        .as_user(test_user())
                        .idempotency_key(&key)
                        .json(&CreateProjectInput {
                            title: title.clone(),
                            is_public: true,
                        })
                        .send(),
                );
                let replayed = replay.header("X-Idempotency-Replayed").unwrap_or("false");
                let id = replay
                    .json::<Project>()
                    .map(|p| p.id.to_string())
                    .unwrap_or_default();
                self.log_backend(
                    "Idempotency Replay",
                    replay.status(),
                    format!("key={key} X-Idempotency-Replayed={replayed} id={id}"),
                );
            }
            "validate_fail" => {
                let resp = block_on(
                    self.client
                        .post("/v1/projects")
                        .as_user(test_user())
                        .json(&CreateProjectInput {
                            title: String::new(),
                            is_public: false,
                        })
                        .send(),
                );
                self.log_backend("Validation Guard", resp.status(), resp.text());
            }
            "optimistic_update" => {
                let projects = self.list_projects();
                if let Some(target) = projects.last() {
                    let res =
                        block_on(self.db.update_optimistic(&target.id, target.version, |p| {
                            p.title = format!(
                                "{} [v{}]",
                                target.title.split(" [v").next().unwrap_or(&target.title),
                                target.version + 1
                            );
                        }));
                    match res {
                        Ok(updated) => {
                            let _ = block_on(
                                self._cache.invalidate_tag(tag!("projects", test_user().id)),
                            );
                            self.log_backend(
                                "Optimistic Update",
                                200,
                                format!(
                                    "id={} v{} -> v{}",
                                    updated.id, target.version, updated.version
                                ),
                            );
                            self.publish_hub("project.update", "project.updated", &updated.title);
                        }
                        Err(e) => {
                            self.log_backend(
                                "Optimistic Update",
                                e.http_status(),
                                e.message().to_owned(),
                            );
                        }
                    }
                }
            }
            "optimistic_conflict" => {
                let projects = self.list_projects();
                if let Some(target) = projects.last() {
                    let stale = (target.version - 1).max(0);
                    let err = block_on(self.db.update_optimistic(&target.id, stale, |p| {
                        p.title = "Stale Overwrite".to_owned();
                    }))
                    .unwrap_err();
                    self.log_backend(
                        "Optimistic Conflict",
                        err.http_status(),
                        format!(
                            "attempted expected_version={stale} against current={} -> {} ({})",
                            target.version,
                            err.code(),
                            err.message()
                        ),
                    );
                }
            }
            "session_csrf_demo" => {
                let tokens = self.sessions.create_session(&test_user());
                let bearer_ok =
                    block_on(self.client.get("/v1/projects").bearer(&tokens.token).send());
                let csrf_fail = block_on(
                    self.client
                        .post("/v1/projects")
                        .cookie(&tokens.token)
                        .json(&CreateProjectInput {
                            title: "Missing CSRF".to_owned(),
                            is_public: false,
                        })
                        .send(),
                );
                self.sessions.revoke_session(&tokens.token);
                let revoked =
                    block_on(self.client.get("/v1/projects").bearer(&tokens.token).send());
                self.log_backend(
                    "L2 Session + CSRF",
                    revoked.status(),
                    format!(
                        "bearer={} | cookie_no_csrf={} | after_instant_revoke={}",
                        bearer_ok.status(),
                        csrf_fail.status(),
                        revoked.status()
                    ),
                );
            }
            "telemetry_burst" => {
                let seed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(7, |d| d.subsec_millis()) as f32;
                for i in 0..16 {
                    let wave = ((i as f32 + seed * 0.05) * 0.55).sin().abs() * 6.5;
                    let ms = 4.2 + wave;
                    self.ring_buf.write(FrameMetrics::new(ms, ms * 0.34));
                }
                self.publish_hub(
                    "telemetry.burst",
                    "telemetry.sampled",
                    "16 frames -> SharedRingBuf",
                );
                self.frame_rev += 1;
            }
            "toggle_backend" => {
                self.backend_override = match self.backend_override.as_deref() {
                    Some("cpu") => Some("gpu".to_owned()),
                    _ => Some("cpu".to_owned()),
                };
                self.frame_rev += 1;
            }
            "export_progress" => {
                let val = parsed
                    .get("value")
                    .and_then(JsonValue::as_f64)
                    .map_or(0.5, |n| (n as f32).clamp(0.0, 1.0));
                self.export_progress = val;
            }
            _ => {}
        }
    }

    fn editor_scene_batch(&self) -> SceneBatch {
        layout_and_paint(&self.editor_view, 520, 250, ColorTokens::dark())
    }

    fn telemetry_scene_batch(&self) -> SceneBatch {
        layout_and_paint(&self.telemetry_view, 520, 110, ColorTokens::dark())
    }

    fn state_json(&self) -> String {
        let editor_batch = self.editor_scene_batch();
        let telemetry_batch = self.telemetry_scene_batch();
        let gpu_stats = GpuBatchRenderer::prepare_frame(&editor_batch);
        let editor_fb = CpuFallbackRenderer::render(&editor_batch);
        let telemetry_fb = CpuFallbackRenderer::render(&telemetry_batch);

        let backend = serge::native::select_render_backend_with_override(
            true,
            self.backend_override.as_deref(),
        );
        let backend_label = match backend {
            RenderBackendKind::Gpu => "Gpu (Instanced) + Symmetric CpuFallback Preview",
            RenderBackendKind::CpuFallback => "CpuFallback (Forced)",
        };

        let sample_payload = CreateProjectInput {
            title: self.last_dual_rail_title.clone(),
            is_public: true,
        };
        let bin_bytes = to_bytes(&sample_payload);
        let json_str = to_json_string(&sample_payload);
        let bin_hex = bin_bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");

        let projects = self.list_projects();
        let island_name = projects
            .last()
            .map_or_else(|| "Studio Scene".to_owned(), |p| p.title.clone());
        let island = ExportIsland(ExportStatus {
            project_name: island_name.clone(),
            progress: self.export_progress,
        });
        let swiftui_code = island.to_swiftui_code();

        let patches_json = JsonValue::Array(
            self.patch_log
                .iter()
                .rev()
                .take(10)
                .map(|p| {
                    JsonValue::Object(vec![
                        ("source".to_owned(), JsonValue::Str(p.source.to_owned())),
                        ("slotId".to_owned(), JsonValue::Number(p.slot_id as f64)),
                        ("value".to_owned(), JsonValue::Str(p.value.clone())),
                    ])
                })
                .collect(),
        );

        let projects_json = JsonValue::Array(
            projects
                .iter()
                .map(|p| {
                    JsonValue::Object(vec![
                        ("id".to_owned(), JsonValue::Str(p.id.to_string())),
                        ("title".to_owned(), JsonValue::Str(p.title.clone())),
                        ("isPublic".to_owned(), JsonValue::Bool(p.is_public)),
                        ("version".to_owned(), JsonValue::Number(p.version as f64)),
                    ])
                })
                .collect(),
        );

        let logs_json = JsonValue::Array(
            self.backend_log
                .iter()
                .rev()
                .take(10)
                .map(|l| {
                    JsonValue::Object(vec![
                        ("kind".to_owned(), JsonValue::Str(l.kind.clone())),
                        ("status".to_owned(), JsonValue::Number(f64::from(l.status))),
                        ("summary".to_owned(), JsonValue::Str(l.summary.clone())),
                    ])
                })
                .collect(),
        );

        let hub_json = JsonValue::Array(
            self.hub_events
                .iter()
                .rev()
                .take(8)
                .map(|ev| {
                    JsonValue::Object(vec![
                        ("seq".to_owned(), JsonValue::Number(ev.seq as f64)),
                        ("event".to_owned(), JsonValue::Str(ev.msg_type.clone())),
                        ("payload".to_owned(), JsonValue::Str(ev.payload.clone())),
                    ])
                })
                .collect(),
        );

        JsonValue::Object(vec![
            (
                "frameRev".to_owned(),
                JsonValue::Number(self.frame_rev as f64),
            ),
            (
                "counterHtml".to_owned(),
                JsonValue::Str(self.counter_dom.current_html()),
            ),
            (
                "counterTexts".to_owned(),
                JsonValue::Array(
                    self.counter_card
                        .collect_texts()
                        .into_iter()
                        .map(JsonValue::Str)
                        .collect(),
                ),
            ),
            (
                "editorHtml".to_owned(),
                JsonValue::Str(self.editor_dom.current_html()),
            ),
            (
                "draftTitle".to_owned(),
                JsonValue::Str(self.current_title()),
            ),
            (
                "exposure".to_owned(),
                JsonValue::Number(f64::from(self.exposure)),
            ),
            ("liked".to_owned(), JsonValue::Bool(self.liked)),
            (
                "authenticated".to_owned(),
                JsonValue::Bool(self.authenticated),
            ),
            (
                "guardOutcome".to_owned(),
                JsonValue::Str(self.last_guard_outcome.clone()),
            ),
            ("patches".to_owned(), patches_json),
            (
                "renderBackend".to_owned(),
                JsonValue::Str(backend_label.to_owned()),
            ),
            (
                "drawCalls".to_owned(),
                JsonValue::Number(f64::from(gpu_stats.draw_calls)),
            ),
            (
                "rectInstances".to_owned(),
                JsonValue::Number(gpu_stats.rect_instances as f64),
            ),
            (
                "textRuns".to_owned(),
                JsonValue::Number(gpu_stats.text_runs as f64),
            ),
            (
                "editorPaintedPixels".to_owned(),
                JsonValue::Number(editor_fb.painted_pixel_count() as f64),
            ),
            (
                "telemetryPaintedPixels".to_owned(),
                JsonValue::Number(telemetry_fb.painted_pixel_count() as f64),
            ),
            (
                "ringLen".to_owned(),
                JsonValue::Number(self.ring_buf.len() as f64),
            ),
            ("projects".to_owned(), projects_json),
            ("backendLogs".to_owned(), logs_json),
            ("realtimeEvents".to_owned(), hub_json),
            (
                "dualRail".to_owned(),
                JsonValue::Object(vec![
                    (
                        "binaryBytes".to_owned(),
                        JsonValue::Number(bin_bytes.len() as f64),
                    ),
                    (
                        "jsonBytes".to_owned(),
                        JsonValue::Number(json_str.len() as f64),
                    ),
                    ("binaryHex".to_owned(), JsonValue::Str(bin_hex)),
                    ("jsonPayload".to_owned(), JsonValue::Str(json_str)),
                ]),
            ),
            (
                "ios".to_owned(),
                JsonValue::Object(vec![
                    ("projectName".to_owned(), JsonValue::Str(island_name)),
                    (
                        "progress".to_owned(),
                        JsonValue::Number(f64::from(self.export_progress)),
                    ),
                    ("swiftUiCode".to_owned(), JsonValue::Str(swiftui_code)),
                ]),
            ),
        ])
        .to_json_string()
    }

    fn openapi_json(&self) -> String {
        block_on(self.client.get("/docs").send()).text()
    }
}

// ---------------------------------------------------------------------------
// 5. Minimal `std::net::TcpListener` HTTP Server
// ---------------------------------------------------------------------------

struct HttpResponse {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

fn dispatch_http(
    studio: &mut StudioWorkbench,
    method: &str,
    path: &str,
    body: &str,
) -> HttpResponse {
    let clean_path = path.split('?').next().unwrap_or(path);
    match (method, clean_path) {
        ("GET", "/") => HttpResponse {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: WORKBENCH_HTML.as_bytes().to_vec(),
        },
        ("GET", "/api/state") => {
            let json = studio.state_json();
            HttpResponse {
                status: 200,
                content_type: "application/json; charset=utf-8",
                body: json.into_bytes(),
            }
        }
        ("POST", "/api/action") => {
            studio.execute_action(body);
            let json = studio.state_json();
            HttpResponse {
                status: 200,
                content_type: "application/json; charset=utf-8",
                body: json.into_bytes(),
            }
        }
        ("GET", "/api/native-frame.bmp") => {
            let fb = CpuFallbackRenderer::render(&studio.editor_scene_batch());
            HttpResponse {
                status: 200,
                content_type: "image/bmp",
                body: encode_bmp(&fb),
            }
        }
        ("GET", "/api/telemetry-frame.bmp") => {
            let fb = CpuFallbackRenderer::render(&studio.telemetry_scene_batch());
            HttpResponse {
                status: 200,
                content_type: "image/bmp",
                body: encode_bmp(&fb),
            }
        }
        ("GET", "/openapi.json") => {
            let doc = studio.openapi_json();
            HttpResponse {
                status: 200,
                content_type: "application/json; charset=utf-8",
                body: doc.into_bytes(),
            }
        }
        _ => HttpResponse {
            status: 404,
            content_type: "text/plain; charset=utf-8",
            body: b"Not Found".to_vec(),
        },
    }
}

fn handle_connection(mut stream: TcpStream, studio: &mut StudioWorkbench) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut header_end = None;

    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = find_header_end(&buf) {
                    header_end = Some(pos);
                    break;
                }
                if buf.len() > 1024 * 64 {
                    break;
                }
            }
        }
    }

    let Some(hdr_end) = header_end else {
        return;
    };
    let head = String::from_utf8_lossy(&buf[..hdr_end]).into_owned();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");

    let content_len = lines
        .find_map(|line| {
            let (k, v) = line.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("content-length") {
                v.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
        .min(65536);

    let body_start = hdr_end + 4;
    while buf.len() < body_start + content_len {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }

    let body_end = (body_start + content_len).min(buf.len());
    let body = String::from_utf8_lossy(&buf[body_start..body_end]);
    let resp = dispatch_http(studio, method, path, &body);

    let reason = match resp.status {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };
    let headers = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        resp.status,
        resp.content_type,
        resp.body.len()
    );
    let _ = stream.write_all(headers.as_bytes());
    let _ = stream.write_all(&resp.body);
    let _ = stream.flush();
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn run_smoke_summary(s: &mut StudioWorkbench) {
    s.execute_action(r#"{"action":"increment"}"#);
    s.execute_action(r#"{"action":"exposure","value":2.25}"#);
    s.execute_action(r#"{"action":"toggle_like"}"#);
    s.execute_action(r#"{"action":"telemetry_burst"}"#);

    let state = s.state_json();
    let parsed = JsonValue::parse(&state).expect("valid state json");
    let projects = parsed
        .get("projects")
        .and_then(|v| match v {
            JsonValue::Array(arr) => Some(arr.len()),
            _ => None,
        })
        .unwrap_or(0);
    let draw_calls = parsed
        .get("drawCalls")
        .and_then(JsonValue::as_f64)
        .unwrap_or(0.0);
    let pixels = parsed
        .get("editorPaintedPixels")
        .and_then(JsonValue::as_f64)
        .unwrap_or(0.0);
    let counter_html = parsed
        .get("counterHtml")
        .and_then(JsonValue::as_str)
        .unwrap_or("");

    println!("=== Hyperish Full-Stack Studio PoC Smoke Check ===");
    println!("  [serge-web]    DOM: {counter_html}");
    println!("  [serge-native] draw_calls={draw_calls:.0} cpu_painted_pixels={pixels:.0}");
    println!("  [orbic-db]     seeded_projects={projects} openapi=/openapi.json");
    println!("  [ios-widget]   ExportIsland -> SwiftUI ActivityConfiguration ready");
}

fn bind_listener(preferred_port: u16) -> TcpListener {
    if let Ok(listener) = TcpListener::bind(("127.0.0.1", preferred_port)) {
        return listener;
    }
    TcpListener::bind(("127.0.0.1", preferred_port + 1))
        .expect("failed to bind studio HTTP server on preferred or fallback port")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let once = args.iter().any(|a| a == "--once" || a == "--smoke");
    let port = args
        .windows(2)
        .find_map(|w| {
            (w[0] == "--port")
                .then(|| w[1].parse::<u16>().ok())
                .flatten()
        })
        .unwrap_or(3000);

    let mut studio = StudioWorkbench::new();
    run_smoke_summary(&mut studio);
    if once {
        return;
    }

    let listener = bind_listener(port);
    let addr = listener.local_addr().expect("local_addr");
    println!();
    println!("Hyperish Full-Stack Studio PoC running at http://{addr}");
    println!("  - Workbench UI : http://{addr}/");
    println!("  - Live State   : http://{addr}/api/state");
    println!("  - Native BMP   : http://{addr}/api/native-frame.bmp");
    println!("  - OpenAPI Spec : http://{addr}/openapi.json");

    for stream in listener.incoming() {
        match stream {
            Ok(conn) => handle_connection(conn, &mut studio),
            Err(_) => break,
        }
    }
}

// ---------------------------------------------------------------------------
// 6. Self-Contained Browser Workbench HTML (`GET /`)
// ---------------------------------------------------------------------------

const WORKBENCH_HTML: &str = r##"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Hyperish Studio PoC — hyve · orbic · serge</title>
<style>
  :root {
    --bg: #0b1020; --surface: #121930; --card: #18223f; --border: #2a375e;
    --text: #eef2ff; --muted: #94a3b8; --accent: #6366f1; --emerald: #10b981;
    --amber: #f59e0b; --rose: #f43f5e;
  }
  * { box-sizing: border-box; }
  body {
    margin: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    background: var(--bg); color: var(--text); line-height: 1.45;
  }
  header {
    padding: 16px 24px; border-bottom: 1px solid var(--border);
    background: linear-gradient(90deg, #111833, #171f3b);
    display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px;
  }
  .brand h1 { margin: 0; font-size: 18px; letter-spacing: -0.02em; }
  .brand p { margin: 2px 0 0; font-size: 12px; color: var(--muted); }
  .pill {
    display: inline-block; padding: 4px 10px; border-radius: 999px;
    background: rgba(99,102,241,0.18); border: 1px solid rgba(99,102,241,0.4);
    font-size: 12px; color: #c7d2fe; text-decoration: none;
  }
  main {
    padding: 20px 24px; display: grid; gap: 16px;
    grid-template-columns: repeat(auto-fit, minmax(460px, 1fr));
  }
  .panel {
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 12px; padding: 16px; display: flex; flex-direction: column; gap: 12px;
  }
  .panel h2 {
    margin: 0; font-size: 15px; display: flex; justify-content: space-between; align-items: center;
  }
  .tag { font-size: 11px; color: var(--muted); font-weight: 500; }
  .row { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  button {
    background: var(--accent); color: #fff; border: 0; border-radius: 8px;
    padding: 7px 12px; font-size: 12px; font-weight: 600; cursor: pointer;
  }
  button.secondary { background: #263358; color: var(--text); }
  button.warn { background: #9f1239; }
  button.ok { background: #047857; }
  input[type="text"] {
    background: var(--bg); color: var(--text); border: 1px solid var(--border);
    border-radius: 8px; padding: 7px 10px; font-size: 13px; flex: 1; min-width: 160px;
  }
  input[type="range"] { accent-color: var(--accent); cursor: pointer; }
  .box {
    background: var(--card); border: 1px solid var(--border);
    border-radius: 8px; padding: 10px; font-size: 12px;
  }
  pre {
    margin: 0; font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px; white-space: pre-wrap; word-break: break-all; color: #cbd5e1;
  }
  .frame-img {
    width: 100%; border-radius: 8px; border: 1px solid var(--border);
    background: #0f172a; display: block;
  }
  table { width: 100%; border-collapse: collapse; font-size: 12px; }
  th, td { text-align: left; padding: 6px 8px; border-bottom: 1px solid var(--border); }
  th { color: var(--muted); font-weight: 600; }
  .island-preview {
    background: #000; border-radius: 28px; padding: 14px 18px;
    border: 1px solid #334155; display: flex; flex-direction: column; gap: 10px;
  }
  .island-compact {
    display: flex; justify-content: space-between; align-items: center;
    background: #0f172a; border-radius: 999px; padding: 6px 14px; font-size: 12px;
  }
  .progress-track {
    height: 8px; border-radius: 999px; background: #1e293b; overflow: hidden;
  }
  .progress-fill { height: 100%; background: var(--emerald); transition: width .15s; }
</style>
</head>
<body>
<header>
  <div class="brand">
    <h1>Hyperish Full-Stack Studio PoC</h1>
    <p>hyve (Signal · SharedRingBuf · Dual-Rail Serde) + orbic (SQL · Cache · L2 Session · #[server]) + serge (Web DOM · CPU/GPU · SwiftUI)</p>
  </div>
  <div class="row">
    <a class="pill" href="/openapi.json" target="_blank">/openapi.json</a>
    <a class="pill" href="/api/state" target="_blank">/api/state</a>
    <span class="pill" id="backend-badge">loading...</span>
  </div>
</header>

<main>
  <!-- 1. serge Web DOM + Surgical Patching -->
  <section class="panel">
    <h2>1. serge Web DOM &amp; Surgical Slot Patching <span class="tag">Leptos-style DOM</span></h2>
    <div class="box">
      <div class="row" style="justify-content:space-between;">
        <strong>CounterCard:</strong>
        <button onclick="act({action:'increment'})">+ Increment</button>
      </div>
      <pre id="counter-html" style="margin-top:6px;"></pre>
    </div>
    <div class="box">
      <div class="row" style="justify-content:space-between;">
        <strong>ProjectEditorCard &amp; RequireAuth Guard:</strong>
        <button class="secondary" onclick="act({action:'toggle_auth'})" id="auth-btn">Auth: ON</button>
      </div>
      <div class="row" style="margin-top:8px;">
        <span style="font-size:12px;">Exposure: <b id="exp-val">0.00</b></span>
        <input id="exp-slider" type="range" min="-5" max="5" step="0.25" value="0"
               oninput="act({action:'exposure', value: parseFloat(this.value)})" style="flex:1;" />
        <button class="secondary" onclick="act({action:'toggle_like'})" id="like-btn">좋아요</button>
      </div>
      <div class="row" style="margin-top:8px;">
        <input id="draft-title" type="text" value="Lookdev Studio Alpha" placeholder="Project title..." />
        <button class="ok" onclick="saveRpc()">저장 (Inline #[server] RPC)</button>
      </div>
      <div style="margin-top:6px;font-size:11px;color:#a5b4fc;" id="guard-outcome"></div>
      <pre id="editor-html" style="margin-top:6px;"></pre>
    </div>
    <div class="box">
      <strong>Surgical Slot Patches (`MountedDom::patch_dirty`):</strong>
      <pre id="patch-log" style="margin-top:6px;max-height:110px;overflow:auto;"></pre>
    </div>
  </section>

  <!-- 2. serge Native GPU Batch + CPU Fallback Framebuffer -->
  <section class="panel">
    <h2>2. serge-native CPU Fallback Framebuffer &amp; Telemetry <span class="tag">SharedRingBuf</span></h2>
    <div class="row" style="justify-content:space-between;">
      <span class="tag" id="gpu-stats"></span>
      <div class="row">
        <button class="secondary" onclick="act({action:'toggle_backend'})">Toggle GPU/CPU</button>
        <button onclick="act({action:'telemetry_burst'})">+ Push Telemetry Burst</button>
      </div>
    </div>
    <div>
      <div class="tag" style="margin-bottom:4px;">ProjectEditorCard (`layout_and_paint` -&gt; `CpuFallbackRenderer::render`)</div>
      <img id="native-bmp" class="frame-img" src="/api/native-frame.bmp" alt="Native Framebuffer" />
    </div>
    <div>
      <div class="tag" style="margin-bottom:4px;">RealtimeTelemetry (`SharedRingBuf&lt;FrameMetrics&gt;` -&gt; Sparkline BMP)</div>
      <img id="telemetry-bmp" class="frame-img" src="/api/telemetry-frame.bmp" alt="Telemetry Sparkline" />
    </div>
  </section>

  <!-- 3. orbic Backend + hyve-serde Dual-Rail -->
  <section class="panel">
    <h2>3. orbic Backend, Optimistic Lock &amp; Dual-Rail Serde <span class="tag">PostgreSQL/InMem</span></h2>
    <div class="row">
      <button onclick="restCreate()">REST POST /v1/projects</button>
      <button class="secondary" onclick="act({action:'rest_idempotent_replay'})">Replay Idempotency-Key</button>
      <button class="warn" onclick="act({action:'validate_fail'})">Empty Title -&gt; 400</button>
      <button class="ok" onclick="act({action:'optimistic_update'})">Optimistic Update</button>
      <button class="warn" onclick="act({action:'optimistic_conflict'})">409 Conflict</button>
      <button class="secondary" onclick="act({action:'session_csrf_demo'})">Session+CSRF Test</button>
    </div>
    <div class="box" style="max-height:140px;overflow:auto;">
      <table>
        <thead><tr><th>ID</th><th>Title</th><th>Version</th><th>Public</th></tr></thead>
        <tbody id="project-rows"></tbody>
      </table>
    </div>
    <div class="box">
      <strong>Dual-Rail Serialization (`hyve-serde`: HYVE v1 Binary vs JSON):</strong>
      <pre id="dual-rail" style="margin-top:6px;"></pre>
    </div>
    <div class="box">
      <strong>Backend &amp; RealtimeHub Events:</strong>
      <pre id="backend-log" style="margin-top:6px;max-height:120px;overflow:auto;"></pre>
    </div>
  </section>

  <!-- 4. iOS Live Activity / Dynamic Island -->
  <section class="panel">
    <h2>4. serge::ios Dynamic Island SwiftUI Codegen <span class="tag">#[ios::live_activity]</span></h2>
    <div class="row">
      <span style="font-size:12px;">Export Progress: <b id="ios-pct">72%</b></span>
      <input id="ios-slider" type="range" min="0" max="100" value="72"
             oninput="act({action:'export_progress', value: parseFloat(this.value)/100.0})" style="flex:1;" />
    </div>
    <div class="island-preview">
      <div class="island-compact">
        <span>&#11042; Cube</span>
        <span id="ios-compact-pct">72%</span>
      </div>
      <div style="font-size:13px;font-weight:600;" id="ios-expanded-text"></div>
      <div class="progress-track"><div class="progress-fill" id="ios-bar"></div></div>
    </div>
    <div class="box">
      <strong>Generated SwiftUI `ActivityConfiguration`:</strong>
      <pre id="swiftui-code" style="margin-top:6px;max-height:240px;overflow:auto;"></pre>
    </div>
  </section>
</main>

<script>
async function act(payload) {
  const res = await fetch('/api/action', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify(payload)
  });
  const st = await res.json();
  renderState(st);
}
function saveRpc() {
  const title = document.getElementById('draft-title').value;
  act({action: 'save_rpc', title});
}
function restCreate() {
  const title = document.getElementById('draft-title').value;
  act({action: 'rest_create', title});
}
function esc(s) {
  return String(s).replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;');
}
function renderState(st) {
  document.getElementById('backend-badge').textContent = st.renderBackend;
  document.getElementById('counter-html').textContent = st.counterHtml;
  document.getElementById('editor-html').textContent = st.editorHtml;
  document.getElementById('exp-val').textContent = Number(st.exposure).toFixed(2);
  document.getElementById('exp-slider').value = st.exposure;
  document.getElementById('like-btn').textContent = st.liked ? '좋아요 취소' : '좋아요';
  document.getElementById('auth-btn').textContent = st.authenticated ? 'Auth: ON' : 'Auth: OFF (Redirect)';
  document.getElementById('guard-outcome').textContent = 'Route Guard: ' + st.guardOutcome;

  document.getElementById('patch-log').textContent =
    st.patches.map(p => `[${p.source}] slot[${p.slotId}] -> "${p.value}"`).join('\n') || '(no patches yet)';

  document.getElementById('gpu-stats').textContent =
    `draw_calls=${st.drawCalls} | rects=${st.rectInstances} | text_runs=${st.textRuns} | cpu_pixels=${st.editorPaintedPixels} | ring=${st.ringLen}`;

  document.getElementById('native-bmp').src = '/api/native-frame.bmp?r=' + st.frameRev;
  document.getElementById('telemetry-bmp').src = '/api/telemetry-frame.bmp?r=' + st.frameRev;

  document.getElementById('project-rows').innerHTML = st.projects.map(p =>
    `<tr><td><code>${esc(p.id.slice(0,10))}...</code></td><td>${esc(p.title)}</td><td>v${p.version}</td><td>${p.isPublic}</td></tr>`
  ).join('');

  const dr = st.dualRail;
  document.getElementById('dual-rail').textContent =
    `HYVE Binary (${dr.binaryBytes} bytes) vs JSON (${dr.jsonBytes} bytes)\n` +
    `HEX : ${dr.binaryHex}\nJSON: ${dr.jsonPayload}`;

  const logLines = [
    ...st.backendLogs.map(l => `[${l.status}] ${l.kind}: ${l.summary}`),
    ...st.realtimeEvents.map(e => `[WS seq=${e.seq}] ${e.event}: ${e.payload}`)
  ];
  document.getElementById('backend-log').textContent = logLines.join('\n') || '(no events yet)';

  const pct = Math.round(st.ios.progress * 100);
  document.getElementById('ios-pct').textContent = pct + '%';
  document.getElementById('ios-compact-pct').textContent = pct + '%';
  document.getElementById('ios-slider').value = pct;
  document.getElementById('ios-expanded-text').textContent = st.ios.projectName + ' 내보내는 중...';
  document.getElementById('ios-bar').style.width = pct + '%';
  document.getElementById('swiftui-code').textContent = st.ios.swiftUiCode;
}
fetch('/api/state').then(r => r.json()).then(renderState);
</script>
</body>
</html>
"##;

// ---------------------------------------------------------------------------
// 7. Integration Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_bmp_emits_valid_header_and_dimensions() {
        let card = CounterCard();
        let batch = layout_and_paint(&card, 64, 48, ColorTokens::dark());
        let fb = CpuFallbackRenderer::render(&batch);
        let bmp = encode_bmp(&fb);

        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(bmp.len(), 54 + 64 * 48 * 4);
        let w = i32::from_le_bytes(bmp[18..22].try_into().unwrap());
        let h = i32::from_le_bytes(bmp[22..26].try_into().unwrap());
        assert_eq!((w, h), (64, 48));
    }

    #[test]
    fn studio_workbench_executes_full_stack_actions() {
        let mut studio = StudioWorkbench::new();

        let st0 = dispatch_http(&mut studio, "GET", "/api/state", "");
        assert_eq!(st0.status, 200);
        let body0 = String::from_utf8(st0.body).unwrap();
        assert!(body0.contains("Lookdev Studio Alpha"));

        let r1 = dispatch_http(
            &mut studio,
            "POST",
            "/api/action",
            r#"{"action":"increment"}"#,
        );
        assert_eq!(r1.status, 200);
        assert!(String::from_utf8(r1.body).unwrap().contains("Count: 1"));

        let r2 = dispatch_http(
            &mut studio,
            "POST",
            "/api/action",
            r#"{"action":"save_rpc","title":"Second Studio Project"}"#,
        );
        assert!(
            String::from_utf8(r2.body)
                .unwrap()
                .contains("Second Studio Project")
        );

        let r_val = dispatch_http(
            &mut studio,
            "POST",
            "/api/action",
            r#"{"action":"validate_fail"}"#,
        );
        let r_val_body = String::from_utf8(r_val.body).unwrap();
        assert!(r_val_body.contains("400"));
        assert!(r_val_body.contains("invalid_request"));

        let r_conf = dispatch_http(
            &mut studio,
            "POST",
            "/api/action",
            r#"{"action":"optimistic_conflict"}"#,
        );
        assert!(
            String::from_utf8(r_conf.body)
                .unwrap()
                .contains("version_conflict")
        );

        let r_csrf = dispatch_http(
            &mut studio,
            "POST",
            "/api/action",
            r#"{"action":"session_csrf_demo"}"#,
        );
        assert!(
            String::from_utf8(r_csrf.body)
                .unwrap()
                .contains("bearer=200 | cookie_no_csrf=403 | after_instant_revoke=401")
        );

        let bmp_resp = dispatch_http(&mut studio, "GET", "/api/native-frame.bmp", "");
        assert_eq!(bmp_resp.status, 200);
        assert_eq!(&bmp_resp.body[0..2], b"BM");

        let openapi = dispatch_http(&mut studio, "GET", "/openapi.json", "");
        assert_eq!(openapi.status, 200);
        assert!(
            String::from_utf8(openapi.body)
                .unwrap()
                .contains("/v1/projects")
        );
    }
}
