//! The generated Xcode app supplies this ABI only when it embeds platform widgets.
use crate::PlatformWidget;
use futures_channel::oneshot;
use libloading::os::unix::Library;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    io,
};

type Completion = extern "C" fn(*mut c_void, i32, *const c_char);
type Publish = unsafe extern "C" fn(
    *const c_char,
    i32,
    *const u8,
    usize,
    *const c_char,
    *mut c_void,
    Completion,
);
type Reply = oneshot::Sender<io::Result<String>>;

extern "C" fn complete(context: *mut c_void, code: i32, value: *const c_char) {
    // SAFETY: the host calls exactly once per accepted call. Its UTF-8 string
    // is borrowed only for this call, and the box was allocated below.
    #[allow(unsafe_code)]
    let (reply, value) = unsafe {
        (
            Box::from_raw(context.cast::<Reply>()),
            CStr::from_ptr(value).to_string_lossy().into_owned(),
        )
    };
    let _ = reply.send(if code == 0 {
        Ok(value)
    } else {
        Err(io::Error::other(value))
    });
}

async fn publish(widget: &PlatformWidget, operation: i32, activity: &str) -> io::Result<String> {
    if widget.state_json.len() > 65536 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "widget state exceeds 64 KiB",
        ));
    }
    let kind = CString::new(widget.kind)?;
    let id = CString::new(activity)?;
    let library = Library::this();
    // SAFETY: this symbol in the application image is the versioned, generated
    // Swift C ABI. The host copies strings/data before returning and owns one completion.
    #[allow(unsafe_code)]
    let function =
        unsafe { library.get::<Publish>(b"serge_widget_publish_v1\0") }.map_err(|_| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "the application does not embed Serge widgets",
            )
        })?;
    let (tx, rx) = oneshot::channel();
    #[allow(unsafe_code)]
    unsafe {
        function(
            kind.as_ptr(),
            operation,
            widget.state_json.as_ptr(),
            widget.state_json.len(),
            id.as_ptr(),
            Box::into_raw(Box::new(tx)).cast(),
            complete,
        )
    };
    rx.await.map_err(io::Error::other)?
}

/// Publish the state of an iOS home or lock-screen widget and request a new timeline.
pub async fn publish_widget(widget: &PlatformWidget) -> io::Result<()> {
    publish(widget, 0, "").await.map(drop)
}
/// Start a local Live Activity and return its ActivityKit identifier.
pub async fn start_activity(widget: &PlatformWidget) -> io::Result<String> {
    publish(widget, 1, "").await
}
/// Update the identified running Live Activity of the same kind.
pub async fn update_activity(widget: &PlatformWidget, id: &str) -> io::Result<()> {
    publish(widget, 2, id).await.map(drop)
}
/// End the activity with its final state and the system's normal dismissal policy.
pub async fn end_activity(widget: &PlatformWidget, id: &str) -> io::Result<()> {
    publish(widget, 3, id).await.map(drop)
}
