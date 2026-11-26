//! Record a producer pass on Serge's device before normal UI composition.
use serge_core::ScenePrimitive;
use serge_core::viewport::{Extent, Slot, TextureKey};
use std::{collections::HashMap, rc::Rc};
pub use wgpu;

/// Host-owned resources borrowed for one producer pass.
pub struct GpuFrame<'a> {
    /// Device that owns the supplied texture.
    pub device: &'a wgpu::Device,
    /// Host submission queue.
    pub queue: &'a wgpu::Queue,
    /// Record the producer pass in this encoder.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// Single-sampled RGBA8 render target.
    pub view: &'a wgpu::TextureView,
    /// Physical dimensions.
    pub extent: Extent,
    /// Changes on resize or surface replacement.
    pub key: TextureKey,
}
/// Platform backend installed with `Viewport::backend`.
pub struct GpuViewport(Rc<dyn Fn(GpuFrame<'_>)>);
impl GpuViewport {
    /// Creates a renderer that records into the provided frame.
    pub fn new(draw: impl Fn(GpuFrame<'_>) + 'static) -> Self {
        Self(Rc::new(draw))
    }
}

pub(super) fn prepare(
    textures: &mut HashMap<TextureKey, wgpu::Texture>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    scene: &[ScenePrimitive],
) {
    textures.retain(|key, _| Slot::resolve(*key).is_some());
    for primitive in scene {
        let ScenePrimitive::ExternalTexture { key, .. } = primitive else {
            continue;
        };
        let Some(slot) = Slot::resolve(*key) else {
            continue;
        };
        let extent = slot.extent();
        if extent.byte_len().is_none()
            || extent.width.max(extent.height) > device.limits().max_texture_dimension_2d
        {
            slot.presented(false);
            continue;
        }
        let gpu = slot.backend::<GpuViewport>();
        // Upload CPU-only producers when their UI is on a hardware GPU.
        let pixels = if gpu.is_none() { slot.rgba() } else { None };
        if gpu.is_none() && pixels.is_none() {
            textures.remove(key);
            slot.presented(false);
            continue;
        }
        let texture = textures.entry(*key).or_insert_with(|| {
            super::texture(
                device,
                "serge external viewport",
                extent.width,
                extent.height,
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST,
            )
        });
        if let Some(gpu) = gpu {
            (gpu.0)(GpuFrame {
                device,
                queue,
                encoder,
                view: &texture.create_view(&Default::default()),
                extent,
                key: *key,
            });
        } else if let Some(bytes) = pixels {
            queue.write_texture(
                texture.as_image_copy(),
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(extent.width * 4),
                    rows_per_image: None,
                },
                texture.size(),
            );
        }
        slot.presented(true);
    }
}
