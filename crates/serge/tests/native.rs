//! The contract examples on the native runtime (headless): signals update
//! text, properties and `if`/`for` regions; handlers and async tasks run in
//! their view and stop when it unmounts.
#![cfg(not(target_arch = "wasm32"))]
#![allow(missing_docs)]

use std::time::Duration;

use hyve::prelude::*;
use serge::prelude::*;
use serge::testing::{Headless, Key};

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

#[test]
fn the_counter_counts() {
    let h = Headless::new(400.0, 300.0, || ui! { CounterCard {} });
    assert!(h.shows("Count: 0"), "{:?}", h.texts());
    h.click_text("Increment");
    h.click_text("Increment");
    assert!(h.shows("Count: 2"), "{:?}", h.texts());
    // The button sits below the text inside the card's padding.
    let (t, b) = (
        h.rect_of("Count: 2").unwrap(),
        h.rect_of("Increment").unwrap(),
    );
    assert!(t.x >= 16.0 && b.y >= t.bottom() + 12.0 - 0.5, "{t:?} {b:?}");
}

#[derive(Clone, PartialEq, Debug)]
pub struct Todo {
    pub id: u32,
    pub title: String,
    pub done: bool,
}

#[component]
pub fn Todos() -> Element {
    let todos = signal(vec![
        Todo {
            id: 1,
            title: "write spec".into(),
            done: false,
        },
        Todo {
            id: 2,
            title: "ship".into(),
            done: false,
        },
    ]);
    let draft = signal(String::new());
    let next = signal(3u32);
    let show_done = signal(true);
    ui! {
        Stack {
            Input {
                value: draft(),
                placeholder: "New task",
                changed(t) => { draft.set(t); },
                submitted => {
                    let title = draft.get();
                    if !title.is_empty() {
                        todos.update(|v| v.push(Todo { id: next.get(), title, done: false }));
                        next.update(|n| *n += 1);
                        draft.set(String::new());
                    }
                }
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
            Button {
                text: if show_done() { "Hide done" } else { "Show done" },
                clicked => { show_done.set(!show_done.get()); }
            }
            Text { "{todos.with(|v| v.iter().filter(|t| !t.done).count())} left" }
        }
    }
}

#[test]
fn regions_lists_and_inputs_follow_signals() {
    let h = Headless::new(400.0, 600.0, || ui! { Todos {} });
    assert!(
        h.shows("write spec") && h.shows("ship") && h.shows("2 left"),
        "{:?}",
        h.texts()
    );
    h.type_into(0, "test it");
    h.press(Key::Enter);
    assert!(h.shows("test it") && h.shows("3 left"), "{:?}", h.texts());
    // Typing again starts from the cleared draft.
    assert_eq!(h.texts()[0], "", "{:?}", h.texts());
    h.click_text("ship");
    assert!(h.shows("2 left"), "{:?}", h.texts());
    h.click_text("Hide done");
    assert!(!h.shows("ship") && h.shows("Show done"), "{:?}", h.texts());
    h.click_text("Show done");
    assert!(h.shows("ship"), "{:?}", h.texts());
    // Order is kept by key.
    let order: Vec<_> = h
        .texts()
        .into_iter()
        .filter(|t| ["write spec", "ship", "test it"].contains(&t.as_str()))
        .collect();
    assert_eq!(order, ["write spec", "ship", "test it"]);
}

#[component]
pub fn Exposure() -> Element {
    let exposure = signal(0.0_f32);
    ui! {
        Card {
            Text { "Exposure: {exposure():.2}" }
            Slider { range: -5.0..=5.0, value: exposure(), changed(v) => { exposure.set(v); } }
            Canvas {
                height: 64px,
                paint(bounds, ctx) => {
                    let w = bounds.width * (exposure() + 5.0) / 10.0;
                    ctx.fill_rect(Rect::new(0.0, 0.0, w, bounds.height), Color::hex(0x3b5bdb));
                }
            }
        }
    }
}

fn canvas_fill_width(frame: &[ScenePrimitive]) -> Option<f32> {
    frame.iter().find_map(|p| match p {
        ScenePrimitive::Quad { color, rect, .. }
            if *color == Color::hex(0x3b5bdb) && rect.height == 64.0 =>
        {
            Some(rect.width)
        }
        _ => None,
    })
}

use serge::ScenePrimitive;

#[test]
fn sliders_drive_text_and_canvas_paint() {
    let h = Headless::new(400.0, 300.0, || ui! { Exposure {} });
    assert!(h.shows("Exposure: 0.00"), "{:?}", h.texts());
    let half = canvas_fill_width(&h.settle()).expect("canvas painted");
    h.slide(0, 1.0);
    assert!(h.shows("Exposure: 5.00"), "{:?}", h.texts());
    let full = canvas_fill_width(&h.settle()).expect("canvas repainted");
    assert!((full - 2.0 * half).abs() < 1.0, "{half} {full}");
}

#[component]
pub fn Loader() -> Element {
    let status = signal(String::from("idle"));
    ui! {
        Stack {
            Text { "{status()}" }
            Button {
                text: "Load",
                clicked => async move {
                    status.set("loading".into());
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    status.set("loaded".into());
                }
            }
        }
    }
}

#[test]
fn async_handlers_run_without_blocking_and_stop_on_unmount() {
    let h = Headless::new(300.0, 200.0, || ui! { Loader {} });
    h.click_text("Load");
    assert!(
        h.shows("loading"),
        "the click returned before the task finished"
    );
    assert!(
        h.wait_until(Duration::from_secs(2), |h| h.shows("loaded")),
        "{:?}",
        h.texts()
    );

    let show = signal(true);
    let h = Headless::new(300.0, 200.0, move || ui! { if show() { Loader {} } });
    h.click_text("Load");
    assert_eq!(h.tasks(), 1);
    show.set(false);
    h.settle();
    assert_eq!(h.tasks(), 0, "unmounting the view dropped its task");
}

#[test]
fn backend_policy() {
    use serge_native::{Backend, BackendError, choose_backend};
    let ok = || Ok(());
    let fail = || Err("no adapter".to_string());
    assert_eq!(choose_backend(None, false, ok), Ok(Backend::Gpu));
    assert_eq!(choose_backend(None, false, fail), Ok(Backend::Cpu));
    assert_eq!(choose_backend(None, true, ok), Ok(Backend::Cpu));
    assert_eq!(choose_backend(Some("gpu"), true, ok), Ok(Backend::Gpu));
    assert_eq!(choose_backend(Some("cpu"), false, ok), Ok(Backend::Cpu));
    assert!(matches!(
        choose_backend(Some("gpu"), false, fail),
        Err(BackendError::GpuUnavailable(_))
    ));
    assert!(matches!(
        choose_backend(Some("vulkan"), false, ok),
        Err(BackendError::Unknown(_))
    ));
}

#[component]
pub fn Tally(label: String) -> Element {
    let mut n = signal(0);
    // Two closures use the owned `label`: clone first (the closure rule).
    let button = format!("+1 {label}");
    ui! {
        Stack {
            Text { "{label}: {n()}" }
            Button { text: button, clicked => { n += 1; } }
        }
    }
}

#[test]
fn components_rebuilt_by_their_props_start_over() {
    let round = signal(1);
    let h = Headless::new(400.0, 400.0, move || {
        ui! {
            Stack {
                Tally { label: "fixed" }
                Tally { label: format!("round {}", round()) }
            }
        }
    });
    h.click_text("+1 fixed");
    h.click_text("+1 round 1");
    assert!(
        h.shows("fixed: 1") && h.shows("round 1: 1"),
        "{:?}",
        h.texts()
    );
    // A prop that reads a signal re-creates its component (local state is
    // gone); a component with fixed props keeps its state.
    round.set(2);
    h.settle();
    assert!(
        h.shows("fixed: 1") && h.shows("round 2: 0"),
        "{:?}",
        h.texts()
    );
}
