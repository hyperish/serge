//! Two real OS windows share the event loop and retain separate view lifetimes.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(any(target_os = "android", target_os = "ios"))
))]
fn main() {
    if std::env::var_os("SERGE_TEST_WINDOWS").is_none() {
        return;
    }
    use hyve::Scope;
    use serge::{
        desktop::{NativeEvent, WindowEvent, WindowHandle, WindowOptions, WindowSpec, run_windows},
        prelude::*,
    };
    use std::{
        sync::mpsc::{self, Sender},
        time::{Duration, Instant},
    };
    type Frame = (WindowHandle, Scope, u32);
    fn window(sender: Sender<Frame>, value: hyve::state::Signal<u32>) -> Element {
        let handle = WindowHandle::current().unwrap();
        let scope = Scope::current().unwrap();
        let last = std::rc::Rc::new(std::cell::Cell::new(None));
        ui! { Canvas { height: 80px, paint(bounds, _ctx) => {
            let current = value();
            if bounds.width > 0.0 && last.replace(Some(current)) != Some(current) {
                let _ = sender.send((handle.clone(), scope.clone(), current));
            }
        } } }
    }
    fn until(what: &str, mut ready: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !ready() {
            assert!(Instant::now() < end, "{what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let (tx, rx) = mpsc::channel::<Frame>();
    let first = tx.clone();
    let value = hyve::state::shared_signal(0);
    let (events, observations) = mpsc::channel();
    let driver = std::thread::spawn(move || {
        let execute = || {
            let (one, scope_one, initial) = rx.recv_timeout(Duration::from_secs(20)).unwrap();
            assert_eq!(initial, 0);
            assert_eq!(
                observations.recv_timeout(Duration::from_secs(5)).unwrap(),
                "resumed"
            );
            assert_eq!(
                observations.recv_timeout(Duration::from_secs(5)).unwrap(),
                "redraw"
            );
            value.set(7);
            let (_, _, repainted) = rx
                .recv_timeout(Duration::from_secs(10))
                .expect("worker woke the UI thread");
            assert_eq!(repainted, 7);
            one.open(
                WindowOptions {
                    title: "Second".into(),
                    ..Default::default()
                },
                Scope::root(),
                move || window(tx, value),
            )
            .unwrap();
            let (two, scope_two, initial) = rx.recv_timeout(Duration::from_secs(20)).unwrap();
            assert_eq!(initial, 7);
            two.focus().unwrap();
            one.close().unwrap();
            until("first window did not dispose", || scope_one.is_disposed());
            assert!(
                !scope_two.is_disposed(),
                "the second window closed with the first"
            );
            two.close().unwrap();
            until("second window did not dispose", || scope_two.is_disposed());
        };
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(execute)).is_err() {
            std::process::exit(1);
        }
    });
    run_windows(vec![
        WindowSpec::new(
            WindowOptions {
                title: "First".into(),
                ..Default::default()
            },
            Scope::root(),
            move || window(first, value),
        )
        .on_event(move |event| {
            let kind = match event {
                NativeEvent::Resumed => "resumed",
                NativeEvent::Window(WindowEvent::RedrawRequested) => "redraw",
                _ => return,
            };
            assert!(WindowHandle::with_native(|w| w.title() == "First").unwrap());
            let _ = events.send(kind);
        }),
    ])
    .unwrap();
    driver.join().unwrap();
    println!("two windows presented frames and closed independently");
}
#[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
fn main() {}
