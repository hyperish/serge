//! Camera2 producer buffers for native GPU import. The image lease prevents producer reuse.
use super::{AndroidSystem, services::class};
use jni::{
    jni_sig, jni_str,
    objects::{JObject, JValue},
    refs::Global,
};
use ndk::{
    hardware_buffer::{HardwareBuffer, HardwareBufferUsage},
    media::image_reader::{AcquireResult, Image, ImageFormat, ImageReader},
};
use std::{io, rc::Rc};

/// One acquired PRIVATE Camera2 image. Keep the frame alive until GPU use completes.
pub struct CameraBufferFrame {
    buffer: HardwareBuffer,
    timestamp_ns: i64,
    // Drop the image before its reader. Releasing the reader would invalidate all of its images.
    _image: Image,
    _reader: Rc<ImageReader>,
}
impl CameraBufferFrame {
    /// Borrow the producer-owned AHardwareBuffer for import, without copying or mapping pixels.
    pub fn buffer(&self) -> &HardwareBuffer {
        &self.buffer
    }
    /// The producer timestamp in nanoseconds.
    pub fn timestamp_ns(&self) -> i64 {
        self.timestamp_ns
    }
}

/// Camera2 capture into an AImageReader queue of GPU-sampleable hardware buffers.
pub struct CameraBufferReader {
    system: AndroidSystem,
    stream: Global<JObject<'static>>,
    reader: Rc<ImageReader>,
}
impl CameraBufferReader {
    /// Starts a PRIVATE stream at a size reported by CameraTexture::sizes.
    pub fn open(system: &AndroidSystem, camera: &str, size: (u32, u32)) -> io::Result<Self> {
        let width = i32::try_from(size.0).map_err(io::Error::other)?;
        let height = i32::try_from(size.1).map_err(io::Error::other)?;
        let reader = Rc::new(
            ImageReader::new_with_usage(
                width,
                height,
                ImageFormat::PRIVATE,
                HardwareBufferUsage::GPU_SAMPLED_IMAGE,
                4,
            )
            .map_err(io::Error::other)?,
        );
        let window = reader.window().map_err(io::Error::other)?;
        let stream = system.access(|env, activity| {
            let api = class(env,activity)?;
            let camera = env.new_string(camera)?;
            // SAFETY: this JNI attachment and the reader-owned window live through the call.
            // ANativeWindow_toSurface creates a new local Java reference.
            #[allow(unsafe_code)]
            let surface = unsafe { JObject::from_raw(env,window.to_surface(env.get_raw().cast()).cast()) };
            if surface.is_null() { return Err(jni::errors::Error::NullPtr("ImageReader surface")); }
            let result = env.call_static_method(api,jni_str!("cameraSurface"),jni_sig!("(Landroid/app/Activity;Ljava/lang/String;IILandroid/view/Surface;)Lorg/hyperish/serge/CameraTexture;"),
                &[JValue::Object(activity),JValue::Object(&camera),JValue::Int(width),JValue::Int(height),JValue::Object(&surface)])?.l()?;
            env.new_global_ref(result)
        })?;
        Ok(Self {
            system: system.clone(),
            stream,
            reader,
        })
    }
    /// Returns the latest ready image. All four outstanding leases must eventually be dropped.
    pub fn try_frame(&self) -> io::Result<Option<CameraBufferFrame>> {
        self.system.access(|env, _| {
            env.call_method(&self.stream, jni_str!("check"), jni_sig!("()V"), &[])?;
            Ok(())
        })?;
        match self
            .reader
            .acquire_latest_image()
            .map_err(io::Error::other)?
        {
            AcquireResult::NoBufferAvailable => Ok(None),
            AcquireResult::MaxImagesAcquired => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "release an outstanding camera buffer",
            )),
            AcquireResult::Image(image) => Ok(Some(CameraBufferFrame {
                buffer: image.hardware_buffer().map_err(io::Error::other)?,
                timestamp_ns: image.timestamp().map_err(io::Error::other)?,
                _image: image,
                _reader: self.reader.clone(),
            })),
        }
    }
}
impl Drop for CameraBufferReader {
    fn drop(&mut self) {
        let _ = self.system.access(|env, _| {
            env.call_method(&self.stream, jni_str!("close"), jni_sig!("()V"), &[])?;
            Ok(())
        });
    }
}
