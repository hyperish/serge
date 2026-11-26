//! Registered URL schemes and universal links delivered by UIKit.
#![allow(unsafe_code)]
use super::IosSystem;
use block2::{DynBlock, RcBlock};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, ProtocolObject},
};
use objc2_foundation::{
    NSArray, NSDictionary, NSObject, NSObjectProtocol, NSString, NSURL, NSUserActivity,
    NSUserActivityTypeBrowsingWeb,
};
use objc2_ui_kit::{UIApplication, UIApplicationDelegate, UIUserActivityRestoring};
use std::{
    io,
    sync::mpsc::{self, Receiver},
};

struct Handler {
    schemes: Vec<String>,
    route: Box<dyn Fn(&str) -> bool>,
}
impl Handler {
    fn deliver(&self, url: &NSURL) -> bool {
        url.scheme().is_some_and(|s| {
            self.schemes
                .iter()
                .any(|scheme| scheme.eq_ignore_ascii_case(&s.to_string()))
        }) && url
            .absoluteString()
            .is_some_and(|text| (self.route)(&text.to_string()))
    }
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SergeApplicationLinkDelegate"]
    #[ivars = Handler]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl UIApplicationDelegate for Delegate {
        #[unsafe(method(application:openURL:options:))]
        fn open(
            &self,
            _: &UIApplication,
            url: &NSURL,
            _: &NSDictionary<NSString, AnyObject>,
        ) -> bool {
            self.ivars().deliver(url)
        }
        #[unsafe(method(application:continueUserActivity:restorationHandler:))]
        fn resume(
            &self,
            _: &UIApplication,
            activity: &NSUserActivity,
            _: &DynBlock<dyn Fn(*mut NSArray<ProtocolObject<dyn UIUserActivityRestoring>>)>,
        ) -> bool {
            activity
                .activityType()
                .isEqualToString(unsafe { NSUserActivityTypeBrowsingWeb })
                && activity
                    .webpageURL()
                    .is_some_and(|url| self.ivars().deliver(&url))
        }
    }
);

/// Routes declared URL schemes on the UIKit main thread. Keep this handle alive while routing.
pub struct IosDeepLinks {
    delegate: Retained<Delegate>,
    application: Option<Retained<UIApplication>>,
}
impl IosDeepLinks {
    /// Creates a handler. A scene or existing native host can forward its open-URL contexts to it.
    pub fn new(schemes: &[&str], route: impl Fn(&str) -> bool + 'static) -> io::Result<Self> {
        let main = MainThreadMarker::new()
            .ok_or_else(|| io::Error::other("deep links require the UIKit main thread"))?;
        if schemes.is_empty()
            || schemes.iter().any(|s| {
                !s.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                    || !s
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"+.-".contains(&c))
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a list of URL schemes",
            ));
        }
        let ivars = Handler {
            schemes: schemes.iter().map(|s| s.to_ascii_lowercase()).collect(),
            route: Box::new(route),
        };
        let allocated = Delegate::alloc(main).set_ivars(ivars);
        // SAFETY: superclass initialization of the private NSObject subclass on the main thread.
        let delegate = unsafe { msg_send![super(allocated), init] };
        Ok(Self {
            delegate,
            application: None,
        })
    }
    /// Connects a launched application that leaves its UIApplicationDelegate to Serge.
    /// Hosts providing their own app or scene delegate use forward_url in their callbacks.
    pub fn install_application(&mut self) -> io::Result<()> {
        if self.application.is_some() {
            return Ok(());
        }
        let app = UIApplication::sharedApplication(self.delegate.mtm());
        // SAFETY: UIKit delegate access and replacement are serialized on the main thread.
        // This handle strongly owns the new delegate until it is detached in Drop.
        unsafe {
            if app.delegate().is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "UIKit already has an app delegate",
                ));
            }
            app.setDelegate(Some(ProtocolObject::from_ref(&*self.delegate)));
        }
        self.application = Some(app);
        Ok(())
    }
    /// Applies the same scheme check and app route to a URL delivered by an app-owned scene.
    pub fn forward_url(&self, url: &NSURL) -> bool {
        self.delegate.ivars().deliver(url)
    }
}
impl Drop for IosDeepLinks {
    fn drop(&mut self) {
        if let Some(app) = self.application.take() {
            let ours = ProtocolObject::from_ref(&*self.delegate);
            // SAFETY: remove our own unretained delegate on its owning main thread.
            unsafe {
                if app.delegate().is_some_and(|d| std::ptr::eq(&*d, ours)) {
                    app.setDelegate(None);
                }
            }
        }
    }
}
impl IosSystem {
    /// Asks iOS to open an absolute URL. UIKit reports whether the system accepted the request.
    pub fn open_url(&self, url: &str) -> io::Result<Receiver<bool>> {
        let url = NSURL::URLWithString(&NSString::from_str(url))
            .filter(|u| u.scheme().is_some())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "expected an absolute URL")
            })?;
        let (tx, rx) = mpsc::channel();
        let completion = RcBlock::new(move |ok: Bool| {
            let _ = tx.send(ok.as_bool());
        });
        // SAFETY: the empty options dictionary is valid for every URL; UIKit copies the block.
        unsafe {
            UIApplication::sharedApplication(self.main).openURL_options_completionHandler(
                &url,
                &NSDictionary::new(),
                Some(&completion),
            );
        }
        Ok(rx)
    }
}
