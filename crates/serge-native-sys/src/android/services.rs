use super::AndroidSystem;
use jni::{
    Env,
    errors::Result,
    jni_sig, jni_str,
    objects::{JByteArray, JClass, JObject, JObjectArray, JString, JValue},
    refs::Global,
};
use std::{io, sync::OnceLock};

pub(super) fn class(env: &mut Env, activity: &JObject) -> Result<&'static Global<JClass<'static>>> {
    static CLASS: OnceLock<Global<JClass<'static>>> = OnceLock::new();
    if let Some(class) = CLASS.get() {
        return Ok(class);
    }
    let loaded = dex_class(
        env,
        activity,
        include_bytes!("../../android/services.dex"),
        "org.hyperish.serge.NativeServices",
    )?;
    let _ = CLASS.set(loaded);
    Ok(CLASS.get().unwrap())
}

pub(super) fn dex_class(
    env: &mut Env,
    activity: &JObject,
    dex: &[u8],
    name: &str,
) -> Result<Global<JClass<'static>>> {
    let parent = env
        .call_method(
            activity,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    let bytes = env.byte_array_from_slice(dex)?;
    let data = env
        .call_static_method(
            jni_str!("java/nio/ByteBuffer"),
            jni_str!("wrap"),
            jni_sig!("([B)Ljava/nio/ByteBuffer;"),
            &[JValue::Object(&bytes)],
        )?
        .l()?;
    let loader = env.new_object(
        jni_str!("dalvik/system/InMemoryDexClassLoader"),
        jni_sig!("(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V"),
        &[JValue::Object(&data), JValue::Object(&parent)],
    )?;
    let name = env.new_string(name)?;
    let resolved = env
        .call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    env.new_cast_global_ref::<JClass>(resolved)
}

pub(super) fn app_class<'local>(
    env: &mut Env<'local>,
    activity: &JObject,
    name: &str,
) -> Result<JClass<'local>> {
    let loader = env
        .call_method(
            activity,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    let name = env.new_string(name)?;
    let resolved = env
        .call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    env.cast_local::<JClass>(resolved)
}

/// The final Android platform dialog result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptResult {
    /// The user or owner dismissed the dialog.
    Cancelled,
    /// An alert, selected zero-based item, ISO date, local HH:MM time, or biometric result.
    Selected(String),
}
/// Pollable native dialog. Dropping it cancels pending UI or biometric work.
pub struct Prompt {
    system: AndroidSystem,
    handle: Global<JObject<'static>>,
}
impl Prompt {
    /// Returns immediately with the result, or None while the OS dialog is open.
    pub fn try_result(&self) -> io::Result<Option<PromptResult>> {
        let (status, value) = self.system.access(|env, _| {
            let status = env
                .call_method(&self.handle, jni_str!("status"), jni_sig!("()I"), &[])?
                .i()?;
            let v = if status >= 2 {
                let text = env
                    .call_method(
                        &self.handle,
                        jni_str!("value"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )?
                    .l()?;
                env.cast_local::<JString>(text)?.to_string()
            } else {
                String::new()
            };
            Ok((status, v))
        })?;
        match status {
            0 => Ok(None),
            1 => Ok(Some(PromptResult::Cancelled)),
            2 => Ok(Some(PromptResult::Selected(value))),
            _ => Err(io::Error::other(value)),
        }
    }
}
impl Drop for Prompt {
    fn drop(&mut self) {
        let _ = self.system.access(|env, _| {
            env.call_method(&self.handle, jni_str!("cancel"), jni_sig!("()V"), &[])?;
            Ok(())
        });
    }
}
/// A service/account entry encrypted with a non-exportable AndroidKeyStore key.
pub struct Credential {
    system: AndroidSystem,
    service: String,
    account: String,
}
impl Credential {
    /// Replaces the secret after encrypting with a fresh GCM nonce.
    pub fn set_secret(&self, secret: &[u8]) -> io::Result<()> {
        self.access(Some(secret), false).map(|_| ())
    }
    /// Returns None after removal or before first use.
    pub fn get_secret(&self) -> io::Result<Option<Vec<u8>>> {
        self.access(None, false)
    }
    /// Deletes the OS keystore entry and its ciphertext.
    pub fn delete(&self) -> io::Result<()> {
        self.access(None, true).map(|_| ())
    }
    fn access(&self, bytes: Option<&[u8]>, remove: bool) -> io::Result<Option<Vec<u8>>> {
        if bytes.is_some_and(|b| b.len() > 65536) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "credential exceeds 64 KiB",
            ));
        }
        self.system.access(|env, a| {
            let c = class(env, a)?;
            let service = env.new_string(&self.service)?;
            let account = env.new_string(&self.account)?;
            let update = match bytes {
                Some(b) => JObject::from(env.byte_array_from_slice(b)?),
                None => JObject::null(),
            };
            let result = env
                .call_static_method(
                    c,
                    jni_str!("credential"),
                    jni_sig!(
                        "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;[BZ)[B"
                    ),
                    &[
                        JValue::Object(a),
                        JValue::Object(&service),
                        JValue::Object(&account),
                        JValue::Object(&update),
                        JValue::Bool(remove),
                    ],
                )?
                .l()?;
            if result.is_null() {
                return Ok(None);
            }
            let array = env.cast_local::<JByteArray>(result)?;
            Ok(Some(env.convert_byte_array(array)?))
        })
    }
}
impl AndroidSystem {
    /// Creates a handle for UTF-8 service and account names in this application's sandbox.
    pub fn credential(&self, service: &str, account: &str) -> io::Result<Credential> {
        if [service, account]
            .iter()
            .any(|s| s.is_empty() || s.len() > 1024 || s.contains('\0'))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid credential identifier",
            ));
        }
        Ok(Credential {
            system: self.clone(),
            service: service.into(),
            account: account.into(),
        })
    }
    /// Requests an Android runtime permission. Read is_granted after the activity regains focus.
    pub fn request_permission(&self, permission: &str, request_id: u16) -> io::Result<()> {
        self.access(|env, a| {
            let c = class(env, a)?;
            let p = env.new_string(permission)?;
            env.call_static_method(
                c,
                jni_str!("requestPermission"),
                jni_sig!("(Landroid/app/Activity;Ljava/lang/String;I)V"),
                &[
                    JValue::Object(a),
                    JValue::Object(&p),
                    JValue::Int(request_id.into()),
                ],
            )?;
            Ok(())
        })
    }
    /// Presents a native OK alert.
    pub fn alert(&self, title: &str, message: &str) -> io::Result<Prompt> {
        self.prompt("alert", title, message, &[])
    }
    /// Presents a labelled list of actions and returns the selected zero-based index as text.
    pub fn choose(&self, title: &str, items: &[&str]) -> io::Result<Prompt> {
        if items.is_empty() || items.len() > 256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected between 1 and 256 actions",
            ));
        }
        self.prompt("options", title, "", items)
    }
    /// Presents the system date picker, initially set to today.
    pub fn pick_date(&self) -> io::Result<Prompt> {
        self.prompt("date", "", "", &[])
    }
    /// Presents the system time picker, initially set to the current local time.
    pub fn pick_time(&self) -> io::Result<Prompt> {
        self.prompt("time", "", "", &[])
    }
    /// Requests device biometrics on API 28+. The app declares USE_BIOMETRIC.
    pub fn authenticate(&self, title: &str, reason: &str) -> io::Result<Prompt> {
        self.prompt("biometric", title, reason, &[])
    }
    pub(super) fn prompt(
        &self,
        kind: &str,
        title: &str,
        message: &str,
        items: &[&str],
    ) -> io::Result<Prompt> {
        let handle=self.access(|env,a|{
            let c=class(env,a)?;
            let kind=env.new_string(kind)?;let title=env.new_string(title)?;let message=env.new_string(message)?;
            let options=JObjectArray::<JString>::new(env,items.len(),&JString::null())?;
            for (index,item) in items.iter().enumerate(){let value=env.new_string(item)?;options.set_element(env,index,&value)?;}
            let h=env.call_static_method(c,jni_str!("prompt"),jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)Lorg/hyperish/serge/NativeServices$Prompt;"),&[JValue::Object(a),JValue::Object(&kind),JValue::Object(&title),JValue::Object(&message),JValue::Object(&options)])?.l()?;
            env.new_global_ref(h)
        })?;
        Ok(Prompt {
            system: self.clone(),
            handle,
        })
    }
}

impl AndroidSystem {
    /// Opens the OS document picker for one MIME type. A selected result is a content URI.
    /// Enable `document_picker = true` in the application's Android Cargo metadata.
    pub fn pick_document(&self, mime: &str) -> io::Result<Prompt> {
        self.document_picker(mime, None)
    }
    /// Lets the user choose the location and name of a new document.
    pub fn create_document(&self, mime: &str, name: &str) -> io::Result<Prompt> {
        if name.is_empty() || name.contains(['/', '\\', '\0']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a document filename",
            ));
        }
        self.document_picker(mime, Some(name))
    }
    fn document_picker(&self, mime: &str, name: Option<&str>) -> io::Result<Prompt> {
        if !mime
            .split_once('/')
            .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
            || mime.chars().any(char::is_control)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a MIME type",
            ));
        }
        let handle = self.access(|env, activity| {
            let picker = app_class(env, activity, "org.hyperish.serge.documents.DocumentPicker")?;
            let mime = env.new_string(mime)?;
            let title = match name {
                Some(name) => JObject::from(env.new_string(name)?),
                None => JObject::null(),
            };
            let ticket = env.call_static_method(&picker, jni_str!("pick"), jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)Lorg/hyperish/serge/documents/DocumentPicker$Ticket;"),
                &[JValue::Object(activity),JValue::Object(&mime),JValue::Object(&title)])?.l()?;
            env.new_global_ref(ticket)
        })?;
        Ok(Prompt {
            system: self.clone(),
            handle,
        })
    }

    /// Opens a document granted by its provider for reading. The file may be a pipe.
    pub fn read_document(&self, uri: &str) -> io::Result<std::fs::File> {
        self.open_document(uri, "r")
    }
    /// Opens a document granted by its provider for writing and truncates existing data.
    pub fn write_document(&self, uri: &str) -> io::Result<std::fs::File> {
        self.open_document(uri, "wt")
    }
    fn open_document(&self, uri: &str, mode: &str) -> io::Result<std::fs::File> {
        use std::os::fd::FromRawFd;
        if !uri.starts_with("content://") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a document content URI",
            ));
        }
        let fd = self.access(|env, activity| {
            let resolver = env
                .call_method(
                    activity,
                    jni_str!("getContentResolver"),
                    jni_sig!("()Landroid/content/ContentResolver;"),
                    &[],
                )?
                .l()?;
            let text = env.new_string(uri)?;
            let uri = env
                .call_static_method(
                    jni_str!("android/net/Uri"),
                    jni_str!("parse"),
                    jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
                    &[JValue::Object(&text)],
                )?
                .l()?;
            let mode = env.new_string(mode)?;
            let parcel = env
                .call_method(
                    &resolver,
                    jni_str!("openFileDescriptor"),
                    jni_sig!(
                        "(Landroid/net/Uri;Ljava/lang/String;)Landroid/os/ParcelFileDescriptor;"
                    ),
                    &[JValue::Object(&uri), JValue::Object(&mode)],
                )?
                .l()?;
            if parcel.is_null() {
                return Ok(-1);
            }
            env.call_method(&parcel, jni_str!("detachFd"), jni_sig!("()I"), &[])?
                .i()
        })?;
        if fd < 0 {
            return Err(io::Error::other("document provider did not open the file"));
        }
        // SAFETY: detachFd transfers the open descriptor into the caller's sole ownership.
        #[allow(unsafe_code)]
        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    }
}
