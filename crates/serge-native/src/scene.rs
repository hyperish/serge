//! Layout and scene collection: the retained tree becomes `taffy` input,
//! then a `ScenePrimitive` list in paint order (`docs/03-adapters-and-pipeline.md`
//! §1.2).

use serge_core::{Attr, Kind, PaintCmd, Point, Rect, ScenePrimitive};
use serge_native_layout::{LayoutNode, WidthHint, compute};
use serge_native_text::TextSystem;
use serge_style::{Color, Length, Style, Theme};

use crate::tree::{NKind, NNode, Tree};

/// The checkbox square's side.
pub(crate) const CHECK: f32 = 18.0;
/// The slider thumb's radius.
pub(crate) const THUMB: f32 = 9.0;

fn px(l: Length) -> f32 {
    l.px().unwrap_or(0.0)
}

/// The content box (inside padding and border).
pub(crate) fn content(n: &NNode) -> Rect {
    let s = &n.style;
    let bx = px(s.padding_x) + px(s.border_width);
    let by = px(s.padding_y) + px(s.border_width);
    Rect::new(
        n.rect.x + bx,
        n.rect.y + by,
        (n.rect.width - 2.0 * bx).max(0.0),
        (n.rect.height - 2.0 * by).max(0.0),
    )
}

fn label(n: &NNode) -> (&str, bool) {
    match n.kind {
        NKind::Widget(Kind::Input) if n.input.is_empty() => {
            (n.attr(Attr::Placeholder).map_or("", |v| v.as_str()), true)
        }
        NKind::Widget(Kind::Input) => (n.input.as_str(), false),
        _ => (n.text(), false),
    }
}

fn measured(kind: NKind) -> bool {
    matches!(
        kind,
        NKind::Widget(Kind::Text | Kind::Link | Kind::Button | Kind::Input | Kind::Checkbox)
    )
}

/// Lays out the tree in `width` × `height`; returns canvases whose size
/// changed (their `bounds` signals are set by the caller, outside the
/// borrow).
pub(crate) fn layout(tree: &mut Tree, text: &mut TextSystem) -> Vec<(usize, Rect)> {
    let (width, height) = tree.size;
    // Flatten groups: layout sees widgets only.
    let mut nodes: Vec<LayoutNode> = Vec::new();
    let mut ids: Vec<usize> = Vec::new();
    fn flat_children(tree: &Tree, id: usize, out: &mut Vec<usize>) {
        if let Some(n) = tree.node(id) {
            for &c in &n.children {
                match tree.node(c).map(|n| n.kind) {
                    Some(NKind::Group) => flat_children(tree, c, out),
                    Some(_) => out.push(c),
                    None => {}
                }
            }
        }
    }
    fn add(tree: &Tree, id: usize, nodes: &mut Vec<LayoutNode>, ids: &mut Vec<usize>) -> usize {
        let n = tree.node(id).expect("alive");
        let index = nodes.len();
        nodes.push(LayoutNode {
            style: n.style.clone(),
            measured: measured(n.kind),
            children: Vec::new(),
        });
        ids.push(id);
        let mut kids = Vec::new();
        flat_children(tree, id, &mut kids);
        let children = kids.into_iter().map(|c| add(tree, c, nodes, ids)).collect();
        nodes[index].children = children;
        index
    }
    let root = tree.root;
    if let Some(r) = tree.node_mut(root) {
        r.style = Style {
            padding_x: Length::Px(0.0),
            padding_y: Length::Px(0.0),
            ..Style::default()
        };
    }
    add(tree, root, &mut nodes, &mut ids);
    let boxes = compute(&nodes, 0, width, Some(height), |i, hint| {
        let n = tree.node(ids[i]).expect("alive");
        let s = &n.style;
        let max = match hint {
            WidthHint::Exact(w) | WidthHint::AtMost(w) => Some(w),
            WidthHint::Min => Some(1.0),
            WidthHint::Max => None,
        };
        match n.kind {
            NKind::Widget(Kind::Checkbox) => {
                let (w, h) = text.measure(
                    n.text(),
                    s.font_px(),
                    s.font_weight,
                    max.map(|m| (m - CHECK - px(s.gap)).max(1.0)),
                );
                (CHECK + px(s.gap) + w, h.max(CHECK))
            }
            NKind::Widget(Kind::Input) => {
                let (t, _) = label(n);
                let (w, h) = text.measure(
                    if t.is_empty() { " " } else { t },
                    s.font_px(),
                    s.font_weight,
                    None,
                );
                (w + 2.0, h)
            }
            _ => {
                let (t, _) = label(n);
                text.measure(t, s.font_px(), s.font_weight, max)
            }
        }
    });
    // Absolute rects.
    let mut resized = Vec::new();
    #[allow(clippy::too_many_arguments)]
    fn place(
        tree: &mut Tree,
        nodes: &[LayoutNode],
        ids: &[usize],
        boxes: &[serge_native_layout::LayoutBox],
        i: usize,
        ox: f32,
        oy: f32,
        resized: &mut Vec<(usize, Rect)>,
    ) {
        let b = boxes[i];
        let rect = Rect::new(ox + b.x, oy + b.y, b.width, b.height);
        if let Some(n) = tree.node_mut(ids[i]) {
            n.rect = rect;
            if let Some(c) = &n.canvas {
                let local = Rect::new(0.0, 0.0, rect.width, rect.height);
                if c.bounds.get_untracked() != local {
                    resized.push((ids[i], local));
                }
            }
        }
        for &c in &nodes[i].children {
            place(tree, nodes, ids, boxes, c, rect.x, rect.y, resized);
        }
    }
    place(
        tree,
        &nodes,
        &ids,
        &boxes,
        0,
        tree.origin.0,
        tree.origin.1,
        &mut resized,
    );
    tree.content_height = boxes[0].height;
    let max_scroll = (tree.content_height - height).max(0.0);
    tree.scroll_y = tree.scroll_y.clamp(0.0, max_scroll);
    resized
}

/// The primitives of the whole window.
pub(crate) fn collect(tree: &Tree, text: &mut TextSystem, theme: &Theme) -> Vec<ScenePrimitive> {
    let (w, h) = tree.size;
    let mut out = vec![ScenePrimitive::Quad {
        rect: Rect::new(tree.origin.0, tree.origin.1, w, h),
        color: theme.background,
        radius: 0.0,
    }];
    let dy = -tree.scroll_y;
    visit(tree, tree.root, text, theme, dy, &mut out);
    out
}

fn shift(r: Rect, dy: f32) -> Rect {
    r.offset(0.0, dy)
}

fn visit(
    tree: &Tree,
    id: usize,
    text: &mut TextSystem,
    theme: &Theme,
    dy: f32,
    out: &mut Vec<ScenePrimitive>,
) {
    let Some(n) = tree.node(id) else { return };
    if let NKind::Widget(kind) = n.kind {
        draw(tree, id, n, kind, text, theme, dy, out);
    }
    for &c in &n.children {
        visit(tree, c, text, theme, dy, out);
    }
    if matches!(n.kind, NKind::Widget(Kind::Canvas)) {
        out.push(ScenePrimitive::PopClip);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    tree: &Tree,
    id: usize,
    n: &NNode,
    kind: Kind,
    text: &mut TextSystem,
    theme: &Theme,
    dy: f32,
    out: &mut Vec<ScenePrimitive>,
) {
    let s = &n.style;
    let rect = shift(n.rect, dy);
    let radius = px(s.radius);
    let hovered = tree.hover == Some(id);
    let pressed = tree.pressed == Some(id) && hovered;
    let disabled = n.flag(Attr::Disabled);
    let mut bg = s.background;
    if kind == Kind::Button && !disabled {
        if pressed {
            bg = bg.mix(Color::BLACK, 0.18);
        } else if hovered {
            bg = bg.mix(Color::BLACK, 0.08);
        }
    }
    if disabled {
        bg = bg.with_alpha(f32::from(bg.a) / 255.0 * 0.5);
    }
    if bg.a > 0 && kind != Kind::Slider {
        out.push(ScenePrimitive::Quad {
            rect,
            color: bg,
            radius,
        });
    }
    let focused = tree.focus == Some(id);
    let (bw, bc) = if focused && n.focusable() {
        (2.0, theme.primary)
    } else {
        (px(s.border_width), s.border_color)
    };
    if bw > 0.0 && bc.a > 0 {
        out.push(ScenePrimitive::Border {
            rect,
            color: bc,
            width: bw,
            radius,
        });
    }
    let c = shift(content(n), dy);
    let size = s.font_px();
    let weight = s.font_weight;
    match kind {
        Kind::View | Kind::Stack | Kind::Card => {}
        Kind::Text | Kind::Link => {
            let l = text.layout_ref(n.text(), size, weight, Some(c.width.max(1.0)));
            out.extend(l.primitives(Point::new(c.x, c.y), s.color));
            if kind == Kind::Link && hovered {
                out.push(ScenePrimitive::Quad {
                    rect: Rect::new(c.x, c.y + l.height - 2.0, l.width, 1.0),
                    color: s.color,
                    radius: 0.0,
                });
            }
        }
        Kind::Button => {
            let l = text.layout_ref(n.text(), size, weight, Some(c.width.max(1.0)));
            let origin = Point::new(
                c.x + (c.width - l.width) / 2.0,
                c.y + (c.height - l.height) / 2.0,
            );
            let color = if disabled {
                s.color.with_alpha(0.6)
            } else {
                s.color
            };
            out.extend(l.primitives(origin, color));
        }
        Kind::Input => {
            let (t, placeholder) = label(n);
            let l = text.layout_ref(t, size, weight, None);
            let y = c.y + (c.height - l.height) / 2.0;
            out.push(ScenePrimitive::PushClip(c));
            out.extend(l.primitives(
                Point::new(c.x, y),
                if placeholder { theme.muted } else { s.color },
            ));
            if focused {
                let x = if placeholder {
                    c.x
                } else {
                    c.x + l.width + 1.0
                };
                out.push(ScenePrimitive::Quad {
                    rect: Rect::new(x, y + 2.0, 1.5, (l.height - 4.0).max(8.0)),
                    color: s.color,
                    radius: 0.0,
                });
            }
            out.push(ScenePrimitive::PopClip);
        }
        Kind::Checkbox => {
            let checked = n.flag(Attr::Checked);
            let b = Rect::new(c.x, c.y + (c.height - CHECK) / 2.0, CHECK, CHECK);
            if checked {
                out.push(ScenePrimitive::Quad {
                    rect: b,
                    color: theme.primary,
                    radius: 4.0,
                });
                out.push(ScenePrimitive::Line {
                    points: vec![
                        Point::new(b.x + 4.0, b.y + 9.5),
                        Point::new(b.x + 7.5, b.y + 13.0),
                        Point::new(b.x + 14.0, b.y + 5.5),
                    ],
                    color: theme.on_primary,
                    width: 2.0,
                });
            } else {
                out.push(ScenePrimitive::Quad {
                    rect: b,
                    color: theme.surface,
                    radius: 4.0,
                });
                out.push(ScenePrimitive::Border {
                    rect: b,
                    color: if hovered { theme.primary } else { theme.border },
                    width: 1.5,
                    radius: 4.0,
                });
            }
            let x = c.x + CHECK + px(s.gap);
            let l = text.layout_ref(n.text(), size, weight, Some((c.right() - x).max(1.0)));
            out.extend(l.primitives(Point::new(x, c.y + (c.height - l.height) / 2.0), s.color));
        }
        Kind::Slider => {
            let (lo, hi) = (n.num(Attr::Min, 0.0), n.num(Attr::Max, 1.0));
            let v = n.num(Attr::Value, lo);
            let t = if hi > lo {
                ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let track = slider_track(n, dy);
            let cy = track.y + track.height / 2.0;
            out.push(ScenePrimitive::Quad {
                rect: track,
                color: theme.border,
                radius: 2.0,
            });
            let fx = track.x + t * track.width;
            out.push(ScenePrimitive::Quad {
                rect: Rect::new(track.x, track.y, fx - track.x, track.height),
                color: theme.primary,
                radius: 2.0,
            });
            let thumb = Rect::new(fx - THUMB, cy - THUMB, THUMB * 2.0, THUMB * 2.0);
            out.push(ScenePrimitive::Quad {
                rect: thumb,
                color: theme.surface,
                radius: THUMB,
            });
            out.push(ScenePrimitive::Border {
                rect: thumb,
                color: theme.primary,
                width: if hovered || tree.drag == Some(id) {
                    3.0
                } else {
                    2.0
                },
                radius: THUMB,
            });
        }
        Kind::ProgressBar => {
            let v = n.num(Attr::Value, 0.0).clamp(0.0, 1.0);
            out.push(ScenePrimitive::Quad {
                rect: Rect::new(rect.x, rect.y, rect.width * v, rect.height),
                color: theme.primary,
                radius,
            });
        }
        Kind::Canvas => {
            out.push(ScenePrimitive::PushClip(rect));
            if let Some(slot) = &n.viewport {
                let size = serge_core::viewport::Extent::pixels(rect, tree.scale);
                out.push(ScenePrimitive::ExternalTexture {
                    key: slot.frame(size),
                    rect,
                });
            }
            if let Some(cv) = &n.canvas {
                for cmd in &cv.commands {
                    paint_cmd(cmd, rect.x, rect.y, text, out);
                }
            }
            // PopClip after the children (see `visit`).
        }
    }
}

/// The slider's track in window coordinates.
pub(crate) fn slider_track(n: &NNode, dy: f32) -> Rect {
    let c = content(n);
    let cy = c.y + c.height / 2.0 + dy;
    Rect::new(c.x + THUMB, cy - 2.0, (c.width - 2.0 * THUMB).max(1.0), 4.0)
}

fn paint_cmd(
    cmd: &PaintCmd,
    ox: f32,
    oy: f32,
    text: &mut TextSystem,
    out: &mut Vec<ScenePrimitive>,
) {
    let p = |q: &Point| Point::new(q.x + ox, q.y + oy);
    match cmd {
        PaintCmd::FillRect {
            rect,
            color,
            radius,
        } => out.push(ScenePrimitive::Quad {
            rect: rect.offset(ox, oy),
            color: *color,
            radius: *radius,
        }),
        PaintCmd::StrokeRect { rect, color, width } => out.push(ScenePrimitive::Border {
            rect: rect.offset(ox, oy),
            color: *color,
            width: *width,
            radius: 0.0,
        }),
        PaintCmd::Polyline {
            points,
            color,
            width,
        } => out.push(ScenePrimitive::Line {
            points: points.iter().map(p).collect(),
            color: *color,
            width: *width,
        }),
        PaintCmd::FillPolygon { points, color } => out.push(ScenePrimitive::Polygon {
            points: points.iter().map(p).collect(),
            color: *color,
        }),
        PaintCmd::Text {
            origin,
            text: t,
            size,
            color,
        } => {
            let l = text.layout_ref(t, *size, 400, None);
            out.extend(l.primitives(p(origin), *color));
        }
    }
}

/// The interactive widget under `(x, y)` (window coordinates), innermost
/// first.
pub(crate) fn hit(tree: &Tree, x: f32, y: f32) -> Option<usize> {
    let p = Point::new(x, y + tree.scroll_y);
    let mut found = None;
    tree.walk(tree.root, &mut |id, n| {
        if n.focusable() && n.rect.contains(p) {
            found = Some(id);
        }
    });
    found
}
