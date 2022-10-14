use wgpu::util::DeviceExt;
use wgpu::BufferUsages;
use wgpu::{
    util::BufferInitDescriptor, BindGroupDescriptor, BindGroupEntry, BindingResource,
    CommandEncoderDescriptor, ComputePassDescriptor, ComputePipelineDescriptor,
    ShaderModuleDescriptor, ShaderSource, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, TextureViewDescriptor,
};

use crate::{capitalize, compute_work_group_count, Operation};

const BOX_BLUR_SHADER: &str = include_str!("shaders/box_blur.wgsl");
const GAUSSIAN_BLUR_SHADER: &str = include_str!("shaders/gaussian_blur.wgsl");

struct Kernel {
    sum: f32,
    values: Vec<f32>,
}

impl Kernel {
    fn new(values: Vec<f32>) -> Self {
        let sum = values.iter().sum();
        Self { sum, values }
    }

    fn packed_data(&self) -> Vec<f32> {
        let mut data = vec![0.0; self.values.len() + 1];
        data[0] = self.sum;
        data[1..].copy_from_slice(&self.values);
        data
    }

    fn size(&self) -> usize {
        self.values.len()
    }
}

impl<'a> Operation<'a> {
    pub fn box_blur(mut self, filter_size: u32) -> Self {
        let name = "box blur";
        let capitalized_filter_name = capitalize(name);

        let vertical_pass_texture = self.device.create_texture(&TextureDescriptor {
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
        let horizontal_pass_texture = self.device.create_texture(&TextureDescriptor {
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

        let shader = self.device.create_shader_module(ShaderModuleDescriptor {
            label: Some(format!("{} shader", capitalized_filter_name).as_str()),
            source: ShaderSource::Wgsl(BOX_BLUR_SHADER.into()),
        });

        let pipeline = self
            .device
            .create_compute_pipeline(&ComputePipelineDescriptor {
                label: Some(format!("{} pipeline", capitalized_filter_name).as_str()),
                layout: None,
                module: &shader,
                entry_point: "main",
            });

        let settings = self.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Image info"),
            contents: bytemuck::cast_slice(&[filter_size]),
            usage: BufferUsages::UNIFORM,
        });

        let compute_constants = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Compute constants"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[BindGroupEntry {
                binding: 0,
                resource: settings.as_entire_binding(),
            }],
        });

        let vertical = self.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Orientation"),
            contents: bytemuck::cast_slice::<u32, u8>(&[1]),
            usage: BufferUsages::UNIFORM,
        });
        let horizontal = self.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Orientation"),
            contents: bytemuck::cast_slice::<u32, u8>(&[0]),
            usage: BufferUsages::UNIFORM,
        });

        let vertical_bind_group = self.device.create_bind_group(&BindGroupDescriptor {
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
                        &vertical_pass_texture.create_view(&TextureViewDescriptor::default()),
                    ),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: vertical.as_entire_binding(),
                },
            ],
        });

        let horizontal_bind_group = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Texture bind group"),
            layout: &pipeline.get_bind_group_layout(1),
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(
                        &vertical_pass_texture.create_view(&TextureViewDescriptor::default()),
                    ),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(
                        &horizontal_pass_texture.create_view(&TextureViewDescriptor::default()),
                    ),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: horizontal.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });
        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some(format!("{} pass", capitalized_filter_name).as_str()),
            });
            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &compute_constants, &[]);
            compute_pass.set_bind_group(1, &vertical_bind_group, &[]);
            let (dispatch_with, dispatch_height) = compute_work_group_count(
                (self.texture_size.width, self.texture_size.height),
                (128, 1),
            );
            compute_pass.dispatch_workgroups(dispatch_with, dispatch_height, 1);
            compute_pass.set_bind_group(1, &horizontal_bind_group, &[]);
            let (dispatch_height, dispatch_with) = compute_work_group_count(
                (self.texture_size.width, self.texture_size.height),
                (1, 128),
            );
            compute_pass.dispatch_workgroups(dispatch_with, dispatch_height, 1);
        }

        self.queue.submit(Some(encoder.finish()));
        self.texture = horizontal_pass_texture;

        self
    }

    pub fn gaussian_blur(mut self, sigma: f32) -> Self {
        let name = "gaussian blur";
        let capitalized_filter_name = capitalize(name);

        let kernel = kernel(sigma);
        let kernel_size = kernel.size() as u32;

        let vertical_pass_texture = self.device.create_texture(&TextureDescriptor {
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
        let horizontal_pass_texture = self.device.create_texture(&TextureDescriptor {
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

        let shader = self.device.create_shader_module(ShaderModuleDescriptor {
            label: Some(format!("{} shader", capitalized_filter_name).as_str()),
            source: ShaderSource::Wgsl(GAUSSIAN_BLUR_SHADER.into()),
        });

        let pipeline = self
            .device
            .create_compute_pipeline(&ComputePipelineDescriptor {
                label: Some(format!("{} pipeline", capitalized_filter_name).as_str()),
                layout: None,
                module: &shader,
                entry_point: "main",
            });

        let settings = self.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Image info"),
            contents: bytemuck::cast_slice(&[kernel_size]),
            usage: BufferUsages::UNIFORM,
        });

        let kernel = self.device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&kernel.packed_data()[..]),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        });

        let compute_constants = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Compute constants"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: settings.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: kernel.as_entire_binding(),
                },
            ],
        });

        let vertical = self.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Orientation"),
            contents: bytemuck::cast_slice::<u32, u8>(&[1]),
            usage: BufferUsages::UNIFORM,
        });
        let horizontal = self.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Orientation"),
            contents: bytemuck::cast_slice::<u32, u8>(&[0]),
            usage: BufferUsages::UNIFORM,
        });

        let vertical_bind_group = self.device.create_bind_group(&BindGroupDescriptor {
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
                        &vertical_pass_texture.create_view(&TextureViewDescriptor::default()),
                    ),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: vertical.as_entire_binding(),
                },
            ],
        });

        let horizontal_bind_group = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Texture bind group"),
            layout: &pipeline.get_bind_group_layout(1),
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(
                        &vertical_pass_texture.create_view(&TextureViewDescriptor::default()),
                    ),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(
                        &horizontal_pass_texture.create_view(&TextureViewDescriptor::default()),
                    ),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: horizontal.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });
        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some(format!("{} pass", capitalized_filter_name).as_str()),
            });
            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &compute_constants, &[]);
            compute_pass.set_bind_group(1, &vertical_bind_group, &[]);
            let (dispatch_with, dispatch_height) = compute_work_group_count(
                (self.texture_size.width, self.texture_size.height),
                (128, 1),
            );
            compute_pass.dispatch_workgroups(dispatch_with, dispatch_height, 1);
            compute_pass.set_bind_group(1, &horizontal_bind_group, &[]);
            let (dispatch_height, dispatch_with) = compute_work_group_count(
                (self.texture_size.width, self.texture_size.height),
                (1, 128),
            );
            compute_pass.dispatch_workgroups(dispatch_with, dispatch_height, 1);
        }

        self.queue.submit(Some(encoder.finish()));
        self.texture = horizontal_pass_texture;

        self
    }
}

fn kernel_size_for_sigma(sigma: f32) -> u32 {
    2 * (sigma * 3.0).ceil() as u32 + 1
}

fn kernel(sigma: f32) -> Kernel {
    let kernel_size = kernel_size_for_sigma(sigma);
    let mut values = vec![0.0; kernel_size as usize];
    let kernel_radius = (kernel_size as usize - 1) / 2;
    for index in 0..=kernel_radius {
        let normpdf = normalized_probablility_density_function(index as f32, sigma);
        values[kernel_radius + index] = normpdf;
        values[kernel_radius - index] = normpdf;
    }

    Kernel::new(values)
}

fn normalized_probablility_density_function(x: f32, sigma: f32) -> f32 {
    0.39894 * (-0.5 * x * x / (sigma * sigma)).exp() / sigma
}

#[cfg(test)]
mod tests {
    use super::{kernel, kernel_size_for_sigma};

    #[test]
    fn kernel_size_sigma_2_dot_2() {
        let kernel_size = kernel_size_for_sigma(2.2);

        assert_eq!(15, kernel_size);
    }

    #[test]
    fn kernel_sigma_1_dot_2() {
        let kernel = kernel(1.2);

        assert_eq!(
            kernel.values,
            [
                0.0012852254,
                0.014606836,
                0.08289714,
                0.23492521,
                0.33244997,
                0.23492521,
                0.08289714,
                0.014606836,
                0.0012852254
            ]
        );

        assert_eq!(kernel.sum, 0.9998788);
    }
}
