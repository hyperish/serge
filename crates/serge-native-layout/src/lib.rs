//! `serge-native-layout` — flexbox layout of serge's style IR on `taffy`
//! (pinned, `docs/02-architecture.md` §3). The tree is rebuilt per layout
//! pass from the caller's nodes; the compact SoA tables and dirty-subtree
//! reuse of the fork direction are later work.

use serge_style::{Align, Layout, Length, Style};
use taffy::prelude::{
    AvailableSpace, Dimension, FlexDirection, LengthPercentage, LengthPercentageAuto, NodeId, Size,
    TaffyTree,
};

/// A node to lay out: its style, whether its content is measured, and its
/// children (indices into the same slice).
#[derive(Debug, Clone)]
pub struct LayoutNode {
    /// The style.
    pub style: Style,
    /// Whether [`compute`] asks the measure function for its content size.
    pub measured: bool,
    /// Children in order.
    pub children: Vec<usize>,
}

/// A node's box relative to its parent's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutBox {
    /// Left.
    pub x: f32,
    /// Top.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

fn dim(l: Length) -> Dimension {
    match l {
        Length::Auto => Dimension::auto(),
        Length::Percent(p) => Dimension::percent(p / 100.0),
        other => Dimension::length(other.px().unwrap_or(0.0)),
    }
}

fn lpa(l: Length) -> LengthPercentageAuto {
    match l {
        Length::Auto => LengthPercentageAuto::auto(),
        Length::Percent(p) => LengthPercentageAuto::percent(p / 100.0),
        other => LengthPercentageAuto::length(other.px().unwrap_or(0.0)),
    }
}

fn lp(l: Length) -> LengthPercentage {
    match l {
        Length::Percent(p) => LengthPercentage::percent(p / 100.0),
        other => LengthPercentage::length(other.px().unwrap_or(0.0)),
    }
}

fn to_taffy(s: &Style) -> taffy::Style {
    let pad_x = lp(s.padding_x);
    let pad_y = lp(s.padding_y);
    let border = lp(s.border_width);
    taffy::Style {
        display: taffy::Display::Flex,
        flex_direction: match s.layout {
            Layout::Column => FlexDirection::Column,
            Layout::Row => FlexDirection::Row,
        },
        size: Size {
            width: dim(s.width),
            height: dim(s.height),
        },
        min_size: Size {
            width: lpa(s.min_width),
            height: LengthPercentageAuto::auto(),
        },
        max_size: Size {
            width: lpa(s.max_width),
            height: LengthPercentageAuto::auto(),
        },
        padding: taffy::Rect {
            left: pad_x,
            right: pad_x,
            top: pad_y,
            bottom: pad_y,
        },
        border: taffy::Rect {
            left: border,
            right: border,
            top: border,
            bottom: border,
        },
        gap: Size {
            width: lp(s.gap),
            height: lp(s.gap),
        },
        align_items: (match s.align {
            Align::Stretch | Align::SpaceBetween => taffy::AlignItems::STRETCH,
            Align::Start => taffy::AlignItems::FLEX_START,
            Align::Center => taffy::AlignItems::CENTER,
            Align::End => taffy::AlignItems::FLEX_END,
        }),
        justify_content: (match s.justify {
            Align::Stretch | Align::Start => taffy::JustifyContent::FLEX_START,
            Align::Center => taffy::JustifyContent::CENTER,
            Align::End => taffy::JustifyContent::FLEX_END,
            Align::SpaceBetween => taffy::JustifyContent::SPACE_BETWEEN,
        }),
        flex_grow: s.grow,
        flex_shrink: if s.layout == Layout::Row { 1.0 } else { 0.0 },
        margin: taffy::Rect {
            left: LengthPercentageAuto::length(0.0),
            right: LengthPercentageAuto::length(0.0),
            top: LengthPercentageAuto::length(0.0),
            bottom: LengthPercentageAuto::length(0.0),
        },
        ..Default::default()
    }
}

/// What a measure function is given: a width the content must fit (when
/// known), for wrapping text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WidthHint {
    /// The content must be exactly this wide.
    Exact(f32),
    /// The content may wrap at this width.
    AtMost(f32),
    /// As narrow as it can get.
    Min,
    /// No wrapping.
    Max,
}

/// Lays out `nodes` from `root` in `width` × `height` (height `None`: as
/// tall as the content) and returns each node's box relative to its parent.
/// `measure(index, hint)` gives the content size of measured nodes.
pub fn compute(
    nodes: &[LayoutNode],
    root: usize,
    width: f32,
    height: Option<f32>,
    mut measure: impl FnMut(usize, WidthHint) -> (f32, f32),
) -> Vec<LayoutBox> {
    let mut tree: TaffyTree<usize> = TaffyTree::new();
    let mut ids: Vec<Option<NodeId>> = vec![None; nodes.len()];
    fn build(
        tree: &mut TaffyTree<usize>,
        nodes: &[LayoutNode],
        ids: &mut [Option<NodeId>],
        i: usize,
    ) -> NodeId {
        let children: Vec<NodeId> = nodes[i]
            .children
            .iter()
            .map(|&c| build(tree, nodes, ids, c))
            .collect();
        let style = to_taffy(&nodes[i].style);
        let id = if children.is_empty() {
            tree.new_leaf_with_context(style, i)
        } else {
            tree.new_with_children(style, &children)
        }
        .expect("taffy node");
        ids[i] = Some(id);
        id
    }
    let root_id = build(&mut tree, nodes, &mut ids, root);
    // The root fills the available width (and at least the height).
    let mut root_style = tree.style(root_id).expect("root").clone();
    if nodes[root].style.width == Length::Auto {
        root_style.size.width = Dimension::length(width);
    }
    if let Some(h) = height {
        root_style.min_size.height = LengthPercentageAuto::length(h);
    }
    tree.set_style(root_id, root_style).expect("root style");
    let available = Size {
        width: AvailableSpace::Definite(width),
        height: height.map_or(AvailableSpace::MaxContent, AvailableSpace::Definite),
    };
    tree.compute_layout_with_measure(root_id, available, |inputs, _id, ctx, style| {
        let i = ctx.map(|c| *c);
        taffy::compute_leaf_layout(
            inputs,
            style,
            |_, _| 0.0,
            |known, avail| {
                let Some(i) = i.filter(|&i| nodes[i].measured) else {
                    return Size::ZERO;
                };
                let hint = match (known.width, avail.width) {
                    (Some(w), _) => WidthHint::Exact(w),
                    (None, AvailableSpace::Definite(w)) => WidthHint::AtMost(w),
                    (None, AvailableSpace::MinContent) => WidthHint::Min,
                    (None, AvailableSpace::MaxContent) => WidthHint::Max,
                };
                let (w, h) = measure(i, hint);
                Size {
                    width: known.width.unwrap_or(w),
                    height: known.height.unwrap_or(h),
                }
            },
        )
    })
    .expect("taffy layout");
    ids.iter()
        .map(|id| {
            id.and_then(|id| tree.layout(id).ok())
                .map(|l| LayoutBox {
                    x: l.location.x,
                    y: l.location.y,
                    width: l.size.width,
                    height: l.size.height,
                })
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serge_style::{StyleProp, px};

    #[test]
    fn columns_with_padding_and_gap() {
        let mut card = Style::default();
        card.apply(&StyleProp::Padding(px(12.0)));
        card.apply(&StyleProp::Gap(px(8.0)));
        let leaf = Style::default();
        let nodes = vec![
            LayoutNode {
                style: card,
                measured: false,
                children: vec![1, 2],
            },
            LayoutNode {
                style: leaf.clone(),
                measured: true,
                children: vec![],
            },
            LayoutNode {
                style: leaf,
                measured: true,
                children: vec![],
            },
        ];
        let boxes = compute(&nodes, 0, 200.0, None, |_, _| (50.0, 20.0));
        assert_eq!(
            boxes[1],
            LayoutBox {
                x: 12.0,
                y: 12.0,
                width: 176.0,
                height: 20.0
            }
        );
        assert_eq!(boxes[2].y, 40.0);
        assert_eq!(boxes[0].height, 72.0);
    }
}
