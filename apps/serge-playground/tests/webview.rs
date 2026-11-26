//! A native webview in a Serge window, including resize and reload after a host-side write.
#[cfg(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "windows",
    target_os = "ios"
))]
#[path = "support/webview_document.rs"]
mod webview_document;
#[cfg(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "windows",
    target_os = "ios"
))]
fn main() {
    if std::env::var_os("SERGE_TEST_WEBVIEW").is_none() {
        return;
    }
    use hyve::{Scope, Value};
    use platform::{WindowHandle, WindowOptions, WindowSpec, run_windows};
    #[cfg(not(target_os = "ios"))]
    use serge::desktop as platform;
    #[cfg(target_os = "ios")]
    use serge::ios as platform;
    use serge::{
        prelude::*,
        webview::{LocalDocument, Rect, Slot, dpi::LogicalSize},
    };
    use std::{
        cell::{Cell, RefCell},
        fs,
        path::PathBuf,
        rc::Rc,
        time::Duration,
    };
    let dir = tempfile::tempdir().unwrap();
    let codec = std::env::var_os("SERGE_TEST_WEBVIEW_WASM").map(|path| {
        let path = PathBuf::from(path);
        (
            fs::read(path.join("webview_codec.js")).unwrap(),
            fs::read(path.join("webview_codec_bg.wasm")).unwrap(),
        )
    });
    let (bundle, hash) = webview_document::verified_page(
        dir.path(),
        include_str!("browser/webview.js"),
        codec
            .as_ref()
            .map(|(js, wasm)| (js.as_slice(), wasm.as_slice())),
    );
    assert!(LocalDocument::from_bundle(&bundle, "page.html", &"0".repeat(64)).is_err());
    let doc = LocalDocument::from_bundle(&bundle, "page.html", &hash).unwrap();
    let saved = Rc::new(Cell::new(0));
    let done = Rc::new(Cell::new(false));
    let count = saved.clone();
    let complete = done.clone();
    let (attached, attachment) = std::sync::mpsc::channel();
    let (stop, deadline) = std::sync::mpsc::channel();
    let watchdog = std::thread::spawn(move || {
        // A cold WebView2 profile is initialized by a separate browser process.
        let startup = if cfg!(target_os = "windows") { 120 } else { 35 };
        if attachment
            .recv_timeout(Duration::from_secs(startup))
            .is_err()
        {
            eprintln!("System WebView did not attach to its native window");
            std::process::exit(1);
        }
        if deadline.recv_timeout(Duration::from_secs(35)).is_err() {
            eprintln!("System WebView did not finish the state and event round trip");
            std::process::exit(1)
        }
    });
    #[cfg(target_os = "ios")]
    let completed = stop.clone();
    let spec = WindowSpec::new(WindowOptions::default(), Scope::root(), move || {
        #[cfg(target_os = "ios")]
        {
            let receipt = PathBuf::from(std::env::var_os("SERGE_TEST_WEBVIEW_RECEIPT").unwrap())
                .with_extension("url");
            let mut links = serge::sys::IosDeepLinks::new(&["serge-slot"], move |url| {
                fs::write(&receipt, url).is_ok()
            })
            .unwrap();
            links.install_application().unwrap();
            hyve::state::on_cleanup(move || drop(links));
            eprintln!("UIKit registered the URL handler");
        }
        #[cfg(not(target_os = "ios"))]
        let control = WindowHandle::current().unwrap();
        eprintln!("Attaching system WebView to its native window");
        let slot = WindowHandle::with_native(|window| {
            Slot::attach(
                window,
                Rect {
                    size: LogicalSize::new(320., 200.).into(),
                    ..Default::default()
                },
                doc,
                16,
                0.into(),
                |payload| {
                    if payload.get("op") == Some(&"denied".into()) {
                        Err(hyve::HyveError::domain("forbidden", "revoked"))
                    } else {
                        Ok(())
                    }
                },
                move |id, payload, slot| match payload.get("op") {
                    Some(Value::String(op)) if op == "save" => {
                        eprintln!("WebView requested save");
                        assert_eq!(count.replace(1), 0);
                        assert_eq!(id.seq, 1);
                        slot.update(1.into()).unwrap();
                        slot.reload().unwrap();
                        slot.finish(id, Ok("saved".into())).unwrap();
                    }
                    Some(Value::String(op)) if op == "done" => {
                        eprintln!("WebView completed its requests: {payload:?}");
                        assert_eq!(id.seq, 3);
                        assert_eq!(payload.get("rendered"), Some(&"1".into()));
                        assert_eq!(payload.get("guard"), Some(&"forbidden".into()));
                        assert_eq!(payload.get("inline"), Some(&false.into()));
                        assert_eq!(payload.get("width"), Some(&Value::Int(284)));
                        assert_eq!(payload.get("height"), Some(&Value::Int(162)));
                        assert_eq!(count.get(), 1);
                        complete.set(true);
                        slot.finish(id, Ok(Value::Null)).unwrap();
                        #[cfg(target_os = "ios")]
                        {
                            let receipt = std::env::var_os("SERGE_TEST_WEBVIEW_RECEIPT").unwrap();
                            fs::write(receipt, b"saved-once resized restored denied").unwrap();
                            completed.send(()).unwrap();
                        }
                        #[cfg(not(target_os = "ios"))]
                        control.close().unwrap();
                    }
                    _ => panic!("unexpected webview event {:?}", payload),
                },
            )
            .unwrap()
        })
        .expect("native current window");
        attached.send(()).unwrap();
        eprintln!("Attached system WebView");
        slot.set_bounds(Rect {
            position: serge::webview::dpi::LogicalPosition::new(12., 8.).into(),
            size: LogicalSize::new(284., 162.).into(),
        })
        .unwrap();
        let slot = RefCell::new(Some(slot));
        hyve::state::on_cleanup(move || {
            slot.borrow_mut().take();
        });
        ui! {Text{"Local slot"}}
    });
    run_windows(vec![spec]).unwrap();
    stop.send(()).unwrap();
    watchdog.join().unwrap();
    assert_eq!(saved.get(), 1);
    assert!(done.get());
    println!(
        "System WebView restored the pending event, displayed the new state and enforced the current guard"
    );
}
#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "windows",
    target_os = "ios"
)))]
fn main() {}
