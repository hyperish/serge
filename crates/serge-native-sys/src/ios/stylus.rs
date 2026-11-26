//! Window-local Apple Pencil contact and coalesced motion delivery.
#![allow(unsafe_code)]
use super::IosSystem;
use crate::stylus::{StylusArea, StylusBatch, StylusPhase, StylusSample};
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_foundation::{NSPoint, NSRect, NSSet, NSSize};
use objc2_ui_kit::{UIEvent, UITouch, UITouchType, UIView};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    f64::consts::FRAC_PI_2,
    io,
};

#[derive(Default)]
struct ContactState {
    next_id: u64,
    contacts: HashMap<*const UITouch, u64>,
    samples: VecDeque<StylusSample>,
    dropped: u64,
}
define_class!(
    // SAFETY: UIView delivers the responder callbacks on the main thread. State and
    // retained views are confined to that thread.
    #[unsafe(super = UIView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = RefCell<ContactState>]
    #[name = "SergeStylusInputView"]
    struct PencilView;
    impl PencilView {
        #[unsafe(method(touchesBegan:withEvent:))]
        fn began(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.capture(touches, event, StylusPhase::Down);
        }
        #[unsafe(method(touchesMoved:withEvent:))]
        fn moved(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.capture(touches, event, StylusPhase::Move);
        }
        #[unsafe(method(touchesEnded:withEvent:))]
        fn ended(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.capture(touches, event, StylusPhase::Up);
        }
        #[unsafe(method(touchesCancelled:withEvent:))]
        fn cancelled(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.capture(touches, event, StylusPhase::Cancel);
        }
    }
);
impl PencilView {
    fn capture(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>, phase: StylusPhase) {
        let scale = self.contentScaleFactor();
        let mut state = self.ivars().borrow_mut();
        for touch in touches
            .iter()
            .filter(|touch| touch.r#type() == UITouchType::Pencil)
        {
            // UIKit supplies the original UITouch throughout the gesture. Coalesced
            // samples can be distinct objects, so they inherit the original's ID.
            let key = &*touch as *const UITouch;
            let id = if let Some(id) = state.contacts.get(&key) {
                *id
            } else {
                state.next_id = state.next_id.wrapping_add(1);
                let id = state.next_id;
                state.contacts.insert(key, id);
                id
            };
            let mut append = |point: &UITouch| {
                let pos = point.preciseLocationInView(Some(self));
                let max = point.maximumPossibleForce();
                let pressure = if max > 0. {
                    (point.force() / max).clamp(0., 1.)
                } else {
                    0.
                };
                if state.samples.len() == 512 {
                    state.samples.pop_front();
                    state.dropped += 1;
                }
                state.samples.push_back(StylusSample {
                    timestamp_ns: (point.timestamp() * 1e9) as u64,
                    pointer_id: id,
                    phase,
                    position: [(pos.x * scale) as f32, (pos.y * scale) as f32],
                    pressure: pressure as f32,
                    tilt: (FRAC_PI_2 - point.altitudeAngle()).clamp(0., FRAC_PI_2) as f32,
                    eraser: false,
                });
            };
            if phase == StylusPhase::Move
                && let Some(points) = event.and_then(|e| e.coalescedTouchesForTouch(&touch))
                && points.count() != 0
            {
                for point in points.iter() {
                    append(&point);
                }
            } else {
                append(&touch);
            }
            if matches!(phase, StylusPhase::Up | StylusPhase::Cancel) {
                state.contacts.remove(&key);
            }
        }
    }
}

/// App-owned transparent drawing surface. Dropping it restores the underlying view's input.
pub struct StylusPad {
    view: Retained<PencilView>,
}
impl IosSystem {
    /// Attaches a pen drawing region to this window's root view on its UI thread.
    pub fn stylus_pad(&self, area: StylusArea) -> io::Result<StylusPad> {
        area.validate()?;
        let root = self
            .window
            .rootViewController()
            .and_then(|vc| vc.view())
            .ok_or_else(|| io::Error::other("window has no root view"))?;
        let scale = root.contentScaleFactor();
        let bounds = root.bounds();
        let rect = NSRect::new(
            NSPoint::new(
                bounds.origin.x + f64::from(area.x) / scale,
                bounds.origin.y + f64::from(area.y) / scale,
            ),
            NSSize::new(
                f64::from(area.width) / scale,
                f64::from(area.height) / scale,
            ),
        );
        let allocated = PencilView::alloc(self.main).set_ivars(RefCell::default());
        // SAFETY: initialize our UIView subclass exactly once with a valid CGRect.
        let view: Retained<PencilView> =
            unsafe { msg_send![super(allocated), initWithFrame: rect] };
        view.setContentScaleFactor(scale);
        view.setOpaque(false);
        view.setMultipleTouchEnabled(true);
        root.addSubview(&view);
        Ok(StylusPad { view })
    }
}
impl StylusPad {
    /// Whether the drawing surface is attached to a UIKit window.
    pub fn is_ready(&self) -> io::Result<bool> {
        Ok(self.view.window().is_some())
    }
    /// Takes all measured samples, including UIKit-coalesced motion.
    pub fn drain(&self) -> io::Result<StylusBatch> {
        let mut state = self.view.ivars().borrow_mut();
        Ok(StylusBatch {
            dropped: std::mem::take(&mut state.dropped),
            samples: state.samples.drain(..).collect(),
        })
    }
}
impl Drop for StylusPad {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}
