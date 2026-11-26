//! The app's UserNotifications categories, local requests and actionable APNs responses.
#![allow(unsafe_code)]
use crate::notification::{NotificationCategory, NotificationResponse, valid_id, validate_message};
use block2::{DynBlock, RcBlock};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send,
    rc::Retained,
    runtime::{Bool, ProtocolObject},
};
use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::{
    collections::{HashMap, HashSet},
    io,
    sync::mpsc::{self, Receiver},
};

struct Commands {
    categories: HashMap<String, HashSet<String>>,
    invoke: Box<dyn Fn(NotificationResponse) + Send + Sync>,
}
define_class!(
    // SAFETY: the delegate only reads immutable state; its app callback is Send + Sync.
    #[unsafe(super = NSObject)]
    #[ivars = Commands]
    #[name = "SergeUserNotificationDelegate"]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn present(
            &self,
            _: &UNUserNotificationCenter,
            _: &UNNotification,
            done: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            done.call((
                UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,
            ));
        }
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn receive(
            &self,
            _: &UNUserNotificationCenter,
            reply: &UNNotificationResponse,
            done: &DynBlock<dyn Fn()>,
        ) {
            let request = reply.notification().request();
            let action = reply.actionIdentifier().to_string();
            let category = request.content().categoryIdentifier().to_string();
            if self
                .ivars()
                .categories
                .get(&category)
                .is_some_and(|actions| actions.contains(&action))
            {
                let response = NotificationResponse {
                    notification: request.identifier().to_string(),
                    action,
                };
                // No panic may escape into the OS. Always return the completion callback.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (self.ivars().invoke)(response)
                }));
            }
            done.call(());
        }
    }
);

fn native_category(
    category: NotificationCategory<'_>,
) -> io::Result<Retained<UNNotificationCategory>> {
    category.validate()?;
    let actions: Vec<_> = category
        .actions
        .iter()
        .map(|a| {
            UNNotificationAction::actionWithIdentifier_title_options(
                &NSString::from_str(a.id),
                &NSString::from_str(a.title),
                UNNotificationActionOptions::empty(),
            )
        })
        .collect();
    Ok(
        UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
            &NSString::from_str(category.id),
            &NSArray::from_retained_slice(&actions),
            &NSArray::new(),
            UNNotificationCategoryOptions::empty(),
        ),
    )
}

fn request(
    id: &str,
    title: &str,
    body: &str,
    category: &str,
) -> io::Result<Retained<UNNotificationRequest>> {
    validate_message(id, title, body)?;
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setCategoryIdentifier(&NSString::from_str(category));
    Ok(
        UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(id),
            &content,
            None,
        ),
    )
}

/// Owns the application's notification-center delegate. Install once during app launch.
pub struct AppleNotifications {
    center: Retained<UNUserNotificationCenter>,
    delegate: Retained<Delegate>,
}
impl AppleNotifications {
    /// Installs the app's complete category list and a handler for local or APNs action buttons.
    /// The OS may call the handler on a background thread. Complete or persist the selection
    /// before returning. Keep the handle alive for the application's lifetime.
    pub fn install(
        categories: &[NotificationCategory<'_>],
        on_action: impl Fn(NotificationResponse) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        if MainThreadMarker::new().is_none() || NSBundle::mainBundle().bundleIdentifier().is_none()
        {
            return Err(io::Error::other(
                "install notifications on the application bundle's main thread",
            ));
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        if center.delegate().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "notification delegate already installed",
            ));
        }
        let mut native = Vec::with_capacity(categories.len());
        let mut registered = HashMap::new();
        for &category in categories {
            native.push(native_category(category)?);
            let keys = category.actions.iter().map(|a| a.id.to_owned()).collect();
            if registered.insert(category.id.to_owned(), keys).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "duplicate notification category",
                ));
            }
        }
        let allocated = Delegate::alloc().set_ivars(Commands {
            categories: registered,
            invoke: Box::new(on_action),
        });
        // SAFETY: initialize our allocated NSObject subclass exactly once.
        let delegate: Retained<Delegate> = unsafe { msg_send![super(allocated), init] };
        center.setNotificationCategories(&NSSet::from_retained_slice(&native));
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        Ok(Self { center, delegate })
    }

    /// Requests alert permission. The receiver resolves with the user's choice or an OS error.
    pub fn request_permission(&self) -> Receiver<io::Result<bool>> {
        let (tx, rx) = mpsc::channel();
        let reply = RcBlock::new(move |granted: Bool, error: *mut NSError| {
            let _ = tx.send(completion(error).map(|()| granted.as_bool()));
        });
        self.center
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert,
                &reply,
            );
        rx
    }
    /// Submits an immediate notification using a registered category. The completion receiver
    /// reports whether UserNotifications accepted the request.
    pub fn post_local_notification(
        &self,
        id: &str,
        title: &str,
        body: &str,
        category: &str,
    ) -> io::Result<Receiver<io::Result<()>>> {
        if !self.delegate.ivars().categories.contains_key(category) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unregistered notification category",
            ));
        }
        let request = request(id, title, body, category)?;
        let (tx, rx) = mpsc::channel();
        let done = RcBlock::new(move |error: *mut NSError| {
            let _ = tx.send(completion(error));
        });
        self.center
            .addNotificationRequest_withCompletionHandler(&request, Some(&done));
        Ok(rx)
    }
    /// Removes both pending and already delivered notifications for this request ID.
    pub fn cancel_local_notification(&self, id: &str) -> io::Result<()> {
        if !valid_id(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid notification ID",
            ));
        }
        let ids = NSArray::from_retained_slice(&[NSString::from_str(id)]);
        self.center
            .removePendingNotificationRequestsWithIdentifiers(&ids);
        self.center
            .removeDeliveredNotificationsWithIdentifiers(&ids);
        Ok(())
    }
}
impl Drop for AppleNotifications {
    fn drop(&mut self) {
        let ours = ProtocolObject::from_ref(&*self.delegate);
        if self
            .center
            .delegate()
            .is_some_and(|installed| std::ptr::eq(&*installed, ours))
        {
            self.center.setDelegate(None);
        }
    }
}
fn completion(error: *mut NSError) -> io::Result<()> {
    // SAFETY: UserNotifications passes nil or a live NSError for this callback only.
    unsafe { error.as_ref() }.map_or(Ok(()), |e| Err(io::Error::other(e.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NotificationAction;
    #[test]
    fn user_notifications_preserves_request_and_button_identifiers() {
        let actions = [NotificationAction {
            id: "archive",
            title: "Archive project",
        }];
        let spec = NotificationCategory {
            id: "project",
            actions: &actions,
        };
        let native = native_category(spec).unwrap();
        assert_eq!(native.identifier().to_string(), "project");
        let button = native.actions().firstObject().unwrap();
        assert_eq!(button.identifier().to_string(), "archive");
        assert_eq!(button.title().to_string(), "Archive project");
        let notification = request("project-17", "Ready", "Body", "project").unwrap();
        assert_eq!(notification.identifier().to_string(), "project-17");
        assert_eq!(notification.content().title().to_string(), "Ready");
        assert_eq!(
            notification.content().categoryIdentifier().to_string(),
            "project"
        );
        let same = [actions[0], actions[0]];
        assert!(
            native_category(NotificationCategory {
                actions: &same,
                ..spec
            })
            .is_err()
        );
    }
}
