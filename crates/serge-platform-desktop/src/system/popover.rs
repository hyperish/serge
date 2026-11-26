//! A menu-bar extra whose content belongs to the application.
#![allow(unsafe_code)]
use super::DesktopError;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_app_kit::{
    NSButton, NSPopover, NSPopoverBehavior, NSStatusBar, NSStatusBarButton, NSStatusItem,
    NSVariableStatusItemLength, NSViewController,
};
use objc2_foundation::{NSObject, NSRectEdge, NSSize, NSString};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SergeMenuBarPopoverTarget"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Retained<NSPopover>]
    struct Target;
    impl Target {
        #[unsafe(method(togglePopover:))]
        fn toggle(&self, button: &NSButton) {
            let popover = self.ivars();
            if popover.isShown() {
                popover.close();
            } else if button.window().is_some() {
                popover.showRelativeToRect_ofView_preferredEdge(button.bounds(), button, NSRectEdge::MinY);
            }
        }
    }
);

/// An AppKit status item and the popover anchored to its button.
pub struct MenuBarPopover {
    item: Retained<NSStatusItem>,
    button: Retained<NSStatusBarButton>,
    target: Retained<Target>,
}
impl MenuBarPopover {
    /// Installs a text status item. The content size is in logical points.
    pub fn new(
        title: &str,
        content: &NSViewController,
        width: f64,
        height: f64,
    ) -> Result<Self, DesktopError> {
        if title.trim().is_empty() || ![width, height].iter().all(|v| v.is_finite() && *v > 0.) {
            return Err(DesktopError::Platform(
                "a popover needs a title and positive finite dimensions".into(),
            ));
        }
        let mtm = content.mtm();
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        let size = NSSize::new(width, height);
        content.view().setFrameSize(size);
        popover.setContentViewController(Some(content));
        popover.setContentSize(size);
        let bar = NSStatusBar::systemStatusBar();
        let item = bar.statusItemWithLength(NSVariableStatusItemLength);
        let Some(button) = item.button(mtm) else {
            bar.removeStatusItem(&item);
            return Err(DesktopError::Platform(
                "AppKit did not provide a status button".into(),
            ));
        };
        button.setTitle(&NSString::from_str(title));
        let allocated = Target::alloc(mtm).set_ivars(popover);
        // SAFETY: the zero-argument NSObject initializer initializes this subclass.
        let target: Retained<Target> = unsafe { msg_send![super(allocated), init] };
        // SAFETY: the target implements this selector and remains alive until Drop clears both.
        unsafe {
            button.setTarget(Some(&target));
            button.setAction(Some(sel!(togglePopover:)));
        }
        Ok(Self {
            item,
            button,
            target,
        })
    }
    /// Access the native button, for example to set a template image or tooltip.
    pub fn button(&self) -> &NSStatusBarButton {
        &self.button
    }
    /// Access the native popover to configure it or dismiss it programmatically.
    pub fn popover(&self) -> &NSPopover {
        self.target.ivars()
    }
}
impl Drop for MenuBarPopover {
    fn drop(&mut self) {
        // SAFETY: the action and its weak target are disconnected on their owning main thread.
        unsafe {
            self.button.setAction(None);
            self.button.setTarget(None);
        }
        self.popover().close();
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}
