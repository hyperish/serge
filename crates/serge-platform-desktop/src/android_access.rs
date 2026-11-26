//! Attach AccessKit to NativeActivity's content view.
use super::Wake;
use accesskit_android::{
    InjectingAdapter,
    jni::{
        JavaVM,
        objects::{JObject, JValue},
    },
};
use serge_native::accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use winit::{
    event_loop::{ActiveEventLoop, EventLoopProxy},
    platform::android::ActiveEventLoopExtAndroid,
    window::WindowId,
};

#[derive(Clone)]
struct Handler {
    window_id: WindowId,
    proxy: EventLoopProxy<Wake>,
}
impl Handler {
    fn send(&self, window_event: accesskit_winit::WindowEvent) {
        let _ = self.proxy.send_event(Wake::Access(accesskit_winit::Event {
            window_id: self.window_id,
            window_event,
        }));
    }
}
impl ActivationHandler for Handler {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.send(accesskit_winit::WindowEvent::InitialTreeRequested);
        None
    }
}
impl ActionHandler for Handler {
    fn do_action(&mut self, request: ActionRequest) {
        self.send(accesskit_winit::WindowEvent::ActionRequested(request));
    }
}
pub(super) struct Adapter(InjectingAdapter);
impl Adapter {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        proxy: EventLoopProxy<Wake>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let app = event_loop.android_app();
        // SAFETY: AndroidApp owns the VM and activity global reference throughout
        // the activity lifecycle. JNI calls below only borrow this reference.
        #[allow(unsafe_code)]
        let (vm, activity) = unsafe {
            (
                JavaVM::from_raw(app.vm_as_ptr().cast())?,
                JObject::from_raw(app.activity_as_ptr().cast()),
            )
        };
        let mut env = vm.attach_current_thread()?;
        // android.R.id.content is the public activity content container.
        let content = env
            .call_method(
                &activity,
                "findViewById",
                "(I)Landroid/view/View;",
                &[JValue::Int(0x0102_0002)],
            )?
            .l()?;
        if content.is_null() {
            return Err("NativeActivity has no content view".into());
        }
        let handler = Handler { window_id, proxy };
        Ok(Self(InjectingAdapter::new(
            &mut env,
            &content,
            handler.clone(),
            handler,
        )))
    }
    pub(super) fn update_if_active(&mut self, update: impl FnOnce() -> TreeUpdate) {
        self.0.update_if_active(update)
    }
}
