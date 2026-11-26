use hyve::Scope;
use serge_core::{
    Attr, Element, Executor, Handler, Kind, LocalFuture, Node, TaskHandle, View, Widget,
    style::Theme,
};
use std::collections::BTreeMap;
use std::rc::Rc;

/// Escapes an HTML text node or quoted attribute.
pub fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

struct ServerExecutor;
impl Executor for ServerExecutor {
    // Client mount effects run when the browser mounts. An SSR handler loads
    // request data before it builds the view.
    fn spawn(&self, _: LocalFuture) -> TaskHandle {
        TaskHandle::new(|| {})
    }
}

struct Root(View);
impl Drop for Root {
    fn drop(&mut self) {
        self.0.dispose()
    }
}

/// Renders a request-local view tree on this thread. The host owns the parent
/// scope and the request's I/O. Dynamic properties are evaluated once.
pub fn render_element(scope: &Scope, theme: &Theme, make: impl FnOnce() -> Element) -> String {
    let root = Root(View::root(scope.child()));
    serge_core::with_executor(Rc::new(ServerExecutor), || {
        root.0.run(|| {
            let mut out = String::new();
            render(make().0, &root.0, theme, &mut out);
            out
        })
    })
}

fn render(node: Node, view: &View, theme: &Theme, out: &mut String) {
    match node {
        Node::Empty => (),
        Node::Fragment(nodes) => {
            for n in nodes {
                render(n, view, theme, out);
            }
        }
        Node::Region(f) => {
            let child = view.child();
            child.run(|| render(f().0, &child, theme, out));
        }
        Node::Keyed(list) => {
            for item in (list.items)() {
                let child = view.child();
                child.run(|| render((item.build)().0, &child, theme, out));
            }
        }
        Node::Scoped(scope, n) => {
            let child = view.child_in(&scope);
            child.run(|| render(*n, &child, theme, out));
        }
        Node::Widget(w) => render_widget(*w, view, theme, out),
    }
}

fn attribute(out: &mut String, k: &str, v: &str) {
    out.push(' ');
    out.push_str(k);
    out.push_str("=\"");
    out.push_str(&escape_html(v));
    out.push('"');
}

fn render_widget(w: Widget, view: &View, theme: &Theme, out: &mut String) {
    let (tag, class) = w.kind.html();
    let mut attributes = BTreeMap::new();
    attributes.insert("data-serge-node", class.to_owned());
    attributes.insert("class", class.to_owned());
    attributes.insert(
        "style",
        Widget::resolve_style(w.kind, theme, &w.style).to_css(),
    );
    match w.kind {
        Kind::Canvas => {
            if let Some(viewport) = w.handlers.iter().find_map(|handler| match handler {
                Handler::Viewport(viewport) => Some(viewport),
                _ => None,
            }) {
                attributes.insert("role", "img".into());
                attributes.insert("aria-label", viewport.label().into());
                attributes.insert("tabindex", "0".into());
            }
        }
        Kind::Slider => {
            attributes.insert("type", "range".into());
        }
        Kind::Input => {
            attributes.insert("type", "text".into());
        }
        Kind::Button => {
            attributes.insert("type", "button".into());
        }
        _ => (),
    }
    let mut text = String::new();
    let mut checked = false;
    let mut progress = 0.0;
    for (attr, value) in &w.attrs {
        let v = value.get();
        match attr {
            Attr::Text => text = v.as_str().to_owned(),
            Attr::Checked => checked = v.as_bool(),
            Attr::Value if w.kind == Kind::ProgressBar => progress = v.as_num().clamp(0., 1.),
            Attr::Value | Attr::Min | Attr::Max | Attr::Step => {
                let name = match attr {
                    Attr::Value => "value",
                    Attr::Max => "max",
                    Attr::Min => "min",
                    _ => "step",
                };
                attributes.insert(
                    name,
                    if *attr == Attr::Step && v.as_num() == 0. {
                        "any".into()
                    } else {
                        v.as_num().to_string()
                    },
                );
            }
            Attr::InputText | Attr::Placeholder | Attr::Href => {
                let name = match attr {
                    Attr::InputText => "value",
                    Attr::Placeholder => "placeholder",
                    _ => "href",
                };
                attributes.insert(name, v.as_str().to_owned());
            }
            Attr::Disabled => {
                if v.as_bool() {
                    attributes.insert("disabled", String::new());
                } else {
                    attributes.remove("disabled");
                }
            }
        }
    }
    for (name, value) in &w.web {
        match *name {
            "class" => {
                attributes.insert("class", format!("{class} {}", value.get()));
            }
            "id" | "title" | "aria-label" | "aria-hidden" | "aria-modal" | "tabindex" | "role" => {
                attributes.insert(*name, value.get());
            }
            _ => (),
        }
    }
    out.push('<');
    out.push_str(tag);
    for (name, value) in attributes {
        attribute(out, name, &value);
    }
    out.push('>');
    if tag == "input" {
        return;
    }
    match w.kind {
        Kind::Checkbox => {
            out.push_str(if checked {
                "<input type=\"checkbox\" checked><span>"
            } else {
                "<input type=\"checkbox\"><span>"
            });
            out.push_str(&escape_html(&text));
            out.push_str("</span>");
        }
        Kind::ProgressBar => {
            out.push_str(&format!("<div style=\"width:{}%\"></div>", progress * 100.));
        }
        Kind::Canvas => (),
        _ => out.push_str(&escape_html(&text)),
    }
    for n in w.children {
        render(n, view, theme, out);
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serge_core::viewport::Viewport;

    #[test]
    fn viewport_is_labelled_before_client_hydration() {
        let output = render_element(&Scope::root(), &Theme::LIGHT, || {
            Viewport::new("A & B").element([])
        });
        assert!(output.contains("aria-label=\"A &amp; B\""));
        assert!(output.contains("role=\"img\""));
    }
}
