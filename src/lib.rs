use wgpu::{
    util::{BufferInitDescriptor, DeviceExt},
    Backends, BindGroupDescriptor, BindGroupEntry, BindingResource, BufferDescriptor, BufferUsages,
    CommandEncoderDescriptor, ComputePassDescriptor, ComputePipelineDescriptor, Device, Extent3d,
    Instance, Queue, ShaderModuleDescriptor, ShaderSource, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
};

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub async fn grayscale(image: &Image) -> Image {
    println!(
        "Applying grayscale for image of dim {} x {}",
        image.width, image.height
    );
    let instance = Instance::new(Backends::all());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptionsBase {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: None,
        })
        .await
        .unwrap();
    let (device, queue) = adapter
        .request_device(&Default::default(), None)
        .await
        .unwrap();

    let texture_size = Extent3d {
        width: image.width,
        height: image.height,
        depth_or_array_layers: 1,
    };
    let input_texture = device.create_texture(&TextureDescriptor {
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        label: Some("texture"),
    });
    queue.write_texture(
        // Tells wgpu where to copy the pixel data
        wgpu::ImageCopyTexture {
            texture: &input_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        // The actual pixel data
        &image.pixels,
        // The layout of the texture
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: std::num::NonZeroU32::new(4 * image.width),
            rows_per_image: std::num::NonZeroU32::new(image.height),
        },
        texture_size,
    );
    let output_texture = device.create_texture(&TextureDescriptor {
        label: None,
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_SRC
            | TextureUsages::STORAGE_BINDING,
    });

    let shader = device.create_shader_module(&ShaderModuleDescriptor {
        label: Some("Shader"),
        source: ShaderSource::Wgsl(include_str!("shaders/grayscale.wgsl").into()),
    });

    let image_info = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("Image info"),
        contents: bytemuck::cast_slice(&[image.width, image.height]),
        usage: BufferUsages::UNIFORM,
    });

    let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: Some("Grayscale"),
        layout: None,
        module: &shader,
        entry_point: "main",
    });

    let compute_constants = device.create_bind_group(&BindGroupDescriptor {
        label: Some("Compute constants"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[BindGroupEntry {
            binding: 0,
            resource: image_info.as_entire_binding(),
        }],
    });

    let texture_bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("Texture bind group"),
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(
                    &input_texture.create_view(&TextureViewDescriptor::default()),
                ),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(
                    &output_texture.create_view(&TextureViewDescriptor::default()),
                ),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
    {
        let (dispatch_with, dispatch_height) = compute_thread_group_size(image, (16, 16));
        println!("Dispatching {} x {}", dispatch_with, dispatch_height);
        let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("Grayscale pass"),
        });
        compute_pass.set_pipeline(&pipeline);
        compute_pass.set_bind_group(0, &compute_constants, &[]);
        compute_pass.set_bind_group(1, &texture_bind_group, &[]);
        compute_pass.dispatch(dispatch_with, dispatch_height, 1);
    }

    queue.submit(Some(encoder.finish()));

    texture_to_cpu(&device, &queue, image.width, image.height, &output_texture).await
}

/// Only works with images whose width are a multiple of 256, which is lame.
///
async fn texture_to_cpu(
    device: &Device,
    queue: &Queue,
    width: u32,
    height: u32,
    texture: &Texture,
) -> Image {
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
    let texture_size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let output_buffer_size = width as u64 * height as u64 * std::mem::size_of::<u32>() as u64;
    let output_buffer = device.create_buffer(&BufferDescriptor {
        label: None,
        size: output_buffer_size,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            aspect: wgpu::TextureAspect::All,
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
        },
        wgpu::ImageCopyBuffer {
            buffer: &output_buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: std::num::NonZeroU32::new(4 * width),
                rows_per_image: std::num::NonZeroU32::new(height),
            },
        },
        texture_size,
    );
    queue.submit(Some(encoder.finish()));

    let buffer_slice = output_buffer.slice(..);
    let mapping = buffer_slice.map_async(wgpu::MapMode::Read);

    device.poll(wgpu::Maintain::Wait);
    mapping.await.unwrap();

    let data = buffer_slice.get_mapped_range();

    Image {
        width,
        height,
        pixels: data.to_vec(),
    }
}

fn compute_thread_group_size(image: &Image, workgroup_size: (u32, u32)) -> (u32, u32) {
    let width = (image.width + workgroup_size.0 - 1) / workgroup_size.0;
    let height = (image.height + workgroup_size.1 - 1) / workgroup_size.1;

    (width, height)
}
