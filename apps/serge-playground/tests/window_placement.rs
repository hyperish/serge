//! Save a moved and resized OS window, then restore it in an independent process.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn main() {
    use serge::{
        desktop::{
            self, NativeEvent, WindowEvent, WindowHandle, WindowOptions, WindowPlacement,
            WindowSpec, dpi,
        },
        prelude::*,
    };
    use std::{cell::Cell, path::PathBuf, process::Command, time::Duration};
    if std::env::var_os("SERGE_TEST_WINDOWS").is_none() {
        return;
    }
    let Some(path) = std::env::var_os("SERGE_PLACEMENT_FILE").map(PathBuf::from) else {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("windows/project.json");
        for step in ["save", "restore"] {
            assert!(
                Command::new(std::env::current_exe().unwrap())
                    .env("SERGE_PLACEMENT_FILE", &path)
                    .env("SERGE_PLACEMENT_STEP", step)
                    .status()
                    .unwrap()
                    .success()
            );
            let record: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(
                (record["width"].as_f64(), record["height"].as_f64()),
                (Some(318.), Some(209.))
            );
        }
        println!("Restored the moved and resized floating window in a new app process");
        return;
    };
    let (done, timeout) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if timeout.recv_timeout(Duration::from_secs(15)).is_err() {
            std::process::exit(1);
        }
    });
    let applied = Cell::new(false);
    let save = std::env::var("SERGE_PLACEMENT_STEP").unwrap() == "save";
    let area_file = path.clone();
    let spec = WindowSpec::new(
        WindowOptions {
            title: "Serge placement acceptance".into(),
            width: 224.,
            height: 182.,
        },
        hyve::Scope::root(),
        || ui! {Text{"Placement"}},
    )
    .placement(WindowPlacement {
        floating: true,
        bounds_file: Some(path),
    })
    .on_event(move |event| {
        if !matches!(event, NativeEvent::Window(WindowEvent::RedrawRequested)) {
            return;
        }
        WindowHandle::with_native(|w| {
            #[cfg(target_os = "macos")]
            {
                use objc2::MainThreadMarker;
                use objc2_app_kit::{NSApplication, NSFloatingWindowLevel};
                let win = NSApplication::sharedApplication(MainThreadMarker::new().unwrap())
                    .windows()
                    .iter()
                    .find(|native| native.title().to_string() == "Serge placement acceptance")
                    .unwrap();
                assert_eq!(win.level(), NSFloatingWindowLevel);
            }
            if save && !applied.replace(true) {
                let pos = w.current_monitor().unwrap().position();
                w.set_outer_position(dpi::PhysicalPosition::new(pos.x + 120, pos.y + 160));
                let _ = w.request_inner_size(dpi::LogicalSize::new(318., 209.));
                w.request_redraw();
                return;
            }
            let size = w.inner_size().to_logical::<f64>(w.scale_factor());
            if (size.width - 318.).abs() > 0.5 || (size.height - 209.).abs() > 0.5 {
                w.request_redraw();
                return;
            }
            if !save {
                let record: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&area_file).unwrap()).unwrap();
                let pos = w.outer_position().unwrap();
                assert!((i64::from(pos.x) - record["position"][0].as_i64().unwrap()).abs() <= 2);
                assert!((i64::from(pos.y) - record["position"][1].as_i64().unwrap()).abs() <= 2);
            }
            WindowHandle::current().unwrap().close().unwrap();
        })
        .unwrap();
    });
    desktop::run_windows(vec![spec]).unwrap();
    done.send(()).unwrap();
}
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn main() {}
