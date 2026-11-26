//! Samples real Camera2/NDK producer frames on a separate Vulkan compute device.
use super::Result;
use ash::{Device, Entry, Instance, vk};
use serge_native_sys::{
    AndroidSystem,
    hw::{CameraBufferFrame, CameraBufferReader, CameraTexture, vulkan_camera::VulkanCameraImage},
};
use std::{
    ffi::CStr,
    time::{Duration, Instant},
};

struct Gpu {
    _entry: Entry,
    instance: Instance,
    physical: vk::PhysicalDevice,
    device: Device,
    queue: vk::Queue,
    family: u32,
}
impl Gpu {
    fn open() -> Result<Option<Self>> {
        unsafe {
            let entry = Entry::load()?;
            let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
            let instance = entry.create_instance(
                &vk::InstanceCreateInfo::default().application_info(&app),
                None,
            )?;
            let required = [
                ash::android::external_memory_android_hardware_buffer::NAME,
                ash::ext::queue_family_foreign::NAME,
            ];
            for physical in instance.enumerate_physical_devices()? {
                if instance
                    .get_physical_device_properties(physical)
                    .api_version
                    < vk::API_VERSION_1_1
                {
                    continue;
                }
                let extensions = instance.enumerate_device_extension_properties(physical)?;
                if !required.iter().all(|name| {
                    extensions
                        .iter()
                        .any(|e| e.extension_name_as_c_str().ok() == Some(*name))
                }) {
                    continue;
                }
                let mut ycbcr = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default();
                instance.get_physical_device_features2(
                    physical,
                    &mut vk::PhysicalDeviceFeatures2::default().push_next(&mut ycbcr),
                );
                if ycbcr.sampler_ycbcr_conversion == 0 {
                    continue;
                }
                let Some(family) = instance
                    .get_physical_device_queue_family_properties(physical)
                    .iter()
                    .position(|q| q.queue_flags.contains(vk::QueueFlags::COMPUTE))
                else {
                    continue;
                };
                let family = family as u32;
                let queues = [vk::DeviceQueueCreateInfo::default()
                    .queue_family_index(family)
                    .queue_priorities(&[1.0])];
                let names = required.map(CStr::as_ptr);
                let create = vk::DeviceCreateInfo::default()
                    .push_next(&mut ycbcr)
                    .queue_create_infos(&queues)
                    .enabled_extension_names(&names);
                let device = instance.create_device(physical, &create, None)?;
                let queue = device.get_device_queue(family, 0);
                return Ok(Some(Self {
                    _entry: entry,
                    instance,
                    physical,
                    device,
                    queue,
                    family,
                }));
            }
            instance.destroy_instance(None);
            Ok(None)
        }
    }
    fn sample(&self, frame: CameraBufferFrame) -> Result<[u32; 64]> {
        let d = &self.device;
        unsafe {
            let image = VulkanCameraImage::import(&self.instance, d, frame)?;
            let mut r = Dispatch::new(d);
            let samplers = [image.sampler()];
            let bindings = [
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .immutable_samplers(&samplers)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE),
            ];
            r.set_layout = d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?;
            let layouts = [r.set_layout];
            r.layout = d.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts),
                None,
            )?;
            let source = include_bytes!("sample_camera.spv");
            let code = ash::util::read_spv(&mut std::io::Cursor::new(source.as_slice()))?;
            r.shader =
                d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)?;
            let stage = vk::PipelineShaderStageCreateInfo::default()
                .name(c"main")
                .module(r.shader)
                .stage(vk::ShaderStageFlags::COMPUTE);
            r.pipeline = d
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[vk::ComputePipelineCreateInfo::default()
                        .layout(r.layout)
                        .stage(stage)],
                    None,
                )
                .map_err(|(_, err)| err)?[0];

            r.buffer = d.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(256)
                    .usage(vk::BufferUsageFlags::STORAGE_BUFFER),
                None,
            )?;
            let requirements = d.get_buffer_memory_requirements(r.buffer);
            let memory = self
                .instance
                .get_physical_device_memory_properties(self.physical);
            let index = (0..memory.memory_type_count)
                .find(|i| {
                    (requirements.memory_type_bits & (1 << i)) != 0
                        && memory.memory_types[*i as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::HOST_VISIBLE)
                })
                .ok_or("no readback memory")?;
            r.memory = d.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(requirements.size)
                    .memory_type_index(index),
                None,
            )?;
            d.bind_buffer_memory(r.buffer, r.memory, 0)?;
            let sizes = [
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                    descriptor_count: 16,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::STORAGE_BUFFER,
                    descriptor_count: 1,
                },
            ];
            r.pool = d.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&sizes),
                None,
            )?;
            let set = d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(r.pool)
                    .set_layouts(&layouts),
            )?[0];
            let source = [vk::DescriptorImageInfo::default()
                .image_view(image.view())
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let dest = [vk::DescriptorBufferInfo::default()
                .buffer(r.buffer)
                .range(256)];
            d.update_descriptor_sets(
                &[
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&source),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .buffer_info(&dest),
                ],
                &[],
            );
            r.commands = d.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(self.family),
                None,
            )?;
            let command = d.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(r.commands)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
            d.begin_command_buffer(command, &vk::CommandBufferBeginInfo::default())?;
            image.acquire(command, self.family);
            d.cmd_bind_pipeline(command, vk::PipelineBindPoint::COMPUTE, r.pipeline);
            d.cmd_bind_descriptor_sets(
                command,
                vk::PipelineBindPoint::COMPUTE,
                r.layout,
                0,
                &[set],
                &[],
            );
            d.cmd_dispatch(command, 1, 1, 1);
            let readback = vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::HOST_READ);
            d.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[readback],
                &[],
                &[],
            );
            image.release(command, self.family);
            d.end_command_buffer(command)?;
            let commands = [command];
            d.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(&commands)],
                vk::Fence::null(),
            )?;
            d.queue_wait_idle(self.queue)?;
            let data = d.map_memory(r.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())?;
            d.invalidate_mapped_memory_ranges(&[vk::MappedMemoryRange::default()
                .memory(r.memory)
                .size(vk::WHOLE_SIZE)])?;
            let pixels = data.cast::<[u32; 64]>().read();
            d.unmap_memory(r.memory);
            Ok(pixels)
        }
    }
}
impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

// Own all temporary GPU objects, including on a partial test setup failure.
struct Dispatch<'a> {
    d: &'a Device,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    shader: vk::ShaderModule,
    pipeline: vk::Pipeline,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    pool: vk::DescriptorPool,
    commands: vk::CommandPool,
}
impl<'a> Dispatch<'a> {
    fn new(d: &'a Device) -> Self {
        Self {
            d,
            set_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            shader: vk::ShaderModule::null(),
            pipeline: vk::Pipeline::null(),
            buffer: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
            pool: vk::DescriptorPool::null(),
            commands: vk::CommandPool::null(),
        }
    }
}
impl Drop for Dispatch<'_> {
    fn drop(&mut self) {
        unsafe {
            self.d.destroy_command_pool(self.commands, None);
            self.d.destroy_descriptor_pool(self.pool, None);
            self.d.destroy_pipeline(self.pipeline, None);
            self.d.destroy_shader_module(self.shader, None);
            self.d.destroy_pipeline_layout(self.layout, None);
            self.d.destroy_descriptor_set_layout(self.set_layout, None);
            self.d.destroy_buffer(self.buffer, None);
            self.d.free_memory(self.memory, None);
        }
    }
}

pub fn run(system: &AndroidSystem) -> Result<()> {
    let Some(device) = Gpu::open()? else {
        if option_env!("SERGE_REQUIRE_VULKAN_CAMERA").is_some() {
            return Err("required Vulkan camera extensions unavailable".into());
        }
        println!("SERGE_VULKAN_CAMERA_UNAVAILABLE");
        return Ok(());
    };
    let cameras = CameraTexture::cameras(system)?;
    let camera = cameras.first().ok_or("no camera")?;
    let sizes = CameraTexture::sizes(system, camera)?;
    let size = sizes
        .iter()
        .min_by_key(|(w, h)| w * h)
        .copied()
        .ok_or("camera has no private output sizes")?;
    let reader = CameraBufferReader::open(system, camera, size)?;
    let until = Instant::now() + Duration::from_secs(8);
    let mut previous = None;
    loop {
        if let Some(frame) = reader.try_frame()? {
            let timestamp = frame.timestamp_ns();
            let pixels = device.sample(frame)?;
            if !pixels.iter().all(|pixel| pixel >> 24 == 255)
                || pixels.iter().all(|pixel| pixel == &pixels[0])
            {
                return Err(
                    format!("sampled camera has no opaque scene detail: {pixels:x?}").into(),
                );
            }
            if previous.is_some_and(|t| timestamp > t) {
                break;
            }
            previous = Some(timestamp);
        }
        if Instant::now() > until {
            return Err("Vulkan camera timed out".into());
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    println!("SERGE_VULKAN_CAMERA_SAMPLED");
    Ok(())
}
