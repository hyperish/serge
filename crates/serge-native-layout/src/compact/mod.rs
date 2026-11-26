//! Flex-only specialization of Taffy 0.15.1's src/tree/taffy_tree.rs,
//! upstream 0064bd54cbe38bf1d8bcd6ec6ab0704d45fb048a, MIT (see LICENSE).
//! NodeId is already Serge's dense scene index. Children belong to the scene,
//! and rounding writes directly to the output boxes consumed by paint and hit testing.
use super::{LayoutBox, LayoutNode, WidthHint, to_taffy};
use serge_style::Length;
use taffy::{
    Cache, CacheTree, Layout, LayoutContainingBlock, LayoutFlexboxContainer, LayoutInput,
    LayoutOutput, LayoutPartialTree, RoundTree, RunMode, TraversePartialTree, TraverseTree,
    compute_cached_layout, compute_flexbox_layout, compute_hidden_layout, compute_leaf_layout,
    compute_root_layout,
    prelude::{AvailableSpace, Dimension, LengthPercentageAuto, NodeId, Size},
    round_layout,
};

type Measure<'a> = dyn FnMut(usize, WidthHint) -> (f32, f32) + 'a;

struct SceneTree<'a> {
    nodes: &'a [LayoutNode],
    styles: Vec<taffy::Style>,
    caches: Vec<Cache>,
    unrounded: Vec<Layout>,
    boxes: Vec<LayoutBox>,
    measure: &'a mut Measure<'a>,
}

pub(super) fn compute(
    nodes: &[LayoutNode],
    root: usize,
    width: f32,
    height: Option<f32>,
    measure: &mut Measure<'_>,
) -> Vec<LayoutBox> {
    let mut styles: Vec<_> = nodes.iter().map(|node| to_taffy(&node.style)).collect();
    if nodes[root].style.width == Length::Auto {
        styles[root].size.width = Dimension::length(width);
    }
    if let Some(h) = height {
        styles[root].min_size.height = LengthPercentageAuto::length(h);
    }
    let mut tree = SceneTree {
        nodes,
        styles,
        caches: vec![Cache::new(); nodes.len()],
        unrounded: vec![Layout::new(); nodes.len()],
        boxes: vec![LayoutBox::default(); nodes.len()],
        measure,
    };
    let available = Size {
        width: AvailableSpace::Definite(width),
        height: height.map_or(AvailableSpace::MaxContent, AvailableSpace::Definite),
    };
    compute_root_layout(&mut tree, root.into(), available);
    round_layout(&mut tree, root.into());
    tree.boxes
}

impl TraversePartialTree for SceneTree<'_> {
    type ChildIter<'a>
        = std::iter::Map<std::iter::Copied<std::slice::Iter<'a, usize>>, fn(usize) -> NodeId>
    where
        Self: 'a;
    fn child_ids(&self, id: NodeId) -> Self::ChildIter<'_> {
        self.nodes[usize::from(id)]
            .children
            .iter()
            .copied()
            .map(NodeId::from)
    }
    fn child_count(&self, id: NodeId) -> usize {
        self.nodes[usize::from(id)].children.len()
    }
    fn get_child_id(&self, id: NodeId, index: usize) -> NodeId {
        self.nodes[usize::from(id)].children[index].into()
    }
}
impl TraverseTree for SceneTree<'_> {}

impl LayoutPartialTree for SceneTree<'_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;
    type CustomIdent = String;

    fn get_core_container_style(&self, id: NodeId) -> &taffy::Style {
        &self.styles[usize::from(id)]
    }
    fn set_unrounded_layout(&mut self, id: NodeId, layout: &Layout) {
        self.unrounded[usize::from(id)] = *layout;
    }
    fn compute_child_layout(&mut self, id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, id);
        }
        compute_cached_layout(self, id, inputs, |tree, id, inputs| {
            let i = usize::from(id);
            if !tree.nodes[i].children.is_empty() {
                return compute_flexbox_layout(tree, id, inputs);
            }
            compute_leaf_layout(
                inputs,
                &tree.styles[i],
                |_, _| 0.0,
                |known, available| {
                    if !tree.nodes[i].measured {
                        return Size::ZERO;
                    }
                    let hint = match (known.width, available.width) {
                        (Some(w), _) => WidthHint::Exact(w),
                        (None, AvailableSpace::Definite(w)) => WidthHint::AtMost(w),
                        (None, AvailableSpace::MinContent) => WidthHint::Min,
                        (None, AvailableSpace::MaxContent) => WidthHint::Max,
                    };
                    let (w, h) = (tree.measure)(i, hint);
                    Size {
                        width: known.width.unwrap_or(w),
                        height: known.height.unwrap_or(h),
                    }
                },
            )
        })
    }
}

impl CacheTree for SceneTree<'_> {
    fn cache_get(&mut self, id: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.caches[usize::from(id)].get(input)
    }
    fn cache_store(&mut self, id: NodeId, input: &LayoutInput, result: LayoutOutput) {
        self.caches[usize::from(id)].store(input, result);
    }
    fn cache_clear(&mut self, id: NodeId) {
        self.caches[usize::from(id)].clear();
    }
}

impl LayoutFlexboxContainer for SceneTree<'_> {
    type FlexboxContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;
    fn get_flexbox_container_style(&self, id: NodeId) -> &taffy::Style {
        self.get_core_container_style(id)
    }
    fn get_flexbox_child_style(&self, id: NodeId) -> &taffy::Style {
        self.get_core_container_style(id)
    }
}

// Serge styles produce in-flow flex nodes. Viewport overlays are composed by the renderer.
impl LayoutContainingBlock for SceneTree<'_> {
    type OofItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;
    fn get_oof_item_style(&self, id: NodeId) -> &taffy::Style {
        self.get_core_container_style(id)
    }
    fn clear_hoisted_children(&mut self, _: NodeId) {}
    fn add_hoisted_children(&mut self, _: NodeId, hoisted: &[NodeId]) {
        assert!(hoisted.is_empty());
    }
}

impl RoundTree for SceneTree<'_> {
    fn get_unrounded_layout(&self, id: NodeId) -> Layout {
        self.unrounded[usize::from(id)]
    }
    fn set_final_layout(&mut self, id: NodeId, layout: &Layout) {
        self.boxes[usize::from(id)] = LayoutBox {
            x: layout.location.x,
            y: layout.location.y,
            width: layout.size.width,
            height: layout.size.height,
        };
    }
    fn is_out_of_flow(&self, _: NodeId) -> bool {
        false
    }
    fn hoisted_child_count(&self, _: NodeId) -> usize {
        0
    }
    fn get_hoisted_child_id(&self, _: NodeId, _: usize) -> NodeId {
        unreachable!("no out-of-flow boxes")
    }
}
