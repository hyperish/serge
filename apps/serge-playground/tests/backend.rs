//! The playground against its orbic backend: remote calls over real HTTP on
//! loopback give the same results and errors as in-process calls, fall back
//! to the standard rail when the schema differs, and are cancelled with
//! their scope; and the app, headless, drives the backend end to end.
#![cfg(feature = "server")]
#![allow(missing_docs)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use hyve::api::{Rail, Transport, provide_transport};
use hyve::prelude::*;
use serge::RemoteTransport;
use serge::testing::Headless;
use serge_playground::*;

/// Starts the backend on an ephemeral loopback port; returns its `/v1` URL.
fn serve() -> String {
    let rt = serge::runtime();
    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let addr = listener.local_addr().unwrap();
    rt.spawn(orbic::serve(backend(), listener, std::future::pending()));
    format!("http://{addr}/v1")
}

fn in_process() -> Arc<dyn Transport> {
    Arc::new(orbic::InProcess::new(backend().build().unwrap()))
}

fn scope_with(t: Arc<dyn Transport>) -> Scope {
    let s = Scope::root();
    provide_transport(&s, t);
    s
}

/// Makes the call inside `scope` (calls find their transport when made) and
/// waits for it.
fn run<F: std::future::IntoFuture>(scope: &Scope, f: impl FnOnce() -> F) -> F::Output {
    let call = scope.enter(|| f().into_future());
    serge::runtime().block_on(scope.scoped(call))
}

#[test]
fn remote_calls_match_in_process_calls() {
    let base = serve();
    for remote in [
        RemoteTransport::new(&base).into_arc(),
        RemoteTransport::new(&base).rail(Rail::Standard).into_arc(),
        // A stale schema: the server answers 415 and the call is retried
        // once on the standard rail.
        RemoteTransport::new(&base)
            .claim_fingerprint(0xdead_beef)
            .into_arc(),
    ] {
        let (r, l) = (scope_with(remote), scope_with(in_process()));
        let a = run(&r, || {
            create_project_rpc(CreateProject {
                title: "alpha".into(),
            })
        });
        let b = run(&l, || {
            create_project_rpc(CreateProject {
                title: "alpha".into(),
            })
        });
        assert_eq!(a.unwrap().title, b.unwrap().title);

        // Validation errors arrive as the same error.
        let a = run(&r, || {
            create_project_rpc(CreateProject {
                title: String::new(),
            })
        })
        .unwrap_err();
        let b = run(&l, || {
            create_project_rpc(CreateProject {
                title: String::new(),
            })
        })
        .unwrap_err();
        assert_eq!((a.code(), a.message()), (b.code(), b.message()));
        assert_eq!(a.details(), b.details());

        let list = run(&r, list_projects).unwrap();
        assert!(list.iter().any(|p| p.title == "alpha"), "{list:?}");
    }
}

#[test]
fn remote_calls_are_cancelled_with_their_scope() {
    let base = serve();
    let scope = scope_with(RemoteTransport::new(&base).into_arc());
    let s2 = scope.clone();
    serge::runtime().spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        s2.dispose();
    });
    let start = Instant::now();
    let e = run(&scope, || build_report(ReportRequest { millis: 5000 })).unwrap_err();
    assert_eq!(e.code(), "cancelled", "{e:?}");
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn the_app_saves_projects_through_the_backend() {
    let app = app(in_process()).assemble().unwrap();
    let h = Headless::with_scope(app.scope.clone(), 900.0, 700.0, move || app.element());
    assert!(h.shows("serge playground"), "{:?}", h.texts());

    // The project screens are guarded.
    h.click_text("Saved");
    assert!(
        h.wait_until(Duration::from_secs(1), |h| h
            .texts()
            .iter()
            .any(|t| t.contains("guarded with RequireAuth"))),
        "{:?}",
        h.texts()
    );
    // `Sign in` appears twice (nav link and button): click the button.
    let buttons = h.ui().widgets(serge::Kind::Button);
    let (r, _) = buttons
        .iter()
        .find(|(_, a)| a.iter().any(|(_, v)| v.as_str() == "Sign in"))
        .cloned()
        .unwrap_or_else(|| panic!("{:?}", h.texts()));
    h.click(r.center().x, r.center().y);
    assert!(
        h.wait_until(Duration::from_secs(1), |h| h.shows("좋아요")),
        "{:?}",
        h.texts()
    );

    h.click_text("Saved");
    assert!(
        h.wait_until(Duration::from_secs(2), |h| h.shows("No projects yet.")),
        "{:?}",
        h.texts()
    );
    h.type_into(0, "First project");
    h.click_text("Save");
    assert!(
        h.wait_until(Duration::from_secs(2), |h| h.shows("#1  First project")),
        "{:?}",
        h.texts()
    );
    assert!(h.shows("Saved #1 on the server."), "{:?}", h.texts());

    // The server's validation error is shown.
    h.click_text("Save");
    assert!(
        h.wait_until(Duration::from_secs(2), |h| h
            .texts()
            .iter()
            .any(|t| t.starts_with("The server refused:"))),
        "{:?}",
        h.texts()
    );
}

#[test]
fn telemetry_streams_into_the_canvas() {
    let app = app(in_process()).assemble().unwrap();
    let h = Headless::with_scope(app.scope.clone(), 900.0, 700.0, move || app.element());
    h.click_text("Telemetry");
    let polyline = |f: &[serge::ScenePrimitive]| {
        f.iter()
            .filter(|p| matches!(p, serge::ScenePrimitive::Line { points, .. } if points.len() > 2))
            .count()
    };
    assert!(
        h.wait_until(Duration::from_secs(2), |h| polyline(&h.settle()) > 0),
        "the sparkline is drawn"
    );
    assert!(
        h.ui().animating(),
        "a canvas reading a ring buffer keeps frames coming"
    );
    let first = h.settle();
    std::thread::sleep(Duration::from_millis(100));
    assert_ne!(h.settle(), first, "new samples repaint the canvas");
    // Leaving the screen stops the producer task.
    h.click_text("Home");
    assert_eq!(h.tasks(), 0);
}

/// Every screen renders on the CPU; with `SERGE_SNAPSHOT_DIR` set the
/// frames are written there (PPM) to look at.
#[test]
fn every_screen_renders() {
    use serge::testing::{CpuRenderer, write_ppm};
    let app = app(in_process()).assemble().unwrap();
    app.scope.get::<serge::AuthState>().unwrap().set(true);
    let h = Headless::with_scope(app.scope.clone(), 900.0, 640.0, move || app.element());
    let mut r = CpuRenderer::new();
    for (link, name) in [
        ("Home", "home"),
        ("Counter", "counter"),
        ("Todos", "todos"),
        ("Editor", "editor"),
        ("Saved", "saved"),
        ("Telemetry", "telemetry"),
    ] {
        h.click_text(link);
        h.wait_until(Duration::from_millis(300), |_| false);
        let frame = r.render(&h.settle(), 1800, 1280, 2.0);
        let distinct: std::collections::HashSet<_> = frame.rgba.as_chunks::<4>().0.iter().collect();
        assert!(distinct.len() > 20, "{name} renders something");
        if let Ok(dir) = std::env::var("SERGE_SNAPSHOT_DIR") {
            write_ppm(
                &frame,
                &std::path::Path::new(&dir).join(format!("{name}.ppm")),
            )
            .unwrap();
        }
    }
}
