//! Retained text adapted from tachys 0.3.0-rc1, `view/strings.rs` and `renderer/dom.rs`,
//! commit c3e11dce0d46f669ec639d01e1e4de4e32f71755 (MIT, see LICENSE).
//! One source slot owns its text node for both initial mounting and SSR adoption.
use serge_core::AttrValue;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, Text};

pub(super) struct TextSlot {
    node: Text,
    previous: String,
}

impl TextSlot {
    pub(super) fn adopt(parent: &Element, document: &Document) -> Self {
        if let Some(node) = parent.first_child()
            && node.next_sibling().is_none()
            && let Ok(text) = node.dyn_into::<Text>()
        {
            return Self {
                previous: text.node_value().unwrap_or_default(),
                node: text,
            };
        }
        let node = document.create_text_node("");
        parent.set_text_content(None);
        parent.append_child(&node).expect("mount text slot");
        Self {
            node,
            previous: String::new(),
        }
    }

    pub(super) fn rebuild(&mut self, next: AttrValue) {
        let text = match next {
            AttrValue::Str(text) => text,
            _ => String::new(),
        };
        if text != self.previous {
            self.node.set_node_value(Some(&text));
            self.previous = text;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn existing_server_text_survives_empty_and_repeated_updates() {
        let doc = crate::document();
        let el = doc.create_element("span").unwrap();
        el.set_text_content(Some("server"));
        let original = el.first_child().unwrap();
        let mut slot = TextSlot::adopt(&el, &doc);
        for s in ["server", "", "client", "client"] {
            slot.rebuild(AttrValue::Str(s.into()));
            assert_eq!(el.text_content().as_deref(), Some(s));
            assert!(original.is_same_node(el.first_child().as_ref()));
        }
    }

    #[wasm_bindgen_test]
    #[ignore = "Chrome release comparison of repeated DOM text updates"]
    fn compare_retained_text() {
        let doc = crate::document();
        let el = doc.create_element("span").unwrap();
        doc.body().unwrap().append_child(&el).unwrap();
        let now = web_sys::window().unwrap().performance().unwrap();
        let mut times = [Vec::new(), Vec::new(), Vec::new()];
        for round in 0..22 {
            for mode in [round % 3, (round + 1) % 3, (round + 2) % 3] {
                let mut slot = TextSlot::adopt(&el, &doc);
                // StringState::rebuild in the pinned tachys version, on this browser's Text.
                let node = slot.node.clone();
                let mut previous = slot.previous.clone();
                let started = now.now();
                for i in 0..40_000 {
                    let value = if i & 2 == 0 { "first" } else { "second" }.to_owned();
                    match mode {
                        0 => slot.rebuild(AttrValue::Str(value)),
                        1 => el.set_text_content(Some(&value)),
                        _ if previous != value => {
                            node.set_node_value(Some(&value));
                            previous = value;
                        }
                        _ => {}
                    }
                }
                if round != 0 {
                    times[mode].push((now.now() - started) * 1_000_000. / 40_000.);
                }
            }
        }
        el.remove();
        for (label, mut samples) in ["Serge slot", "element textContent", "tachys rebuild"]
            .into_iter()
            .zip(times)
        {
            samples.sort_by(f64::total_cmp);
            console_log!(
                "{label} ns per update median {:.1} ({:.1}..{:.1})",
                samples[10],
                samples[0],
                samples[20]
            );
        }
    }
}
