mod date_time;
mod links;
pub use links::IosDeepLinks;
mod documents;
pub use date_time::{IosDateTimeRequest, IosDateTimeSelection};
mod stylus;
use block2::RcBlock;
pub use documents::{IosDocumentRequest, IosDocumentResult};
use objc2::{MainThreadMarker, MainThreadOnly, Message, rc::Retained, runtime::AnyObject};
use objc2_foundation::{
    NSArray, NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask, NSString,
};
use objc2_ui_kit::{
    UIActivityViewController, UIAlertAction, UIAlertActionStyle, UIAlertController,
    UIAlertControllerStyle, UIApplication, UIBackgroundTaskInvalid, UIImpactFeedbackGenerator,
    UIImpactFeedbackStyle, UIPasteboard, UISceneActivationState, UIViewController, UIWindow,
    UIWindowScene,
};
use std::{cell::Cell, io, path::PathBuf, ptr::NonNull, rc::Rc};
pub use stylus::StylusPad;

/// UIKit services associated with one application's window. Use on the UI thread.
pub struct IosSystem {
    window: Retained<UIWindow>,
    main: MainThreadMarker,
}

impl IosSystem {
    /// Binds a specific scene's window, including in a multi-window app.
    pub fn for_window(window: &UIWindow) -> Self {
        Self {
            window: window.retain(),
            main: window.mtm(),
        }
    }

    /// Binds the key window when exactly one scene is foreground-active.
    pub fn foreground() -> io::Result<Self> {
        let main = MainThreadMarker::new()
            .ok_or_else(|| io::Error::other("UIKit requires the UI thread"))?;
        let scenes = UIApplication::sharedApplication(main).connectedScenes();
        let mut windows = scenes
            .iter()
            .filter(|scene| scene.activationState() == UISceneActivationState::ForegroundActive)
            .filter_map(|scene| scene.downcast::<UIWindowScene>().ok()?.keyWindow());
        let window = windows
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no active key window"))?;
        if windows.next().is_some() {
            return Err(io::Error::other(
                "select the window using IosSystem::for_window",
            ));
        }
        Ok(Self { window, main })
    }

    fn present(&self, controller: &UIViewController) -> io::Result<()> {
        let mut presenter = self
            .window
            .rootViewController()
            .ok_or_else(|| io::Error::other("window has no root controller"))?;
        while let Some(next) = presenter.presentedViewController() {
            presenter = next;
        }
        if !self.window.isKeyWindow()
            || presenter.isBeingDismissed()
            || presenter.isBeingPresented()
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "window is not ready for a presentation",
            ));
        }
        if let Some(popover) = controller.popoverPresentationController() {
            let view = presenter
                .view()
                .ok_or_else(|| io::Error::other("presenter has no view"))?;
            popover.setSourceView(Some(&view));
            popover.setSourceRect(view.bounds());
        }
        presenter.presentViewController_animated_completion(controller, true, None);
        Ok(())
    }

    /// Presents a native alert with one localized confirmation button.
    pub fn alert(&self, title: &str, message: &str, ok: &str) -> io::Result<IosPrompt> {
        self.prompt(UIAlertControllerStyle::Alert, title, message, &[ok], None)
    }
    /// Presents a native action sheet. The result is the original zero-based action index.
    pub fn choose(&self, title: &str, actions: &[&str], cancel: &str) -> io::Result<IosPrompt> {
        self.prompt(
            UIAlertControllerStyle::ActionSheet,
            title,
            "",
            actions,
            Some(cancel),
        )
    }
    fn prompt(
        &self,
        style: UIAlertControllerStyle,
        title: &str,
        message: &str,
        actions: &[&str],
        cancel: Option<&str>,
    ) -> io::Result<IosPrompt> {
        if actions.is_empty() || actions.len() > 256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected 1 to 256 actions",
            ));
        }
        let controller = UIAlertController::alertControllerWithTitle_message_preferredStyle(
            Some(&NSString::from_str(title)),
            Some(&NSString::from_str(message)),
            style,
            self.main,
        );
        let result = Rc::new(Cell::new(None));
        let add = |label: &str, style, selection| {
            let state = result.clone();
            let callback =
                RcBlock::new(move |_: NonNull<UIAlertAction>| state.set(Some(selection)));
            let action = UIAlertAction::actionWithTitle_style_handler(
                Some(&NSString::from_str(label)),
                style,
                Some(&callback),
                self.main,
            );
            controller.addAction(&action);
        };
        for (index, label) in actions.iter().enumerate() {
            add(
                label,
                UIAlertActionStyle::Default,
                SelectedAction::Index(index),
            );
        }
        if let Some(cancel) = cancel {
            add(
                cancel,
                UIAlertActionStyle::Cancel,
                SelectedAction::Cancelled,
            );
        }
        self.present(&controller)?;
        Ok(IosPrompt { controller, result })
    }

    /// Presents the OS share sheet for text in the bound window.
    pub fn share_text(&self, text: &str) -> io::Result<()> {
        let text = NSString::from_str(text);
        let items: Retained<NSArray<AnyObject>> = NSArray::from_slice(&[&text]);
        // SAFETY: NSString conforms to the supported activity item types.
        #[allow(unsafe_code)]
        let sheet = unsafe {
            UIActivityViewController::initWithActivityItems_applicationActivities(
                UIActivityViewController::alloc(self.main),
                &items,
                None,
            )
        };
        self.present(&sheet)
    }
    /// Reads text from the general pasteboard subject to the OS paste permission.
    pub fn clipboard_text(&self) -> Option<String> {
        // SAFETY: the window-scoped handle guarantees UI-thread access to the pasteboard.
        #[allow(unsafe_code)]
        unsafe {
            UIPasteboard::generalPasteboard()
                .string()
                .map(|v| v.to_string())
        }
    }
    /// Replaces the system text clip.
    pub fn set_clipboard_text(&self, text: &str) {
        #[allow(unsafe_code)]
        unsafe {
            UIPasteboard::generalPasteboard().setString(Some(&NSString::from_str(text)));
        }
    }
    /// Creates the application's private Application Support directory and returns its actual sandbox path.
    pub fn data_directory(&self) -> io::Result<PathBuf> {
        NSFileManager::defaultManager()
            .URLForDirectory_inDomain_appropriateForURL_create_error(
                NSSearchPathDirectory::ApplicationSupportDirectory,
                NSSearchPathDomainMask::UserDomainMask,
                None,
                true,
            )
            .map_err(|e| io::Error::other(e.to_string()))?
            .to_file_path()
            .ok_or_else(|| io::Error::other("application support is not a file URL"))
    }
    /// Plays an impact with intensity in 0..=1 on supported devices.
    pub fn impact(&self, intensity: f64) -> io::Result<()> {
        if !(0.0..=1.0).contains(&intensity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "intensity must be in 0..=1",
            ));
        }
        // This initializer also supports the minimum iOS 16 deployment target.
        #[allow(deprecated)]
        let generator = UIImpactFeedbackGenerator::initWithStyle(
            UIImpactFeedbackGenerator::alloc(self.main),
            UIImpactFeedbackStyle::Medium,
        );
        generator.prepare();
        generator.impactOccurredWithIntensity(intensity);
        Ok(())
    }
    /// Requests a limited period to finish foreground work after the app enters the background.
    pub fn background_task(&self, name: &str) -> io::Result<BackgroundTask> {
        let app = UIApplication::sharedApplication(self.main);
        // SAFETY: UIKit exports the invalid task identifier on all supported iOS versions.
        #[allow(unsafe_code)]
        let invalid = unsafe { UIBackgroundTaskInvalid };
        let state = Rc::new(Cell::new(invalid));
        let expired = Rc::new(Cell::new(false));
        let callback = {
            let (state, expired, app) = (state.clone(), expired.clone(), app.clone());
            // UIKit dispatches expiration on the main thread.
            RcBlock::new(move || {
                expired.set(true);
                let id = state.replace(invalid);
                if id != invalid {
                    app.endBackgroundTask(id);
                }
            })
        };
        let id = app.beginBackgroundTaskWithName_expirationHandler(
            Some(&NSString::from_str(name)),
            Some(&callback),
        );
        if id == invalid {
            return Err(io::Error::other("iOS declined background execution"));
        }
        if expired.get() {
            app.endBackgroundTask(id);
        } else {
            state.set(id);
        }
        Ok(BackgroundTask {
            app,
            state,
            expired,
            invalid,
        })
    }
}

/// Result of an alert or an action sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectedAction {
    /// Zero-based action index.
    Index(usize),
    /// The sheet was dismissed.
    Cancelled,
}
/// Owns one native alert or action sheet. Dropping it dismisses any pending presentation.
pub struct IosPrompt {
    controller: Retained<UIAlertController>,
    result: Rc<Cell<Option<SelectedAction>>>,
}
impl IosPrompt {
    /// Polls without blocking the event loop, including interactive dismissal.
    pub fn try_result(&self) -> Option<SelectedAction> {
        self.result.get().or_else(|| {
            self.controller
                .presentingViewController()
                .is_none()
                .then_some(SelectedAction::Cancelled)
        })
    }
}
impl Drop for IosPrompt {
    fn drop(&mut self) {
        if self.result.get().is_none()
            && !self.controller.isBeingDismissed()
            && self.controller.presentingViewController().is_some()
        {
            self.controller
                .dismissViewControllerAnimated_completion(true, None);
        }
    }
}

/// UI-thread owner of an iOS background assertion. Dropping it finishes the task.
pub struct BackgroundTask {
    app: Retained<UIApplication>,
    state: Rc<Cell<usize>>,
    expired: Rc<Cell<bool>>,
    invalid: usize,
}
impl BackgroundTask {
    /// Reports whether the OS asked the application to finish before the guard was dropped.
    pub fn expired(&self) -> bool {
        self.expired.get()
    }
}
impl Drop for BackgroundTask {
    fn drop(&mut self) {
        let id = self.state.replace(self.invalid);
        if id != self.invalid {
            self.app.endBackgroundTask(id);
        }
    }
}
