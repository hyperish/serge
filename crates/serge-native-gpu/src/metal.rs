//! A native Metal batch on the renderer's own queue, compiled from the same WGSL.
#![allow(unsafe_code)]
use super::{GpuError, INSTANCE_BYTES, Target, TextureKey, err};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::MTLTexture;
use std::{
    collections::HashMap,
    ffi::{CStr, CString, c_char, c_void},
    ptr::NonNull,
};
use wgpu::hal;

#[repr(C)]
struct ViewportBinding {
    index: u32,
    texture: *const c_void,
}
unsafe extern "C" {
    fn serge_scene_new(
        device: *mut c_void,
        source: *const c_char,
        vs: *const c_char,
        fs: *const c_char,
        error: *mut c_char,
        capacity: usize,
    ) -> *mut c_void;
    fn serge_scene_release(program: *mut c_void);
    fn serge_scene_draw(
        program: *mut c_void,
        command: *const c_void,
        target: *const c_void,
        format: u32,
        glyphs: *const c_void,
        paths: *const c_void,
        bytes: *const c_void,
        count: u32,
        slots: *const ViewportBinding,
        slots_len: usize,
        draws: *mut u32,
        error: *mut c_char,
        capacity: usize,
    ) -> bool;
}

pub(super) struct MetalBatch(NonNull<c_void>);
// SAFETY: the program holds retained Metal objects. Pipeline cache access is exclusive through &mut self.
unsafe impl Send for MetalBatch {}
unsafe impl Sync for MetalBatch {}
impl Drop for MetalBatch {
    fn drop(&mut self) {
        unsafe {
            serge_scene_release(self.0.as_ptr());
        }
    }
}
fn failure(message: &[c_char]) -> GpuError {
    // The buffers start zeroed and native errors use snprintf with the full capacity.
    err(unsafe { CStr::from_ptr(message.as_ptr()) }.to_string_lossy())
}
fn texture(texture: &wgpu::Texture) -> Result<Retained<ProtocolObject<dyn MTLTexture>>, GpuError> {
    // SAFETY: retain the handle while wgpu's guard still prevents destruction.
    let native = unsafe { texture.as_hal::<hal::api::Metal>() }
        .ok_or_else(|| err("expected Metal texture"))?;
    let reference = std::ptr::from_ref(native.raw_handle()).cast_mut();
    Ok(unsafe { Retained::retain(reference) }.expect("live Metal texture"))
}

pub(super) struct Scene<'a> {
    pub instances: &'a [u8],
    pub glyphs: &'a wgpu::Texture,
    pub paths: &'a wgpu::Texture,
    pub viewports: &'a [(u32, TextureKey)],
    pub textures: &'a HashMap<TextureKey, wgpu::Texture>,
}

impl MetalBatch {
    pub fn new(device: &wgpu::Device) -> Result<Option<Self>, GpuError> {
        let Some(native) = (unsafe { device.as_hal::<hal::api::Metal>() }) else {
            return Ok(None);
        };
        let source = CString::new(include_str!(concat!(env!("OUT_DIR"), "/scene.metal")))
            .expect("generated Metal source");
        let vs = CString::new(env!("SERGE_METAL_vs_main")).unwrap();
        let fs = CString::new(env!("SERGE_METAL_fs_main")).unwrap();
        let mut message = [0; 1024];
        let program = unsafe {
            serge_scene_new(
                Retained::as_ptr(native.raw_device()).cast_mut().cast(),
                source.as_ptr(),
                vs.as_ptr(),
                fs.as_ptr(),
                message.as_mut_ptr(),
                message.len(),
            )
        };
        NonNull::new(program)
            .map(|p| Some(Self(p)))
            .ok_or_else(|| failure(&message))
    }

    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        preparation: &mut wgpu::CommandEncoder,
        target: Target<'_>,
        scene: Scene<'_>,
    ) -> Result<(wgpu::CommandBuffer, u32), GpuError> {
        let format = match target.format {
            wgpu::TextureFormat::Rgba8Unorm => 0,
            wgpu::TextureFormat::Bgra8Unorm => 1,
            wgpu::TextureFormat::Rgba8UnormSrgb => 2,
            wgpu::TextureFormat::Bgra8UnormSrgb => 3,
            _ => return Err(err("unsupported Metal scene color format")),
        };
        let images = || scene.viewports.iter().map(|(_, key)| &scene.textures[key]);
        preparation.transition_resources(
            std::iter::empty(),
            [scene.glyphs, scene.paths]
                .into_iter()
                .chain(images())
                .map(|texture| wgpu::TextureTransition {
                    texture,
                    selector: None,
                    state: wgpu::TextureUses::RESOURCE,
                }),
        );
        // Mark this target initialized in wgpu before a subsequent wgpu readback or presentation.
        // Recording raw commands and validated commands requires distinct command encoders.
        drop(preparation.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Serge Metal target initialization"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        }));
        let surface = texture(target.view.texture())?;
        let glyphs = texture(scene.glyphs)?;
        let paths = texture(scene.paths)?;
        let native_images = images().map(texture).collect::<Result<Vec<_>, _>>()?;
        let bindings: Vec<_> = scene
            .viewports
            .iter()
            .zip(&native_images)
            .map(|((index, _), tex)| ViewportBinding {
                index: *index,
                texture: Retained::as_ptr(tex).cast(),
            })
            .collect();
        let mut raw = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Serge native Metal encoder"),
        });
        let mut message = [0; 1024];
        let mut draws = 0;
        let count = u32::try_from(scene.instances.len() as u64 / INSTANCE_BYTES).map_err(err)?;
        // SAFETY: all resources belong to this device. Native encoding ends its pass, copies the
        // instance bytes, and retains textures/pipeline/buffer until the command buffer completes.
        // The caller submits preparation, raw draw and optional readback together in that order.
        unsafe {
            raw.as_hal_mut::<hal::api::Metal, _, _>(|raw| {
                let cmd = raw
                    .and_then(|e| e.raw_command_buffer())
                    .ok_or_else(|| err("missing Metal command buffer"))?;
                let ok = serge_scene_draw(
                    self.0.as_ptr(),
                    std::ptr::from_ref(cmd).cast(),
                    Retained::as_ptr(&surface).cast(),
                    format,
                    Retained::as_ptr(&glyphs).cast(),
                    Retained::as_ptr(&paths).cast(),
                    scene.instances.as_ptr().cast(),
                    count,
                    bindings.as_ptr(),
                    bindings.len(),
                    &mut draws,
                    message.as_mut_ptr(),
                    message.len(),
                );
                if ok { Ok(()) } else { Err(failure(&message)) }
            })
        }?;
        Ok((raw.finish(), draws))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Color, GpuRenderer, Rect, ScenePrimitive};

    fn scene() -> Vec<ScenePrimitive> {
        (0..256)
            .map(|i| ScenePrimitive::Quad {
                rect: Rect::new((i % 16) as f32 * 8.0, (i / 16) as f32 * 8.0, 7.5, 7.5),
                color: Color::rgba(i as u8, 85, 150, 211),
                radius: 2.0,
            })
            .collect()
    }

    #[test]
    fn a_native_metal_buffer_matches_the_validated_batch() {
        let mut gpu = GpuRenderer::headless().unwrap();
        assert!(gpu.uses_native_metal());
        let scene = scene();
        let native = gpu.render(&scene, 128, 128, 1.).unwrap();
        assert_eq!(gpu.stats().draw_calls, 1);
        assert_eq!(gpu.stats().instances, 256);
        gpu.metal.take();
        let reference = gpu.render(&scene, 128, 128, 1.).unwrap();
        assert_eq!(native.rgba, reference.rgba);
    }

    #[test]
    #[ignore = "local release comparison including GPU completion and readback"]
    fn compare_metal_batches() {
        let mut gpu = GpuRenderer::headless().unwrap();
        let scene = scene();
        let mut metal = gpu.metal.take();
        assert!(metal.is_some());
        let mut measurements = [Vec::new(), Vec::new()];
        for round in 0..22 {
            for mode in [round % 2, 1 - round % 2] {
                if mode == 0 {
                    gpu.metal = metal.take();
                }
                let start = std::time::Instant::now();
                for _ in 0..12 {
                    std::hint::black_box(gpu.render(&scene, 128, 128, 1.).unwrap());
                }
                if round > 0 {
                    measurements[mode].push(start.elapsed().as_nanos() / 12);
                }
                if mode == 0 {
                    metal = gpu.metal.take();
                }
            }
        }
        for (label, mut samples) in ["native Metal", "wgpu"].into_iter().zip(measurements) {
            samples.sort_unstable();
            println!(
                "{label} ns per frame median {} ({}..{})",
                samples[10], samples[0], samples[20]
            );
        }
    }
}
