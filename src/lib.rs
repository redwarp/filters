use wgpu::{
    util::{BufferInitDescriptor, DeviceExt},
    Backends, BindGroupDescriptor, BindGroupEntry, BindingResource, BufferDescriptor, BufferUsages,
    CommandEncoderDescriptor, ComputePassDescriptor, ComputePipelineDescriptor, Device, Extent3d,
    Instance, PowerPreference, Queue, ShaderModuleDescriptor, ShaderSource, Texture,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
};

const INVERSE_SHADER: &str = include_str!("shaders/inverse.wgsl");
const GRAYSCALE_SHADER: &str = include_str!("shaders/grayscale.wgsl");
const HFLIP_SHADER: &str = include_str!("shaders/hflip.wgsl");
const VFLIP_SHADER: &str = include_str!("shaders/vflip.wgsl");

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    pub async fn grayscale(&self) -> Image {
        self.simple_filter("grayscale", GRAYSCALE_SHADER).await
    }

    pub async fn inverse(&self) -> Image {
        self.simple_filter("inverse", INVERSE_SHADER).await
    }

    pub async fn hflip(&self) -> Image {
        self.simple_filter("hflip", HFLIP_SHADER).await
    }
    pub async fn vflip(&self) -> Image {
        self.simple_filter("vflip", VFLIP_SHADER).await
    }

    async fn simple_filter(&self, name: &str, shader_string: &str) -> Image {
        let captitalized_filter_name = capitalize(name);

        println!(
            "Filter {} for image of dim {} x {}",
            name, self.width, self.height
        );
        let instance = Instance::new(Backends::all());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptionsBase {
                power_preference: PowerPreference::HighPerformance,
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
            width: self.width,
            height: self.height,
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
            wgpu::ImageCopyTexture {
                texture: &input_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: std::num::NonZeroU32::new(4 * self.width),
                rows_per_image: std::num::NonZeroU32::new(self.height),
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
            source: ShaderSource::Wgsl(shader_string.into()),
        });

        let image_info = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Image info"),
            contents: bytemuck::cast_slice(&[self.width, self.height]),
            usage: BufferUsages::UNIFORM,
        });

        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some(format!("{} pipeline", captitalized_filter_name).as_str()),
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
            let (dispatch_with, dispatch_height) = compute_work_group_count(self, (16, 16));
            println!("Dispatching {} x {}", dispatch_with, dispatch_height);
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some(format!("{} pass", captitalized_filter_name).as_str()),
            });
            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &compute_constants, &[]);
            compute_pass.set_bind_group(1, &texture_bind_group, &[]);
            compute_pass.dispatch(dispatch_with, dispatch_height, 1);
        }

        queue.submit(Some(encoder.finish()));

        texture_to_cpu(&device, &queue, self.width, self.height, &output_texture).await
    }
}

/// Copies a texture from the gpu to the cpu. The tricky part here is that the encoder's method `copy_texture_to_buffer`
/// only works when the image copy buffer's bytes per row are a multiple of 256.
/// So this operation needs to happen in two faces: First, we copy to a buffer, padding the width so it's a multiple of 256.
/// Then, we copy the buffer to the final image, slice by slice, by ignoring the extra padded bits of the buffer.
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

    let padded_bytes_per_row = padded_bytes_per_row(width);
    let unpadded_bytes_per_row = width as usize * 4;

    let output_buffer_size =
        padded_bytes_per_row as u64 * height as u64 * std::mem::size_of::<u8>() as u64;
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
                bytes_per_row: std::num::NonZeroU32::new(padded_bytes_per_row as u32),
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

    let padded_data = buffer_slice.get_mapped_range();
    let mut pixels: Vec<u8> = vec![0; unpadded_bytes_per_row * height as usize];

    for (padded, pixels) in padded_data
        .chunks_exact(padded_bytes_per_row)
        .zip(pixels.chunks_exact_mut(unpadded_bytes_per_row))
    {
        pixels.copy_from_slice(&padded[..unpadded_bytes_per_row]);
    }

    Image {
        width,
        height,
        pixels,
    }
}

/// Compute the amount of work groups to be dispatched for an image, based on the work group size.
/// Chances are, the group will not match perfectly, like an image of width 100, for a workgroup size of 32.
/// To make sure the that the whole 100 pixels are visited, then we would need a count of 4, as 4 * 32 = 128,
/// which is bigger than 100. A count of 3 would be too little, as it means 96, so four columns (or, 100 - 96) would be ignored.
///
/// # Arguments
///
/// * `image` - The image we are working on.
/// * `workgroup_size` - The width and height dimensions of the compute workgroup.
fn compute_work_group_count(image: &Image, workgroup_size: (u32, u32)) -> (u32, u32) {
    let width = (image.width + workgroup_size.0 - 1) / workgroup_size.0;
    let height = (image.height + workgroup_size.1 - 1) / workgroup_size.1;

    (width, height)
}

/// Compute the next multiple of 256 for texture retrival padding.
fn padded_bytes_per_row(width: u32) -> usize {
    let bytes_per_row = width as usize * 4;
    let padding = (256 - bytes_per_row % 256) % 256;
    bytes_per_row + padding
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{compute_work_group_count, padded_bytes_per_row, Image};

    #[test]
    fn padded_bytes_per_row_width_4() {
        let padded = padded_bytes_per_row(4);

        assert_eq!(256, padded)
    }

    #[test]
    fn padded_bytes_per_row_width_64() {
        let padded = padded_bytes_per_row(64);

        assert_eq!(256, padded)
    }

    #[test]
    fn padded_bytes_per_row_width_65() {
        let padded = padded_bytes_per_row(65);

        assert_eq!(512, padded)
    }

    #[test]
    fn compute_work_group_count_100x200_group_32x32() {
        let image = Image {
            width: 100,
            height: 200,
            pixels: vec![],
        };

        let group_count = compute_work_group_count(&image, (32, 32));

        assert_eq!((4, 7), group_count);
    }
}
