//! `serge` — Unified cross-platform Web DOM and Native GPU/CPU UI framework.
//!
//! Built directly on `hyve`'s vertical core (`Signal`, `Ctx`, `SharedRingBuf`, `hyve-serde`,
//! `hyve-validate`) with a single QML/Slint + `rsx!`-style `ui!` syntax targeting:
//! - Web (Leptos-inspired static template extraction + fine-grained slot patching + SSR handoff)
//! - Native (layout solver + `ScenePrimitive` GPU batching + mandatory CPU software fallback)
//! - iOS Dynamic Island & Live Activity SwiftUI codegen (`#[ios::live_activity]`)

pub mod core;
pub mod ios;
pub mod native;
pub mod style;
pub mod web;

pub use hyve;
pub use serge_macros::{component, live_activity, ui};

/// Convenient glob re-exports (`use serge::prelude::*;`).
pub mod prelude {
    pub use crate::core::{
        Element, FrameMetrics, PaintCtx, PersistentSignal, PropValue, Rect, RequireAuth,
        RouteOutcome, ScenePrimitive, SparklineSample, UiFeature, feature,
    };
    pub use crate::ios::{self, LiveActivity};
    pub use crate::native::{
        CpuFallbackRenderer, CpuFrameBuffer, GpuBatchRenderer, GpuFrameStats, RenderBackendKind,
        SceneBatch, layout_and_paint, select_render_backend,
    };
    pub use crate::style::{
        Color, ColorTokens, LayoutDirection, Length, ThemeMode, h_stack, v_stack, z_stack,
    };
    pub use crate::web::{
        DomTemplate, FeatureSsrHandler, ManifestRoute, MountedDom, RouteRenderMode,
        SergeBundleManifest, SergeSsrHandler, SlotPatch, SsrRequest, SsrResponse,
    };
    pub use serge_macros::{component, live_activity, ui};
}
