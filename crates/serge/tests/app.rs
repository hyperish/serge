//! The `docs/01-dx-and-dsl.md` §1 example (`project_feature`,
//! `ProjectEditorCard`) as an app on the headless native runtime: guard
//! redirect, sign-in, the inline `#[server]` call through a transport,
//! feature-scoped state across screens, and cancellation on screen change.
#![cfg(not(target_arch = "wasm32"))]
#![allow(missing_docs)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyve::api::{BoxFuture, Call, Reply, Transport};
use hyve::prelude::*;
use serge::prelude::*;
use serge::testing::Headless;

// The app's own types (the example leaves them to the app).
#[model]
#[derive(Debug, Clone, PartialEq)]
pub struct CreateProject {
    pub title: String,
}

#[model]
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub title: String,
}

#[server(POST "/projects")]
pub async fn create_project_rpc(input: CreateProject) -> HyveResult<Project> {
    Ok(Project { title: input.title })
}

pub struct ProjectDraftStore {
    pub title: Signal<String>,
    pub exposure: Signal<f32>,
}

/// The example's `ctx.draw_histogram(..)` is an app helper over `PaintCtx`.
pub trait Histogram {
    fn draw_histogram(&mut self, bounds: Rect, exposure: f32);
}

impl Histogram for PaintCtx {
    fn draw_histogram(&mut self, bounds: Rect, exposure: f32) {
        let bars = 16;
        let w = bounds.width / bars as f32;
        for i in 0..bars {
            let t = i as f32 / bars as f32;
            let h =
                bounds.height * (0.2 + 0.8 * (1.0 - (t - (exposure + 5.0) / 10.0).abs()).max(0.0));
            self.fill_rect(
                Rect::new(i as f32 * w, bounds.height - h, w - 1.0, h),
                Color::hex(0x7b93ff),
            );
        }
    }
}

// ---- docs/01-dx-and-dsl.md §1, verbatim from here ----

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

#[component]
pub fn Login(auth: Ctx<AuthState>) -> Element {
    ui! {
        Card {
            Text { "Please sign in" }
            Button { text: "Sign in", clicked => { auth.set(true); navigate("/projects"); } }
        }
    }
}

/// Records calls; with `hold`, waits for cancellation instead of answering.
#[derive(Default)]
struct Recorder {
    log: Mutex<Vec<String>>,
    hold: Mutex<bool>,
    held: Mutex<Option<CancellationToken>>,
}

impl Transport for Recorder {
    fn call(&self, call: Call) -> BoxFuture<'_, HyveResult<Reply>> {
        let hold = *self.hold.lock().unwrap();
        Box::pin(async move {
            let body = call.encode_json();
            if hold {
                self.log.lock().unwrap().push(format!("held {body}"));
                *self.held.lock().unwrap() = Some(call.cancellation().clone());
                call.cancellation().cancelled().await;
                self.log.lock().unwrap().push("cancelled".into());
                return Err(hyve::api::errors::cancelled("screen left"));
            }
            self.log.lock().unwrap().push(format!("sent {body}"));
            Ok(Reply::value(Project { title: "x".into() }))
        })
    }
}

fn app(transport: Arc<Recorder>) -> serge::Assembled {
    SergeApp::new()
        .provide(AuthState::new(false))
        .transport(transport)
        .start_path("/projects")
        .feature(project_feature())
        .feature(feature().route("/login", Login))
        .assemble()
        .expect("valid app")
}

#[test]
fn the_example_app_runs_end_to_end() {
    let t = Arc::new(Recorder::default());
    let app = app(t.clone());
    let h = Headless::with_scope(app.scope.clone(), 480.0, 480.0, || app.element());
    // Signed out: the guard sends `/projects` to `/login`.
    assert!(
        h.wait_until(Duration::from_secs(1), |h| h.shows("Please sign in")),
        "{:?}",
        h.texts()
    );
    h.click_text("Sign in");
    assert!(
        h.shows("Exposure: 0.00") && h.shows("좋아요"),
        "{:?}",
        h.texts()
    );
    h.click_text("좋아요");
    assert!(h.shows("좋아요 취소"), "{:?}", h.texts());
    h.slide(0, 1.0);
    assert!(h.shows("Exposure: 5.00"), "{:?}", h.texts());

    // The inline server call goes through the provided transport.
    h.click_text("저장");
    assert!(h.wait_until(Duration::from_secs(1), |_| {
        !t.log.lock().unwrap().is_empty()
    }));
    assert_eq!(t.log.lock().unwrap()[0], r#"sent {"title":""}"#);

    // The store is feature-scoped: it survives leaving and re-entering.
    *t.hold.lock().unwrap() = true;
    h.click_text("저장");
    assert!(h.wait_until(Duration::from_secs(1), |_| t.log.lock().unwrap().len() == 2));
    navigate("/login");
    h.settle();
    // Leaving the screen dropped its task and cancelled the call's token
    // (what a transport doing I/O elsewhere watches).
    let token = t.held.lock().unwrap().clone().expect("held call");
    assert!(token.is_cancelled(), "{:?}", t.log.lock().unwrap());
    assert_eq!(h.tasks(), 0);
    navigate("/projects");
    h.settle();
    assert!(h.shows("Exposure: 5.00"), "{:?}", h.texts());
    // `liked` was the screen's own signal: it starts over.
    assert!(h.shows("좋아요"), "{:?}", h.texts());
}

#[test]
fn invalid_apps_are_reported_at_assembly() {
    let e = SergeApp::new()
        .feature(project_feature())
        .assemble()
        .unwrap_err();
    assert!(e.to_string().contains("no `AuthState` is provided"), "{e}");
    let e = SergeApp::new().assemble().unwrap_err();
    assert!(e.to_string().contains("no routes"), "{e}");
    let e = SergeApp::new()
        .provide(AuthState::new(true))
        .feature(feature().route("/a", Login).route("/a", Login))
        .assemble()
        .unwrap_err();
    assert!(e.to_string().contains("defined twice"), "{e}");
}
