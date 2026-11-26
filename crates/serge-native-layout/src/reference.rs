//! Reference adapter from the published alpha.1 release for differential tests.
use super::*;
use taffy::prelude::{AvailableSpace, NodeId, TaffyTree};

pub(super) fn compute(
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

fn scene(rows: usize) -> Vec<LayoutNode> {
    let root = LayoutNode {
        style: Style::default(),
        measured: false,
        children: Vec::new(),
    };
    // A detached node before the root exercises nonzero root indices and unused output boxes.
    let mut nodes = vec![root.clone(), root];
    for row in 0..rows {
        let i = nodes.len();
        nodes[1].children.push(i);
        nodes.push(LayoutNode {
            style: Style {
                layout: Layout::Row,
                gap: Length::Px(2.5),
                padding_x: Length::Px(3.0),
                ..Style::default()
            },
            children: vec![i + 1, i + 2, i + 3],
            measured: false,
        });
        for col in 0..3 {
            let width = if col == 0 {
                Length::Percent(27.5)
            } else {
                Length::Auto
            };
            nodes.push(LayoutNode {
                style: Style {
                    width,
                    grow: (col == 2) as u8 as f32,
                    ..Style::default()
                },
                measured: (row + col) % 3 != 2,
                children: Vec::new(),
            });
        }
    }
    nodes
}

fn measure(i: usize, hint: WidthHint) -> (f32, f32) {
    let ideal = 30.25 + (i % 25) as f32;
    let width = match hint {
        WidthHint::Min => 3.0,
        WidthHint::Max => ideal,
        WidthHint::AtMost(w) => ideal.min(w),
        WidthHint::Exact(w) => w,
    };
    (width, 11.5 * (ideal / width.max(1.0)).ceil())
}

#[test]
fn flat_scene_boxes_match_the_published_adapter() {
    for rows in [0, 1, 32] {
        let nodes = scene(rows);
        for width in [16.25, 200.0, 200.5, 1280.0] {
            for height in [None, Some(57.2), Some(600.0)] {
                assert_eq!(
                    super::compute(&nodes, 1, width, height, measure),
                    compute(&nodes, 1, width, height, measure)
                );
            }
        }
    }
}

#[test]
#[ignore = "timing comparison, run with --release --ignored --nocapture"]
fn benchmark_scene_layout() {
    use std::{hint::black_box, time::Instant};
    let nodes = scene(128);
    let mut results = [Vec::new(), Vec::new()];
    for sample in 0..21 {
        for index in [sample % 2, 1 - sample % 2] {
            let start = Instant::now();
            for _ in 0..12 {
                let boxes = if index == 0 {
                    super::compute(&nodes, 1, 704.5, Some(600.0), measure)
                } else {
                    compute(&nodes, 1, 704.5, Some(600.0), measure)
                };
                black_box(boxes);
            }
            results[index].push(start.elapsed().as_micros() / 12);
        }
    }
    for (label, mut times) in ["scene", "TaffyTree"].into_iter().zip(results) {
        times.sort_unstable();
        println!(
            "{label}: {} µs/layout ({}–{})",
            times[10], times[0], times[20]
        );
    }
}
