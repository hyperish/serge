use super::{Field, Node, Part, Platform, Widget};
use std::fmt::Write;
use syn::Result;

fn literal(text: &str, platform: Platform) -> String {
    let mut s = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => s.push_str("\\\\"),
            '"' => s.push_str("\\\""),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            '$' if !platform.is_apple() => s.push_str("\\$"),
            c if c.is_control() && !platform.is_apple() => {
                write!(s, "\\u{:04x}", u32::from(c)).unwrap()
            }
            c if c.is_control() => write!(s, "\\u{{{:x}}}", u32::from(c)).unwrap(),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}
impl Widget {
    fn value(&self, name: &str) -> String {
        if self.platform.is_apple() {
            let i = self
                .fields
                .keys()
                .position(|n| n == name)
                .expect("parsed widget field");
            let entry = if self.platform == Platform::LiveActivity {
                "context"
            } else {
                "entry"
            };
            format!("{entry}.state.v{i}")
        } else {
            let key = literal(name, self.platform);
            match self.fields[name] {
                Field::Text => format!("state.optString({key}, \"\")"),
                Field::Number => format!("state.optDouble({key}, 0.0)"),
            }
        }
    }
    fn text_expression(&self, parts: &[Part]) -> String {
        parts
            .iter()
            .map(|p| match p {
                Part::Literal(s) => literal(s, self.platform),
                Part::Field(n) => self.value(n),
                Part::Percent(n) => {
                    let value = self.value(n);
                    if self.platform.is_apple() {
                        format!("String(UInt32(min(Double(UInt32.max), max(0, {value} * 100.0))))")
                    } else {
                        format!(
                            "(({value} * 100.0).coerceIn(0.0, 4294967295.0).toLong()).toString()"
                        )
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" + ")
    }
    fn view(&self, node: &Node, level: usize, out: &mut String) {
        let indent = "    ".repeat(level);
        let swift = self.platform.is_apple();
        let mut line = |v: &str| writeln!(out, "{indent}{v}").unwrap();
        match node {
            Node::Text(parts) => {
                let text = self.text_expression(parts);
                line(&if swift {
                    format!("Text(verbatim: {text})")
                } else {
                    format!("Text({text})")
                });
            }
            Node::Icon(icon) => {
                let name = literal(icon, self.platform);
                if swift {
                    line(&format!("Image(systemName: {name})"));
                } else {
                    line(&format!(
                        "Image(ImageProvider(R.drawable.{icon}), contentDescription = {name})"
                    ));
                }
            }
            Node::Progress(key) => {
                let v = self.value(key);
                line(&if swift {
                    format!("ProgressView(value: min(1.0, max(0.0, {v})))")
                } else {
                    format!("LinearProgressIndicator(progress = {v}.coerceIn(0.0, 1.0).toFloat())")
                });
            }
            Node::Stack {
                horizontal,
                gap,
                children,
            } => {
                let (layout, dimension) = if *horizontal {
                    ("Row", "width")
                } else {
                    ("Column", "height")
                };
                let name = if swift {
                    if *horizontal { "HStack" } else { "VStack" }
                } else {
                    layout
                };
                line(&if swift {
                    format!("{name}(spacing: {gap}) {{")
                } else {
                    format!("{name} {{")
                });
                for (i, child) in children.iter().enumerate() {
                    if !swift && i > 0 && *gap > 0 {
                        writeln!(
                            out,
                            "{indent}    Spacer(GlanceModifier.{dimension}({gap}.dp))"
                        )
                        .unwrap();
                    }
                    self.view(child, level + 1, out);
                }
                writeln!(out, "{indent}}}").unwrap();
            }
        }
    }
}
fn android_ident(s: &str) -> bool {
    !s.is_empty()
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn check_package(w: &Widget, name: &str) -> Result<()> {
    if !name.contains('.') || !name.split('.').all(android_ident) {
        return Err(syn::Error::new(
            w.name.span(),
            "android_package must be a dotted Android package namespace",
        ));
    }
    Ok(())
}
pub(super) fn source(w: &Widget, android_package: &str) -> Result<String> {
    if !w
        .name
        .to_string()
        .bytes()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
        || !w
            .name
            .to_string()
            .bytes()
            .all(|c| c.is_ascii_alphanumeric())
    {
        return Err(syn::Error::new(
            w.name.span(),
            "use an ASCII UpperCamelCase widget type name",
        ));
    }
    let name = &w.name;
    let mut out = String::new();
    match w.platform {
        Platform::LiveActivity => {
            writeln!(out,"import ActivityKit\nimport WidgetKit\nimport SwiftUI\n\n@available(iOS 16.2, *)\nstruct {name}Attributes: ActivityAttributes {{\n    struct ContentState: Codable, Hashable {{").unwrap();
            for (i, ty) in w.fields.values().enumerate() {
                let ty = if *ty == Field::Text {
                    "String"
                } else {
                    "Double"
                };
                writeln!(out, "        var v{i}: {ty}").unwrap();
            }
            if !w.fields.is_empty() {
                out.push_str("        enum CodingKeys: String, CodingKey {\n");
                for (i, key) in w.fields.keys().enumerate() {
                    writeln!(out, "            case v{i} = {}", literal(key, w.platform)).unwrap();
                }
                out.push_str("        }\n");
            }
            writeln!(out,"    }}\n}}\n\n@available(iOS 16.2, *)\nstruct {name}: Widget {{\n    var body: some WidgetConfiguration {{\n        ActivityConfiguration(for: {name}Attributes.self) {{ context in").unwrap();
            w.view(&w.sections["expanded"], 3, &mut out);
            out.push_str("        } dynamicIsland: { context in\n            DynamicIsland {\n                DynamicIslandExpandedRegion(.center) {\n");
            w.view(&w.sections["expanded"], 5, &mut out);
            out.push_str("                }\n            } compactLeading: {\n");
            w.view(&w.sections["compact_leading"], 4, &mut out);
            out.push_str("            } compactTrailing: {\n");
            w.view(&w.sections["compact_trailing"], 4, &mut out);
            out.push_str("            } minimal: {\n");
            w.view(&w.sections["compact_leading"], 4, &mut out);
            out.push_str("            }\n        }\n    }\n}\n");
        }
        Platform::WidgetKit => return Ok(widgetkit(w)),
        Platform::Ongoing => {
            check_package(w, android_package)?;
            return Ok(notification(w, android_package));
        }
        Platform::Glance => {
            check_package(w, android_package)?;
            // Android resource and class names must be valid at compile time.
            fn icons(n: &Node) -> bool {
                match n {
                    Node::Icon(s) => android_ident(s),
                    Node::Stack { children, .. } => children.iter().all(icons),
                    _ => true,
                }
            }
            if !icons(&w.sections["body"]) {
                return Err(syn::Error::new(
                    w.name.span(),
                    "an Android Icon names a lowercase drawable resource",
                ));
            }
            let package = android_package
                .split('.')
                .map(|s| format!("\u{60}{s}\u{60}"))
                .collect::<Vec<_>>()
                .join(".");
            writeln!(out, "package {package}\n").unwrap();
            out.push_str("import android.content.Context\nimport androidx.compose.ui.unit.dp\nimport androidx.datastore.preferences.core.Preferences\nimport androidx.datastore.preferences.core.stringPreferencesKey\nimport androidx.glance.*\nimport androidx.glance.appwidget.*\nimport androidx.glance.appwidget.state.updateAppWidgetState\nimport androidx.glance.layout.*\nimport androidx.glance.text.Text\nimport org.json.JSONObject\n\n");
            writeln!(out,"class {name} : GlanceAppWidget() {{\n    companion object {{\n        private val key = stringPreferencesKey(\"serge.{name}.state\")\n\n        suspend fun updateState(context: Context, id: GlanceId, json: String) {{\n            JSONObject(json)\n            updateAppWidgetState(context, id) {{ prefs -> prefs[key] = json }}\n            {name}().update(context, id)\n        }}\n    }}\n\n    override suspend fun provideGlance(context: Context, id: GlanceId) {{\n        provideContent {{\n            val state = JSONObject(currentState<Preferences>()[key] ?: \"{{}}\")").unwrap();
            w.view(&w.sections["body"], 3, &mut out);
            writeln!(out,"        }}\n    }}\n}}\n\nclass {name}Receiver : GlanceAppWidgetReceiver() {{\n    override val glanceAppWidget = {name}()\n}}").unwrap();
        }
    }
    Ok(out)
}
pub(super) fn resources(w: &Widget, package: &str) -> Result<Vec<(String, String)>> {
    if w.platform != Platform::Glance {
        return Ok(vec![]);
    }
    check_package(w, package)?;
    let filename = format!("serge_{}", w.name.to_string().to_ascii_lowercase());
    let provider="<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<appwidget-provider xmlns:android=\"http://schemas.android.com/apk/res/android\" android:minWidth=\"110dp\" android:minHeight=\"40dp\" android:resizeMode=\"horizontal|vertical\" android:widgetCategory=\"home_screen\" android:initialLayout=\"@layout/glance_default_loading_layout\" android:updatePeriodMillis=\"0\" />\n".to_string();
    let receiver = format!(
        "    <receiver android:name=\"{package}.{}Receiver\" android:exported=\"false\">\n      <intent-filter><action android:name=\"android.appwidget.action.APPWIDGET_UPDATE\" /></intent-filter>\n      <meta-data android:name=\"android.appwidget.provider\" android:resource=\"@xml/{filename}\" />\n    </receiver>\n",
        w.name
    );
    Ok(vec![
        (format!("res/xml/{filename}.xml"), provider),
        ("receiver".into(), receiver),
    ])
}

fn widgetkit(widget: &Widget) -> String {
    let mut fields = String::new();
    let mut keys = String::new();
    if !widget.fields.is_empty() {
        keys.push_str("    enum CodingKeys: String, CodingKey {\n");
        for (i, (name, field)) in widget.fields.iter().enumerate() {
            let declaration = match field {
                Field::Text => "String = \"\"",
                Field::Number => "Double = 0",
            };
            writeln!(fields, "    var v{i}: {declaration}").unwrap();
            writeln!(
                keys,
                "        case v{i} = {}",
                literal(name, widget.platform)
            )
            .unwrap();
        }
        keys.push_str("    }\n");
    }
    let mut view = String::new();
    widget.view(&widget.sections["body"], 4, &mut view);
    include_str!("../templates/widget.swift")
        .replace("{{name}}", &widget.name())
        .replace("{{fields}}", &fields)
        .replace("{{keys}}", &keys)
        .replace("{{view}}", &view)
}

fn notification(widget: &Widget, package: &str) -> String {
    let text = |section: &str| match widget.sections.get(section) {
        Some(Node::Text(parts)) => Some(widget.text_expression(parts)),
        _ => None,
    };
    let progress = match widget.sections.get("progress") {
        Some(Node::Progress(key)) => format!(
            "        builder.setProgress(100, ({}.coerceIn(0.0, 1.0) * 100.0).toInt(), false)\n",
            widget.value(key)
        ),
        _ => String::new(),
    };
    let package = package
        .split('.')
        .map(|s| format!("\u{0060}{s}\u{0060}"))
        .collect::<Vec<_>>()
        .join(".");
    let source = include_str!("../templates/ongoing.kt");
    // Substitute each section once. User-visible text may itself contain braces.
    let (head, tail) = source.split_once("{{title}}").unwrap();
    let (middle, tail) = tail.split_once("{{body}}{{progress}}").unwrap();
    let head = head
        .replace("{{package}}", &package)
        .replace("{{name}}", &widget.name());
    let tail = tail.replace("{{name}}", &widget.name());
    let title = text("title").expect("parsed notification title");
    let body = text("body")
        .map(|s| format!("        builder.setContentText({s})\n"))
        .unwrap_or_default();
    format!("{head}{title}{middle}{body}{progress}{tail}")
}
