//! Window-specific native date and time sheets with localized system buttons.
#![allow(unsafe_code)]
use super::IosSystem;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_foundation::{NSDateFormatter, NSLocale, NSString};
use objc2_ui_kit::{
    UIBarButtonItem, UIBarButtonSystemItem, UIDatePicker, UIDatePickerMode, UIDatePickerStyle,
    UINavigationController, UIViewController,
};
use std::{cell::RefCell, io};

/// Date or time selection from UIKit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IosDateTimeSelection {
    /// Local Gregorian date (YYYY-MM-DD) or local 24-hour time (HH:MM).
    Selected(String),
    /// Cancel button, interactive dismissal, or owner cancellation.
    Cancelled,
}
struct Selection {
    picker: Retained<UIDatePicker>,
    result: RefCell<Option<IosDateTimeSelection>>,
}
define_class!(
    // SAFETY: the controller and its UIKit picker are restricted to the main thread.
    #[unsafe(super = UIViewController)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Selection]
    #[name = "SergeDateTimeController"]
    struct Panel;
    impl Panel {
        #[unsafe(method(accept:))]
        fn accept(&self, _: &UIBarButtonItem) {
            let picker = &self.ivars().picker;
            let format = NSDateFormatter::new();
            let locale = NSLocale::localeWithLocaleIdentifier(&NSString::from_str("en_US_POSIX"));
            format.setLocale(Some(&locale));
            format.setTimeZone(picker.timeZone().as_deref());
            format.setDateFormat(Some(&NSString::from_str(if picker.datePickerMode() == UIDatePickerMode::Date {
                "yyyy-MM-dd"
            } else { "HH:mm" })));
            self.finish(IosDateTimeSelection::Selected(format.stringFromDate(&picker.date()).to_string()));
        }
        #[unsafe(method(cancel:))]
        fn cancel(&self, _: &UIBarButtonItem) { self.finish(IosDateTimeSelection::Cancelled); }
    }
);
impl Panel {
    fn finish(&self, value: IosDateTimeSelection) {
        if self.ivars().result.borrow().is_some() {
            return;
        }
        self.ivars().result.replace(Some(value));
        if let Some(nav) = self.navigationController() {
            nav.dismissViewControllerAnimated_completion(true, None);
        }
    }
}

/// Owns the presented UIKit navigation controller until the selection completes.
pub struct IosDateTimeRequest {
    panel: Retained<Panel>,
    navigation: Retained<UINavigationController>,
}
impl IosDateTimeRequest {
    /// Reads the result on the UI thread, or None while the picker is still open.
    pub fn try_result(&self) -> Option<IosDateTimeSelection> {
        self.panel.ivars().result.borrow().clone().or_else(|| {
            self.navigation
                .presentingViewController()
                .is_none()
                .then_some(IosDateTimeSelection::Cancelled)
        })
    }
}
impl Drop for IosDateTimeRequest {
    fn drop(&mut self) {
        if self.panel.ivars().result.borrow().is_none()
            && !self.navigation.isBeingDismissed()
            && self.navigation.presentingViewController().is_some()
        {
            self.navigation
                .dismissViewControllerAnimated_completion(true, None);
        }
    }
}
impl IosSystem {
    /// Presents a calendar date control, initialized to today in the user's zone.
    pub fn pick_date(&self) -> io::Result<IosDateTimeRequest> {
        self.date_time(UIDatePickerMode::Date)
    }
    /// Presents a local time control, initialized to the current time.
    pub fn pick_time(&self) -> io::Result<IosDateTimeRequest> {
        self.date_time(UIDatePickerMode::Time)
    }
    fn date_time(&self, mode: UIDatePickerMode) -> io::Result<IosDateTimeRequest> {
        let picker = UIDatePicker::new(self.main);
        picker.setDatePickerMode(mode);
        picker.setPreferredDatePickerStyle(UIDatePickerStyle::Wheels);
        let allocated = Panel::alloc(self.main).set_ivars(Selection {
            picker: picker.clone(),
            result: RefCell::default(),
        });
        // SAFETY: initialize the allocated subclass once. Both target-action selectors are
        // implemented above and stay alive for the entire presentation.
        let panel: Retained<Panel> = unsafe {
            msg_send![super(allocated), initWithNibName: Option::<&NSString>::None, bundle: Option::<&objc2_foundation::NSBundle>::None]
        };
        panel.setView(Some(&picker));
        let item = panel.navigationItem();
        for (style, selector) in [
            (UIBarButtonSystemItem::Done, sel!(accept:)),
            (UIBarButtonSystemItem::Cancel, sel!(cancel:)),
        ] {
            let button = unsafe {
                UIBarButtonItem::initWithBarButtonSystemItem_target_action(
                    UIBarButtonItem::alloc(self.main),
                    style,
                    Some(&panel),
                    Some(selector),
                )
            };
            if style == UIBarButtonSystemItem::Done {
                item.setRightBarButtonItem(Some(&button));
            } else {
                item.setLeftBarButtonItem(Some(&button));
            }
        }
        let navigation = UINavigationController::initWithRootViewController(
            UINavigationController::alloc(self.main),
            &panel,
        );
        self.present(&navigation)?;
        Ok(IosDateTimeRequest { panel, navigation })
    }
}
