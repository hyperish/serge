//! AVFoundation camera output imported as wgpu/Metal textures without copying pixels.
//! The HAL drop callback retains the pixel buffer until the GPU has finished using it.
#![allow(unsafe_code)]

use futures_channel::oneshot;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::{MTLTexture, MTLTextureType};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    io,
    ptr::NonNull,
};
use wgpu::{TextureFormat, TextureUsages, hal};

unsafe extern "C" {
    fn serge_camera_cache(device: *mut c_void) -> *mut c_void;
    fn serge_camera_release(object: *mut c_void);
    fn serge_camera_image(cache: *mut c_void, pixel: *mut c_void) -> *mut c_void;
    fn serge_camera_texture(
        image: *mut c_void,
        width: *mut u32,
        height: *mut u32,
    ) -> *mut ProtocolObject<dyn MTLTexture>;
    fn serge_camera_open(id: *const c_char, error: *mut c_char, capacity: usize) -> *mut c_void;
    fn serge_camera_take(capture: *mut c_void, time: *mut i64) -> *mut c_void;
    fn serge_camera_stop(capture: *mut c_void);
    fn serge_camera_permission(ctx: *mut c_void, done: extern "C" fn(*mut c_void, bool));
}
struct Owned(NonNull<c_void>);
// SAFETY: the values are retained Core Foundation references or SGMetalImage/SGCapture.
// Release is thread-safe. SGCapture publishes its buffer under @synchronized and stops
// the serial camera queue before releasing its final reference.
unsafe impl Send for Owned {}
unsafe impl Sync for Owned {}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { serge_camera_release(self.0.as_ptr()) };
    }
}

/// Presents the system camera permission dialog. The app supplies NSCameraUsageDescription.
pub async fn request_camera_permission() -> io::Result<bool> {
    extern "C" fn complete(context: *mut c_void, granted: bool) {
        // SAFETY: AVFoundation calls its completion exactly once.
        let sender = unsafe { Box::from_raw(context.cast::<oneshot::Sender<bool>>()) };
        let _ = sender.send(granted);
    }
    let (sender, reply) = oneshot::channel();
    unsafe {
        serge_camera_permission(Box::into_raw(Box::new(sender)).cast(), complete);
    }
    reply.await.map_err(io::Error::other)
}

/// A reusable Core Video texture cache belonging to one wgpu Metal device.
pub struct MetalCameraTextures {
    cache: Owned,
    device: wgpu::Device,
}
impl MetalCameraTextures {
    /// Select the same Metal device as the renderer.
    pub fn new(device: &wgpu::Device) -> io::Result<Self> {
        let metal = unsafe { device.as_hal::<hal::api::Metal>() }.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "camera frames require a Metal device",
            )
        })?;
        let ptr = Retained::as_ptr(metal.raw_device()).cast_mut().cast();
        let cache = NonNull::new(unsafe { serge_camera_cache(ptr) })
            .ok_or_else(|| io::Error::other("could not create Core Video texture cache"))?;
        Ok(Self {
            cache: Owned(cache),
            device: device.clone(),
        })
    }
    /// Import a Metal-compatible BGRA CVPixelBuffer from another AVFoundation producer.
    ///
    /// # Safety
    /// The pointer must identify a live CVPixelBuffer for this call. Do not write to the
    /// underlying IOSurface while the resulting texture can be in flight on the GPU.
    pub unsafe fn import_bgra(&self, buffer: NonNull<c_void>) -> io::Result<wgpu::Texture> {
        // The Objective-C owner retains both the pixel buffer and the CVMetalTexture.
        let frame = Owned(
            NonNull::new(unsafe { serge_camera_image(self.cache.0.as_ptr(), buffer.as_ptr()) })
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "expected a Metal-compatible BGRA pixel buffer",
                    )
                })?,
        );
        let (mut width, mut height) = (0, 0);
        let raw = unsafe {
            Retained::from_raw(serge_camera_texture(
                frame.0.as_ptr(),
                &mut width,
                &mut height,
            ))
        }
        .ok_or_else(|| io::Error::other("missing Metal texture"))?;
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let format = TextureFormat::Bgra8Unorm;
        // SAFETY: the texture cache was created from this device. It produced one BGRA
        // plane with the dimensions queried from the texture and a single mip and layer.
        let texture = unsafe {
            hal::metal::Device::texture_from_raw(
                raw,
                format,
                MTLTextureType::Type2D,
                1,
                1,
                size.into(),
                Some(Box::new(move || drop(frame))),
            )
        };
        Ok(unsafe {
            self.device.create_texture_from_hal::<hal::api::Metal>(
                texture,
                &wgpu::TextureDescriptor {
                    label: Some("Serge camera frame"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC,
                    view_formats: &[],
                },
                wgpu::TextureUses::RESOURCE,
            )
        })
    }
}

/// A captured frame and its producer presentation timestamp.
pub struct MetalCameraFrame {
    /// Sample this texture in the renderer. Its backing buffer is retained through GPU execution.
    pub texture: wgpu::Texture,
    /// Nanoseconds on the capture session's media timeline.
    pub timestamp_ns: i64,
}
/// Running capture session. Releases the device and its delegate when dropped.
pub struct MetalCamera {
    capture: Owned,
    textures: MetalCameraTextures,
}
impl MetalCamera {
    /// Start a camera for which the app already has permission. None selects the default.
    /// Starting AVFoundation can block, so open it off the UI thread.
    pub fn open(device: &wgpu::Device, id: Option<&str>) -> io::Result<Self> {
        let textures = MetalCameraTextures::new(device)?;
        let id = id.map(CString::new).transpose()?;
        let mut error = [0 as c_char; 256];
        let capture = NonNull::new(unsafe {
            serge_camera_open(
                id.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
                error.as_mut_ptr(),
                error.len(),
            )
        })
        .ok_or_else(|| {
            io::Error::other(
                unsafe { CStr::from_ptr(error.as_ptr()) }
                    .to_string_lossy()
                    .into_owned(),
            )
        })?;
        Ok(Self {
            capture: Owned(capture),
            textures,
        })
    }
    /// Take the newest available image. Older unconsumed frames are released.
    pub fn try_frame(&self) -> io::Result<Option<MetalCameraFrame>> {
        let mut timestamp_ns = 0;
        let Some(image) =
            NonNull::new(unsafe { serge_camera_take(self.capture.0.as_ptr(), &mut timestamp_ns) })
        else {
            return Ok(None);
        };
        let image = Owned(image);
        let texture = unsafe { self.textures.import_bgra(image.0) }?;
        Ok(Some(MetalCameraFrame {
            texture,
            timestamp_ns,
        }))
    }
}
impl Drop for MetalCamera {
    fn drop(&mut self) {
        unsafe { serge_camera_stop(self.capture.0.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::{
        base::{CFRelease, TCFType},
        boolean::CFBoolean,
        dictionary::{CFDictionary, CFDictionaryRef},
        string::CFString,
    };
    unsafe extern "C" {
        fn CVPixelBufferCreate(
            alloc: *const c_void,
            width: usize,
            height: usize,
            format: u32,
            attrs: CFDictionaryRef,
            out: *mut *mut c_void,
        ) -> i32;
        fn CVPixelBufferLockBaseAddress(buffer: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferUnlockBaseAddress(buffer: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferGetBaseAddress(buffer: *mut c_void) -> *mut u8;
    }

    #[test]
    fn metal_reads_the_iosurface_after_its_producer_and_import_handle_are_dropped() {
        let renderer = crate::GpuRenderer::headless().expect("a real Metal device");
        let cache = MetalCameraTextures::new(&renderer.device).unwrap();
        let empty = CFDictionary::<CFString, CFString>::from_CFType_pairs(&[]);
        let attrs = CFDictionary::from_CFType_pairs(&[
            (CFString::new("IOSurfaceProperties"), empty.as_CFType()),
            (
                CFString::new("MetalCompatibility"),
                CFBoolean::true_value().as_CFType(),
            ),
        ]);
        let mut pixel = std::ptr::null_mut();
        // Allocate and fill one synthetic BGRA camera buffer, then return ownership to Core Video.
        // No capture device or camera permission is used in this test.
        unsafe {
            assert_eq!(
                CVPixelBufferCreate(
                    std::ptr::null(),
                    1,
                    1,
                    u32::from_be_bytes(*b"BGRA"),
                    attrs.as_concrete_TypeRef(),
                    &mut pixel
                ),
                0
            );
            assert_eq!(CVPixelBufferLockBaseAddress(pixel, 0), 0);
            CVPixelBufferGetBaseAddress(pixel)
                .copy_from_nonoverlapping([12u8, 43, 89, 255].as_ptr(), 4);
            assert_eq!(CVPixelBufferUnlockBaseAddress(pixel, 0), 0);
        }
        let texture = unsafe { cache.import_bgra(NonNull::new(pixel).unwrap()) }.unwrap();
        unsafe { CFRelease(pixel.cast()) }
        drop(cache);
        let read = renderer.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            texture.size(),
        );
        renderer.queue.submit([encoder.finish()]);
        drop(texture);
        let (tx, rx) = std::sync::mpsc::channel();
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        renderer
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .unwrap();
        rx.recv().unwrap().unwrap();
        assert_eq!(
            &read.slice(..).get_mapped_range().unwrap()[..4],
            &[12, 43, 89, 255]
        );
    }
}
