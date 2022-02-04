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
    pub async fn operation(&self) -> Operation {
        Operation::new(self).await
    }
}

pub struct Operation {
    device: Device,
    queue: Queue,
    texture: Texture,
    texture_size: Extent3d,
}

impl Operation {
    async fn new(image: &Image) -> Self {
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
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&TextureDescriptor {
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
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: std::num::NonZeroU32::new(4 * image.width),
                rows_per_image: std::num::NonZeroU32::new(image.height),
            },
            texture_size,
        );

        Self {
            device,
            queue,
            texture,
            texture_size,
        }
    }

    pub fn grayscale(self) -> Self {
        self.simple_filter("grayscale", GRAYSCALE_SHADER)
    }

    pub fn inverse(self) -> Self {
        self.simple_filter("inverse", INVERSE_SHADER)
    }

    pub fn hflip(self) -> Self {
        self.simple_filter("hflip", HFLIP_SHADER)
    }
    pub fn vflip(self) -> Self {
        self.simple_filter("vflip", VFLIP_SHADER)
    }

    fn simple_filter(mut self, name: &str, shader_string: &str) -> Self {
        let capitalized_filter_name = capitalize(name);

        println!(
            "Filter {} for image of dim {} x {}",
            name, self.texture_size.width, self.texture_size.height
        );

        let output_texture = self.device.create_texture(&TextureDescriptor {
            label: None,
            size: self.texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::STORAGE_BINDING,
        });

        let shader = self.device.create_shader_module(&ShaderModuleDescriptor {
            label: Some("Shader"),
            source: ShaderSource::Wgsl(shader_string.into()),
        });

        let image_info = self.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Image info"),
            contents: bytemuck::cast_slice(&[self.texture_size.width, self.texture_size.height]),
            usage: BufferUsages::UNIFORM,
        });

        let pipeline = self
            .device
            .create_compute_pipeline(&ComputePipelineDescriptor {
                label: Some(format!("{} pipeline", capitalized_filter_name).as_str()),
                layout: None,
                module: &shader,
                entry_point: "main",
            });

        let compute_constants = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Compute constants"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[BindGroupEntry {
                binding: 0,
                resource: image_info.as_entire_binding(),
            }],
        });

        let texture_bind_group = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Texture bind group"),
            layout: &pipeline.get_bind_group_layout(1),
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(
                        &self.texture.create_view(&TextureViewDescriptor::default()),
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

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });
        {
            let (dispatch_with, dispatch_height) = compute_work_group_count(
                (self.texture_size.width, self.texture_size.height),
                (16, 16),
            );
            println!("Dispatching {} x {}", dispatch_with, dispatch_height);
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some(format!("{} pass", capitalized_filter_name).as_str()),
            });
            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &compute_constants, &[]);
            compute_pass.set_bind_group(1, &texture_bind_group, &[]);
            compute_pass.dispatch(dispatch_with, dispatch_height, 1);
        }

        self.queue.submit(Some(encoder.finish()));
        self.texture = output_texture;

        self
    }

    pub async fn execute(self) -> Image {
        texture_to_cpu(
            &self.device,
            &self.queue,
            self.texture_size.width,
            self.texture_size.height,
            &self.texture,
        )
        .await
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
/// * `(width, height)` - The dimension of the image we are working on.
/// * `(workgroup_width, workgroup_height)` - The width and height dimensions of the compute workgroup.
fn compute_work_group_count(
    (width, height): (u32, u32),
    (workgroup_width, workgroup_height): (u32, u32),
) -> (u32, u32) {
    let width = (width + workgroup_width - 1) / workgroup_width;
    let height = (height + workgroup_height - 1) / workgroup_height;

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
    use crate::{compute_work_group_count, padded_bytes_per_row};

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
        let group_count = compute_work_group_count((100, 200), (32, 32));

        assert_eq!((4, 7), group_count);
    }
}
