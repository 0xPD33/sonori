use image::{DynamicImage, GenericImageView};
use std::path::Path;
use wgpu::{self, Device, Queue, TextureFormat};

#[derive(Debug, Clone)]
pub struct ButtonTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
}

impl ButtonTexture {
    /// Create a new button texture from an image file
    pub fn from_file(
        device: &Device,
        queue: &Queue,
        path: &Path,
        label: Option<&str>,
        format: TextureFormat,
    ) -> Result<Self, String> {
        let img = match image::open(path) {
            Ok(img) => img,
            Err(e) => return Err(format!("Failed to load image: {}", e)),
        };

        Self::from_image(device, queue, &img, label, format)
    }

    /// Create a texture from bytes (useful for embedded resources)
    pub fn from_bytes(
        device: &Device,
        queue: &Queue,
        bytes: &[u8],
        label: Option<&str>,
        format: TextureFormat,
    ) -> Result<Self, String> {
        let img = match image::load_from_memory(bytes) {
            Ok(img) => img,
            Err(e) => return Err(format!("Failed to load image from bytes: {}", e)),
        };

        Self::from_image(device, queue, &img, label, format)
    }

    /// Create a texture from an image in memory
    pub fn from_image(
        device: &Device,
        queue: &Queue,
        img: &DynamicImage,
        label: Option<&str>,
        _format: TextureFormat,
    ) -> Result<Self, String> {
        // Convert image to RGBA
        let rgba = img.to_rgba8();
        let dimensions = img.dimensions();

        // Create texture
        let size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Upload the image data to the texture
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * dimensions.0),
                rows_per_image: Some(dimensions.1),
            },
            size,
        );

        // Create texture view
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Create sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                // Texture binding
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                // Sampler binding
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("button_texture_bind_group_layout"),
        });

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
            label: Some("button_texture_bind_group"),
        });

        Ok(Self {
            texture,
            view,
            sampler,
            bind_group,
            bind_group_layout,
        })
    }

    /// Create a default texture for testing
    pub fn create_default(
        device: &Device,
        queue: &Queue,
        _format: TextureFormat,
    ) -> Result<Self, String> {
        // Create a simple 32x32 checkerboard pattern
        let size = 24;
        let mut rgba = Vec::with_capacity((size * size * 4) as usize);

        for y in 0..size {
            for x in 0..size {
                let is_checkerboard = (x / 6 + y / 6) % 2 == 0;

                if is_checkerboard {
                    // Light color (for the checkerboard pattern)
                    rgba.push(200); // R
                    rgba.push(200); // G
                    rgba.push(200); // B
                    rgba.push(255); // A
                } else {
                    // Dark color
                    rgba.push(100);
                    rgba.push(100);
                    rgba.push(100);
                    rgba.push(255);
                }
            }
        }

        // Create texture with the checkerboard pattern
        let texture_size = wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Default Button Texture"),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * size),
                rows_per_image: Some(size),
            },
            texture_size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("button_texture_bind_group_layout"),
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
            label: Some("button_texture_bind_group"),
        });

        Ok(Self {
            texture,
            view,
            sampler,
            bind_group,
            bind_group_layout,
        })
    }
}
