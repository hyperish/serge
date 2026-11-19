//! iOS Dynamic Island / Live Activity & WidgetKit SwiftUI codegen bridge (`#[ios::live_activity]`).

use crate::core::Element;
use std::collections::BTreeMap;

/// Marks a SwiftUI Live Activity / Dynamic Island component (`#[ios::live_activity]`).
pub use serge_macros::live_activity_attr as live_activity;

/// Static UI snapshot tree describing an iOS Dynamic Island & Lock Screen Live Activity.
#[derive(Clone, Default)]
pub struct LiveActivity {
    name: String,
    compact_leading: Option<Element>,
    compact_trailing: Option<Element>,
    expanded: Option<Element>,
    minimal: Option<Element>,
    extra_slots: BTreeMap<String, Element>,
}

impl LiveActivity {
    /// Creates an empty [`LiveActivity`] descriptor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            name: "SergeLiveActivity".to_owned(),
            ..Self::default()
        }
    }

    /// Assigns the SwiftUI widget/activity type name (set automatically by `#[ios::live_activity]`).
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Returns the component activity identifier.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the Dynamic Island compact leading slot.
    #[must_use]
    pub fn compact_leading(mut self, element: Element) -> Self {
        self.compact_leading = Some(element);
        self
    }

    /// Sets the Dynamic Island compact trailing slot.
    #[must_use]
    pub fn compact_trailing(mut self, element: Element) -> Self {
        self.compact_trailing = Some(element);
        self
    }

    /// Sets the Dynamic Island expanded region slot.
    #[must_use]
    pub fn expanded(mut self, element: Element) -> Self {
        self.expanded = Some(element);
        self
    }

    /// Sets the Dynamic Island minimal pill slot.
    #[must_use]
    pub fn minimal(mut self, element: Element) -> Self {
        self.minimal = Some(element);
        self
    }

    /// Sets an arbitrary named Live Activity slot.
    #[must_use]
    pub fn slot(mut self, name: impl Into<String>, element: Element) -> Self {
        self.extra_slots.insert(name.into(), element);
        self
    }

    /// Transpiles this static `ui!` snapshot into SwiftUI `ActivityConfiguration` Widget Extension code.
    #[must_use]
    pub fn to_swiftui_code(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "import ActivityKit\nimport SwiftUI\nimport WidgetKit\n\nstruct {}Widget: Widget {{\n    var body: some WidgetConfiguration {{\n        ActivityConfiguration(for: {}Attributes.self) {{ context in\n",
            self.name, self.name
        ));
        if let Some(exp) = &self.expanded {
            emit_swiftui_node(exp, 3, &mut out);
        }
        out.push_str("        } dynamicIsland: { context in\n            DynamicIsland {\n                DynamicIslandExpandedRegion(.bottom) {\n");
        if let Some(exp) = &self.expanded {
            emit_swiftui_node(exp, 5, &mut out);
        }
        out.push_str("                }\n            } compactLeading: {\n");
        if let Some(cl) = &self.compact_leading {
            emit_swiftui_node(cl, 4, &mut out);
        }
        out.push_str("            } compactTrailing: {\n");
        if let Some(ct) = &self.compact_trailing {
            emit_swiftui_node(ct, 4, &mut out);
        }
        out.push_str("            } minimal: {\n");
        if let Some(min) = self.minimal.as_ref().or(self.compact_leading.as_ref()) {
            emit_swiftui_node(min, 4, &mut out);
        }
        out.push_str("            }\n        }\n    }\n}\n");
        out
    }
}

fn emit_swiftui_node(el: &Element, indent: usize, out: &mut String) {
    let pad = "    ".repeat(indent);
    match el.tag.as_str() {
        "Icon" => {
            let sym = el.resolved_text().unwrap_or_else(|| "circle".to_owned());
            out.push_str(&format!("{pad}Image(systemName: \"{sym}\")\n"));
        }
        "Text" => {
            let txt = el.resolved_text().unwrap_or_default().replace('"', "\\\"");
            out.push_str(&format!("{pad}Text(\"{txt}\")\n"));
        }
        "ProgressBar" => {
            let val = el
                .resolved_value()
                .and_then(|v| v.as_f32())
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            out.push_str(&format!("{pad}ProgressView(value: {val:.3})\n"));
        }
        "Stack" | "Card" => {
            let gap = el.props.get("gap").map_or(8.0, |p| p.as_px(8.0));
            out.push_str(&format!(
                "{pad}VStack(alignment: .leading, spacing: {gap}) {{\n"
            ));
            for ch in el.resolved_children() {
                emit_swiftui_node(&ch, indent + 1, out);
            }
            out.push_str(&format!("{pad}}}\n"));
        }
        _ => {
            for ch in el.resolved_children() {
                emit_swiftui_node(&ch, indent, out);
            }
        }
    }
}
