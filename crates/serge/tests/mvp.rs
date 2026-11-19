//! Integration tests for `serge` Phase 1 MVP matching `README.md` and `docs/01-dx-and-dsl.md`.
#![allow(missing_docs)]

use hyve::prelude::*;
use serge::prelude::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// 1. Confirmed syntax from serge/README.md
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// 2. Confirmed syntax from serge/docs/01-dx-and-dsl.md
// ---------------------------------------------------------------------------

pub struct ProjectDraftStore {
    pub title: Signal<String>,
}

impl ProjectDraftStore {
    pub fn title(&self) -> String {
        self.title.get()
    }
}

pub struct CreateProject {
    pub title: String,
}

static SAVED_TITLES: Mutex<Option<Vec<String>>> = Mutex::new(None);

async fn create_project_rpc(req: CreateProject) -> HyveResult<()> {
    let mut guard = SAVED_TITLES.lock().expect("lock");
    guard.get_or_insert_with(Vec::new).push(req.title);
    Ok(())
}

pub fn project_feature() -> UiFeature {
    feature()
        .provide(|| ProjectDraftStore {
            title: signal(String::new()),
        })
        .wrap(RequireAuth::redirect("/login"))
        .route("/projects", ProjectEditorCard)
}

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
                    let _ = create_project_rpc(CreateProject { title: store.title() }).await;
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

pub struct ExportStatus {
    pub project_name: String,
    pub progress: f32,
}

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
// Tests
// ---------------------------------------------------------------------------

#[test]
fn counter_card_web_dom_patching_and_native_cpu_gpu_symmetry() {
    let card = CounterCard();
    let mut dom = DomTemplate::compile(&card).mount();

    assert!(dom.current_html().contains("Count: 0"));
    assert!(dom.current_html().contains("Increment"));
    assert!(dom.patch_dirty().is_empty());

    assert!(card.click_first_button("Increment"));
    let patches = dom.patch_dirty();
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].new_value, "Count: 1");
    assert!(dom.current_html().contains("Count: 1"));

    // Native layout + GPU batching + CPU software fallback share the exact same SceneBatch.
    let batch = layout_and_paint(&card, 320, 240, ColorTokens::dark());
    let gpu_stats = GpuBatchRenderer::prepare_frame(&batch);
    assert!(gpu_stats.draw_calls >= 1 && gpu_stats.draw_calls <= 3);
    assert!(gpu_stats.rect_instances >= 2);
    assert!(gpu_stats.text_runs >= 2);

    let cpu_fb = CpuFallbackRenderer::render(&batch);
    assert!(cpu_fb.painted_pixel_count() > 200);
    assert_eq!(
        serge::native::select_render_backend_with_override(true, Some("cpu")),
        RenderBackendKind::CpuFallback
    );
    assert_eq!(
        serge::native::select_render_backend_with_override(false, None),
        RenderBackendKind::CpuFallback
    );
}

#[test]
fn project_feature_di_auth_guard_async_click_and_ssr_handoff() {
    let feat = project_feature();

    // Unauthenticated route guard triggers redirect to /login.
    match feat.navigate("/projects", false) {
        RouteOutcome::Redirect(target) => assert_eq!(target, "/login"),
        _ => panic!("expected auth redirect to /login"),
    }

    // Authenticated navigation injects Ctx<ProjectDraftStore> and Signal<f32>.
    let store = feat.context::<ProjectDraftStore>();
    store.title.set("Lookdev Look".to_owned());

    let view = match feat.navigate("/projects", true) {
        RouteOutcome::Rendered { path, element } => {
            assert_eq!(path, "/projects");
            element
        }
        _ => panic!("expected rendered /projects route"),
    };

    assert!(view.collect_texts().iter().any(|t| t == "Exposure: 0.00"));
    assert!(view.set_slider_value(2.5));
    assert!(view.collect_texts().iter().any(|t| t == "Exposure: 2.50"));

    assert!(view.click_first_button("좋아요"));
    assert!(view.collect_texts().iter().any(|t| t == "좋아요 취소"));

    assert!(view.click_first_button("저장"));
    let saved = SAVED_TITLES.lock().expect("lock");
    assert!(
        saved
            .as_ref()
            .is_some_and(|v| v.contains(&"Lookdev Look".to_owned()))
    );

    // SSR manifest round-trip & cooperative cancellation contract.
    let mut assets = BTreeMap::new();
    assets.insert("app.wasm".to_owned(), "app.8a1f2c.wasm".to_owned());
    let manifest = SergeBundleManifest::from_feature(&feat, assets);
    let manifest_roundtrip = from_json_str::<SergeBundleManifest>(&to_json_string(&manifest))
        .expect("manifest roundtrip");
    assert_eq!(manifest, manifest_roundtrip);

    let ssr = FeatureSsrHandler::new(feat);
    let cancel = CancellationToken::new();
    let ok = ssr
        .render_ssr(&SsrRequest {
            path: "/projects".to_owned(),
            authenticated: true,
            cancel: cancel.clone(),
        })
        .expect("ssr ok");
    assert_eq!(ok.status, 200);
    assert!(ok.body_html.contains("Exposure:"));

    cancel.cancel();
    let cancelled = ssr.render_ssr(&SsrRequest {
        path: "/projects".to_owned(),
        authenticated: true,
        cancel,
    });
    assert_eq!(cancelled.unwrap_err().code(), "cancelled");
}

#[test]
fn realtime_telemetry_ring_buffer_renders_sparkline_on_cpu_fallback() {
    let ring = SharedRingBuf::<FrameMetrics>::new(128);
    for i in 1..=24 {
        ring.write(FrameMetrics::new(4.0 + (i as f32) * 0.25, 1.8));
    }

    let telemetry = RealtimeTelemetry(ring);
    let batch = layout_and_paint(&telemetry, 320, 100, ColorTokens::dark());
    let spark_count = batch
        .primitives
        .iter()
        .filter(|p| matches!(p, ScenePrimitive::SparklineSegment { .. }))
        .count();
    assert_eq!(spark_count, 23);

    let stats = GpuBatchRenderer::prepare_frame(&batch);
    assert_eq!(stats.line_segments, 23);

    let fb = CpuFallbackRenderer::render(&batch);
    assert!(fb.painted_pixel_count() > 20);
}

#[test]
fn ios_live_activity_swiftui_codegen_and_dsl_escape_hatches() {
    let island = ExportIsland(ExportStatus {
        project_name: "Studio Scene".to_owned(),
        progress: 0.75,
    });
    assert_eq!(island.name(), "ExportIsland");
    let swiftui = island.to_swiftui_code();
    assert!(swiftui.contains("ActivityConfiguration(for: ExportIslandAttributes.self)"));
    assert!(swiftui.contains("Image(systemName: \"cube\")"));
    assert!(swiftui.contains("Text(\"75%\")"));
    assert!(swiftui.contains("Text(\"Studio Scene 내보내는 중...\")"));
    assert!(swiftui.contains("ProgressView(value: 0.750)"));

    // Control flow (`if`, `for`), SEO `<Title>`/`<Meta>`, platform attributes, and PersistentSignal.
    let show_banner = signal(true);
    let items = vec!["Alpha", "Beta"];
    let page = ui! {
        Card {
            web::class: "hero-card",
            native::elevation: 4,
            Title { "Hyperish Serge" }
            Meta { name: "description", content: "Cross-platform UI" }
            if show_banner() {
                Text { "Ready" }
            } else {
                Text { "Loading" }
            }
            for item in items {
                Text { "{item}" }
            }
        }
    };
    let head = page.collect_head_metadata();
    assert_eq!(head.title.as_deref(), Some("Hyperish Serge"));
    assert_eq!(
        page.web_attrs.get("class"),
        Some(&serge::core::AttrVal::Str("hero-card".to_owned()))
    );
    assert!(page.collect_texts().contains(&"Ready".to_owned()));
    assert!(page.collect_texts().contains(&"Beta".to_owned()));

    let persisted = PersistentSignal::new("theme", Arc::new("dark".to_owned()).as_ref().clone());
    assert_eq!(persisted.get(), "dark");
    persisted.set("light".to_owned());
    assert_eq!(persisted.snapshot_json().expect("json"), "\"light\"");
}
