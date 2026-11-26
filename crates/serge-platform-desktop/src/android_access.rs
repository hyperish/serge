//! Attach to NativeActivity's content view for accessibility and window insets.
use super::Wake;
use accesskit_android::{
    InjectingAdapter,
    jni::{
        JavaVM,
        errors::Result as JniResult,
        objects::{GlobalRef, JObject, JValue},
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
pub(super) struct Adapter {
    accessibility: InjectingAdapter,
    vm: JavaVM,
    content: GlobalRef,
    inset_types: i32,
}
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
        let accessibility = InjectingAdapter::new(&mut env, &content, handler.clone(), handler);
        let content = env.new_global_ref(content)?;
        let mut inset_types = 0;
        if env
            .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
            .i()?
            >= 30
        {
            for kind in ["systemBars", "displayCutout", "ime"] {
                inset_types |= env
                    .call_static_method("android/view/WindowInsets$Type", kind, "()I", &[])?
                    .i()?;
            }
        }
        drop(env);
        Ok(Self {
            accessibility,
            vm,
            content,
            inset_types,
        })
    }
    pub(super) fn update_if_active(&mut self, update: impl FnOnce() -> TreeUpdate) {
        self.accessibility.update_if_active(update)
    }

    /// Safe edges in surface pixels. Decor insets are window-relative, while
    /// a NativeActivity's content surface can already be offset inside it.
    pub(super) fn safe_bounds(&self) -> Option<[i32; 4]> {
        if self.inset_types == 0 {
            return None;
        }
        let mut env = self.vm.attach_current_thread().ok()?;
        let result = env.with_local_frame(16, |env| -> JniResult<_> {
            let view = self.content.as_obj();
            let root = env
                .call_method(view, "getRootView", "()Landroid/view/View;", &[])?
                .l()?;
            let window = env
                .call_method(
                    &root,
                    "getRootWindowInsets",
                    "()Landroid/view/WindowInsets;",
                    &[],
                )?
                .l()?;
            if window.is_null() {
                return Ok(None);
            }
            let insets = env
                .call_method(
                    &window,
                    "getInsets",
                    "(I)Landroid/graphics/Insets;",
                    &[JValue::Int(self.inset_types)],
                )?
                .l()?;
            let origin = env.new_int_array(2)?;
            env.call_method(
                view,
                "getLocationInWindow",
                "([I)V",
                &[JValue::Object(origin.as_ref())],
            )?;
            let mut xy = [0; 2];
            env.get_int_array_region(&origin, 0, &mut xy)?;
            let width = env.call_method(&root, "getWidth", "()I", &[])?.i()?;
            let height = env.call_method(&root, "getHeight", "()I", &[])?.i()?;
            if width <= 0 || height <= 0 {
                return Ok(None);
            }
            Ok(Some([
                env.get_field(&insets, "left", "I")?.i()? - xy[0],
                env.get_field(&insets, "top", "I")?.i()? - xy[1],
                width - env.get_field(&insets, "right", "I")?.i()? - xy[0],
                height - env.get_field(&insets, "bottom", "I")?.i()? - xy[1],
            ]))
        });
        if result.is_err() && env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        result.ok().flatten()
    }
}
