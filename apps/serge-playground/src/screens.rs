//! The playground's screens. `CounterCard`, `project_feature`,
//! `ProjectEditorCard` and `RealtimeTelemetry` are the documented examples
//! verbatim; the rest frames them.

use std::time::Duration;

use hyve::prelude::*;
use serge::prelude::*;

use crate::server::{
    CreateProject, Project, ReportRequest, build_report, create_project_rpc, list_projects,
};

/// Secondary text (readable on the light and the dark theme).
pub const MUTED: Color = Color::hex(0x7a8190);

// ---------------------------------------------------------------------------
// Frame

#[component]
pub fn NavLink(to: String, label: String) -> Element {
    let here = to.clone();
    ui! {
        Link {
            to: to,
            font_weight: if location().split('?').next() == Some(here.as_str()) { 700 } else { 400 },
            "{label}"
        }
    }
}

#[component]
pub fn Shell(auth: Ctx<AuthState>, children: Element) -> Element {
    let auth = *auth;
    ui! {
        View { padding: 20px, gap: 16px,
            Stack { layout: h_stack, gap: 16px, align: Align::Center,
                Text { font_size: 18px, font_weight: 700, "serge" }
                NavLink { to: "/", label: "Home" }
                NavLink { to: "/counter", label: "Counter" }
                NavLink { to: "/todos", label: "Todos" }
                NavLink { to: "/projects", label: "Editor" }
                NavLink { to: "/projects/saved", label: "Saved" }
                NavLink { to: "/telemetry", label: "Telemetry" }
                View { grow: 1.0 }
                if auth.is_signed_in() {
                    Button { text: "Sign out", clicked => { auth.set(false); navigate("/"); } }
                } else {
                    NavLink { to: "/login", label: "Sign in" }
                }
            }
            {children}
        }
    }
}

#[component]
pub fn Home() -> Element {
    ui! {
        Card { gap: 12px,
            Text { font_size: 22px, font_weight: 700, "serge playground" }
            Text { color: MUTED, "The same Rust UI on the web (real DOM) and on the desktop (native window, CPU renderer)." }
            Link { to: "/counter", "Counter — the README example" }
            Link { to: "/todos", "Todos — inputs, checkboxes, keyed lists, if-regions" }
            Link { to: "/projects", "Project editor — the docs example (needs sign-in)" }
            Link { to: "/projects/saved", "Saved projects — #[server] calls, validation errors, cancellation" }
            Link { to: "/telemetry", "Telemetry — SharedRingBuf streaming into a Canvas" }
        }
    }
}

// ---------------------------------------------------------------------------
// README, verbatim.

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

#[component]
pub fn CounterScreen() -> Element {
    ui! {
        Stack { gap: 12px,
            CounterCard {}
            CounterCard {}
            Text { color: MUTED, "Each card owns its own signal; leaving the screen drops them." }
        }
    }
}

// ---------------------------------------------------------------------------
// Todos

#[derive(Clone, PartialEq, Debug)]
pub struct Todo {
    pub id: u32,
    pub title: String,
    pub done: bool,
}

#[component]
pub fn TodoScreen() -> Element {
    let todos = signal(vec![
        Todo {
            id: 1,
            title: "Read the spec".into(),
            done: true,
        },
        Todo {
            id: 2,
            title: "Try the playground".into(),
            done: false,
        },
    ]);
    let draft = signal(String::new());
    let next = signal(3u32);
    let show_done = signal(true);
    let add = move || {
        let title = draft.get_untracked().trim().to_owned();
        if !title.is_empty() {
            todos.update(|v| {
                v.push(Todo {
                    id: next.get_untracked(),
                    title,
                    done: false,
                })
            });
            next.update(|n| *n += 1);
            draft.set(String::new());
        }
    };
    ui! {
        Card { gap: 10px,
            Text { font_size: 18px, font_weight: 700, "Todos" }
            Stack { layout: h_stack, gap: 8px, align: Align::Center,
                Input {
                    grow: 1.0,
                    value: draft(),
                    placeholder: "What needs doing? (Enter adds)",
                    changed(t) => { draft.set(t); },
                    submitted => { add(); }
                }
                Button { text: "Add", disabled: draft().trim().is_empty(), clicked => { add(); } }
            }
            for todo in todos() {
                key: todo.id,
                if show_done() || !todo.done {
                    Checkbox {
                        text: todo.title.clone(),
                        checked: todo.done,
                        changed(c) => {
                            let id = todo.id;
                            todos.update(|v| if let Some(t) = v.iter_mut().find(|t| t.id == id) { t.done = c; });
                        }
                    }
                }
            }
            Stack { layout: h_stack, gap: 8px, align: Align::Center,
                Text { grow: 1.0, color: MUTED, "{todos.with(|v| v.iter().filter(|t| !t.done).count())} left" }
                Button {
                    text: if show_done() { "Hide done" } else { "Show done" },
                    clicked => { show_done.set(!show_done.get()); }
                }
                Button { text: "Clear done", clicked => { todos.update(|v| v.retain(|t| !t.done)); } }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `docs/01-dx-and-dsl.md` §1: the app's own pieces the example leaves out.

pub struct ProjectDraftStore {
    pub title: Signal<String>,
    pub exposure: Signal<f32>,
}

/// The example's `ctx.draw_histogram(..)`: an app helper over `PaintCtx`.
pub trait Histogram {
    fn draw_histogram(&mut self, bounds: Rect, exposure: f32);
}

impl Histogram for PaintCtx {
    fn draw_histogram(&mut self, bounds: Rect, exposure: f32) {
        let bars = 32;
        let w = bounds.width / bars as f32;
        let peak = (exposure + 5.0) / 10.0;
        self.fill_rounded_rect(bounds, 6.0, Color::hex(0x0f1424));
        for i in 0..bars {
            let t = (i as f32 + 0.5) / bars as f32;
            let d = (t - peak) / 0.22;
            let h = bounds.height * (0.08 + 0.85 * (-d * d).exp());
            self.fill_rect(
                Rect::new(i as f32 * w + 1.0, bounds.height - h, w - 2.0, h),
                Color::hex(0x7b93ff).mix(Color::hex(0xffb86b), t),
            );
        }
    }
}

// ---- `docs/01-dx-and-dsl.md` §1, verbatim from here ----

// 1) 도메인/기능 단위 조립: `serge::feature()` 순수 Rust 체이닝 고정 (`orbic::component()`와 충돌 0)
pub fn project_feature() -> UiFeature {
    feature()
        .provide(|| ProjectDraftStore {
            title: signal(String::new()),
            exposure: signal(0.0_f32),
        })
        .wrap(RequireAuth::redirect("/login"))
        .route("/projects", ProjectEditorCard)
}

// 2) 뷰(UI 트리) 마크업: `ui!` 5대 최소 문법 + 인라인 `#[server]` 호출 (Web DOM & Native GPU/CPU 공통)
#[component]
pub fn ProjectEditorCard(store: Ctx<ProjectDraftStore>) -> Element {
    // 라우트 진입 컴포넌트는 주입(`Ctx<T>`)과 라우트 파라미터만 받는다. `Signal<T>`는 `Copy` 핸들이라 꺼내 써도 된다.
    let exposure = store.exposure;
    let liked = signal(false);

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
                    // `#[server]` 함수 호출: 클라이언트 빌드에서는 서버 본문·DB 의존성이 완전히 제거된 RPC 스텁으로 치환됨
                    let _ = create_project_rpc(CreateProject { title: store.title.get() }).await;
                }
            }
            // 저수준 GPU/CPU 또는 Canvas/WebGPU 직접 드로잉이 필요할 때는 Canvas 위젯의 paint 이벤트로 탈출
            Canvas {
                height: 64px,
                paint(bounds, ctx) => {
                    ctx.draw_histogram(bounds, exposure());
                }
            }
        }
    }
}

// ---- end of the example ----

/// The saved projects, from the server: the same feature (and store) as
/// the editor, so the title typed here is what the editor's 저장 sends.
#[component]
pub fn SavedProjects(store: Ctx<ProjectDraftStore>) -> Element {
    let title = store.title;
    let projects = signal(Vec::<Project>::new());
    let status = signal(String::from("Loading…"));
    let report = signal(String::new());
    let busy = signal(false);

    spawn(async move {
        match list_projects().await {
            Ok(v) => {
                status.set(if v.is_empty() {
                    "No projects yet.".into()
                } else {
                    String::new()
                });
                projects.set(v);
            }
            Err(e) => status.set(format!("Could not load: {} ({})", e.message(), e.code())),
        }
    });
    let save = move || {
        spawn(async move {
            busy.set(true);
            let r = create_project_rpc(CreateProject {
                title: title.get_untracked(),
            })
            .await;
            busy.set(false);
            match r {
                Ok(p) => {
                    status.set(format!("Saved #{} on the server.", p.id));
                    projects.update(|v| v.push(p));
                    title.set(String::new());
                }
                Err(e) => status.set(format!(
                    "The server refused: {} ({})",
                    e.message(),
                    e.code()
                )),
            }
        })
    };

    ui! {
        Card { gap: 10px,
            Text { font_size: 18px, font_weight: 700, "Saved projects" }
            Text { color: MUTED, "Saved through #[server] calls. Titles are validated on the server (1–80 characters): try saving an empty one." }
            Stack { layout: h_stack, gap: 8px, align: Align::Center,
                Input {
                    grow: 1.0,
                    value: title(),
                    placeholder: "Project title",
                    changed(t) => { title.set(t); },
                    submitted => { save(); }
                }
                Button { text: if busy() { "Saving…" } else { "Save" }, disabled: busy(), clicked => { save(); } }
            }
            if !status().is_empty() {
                Text { "{status()}" }
            }
            for p in projects() {
                key: p.id,
                Text { "#{p.id}  {p.title}" }
            }
            Stack { layout: h_stack, gap: 8px, align: Align::Center,
                Button {
                    text: "Build a slow report (3 s)",
                    clicked => async move {
                        report.set("Building… leave the screen to cancel the call.".into());
                        match build_report(ReportRequest { millis: 3000 }).await {
                            Ok(s) => report.set(s),
                            Err(e) => report.set(format!("{} ({})", e.message(), e.code())),
                        }
                    }
                }
                Text { grow: 1.0, color: MUTED, "{report()}" }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Telemetry: `docs/01-dx-and-dsl.md` §1 (3), with the app's sample type and
// drawing helper.

/// One engine sample (`#[repr(C)]` POD, what an engine thread or a shared
/// memory ring would carry).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, serge::bytemuck::Pod, serge::bytemuck::Zeroable)]
#[bytemuck(crate = "serge::bytemuck")]
pub struct FrameMetrics {
    pub frame_ms: f32,
}

impl FrameMetrics {
    pub const ZERO: FrameMetrics = FrameMetrics { frame_ms: 0.0 };
}

/// The example's `ctx.draw_sparkline(..)`.
pub trait Sparkline {
    fn draw_sparkline(&mut self, bounds: Rect, samples: &[FrameMetrics]);
}

impl Sparkline for PaintCtx {
    fn draw_sparkline(&mut self, bounds: Rect, samples: &[FrameMetrics]) {
        self.fill_rounded_rect(bounds, 6.0, Color::hex(0x0f1424));
        let max_ms = 40.0_f32;
        let y =
            |ms: f32| bounds.height - 6.0 - (ms / max_ms).clamp(0.0, 1.0) * (bounds.height - 12.0);
        // 60 fps budget.
        self.line(
            Point::new(0.0, y(16.7)),
            Point::new(bounds.width, y(16.7)),
            1.0,
            Color::rgba(255, 255, 255, 60),
        );
        if samples.len() < 2 {
            return;
        }
        let step = bounds.width / (samples.len() - 1) as f32;
        let points: Vec<Point> = samples
            .iter()
            .enumerate()
            .map(|(i, s)| Point::new(i as f32 * step, y(s.frame_ms)))
            .collect();
        self.polyline(&points, 2.0, Color::hex(0x5eead4));
        let last = samples[samples.len() - 1].frame_ms;
        self.text(
            Point::new(8.0, 6.0),
            format!("{last:.1} ms"),
            12.0,
            Color::hex(0xe8eaef),
        );
    }
}

// ---- verbatim ----

// 3) 로컬 엔진 공유 메모리 Ring Buffer(`SharedRingBuf`, #[repr(C)] POD) & iOS Dynamic Island(`#[ios::live_activity]`) 활용
#[component]
pub fn RealtimeTelemetry(ring_buf: SharedRingBuf<FrameMetrics>) -> Element {
    ui! {
        Canvas {
            height: 80px,
            paint(bounds, ctx) => {
                // 인프로세스든 로컬 데몬(Shm IPC)이든 찢어짐(tearing) 없는 스냅샷/읽기 가드로 최신 샘플을 안전하게 드로잉
                let mut samples = [FrameMetrics::ZERO; 128];
                let n = ring_buf.read_latest_snapshot(&mut samples);
                ctx.draw_sparkline(bounds, &samples[..n]);
            }
        }
    }
}

// ---- end ----

#[component]
pub fn TelemetryScreen() -> Element {
    let ring = SharedRingBuf::new(512);
    let running = signal(true);
    let load = signal(0.3_f32);
    let samples = signal(0u64);

    // The "engine": a sample every 16 ms until the screen is left (the task
    // belongs to this screen's view).
    let engine = ring.clone();
    spawn(async move {
        let (mut t, mut seed) = (0.0_f32, 0x2545_f491_u32);
        loop {
            sleep(Duration::from_millis(16)).await;
            if !running.get_untracked() {
                continue;
            }
            t += 0.016;
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed % 1000) as f32 / 1000.0 - 0.5;
            let spike = if seed % 97 == 0 { 14.0 } else { 0.0 };
            let ms =
                6.0 + load.get_untracked() * 22.0 + (t * 2.5).sin() * 2.0 + noise * 3.0 + spike;
            engine.push(FrameMetrics {
                frame_ms: ms.max(0.5),
            });
            samples.update(|n| *n += 1);
        }
    });

    ui! {
        Card { gap: 10px,
            Text { font_size: 18px, font_weight: 700, "Realtime telemetry" }
            Text { color: MUTED, "A producer pushes a frame sample every 16 ms into a SharedRingBuf. The canvas reads it in paint and repaints on new data; no signal per sample." }
            RealtimeTelemetry { ring_buf: ring.clone() }
            Text { "Load {load() * 100.0:.0}%  ·  {samples()} samples" }
            Slider { range: 0.0..=1.0, value: load(), changed(v) => { load.set(v); } }
            Button {
                text: if running() { "Pause" } else { "Resume" },
                clicked => { running.set(!running.get()); }
            }
        }
    }
}

// ---------------------------------------------------------------------------

#[component]
pub fn Login(auth: Ctx<AuthState>) -> Element {
    let auth = *auth;
    ui! {
        Card { gap: 10px,
            Text { font_size: 18px, font_weight: 700, "Sign in" }
            Text { color: MUTED, "The project screens are guarded with RequireAuth. This demo sign-in only flips the UI's AuthState." }
            Button { text: "Sign in", clicked => { auth.set(true); navigate("/projects"); } }
        }
    }
}
