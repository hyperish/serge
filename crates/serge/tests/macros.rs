//! `ui!` and `#[component]` expand to code that compiles for the contract
//! examples (behavior is tested by the target harnesses).
#![allow(missing_docs)]

use hyve::prelude::*;
use serge::prelude::*;

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

#[derive(Clone, PartialEq, Debug)]
pub struct Todo {
    pub id: u32,
    pub title: String,
}

#[component]
pub fn Panel(
    title: String,
    subtitle: Option<String>,
    closed: Callback,
    children: Element,
) -> Element {
    let sub = subtitle.unwrap_or_default();
    ui! {
        Card {
            web::class: "panel",
            native::cursor: native::pointer,
            Text { "{title} — {sub}" }
            {children}
            Button { text: "Close", clicked => { closed.call(()); } }
        }
    }
}

#[component]
pub fn Synthetic() -> Element {
    let todos = signal(vec![Todo {
        id: 1,
        title: "a".into(),
    }]);
    let show = signal(true);
    let label = String::from("owned label");
    // Rust's move rule: an owned value used by two children is cloned first.
    let title = label.clone();
    let ratio = signal(0.25_f32);
    ui! {
        View { layout: h_stack, gap: 8px, width: 50%,
            if show() {
                Text { "{label} {ratio():.2}" }
                Panel { title: title.clone(), closed => { show.set(false); },
                    Text { "inside" }
                }
            } else {
                Text { "hidden" }
            }
            for todo in todos() {
                key: todo.id,
                Checkbox { text: todo.title.clone(), checked: false }
            }
            Slider { range: 0.0..=1.0, value: ratio(), changed(v) => { ratio.set(v); } }
            Button { text: if show() { "Hide" } else { "Show" }, clicked => async move { show.set(!show.get()); } }
            Canvas { height: 40px, paint(bounds, ctx) => { ctx.fill_rect(bounds, Color::hex(0x3366ff)); } }
            Text { "literal {{braces}}" }
        }
    }
}

#[test]
fn contract_examples_expand() {
    let owner = Owner::new();
    owner.with(|| {
        let e = CounterCard();
        assert!(format!("{e:?}").contains("Card"), "{e:?}");
        let e = Synthetic();
        assert!(format!("{e:?}").contains("View"), "{e:?}");
    });
    owner.dispose();
}
