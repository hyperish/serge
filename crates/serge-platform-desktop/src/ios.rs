//! UIKit reports the drawable bounds, safe insets and keyboard in view-local points.
use objc2::{runtime::NSObjectProtocol, sel};
use objc2_ui_kit::UIView;
use serge_core::Rect;
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

pub(super) fn content(window: &Window) -> Option<Rect> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: winit owns this UIView. This function is only called by the
    // UIKit main-thread event loop while its window and handle are borrowed.
    #[allow(unsafe_code)]
    let view = unsafe { handle.ui_view.cast::<UIView>().as_ref() };
    let bounds = view.bounds();
    let insets = view.safeAreaInsets();
    let width = (bounds.size.width - insets.left - insets.right).max(0.);
    let mut bottom = bounds.size.height - insets.bottom;
    if view.respondsToSelector(sel!(keyboardLayoutGuide)) {
        // SAFETY: availability of the iOS 15 selector was checked on this view.
        #[allow(unsafe_code)]
        let kb = unsafe { view.keyboardLayoutGuide().layoutFrame() };
        let top = kb.origin.y - bounds.origin.y;
        // A floating keyboard does not cover the entire content width.
        if kb.size.height > 0. && kb.size.width >= width && top >= 0. {
            bottom = bottom.min(top);
        }
    }
    Some(Rect::new(
        insets.left as f32,
        insets.top as f32,
        width as f32,
        (bottom - insets.top).max(0.) as f32,
    ))
}
