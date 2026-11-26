//! Native Vulkan sampling of acquired Camera2 AHardwareBuffers, including external YCbCr formats.
#![allow(unsafe_code)]
use super::CameraBufferFrame;
use ash::{Device, Instance, android::external_memory_android_hardware_buffer, vk};
use std::io;

/// A view and immutable sampler backed by the camera's buffer. Owns the producer lease.
pub struct VulkanCameraImage<'device> {
    device: &'device Device,
    frame: CameraBufferFrame,
    image: vk::Image,
    memory: vk::DeviceMemory,
    conversion: vk::SamplerYcbcrConversion,
    view: vk::ImageView,
    sampler: vk::Sampler,
}
fn error(result: vk::Result) -> io::Error {
    io::Error::other(format!("Vulkan error {}", result.as_raw()))
}

impl<'device> VulkanCameraImage<'device> {
    /// Imports the frame as an optimal-tiled, sampled 2D image without mapping its pixels.
    ///
    /// # Safety
    /// `device` must belong to `instance`, support Vulkan 1.1, and have enabled
    /// VK_ANDROID_external_memory_android_hardware_buffer, VK_EXT_queue_family_foreign,
    /// and samplerYcbcrConversion. The caller must keep the device alive and finish GPU use
    /// (including the release barrier) before dropping this object and its camera lease.
    pub unsafe fn import(
        instance: &Instance,
        device: &'device Device,
        frame: CameraBufferFrame,
    ) -> io::Result<Self> {
        let mut owner = Self {
            device,
            frame,
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            conversion: vk::SamplerYcbcrConversion::null(),
            view: vk::ImageView::null(),
            sampler: vk::Sampler::null(),
        };
        let buffer = owner.frame.buffer();
        let size = buffer.describe();
        let ptr = buffer.as_ptr().cast();
        let extension = external_memory_android_hardware_buffer::Device::new(instance, device);
        let mut info = vk::AndroidHardwareBufferFormatPropertiesANDROID::default();
        let mut props = vk::AndroidHardwareBufferPropertiesANDROID::default().push_next(&mut info);
        unsafe { extension.get_android_hardware_buffer_properties(ptr, &mut props) }
            .map_err(error)?;
        let bits = props.memory_type_bits;
        let allocation_size = props.allocation_size;
        if bits == 0
            || !info
                .format_features
                .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE)
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "buffer has no compatible sampled memory",
            ));
        }
        let opaque = info.format == vk::Format::UNDEFINED;
        if opaque && info.external_format == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "camera buffer has no Vulkan format",
            ));
        }
        let format = info.format;
        let mut external =
            vk::ExternalFormatANDROID::default().external_format(info.external_format);
        let mut memory = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::ANDROID_HARDWARE_BUFFER_ANDROID);
        let mut image = vk::ImageCreateInfo::default()
            .push_next(&mut memory)
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: size.width,
                height: size.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        if opaque {
            image = image.push_next(&mut external);
        }
        owner.image = unsafe { device.create_image(&image, None) }.map_err(error)?;
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(owner.image);
        let mut import = vk::ImportAndroidHardwareBufferInfoANDROID::default().buffer(ptr);
        let allocation = vk::MemoryAllocateInfo::default()
            .allocation_size(allocation_size)
            .memory_type_index(bits.trailing_zeros())
            .push_next(&mut dedicated)
            .push_next(&mut import);
        owner.memory = unsafe { device.allocate_memory(&allocation, None) }.map_err(error)?;
        unsafe { device.bind_image_memory(owner.image, owner.memory, 0) }.map_err(error)?;
        // Standard multiplanar formats also need a conversion. RGBA formats can be sampled directly.
        let ycbcr = opaque
            || info.format_features.intersects(
                vk::FormatFeatureFlags::COSITED_CHROMA_SAMPLES
                    | vk::FormatFeatureFlags::MIDPOINT_CHROMA_SAMPLES,
            );
        if ycbcr {
            let mut external =
                vk::ExternalFormatANDROID::default().external_format(info.external_format);
            let mut request = vk::SamplerYcbcrConversionCreateInfo::default()
                .format(format)
                .ycbcr_model(info.suggested_ycbcr_model)
                .ycbcr_range(info.suggested_ycbcr_range)
                .components(info.sampler_ycbcr_conversion_components)
                .x_chroma_offset(info.suggested_x_chroma_offset)
                .y_chroma_offset(info.suggested_y_chroma_offset)
                .chroma_filter(vk::Filter::NEAREST);
            if opaque {
                request = request.push_next(&mut external);
            }
            owner.conversion =
                unsafe { device.create_sampler_ycbcr_conversion(&request, None) }.map_err(error)?;
        }
        let mut conversion = vk::SamplerYcbcrConversionInfo::default().conversion(owner.conversion);
        let mut view = vk::ImageViewCreateInfo::default()
            .image(owner.image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(Self::range());
        if ycbcr {
            view = view.push_next(&mut conversion);
        }
        owner.view = unsafe { device.create_image_view(&view, None) }.map_err(error)?;
        let mut conversion = vk::SamplerYcbcrConversionInfo::default().conversion(owner.conversion);
        let mut sampler = vk::SamplerCreateInfo::default()
            .min_filter(vk::Filter::NEAREST)
            .mag_filter(vk::Filter::NEAREST)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
        if ycbcr {
            sampler = sampler.push_next(&mut conversion);
        }
        owner.sampler = unsafe { device.create_sampler(&sampler, None) }.map_err(error)?;
        Ok(owner)
    }

    fn range() -> vk::ImageSubresourceRange {
        vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1)
    }
    /// Sampled view, used with this frame's immutable combined image sampler.
    pub fn view(&self) -> vk::ImageView {
        self.view
    }
    /// Use as an immutable sampler in the descriptor set layout for `view()`.
    pub fn sampler(&self) -> vk::Sampler {
        self.sampler
    }
    /// Camera timestamp in nanoseconds.
    pub fn timestamp_ns(&self) -> i64 {
        self.frame.timestamp_ns()
    }
    /// Records the foreign-family acquisition before the first shader read.
    ///
    /// # Safety
    /// Record in an active command buffer for the given family, after acquiring this ready frame.
    pub unsafe fn acquire(&self, command: vk::CommandBuffer, family: u32) {
        let barrier = vk::ImageMemoryBarrier::default()
            .image(self.image)
            .subresource_range(Self::range())
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .dst_queue_family_index(family)
            .dst_access_mask(vk::AccessFlags::SHADER_READ);
        unsafe {
            self.device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
        }
    }
    /// Records the matching release back to the camera's buffer queue.
    ///
    /// # Safety
    /// Record after the last use on the acquiring queue and wait for submission completion before drop.
    pub unsafe fn release(&self, command: vk::CommandBuffer, family: u32) {
        let barrier = vk::ImageMemoryBarrier::default()
            .image(self.image)
            .subresource_range(Self::range())
            .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_queue_family_index(family)
            .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .src_access_mask(vk::AccessFlags::SHADER_READ);
        unsafe {
            self.device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
        }
    }
}
impl Drop for VulkanCameraImage<'_> {
    fn drop(&mut self) {
        // Vulkan accepts null handles. This also unwinds partially initialized imports.
        unsafe {
            self.device.destroy_sampler(self.sampler, None);
            self.device.destroy_image_view(self.view, None);
            self.device
                .destroy_sampler_ycbcr_conversion(self.conversion, None);
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
        }
    }
}
