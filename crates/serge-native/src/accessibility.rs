//! The retained native view tree is also the source of platform semantics.
//! IDs follow node lifetimes, including keyed moves and arena slot reuse.
use super::{Ui, tree::NKind};
use accesskit::{
    Action, ActionData, ActionRequest, Node, Rect, Role, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use serge_core::{Attr, Kind};

impl Ui {
    /// Describes the current native tree and keyboard focus. Bounds are in
    /// window pixels at the supplied scale, matching the last layout.
    pub fn accessibility_tree(&self, title: &str, scale: f64) -> TreeUpdate {
        let tree = self.shared.tree.borrow();
        let root = tree.node(tree.root).expect("root").access_id;
        let mut nodes = Vec::new();
        tree.walk(tree.root, &mut |_, n| {
            let role = match n.kind {
                NKind::Root => Role::Window,
                NKind::Group | NKind::Widget(Kind::Stack | Kind::View | Kind::Card) => {
                    Role::GenericContainer
                }
                NKind::Widget(Kind::Text) => Role::Label,
                NKind::Widget(Kind::Button) => Role::Button,
                NKind::Widget(Kind::Link) => Role::Link,
                NKind::Widget(Kind::Checkbox) => Role::CheckBox,
                NKind::Widget(Kind::Input) => Role::TextInput,
                NKind::Widget(Kind::ProgressBar) => Role::ProgressIndicator,
                NKind::Widget(Kind::Slider) => Role::Slider,
                NKind::Widget(Kind::Canvas) => Role::Canvas,
            };
            let mut node = Node::new(role);
            if let Some(slot) = &n.viewport {
                if slot.unavailable() {
                    node.set_label(format!("{} (viewport unavailable)", slot.label()));
                } else {
                    node.set_label(slot.label());
                }
            }
            for child in n.children.iter().filter_map(|id| tree.node(*id)) {
                node.push_child(child.access_id);
            }
            let b = n.rect;
            let y = if n.kind == NKind::Root {
                b.y
            } else {
                b.y - tree.scroll_y
            };
            node.set_bounds(Rect {
                x0: f64::from(b.x) * scale,
                y0: f64::from(y) * scale,
                x1: f64::from(b.right()) * scale,
                y1: f64::from(y + b.height) * scale,
            });
            if n.kind == NKind::Root {
                node.set_label(title);
            }
            if !n.text().is_empty() {
                if role == Role::Label {
                    node.set_value(n.text());
                } else {
                    node.set_label(n.text());
                }
            }
            if let Some(p) = n.attr(Attr::Placeholder) {
                node.set_placeholder(p.as_str());
            }
            let disabled = n.flag(Attr::Disabled);
            if disabled {
                node.set_disabled();
            }
            if n.focusable() {
                node.add_action(Action::Focus);
            }
            match role {
                Role::Button | Role::Link | Role::CheckBox => {
                    if !disabled {
                        node.add_action(Action::Click);
                    }
                    if role == Role::CheckBox {
                        node.set_toggled(if n.flag(Attr::Checked) {
                            Toggled::True
                        } else {
                            Toggled::False
                        });
                    }
                    if let Some(href) = n.attr(Attr::Href) {
                        node.set_url(href.as_str());
                    }
                }
                Role::TextInput => {
                    node.set_value(n.input.as_str());
                    if !disabled {
                        node.add_action(Action::SetValue);
                    }
                }
                Role::Slider | Role::ProgressIndicator => {
                    node.set_numeric_value(f64::from(n.num(Attr::Value, 0.0)));
                    node.set_min_numeric_value(f64::from(n.num(Attr::Min, 0.0)));
                    node.set_max_numeric_value(f64::from(n.num(Attr::Max, 1.0)));
                    if role == Role::Slider && !disabled {
                        let step = n.num(Attr::Step, 0.0);
                        if step > 0.0 {
                            node.set_numeric_value_step(f64::from(step));
                        }
                        node.add_action(Action::SetValue);
                        node.add_action(Action::Increment);
                        node.add_action(Action::Decrement);
                    }
                }
                _ => {}
            }
            nodes.push((n.access_id, node));
        });
        let focus = tree
            .focus
            .and_then(|id| tree.node(id))
            .map_or(root, |n| n.access_id);
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    /// Applies a platform action to a live enabled widget using its normal
    /// view scope and input handlers. Late requests for unmounted nodes expire.
    pub fn accessibility_action(&self, request: ActionRequest) {
        if request.target_tree != TreeId::ROOT {
            return;
        }
        let tree = self.shared.tree.borrow();
        let Some((id, n)) = tree
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(id, n)| Some((id, n.as_ref()?)))
            .find(|(_, n)| n.access_id == request.target_node)
        else {
            return;
        };
        if n.flag(Attr::Disabled) {
            return;
        }
        let NKind::Widget(kind) = n.kind else {
            return;
        };
        match (request.action, kind, request.data) {
            (
                Action::Focus,
                Kind::Button | Kind::Link | Kind::Checkbox | Kind::Input | Kind::Slider,
                _,
            ) => {
                drop(tree);
                self.shared.tree.borrow_mut().focus = Some(id);
                self.shared.changed();
            }
            (Action::Click, Kind::Button | Kind::Link | Kind::Checkbox, _) => {
                drop(tree);
                self.activate(id);
            }
            (Action::SetValue, Kind::Input, Some(ActionData::Value(value))) => {
                drop(tree);
                self.shared.tree.borrow_mut().focus = Some(id);
                self.edit(|s| *s = value.chars().filter(|c| !c.is_control()).collect());
            }
            (action, Kind::Slider, data) => {
                drop(tree);
                match (action, data) {
                    (Action::SetValue, Some(ActionData::NumericValue(v))) => {
                        self.set_slider(id, |_, _| v as f32)
                    }
                    (Action::Increment | Action::Decrement, _) => self.set_slider(id, |n, step| {
                        n.num(Attr::Value, n.num(Attr::Min, 0.0))
                            + if action == Action::Increment {
                                step
                            } else {
                                -step
                            }
                    }),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
