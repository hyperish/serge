//! The playground against its orbic backend: remote calls over real HTTP on
//! loopback give the same results and errors as in-process calls, fall back
//! to the standard rail when the schema differs, and are cancelled with
//! their scope; and the app, headless, drives the backend end to end.
#![cfg(feature = "server")]
#![allow(missing_docs)]

use std::sync::Arc;
use std::time::{Duration, Instant};

mod common;
use common::{database_backend, in_process, run, scope_with, signed_in};
use hyve::api::Rail;
use orbic::{Db, OrbicApp};
use serge::RemoteTransport;
use serge::testing::{CpuRenderer, Headless};
use serge_playground::*;

/// Starts the backend on an ephemeral loopback port; returns its `/v1` URL.
fn serve(app: OrbicApp) -> String {
    let rt = serge::runtime();
    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let addr = listener.local_addr().unwrap();
    rt.spawn(orbic::serve(app, listener, std::future::pending()));
    format!("http://{addr}/v1")
}

#[test]
fn remote_calls_match_in_process_calls() {
    let base = serve(database_backend().1);
    for remote in [
        RemoteTransport::new(&base),
        RemoteTransport::new(&base).rail(Rail::Standard),
        // A stale schema: the server answers 415 and the call is retried
        // once on the standard rail.
        RemoteTransport::new(&base).claim_fingerprint(0xdead_beef),
    ] {
        let r = signed_in(PlaygroundSession::remote(remote));
        let l = signed_in(in_process());
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
    let base = serve(database_backend().1);
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
    let (db, backend) = database_backend();
    let session = PlaygroundSession::in_process(backend.build().unwrap());
    let caller = scope_with(Arc::new(session.clone()));
    let refused = run(&caller, || {
        create_project_rpc(CreateProject {
            title: "Guest project".into(),
        })
    })
    .unwrap_err();
    assert_eq!(refused.code(), "unauthenticated");
    assert!(stored(&db).is_empty());

    let app = app(session).assemble().unwrap();
    let h = Headless::with_scope(app.scope.clone(), 900.0, 700.0, move || app.element());
    assert!(h.shows("serge playground"), "{:?}", h.texts());

    // The project screens are guarded.
    h.click_text("Saved");
    assert!(
        h.wait_until(Duration::from_secs(1), |h| h
            .texts()
            .iter()
            .any(|t| t.contains("shared playground demo account"))),
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
    assert_stored(&db, "First project");

    // A fresh screen reads SQLite again; the previous screen's signal is gone.
    h.click_text("Home");
    h.click_text("Saved");
    assert!(h.wait_until(Duration::from_secs(2), |h| h.shows("#1  First project")));
    let rect = h.rect_of("#1  First project").unwrap();
    let frame = CpuRenderer::new().render(&h.settle(), 900, 700, 1.0);
    let background = frame.pixel(rect.x as u16, rect.y as u16);
    assert!(
        (rect.y as u16..rect.bottom() as u16)
            .any(|y| (rect.x as u16..rect.right() as u16).any(|x| frame.pixel(x, y) != background)),
        "the CPU renderer must draw the stored project label"
    );

    // An empty title is rejected and the stored row is unchanged.
    h.click_text("Save");
    assert!(
        h.wait_until(Duration::from_secs(2), |h| h
            .texts()
            .iter()
            .any(|t| t.starts_with("The server refused:"))),
        "{:?}",
        h.texts()
    );
    assert_stored(&db, "First project");
    h.click_text("Sign out");
    assert!(h.wait_until(Duration::from_secs(2), |h| h.shows("Sign in")));
    assert_eq!(
        run(&caller, list_projects).unwrap_err().code(),
        "unauthenticated"
    );
}

fn stored(db: &Db) -> Vec<StoredProject> {
    serge::runtime()
        .block_on(db.query::<StoredProject>().fetch_all())
        .unwrap()
}

fn assert_stored(db: &Db, title: &str) {
    let rows = stored(db);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].id, rows[0].owner.as_str(), rows[0].title.as_str()),
        (1, "demo", title)
    );
}

/// Exercises the shipped WASM app and its same-origin API proxy. The browser
/// CI job runs this test explicitly with a matching ChromeDriver.
#[test]
#[ignore = "requires Chrome, chromedriver, wasm32-unknown-unknown and the wasm-bindgen CLI"]
fn web_ui_saves_projects_through_the_backend() {
    use serge_web_dev::{BuildOptions, DevServer, Proxy, Workspace, build_web};
    use std::{path::Path, process::Command};

    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ws = Workspace::load(dir).unwrap();
    let package = ws.web_app(Some("serge-playground")).unwrap();
    let files = ws
        .target_dir
        .join(format!("playground-browser-{}", hyve::Ulid::new()));
    let options = BuildOptions {
        out_dir: Some(files.join("web")),
        ..Default::default()
    };
    let bundle = build_web(&ws, package, &options).unwrap();
    let url = format!(
        "sqlite:{}?mode=rwc",
        files.join("projects.sqlite").display()
    );
    let rt = serge::runtime();
    let db = rt.block_on(Db::connect(&url)).unwrap();
    let app = rt.block_on(backend(db.clone())).unwrap();
    let proxy = Proxy::from_url(&serve(app));
    let web = DevServer::start("127.0.0.1:0", bundle.dir, Some(proxy)).unwrap();
    let status = Command::new("python3")
        .arg(dir.join("tests/browser/projects.py"))
        .arg(format!("http://{}/", web.addr()))
        .status()
        .expect("start the browser test");
    assert!(status.success(), "browser test exited with {status}");
    rt.block_on(db.close());
    let reopened = rt.block_on(Db::connect(&url)).unwrap();
    assert_stored(&reopened, "Browser project");
    rt.block_on(reopened.close());
    std::fs::remove_dir_all(files).unwrap();
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
    use serge::testing::{CpuRenderer, Parity, gpu_test_renderer, write_ppm};
    let session = in_process();
    signed_in(session.clone());
    let app = app(session).assemble().unwrap();
    app.scope.get::<serge::AuthState>().unwrap().set(true);
    let h = Headless::with_scope(app.scope.clone(), 900.0, 640.0, move || app.element());
    let mut r = CpuRenderer::new();
    // The GPU path draws every screen like the CPU path (renderer symmetry,
    // `docs/03-adapters-and-pipeline.md` §1.2 tolerance), in one draw call.
    let mut gpu = gpu_test_renderer();
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
        let scene = h.settle();
        let frame = r.render(&scene, 1800, 1280, 2.0);
        let distinct: std::collections::HashSet<_> = frame.rgba.as_chunks::<4>().0.iter().collect();
        assert!(distinct.len() > 20, "{name} renders something");
        if let Some(gpu) = gpu.as_mut() {
            for (scale, w, h) in [(2.0, 1800, 1280), (1.0, 900, 640)] {
                let cpu = r.render(&scene, w, h, scale);
                let g = gpu.render(&scene, w, h, scale).expect("GPU frame");
                let p = Parity::of(&cpu, &g);
                eprintln!("{name} @{scale}x: {p:?}");
                assert!(p.within_tolerance(), "{name} @{scale}x: {p:?}");
                assert_eq!(gpu.stats().draw_calls, 1, "{name}: {:?}", gpu.stats());
            }
        }
        if let Ok(dir) = std::env::var("SERGE_SNAPSHOT_DIR") {
            write_ppm(
                &frame,
                &std::path::Path::new(&dir).join(format!("{name}.ppm")),
            )
            .unwrap();
        }
    }
}
