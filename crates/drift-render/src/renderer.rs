use std::borrow::Cow;
use std::collections::HashMap;
use std::mem::size_of;
use std::num::NonZeroU64;
use std::ops::Range;
use std::sync::Arc;

use crate::frame::{
    ArcInstance, FillVertex, Frame, Globals, GroupRanges, ImageVertex, SegmentInstance,
};

/// GPU resources for drawing the canvas. Create once, then [`prepare`](Self::prepare) and
/// [`paint`](Self::paint) every frame.
pub struct CanvasRenderer {
    grid_pipeline: wgpu::RenderPipeline,
    fill_pipeline: wgpu::RenderPipeline,
    segment_pipeline: wgpu::RenderPipeline,
    arc_pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    segments: InstanceBuffer,
    arcs: InstanceBuffer,
    under_segments: InstanceBuffer,
    under_arcs: InstanceBuffer,
    fills: InstanceBuffer,
    image_pipeline: wgpu::RenderPipeline,
    image_layout: wgpu::BindGroupLayout,
    image_sampler: wgpu::Sampler,
    image_vertices: InstanceBuffer,
    /// Pictures on the GPU, by their pixels (whose buffer each holds, so it can't be freed
    /// and another take its place while cached).
    textures: HashMap<usize, CachedTexture>,
    /// This frame's pictures: which texture, and which of `image_vertices`.
    image_draws: Vec<(usize, Range<u32>)>,
    /// This frame's draw groups (see [`Frame::groups`]).
    groups: Vec<GroupRanges>,
}

struct CachedTexture {
    _pixels: Arc<Vec<u8>>,
    bind_group: wgpu::BindGroup,
    used: bool,
}

impl CanvasRenderer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("drift_canvas"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("canvas.wgsl"))),
        });

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("drift_globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("drift_globals_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(size_of::<Globals>() as u64),
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("drift_globals_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("drift_canvas_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        // Pictures also take their texture and sampler.
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("drift_image_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        });
        let image_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("drift_image_pipeline_layout"),
                bind_group_layouts: &[Some(&bind_group_layout), Some(&image_layout)],
                immediate_size: 0,
            });
        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("drift_image_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let pipeline_in =
            |layout: &wgpu::PipelineLayout,
             label: &str,
             vs: &str,
             fs: &str,
             topology: wgpu::PrimitiveTopology,
             buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some(vs),
                        buffers,
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    primitive: wgpu::PrimitiveState {
                        topology,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(fs),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: target_format,
                            blend: Some(PREMULTIPLIED_BLEND),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
        let pipeline = |label: &str,
                        vs: &str,
                        fs: &str,
                        topology: wgpu::PrimitiveTopology,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
            pipeline_in(&layout, label, vs, fs, topology, buffers)
        };
        let image_pipeline = pipeline_in(
            &image_pipeline_layout,
            "drift_images",
            "vs_image",
            "fs_image",
            wgpu::PrimitiveTopology::TriangleList,
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<ImageVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32],
            })],
        );

        let grid_pipeline = pipeline(
            "drift_grid",
            "vs_grid",
            "fs_grid",
            wgpu::PrimitiveTopology::TriangleList,
            &[],
        );
        let fill_pipeline = pipeline(
            "drift_fills",
            "vs_fill",
            "fs_fill",
            wgpu::PrimitiveTopology::TriangleList,
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<FillVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Unorm8x4],
            })],
        );
        let segment_pipeline = pipeline(
            "drift_segments",
            "vs_segment",
            "fs_segment",
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<SegmentInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4, 3 => Float32, 4 => Float32x4
                ],
            })],
        );
        let arc_pipeline = pipeline(
            "drift_arcs",
            "vs_arc",
            "fs_arc",
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<ArcInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x2, 1 => Float32x4, 2 => Unorm8x4, 3 => Float32x4
                ],
            })],
        );

        Self {
            grid_pipeline,
            fill_pipeline,
            segment_pipeline,
            arc_pipeline,
            globals,
            bind_group,
            segments: InstanceBuffer::new(device, "drift_segment_instances"),
            arcs: InstanceBuffer::new(device, "drift_arc_instances"),
            under_segments: InstanceBuffer::new(device, "drift_under_segment_instances"),
            under_arcs: InstanceBuffer::new(device, "drift_under_arc_instances"),
            fills: InstanceBuffer::new(device, "drift_fill_vertices"),
            image_pipeline,
            image_layout,
            image_sampler,
            image_vertices: InstanceBuffer::new(device, "drift_image_vertices"),
            textures: HashMap::new(),
            image_draws: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// Upload a frame's data to the GPU.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&frame.globals));
        self.groups.clone_from(&frame.groups);
        self.segments.upload(device, queue, &frame.segments);
        self.arcs.upload(device, queue, &frame.arcs);
        self.under_segments
            .upload(device, queue, &frame.under_segments);
        self.under_arcs.upload(device, queue, &frame.under_arcs);
        self.fills.upload(device, queue, &frame.fills);
        // Pictures: their textures made once and kept while they're drawn.
        for t in self.textures.values_mut() {
            t.used = false;
        }
        let mut vertices: Vec<ImageVertex> = Vec::new();
        self.image_draws.clear();
        let max_side = device.limits().max_texture_dimension_2d;
        for image in &frame.images {
            let key = Arc::as_ptr(&image.pixels) as usize;
            if !self.textures.contains_key(&key) {
                let Some(bind_group) = self.upload_texture(device, queue, image, max_side) else {
                    continue;
                };
                self.textures.insert(
                    key,
                    CachedTexture {
                        _pixels: image.pixels.clone(),
                        bind_group,
                        used: false,
                    },
                );
            }
            if let Some(t) = self.textures.get_mut(&key) {
                t.used = true;
            }
            let start = vertices.len() as u32;
            vertices.extend_from_slice(&image.vertices);
            self.image_draws.push((key, start..vertices.len() as u32));
        }
        self.textures.retain(|_, t| t.used);
        self.image_vertices.upload(device, queue, &vertices);
    }

    /// A picture's texture, premultiplied, with its mipmaps (made on the CPU: WebGL2 can't
    /// make them), from the largest level the device takes.
    fn upload_texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &crate::frame::FrameImage,
        max_side: u32,
    ) -> Option<wgpu::BindGroup> {
        let (w, h) = (image.width, image.height);
        if image.pixels.len() < (w as usize) * (h as usize) * 4 || w == 0 || h == 0 {
            return None;
        }
        let mut level: Vec<u8> = image.pixels[..(w as usize) * (h as usize) * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|c| {
                let a = u16::from(c[3]);
                let m = |v: u8| ((u16::from(v) * a + 127) / 255) as u8;
                [m(c[0]), m(c[1]), m(c[2]), c[3]]
            })
            .collect();
        let (mut lw, mut lh) = (w, h);
        let mut levels: Vec<(u32, u32, Vec<u8>)> = Vec::new();
        loop {
            if lw <= max_side && lh <= max_side {
                levels.push((lw, lh, level.clone()));
            }
            if lw == 1 && lh == 1 {
                break;
            }
            (level, lw, lh) = halve(&level, lw, lh);
        }
        let (bw, bh, _) = levels.first()?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("drift_image"),
            size: wgpu::Extent3d {
                width: *bw,
                height: *bh,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (mip, (lw, lh, data)) in levels.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(lw * 4),
                    rows_per_image: Some(*lh),
                },
                wgpu::Extent3d {
                    width: *lw,
                    height: *lh,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("drift_image_bind_group"),
            layout: &self.image_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                },
            ],
        }))
    }

    /// Record draw calls. The render pass viewport must already cover the canvas.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind_group, &[]);

        pass.set_pipeline(&self.grid_pipeline);
        pass.draw(0..3, 0..1);

        if self.groups.is_empty() {
            // Every fill under every curve.
            self.draw_fills(pass, 0..self.fills.count);
            self.draw_images(pass);
            self.draw_under(pass);
            self.draw_curves(pass, &self.arc_pipeline, &self.arcs, 0..self.arcs.count);
            self.draw_curves(
                pass,
                &self.segment_pipeline,
                &self.segments,
                0..self.segments.count,
            );
        } else {
            // Stacked: pictures and underlays at the bottom, then each group over the last.
            self.draw_images(pass);
            self.draw_under(pass);
            for g in &self.groups {
                self.draw_fills(pass, g.fills.clone());
                self.draw_curves(pass, &self.arc_pipeline, &self.arcs, g.arcs.clone());
                self.draw_curves(
                    pass,
                    &self.segment_pipeline,
                    &self.segments,
                    g.segments.clone(),
                );
            }
        }
    }

    fn draw_fills(&self, pass: &mut wgpu::RenderPass<'_>, range: Range<u32>) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.fill_pipeline);
        pass.set_vertex_buffer(0, self.fills.buffer.slice(..self.fills.bytes));
        pass.draw(range, 0..1);
    }

    /// Draw a range of a buffer's instances (arcs or segments) with `pipeline`.
    fn draw_curves(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: &wgpu::RenderPipeline,
        buffer: &InstanceBuffer,
        range: Range<u32>,
    ) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_vertex_buffer(0, buffer.buffer.slice(..buffer.bytes));
        pass.draw(0..4, range);
    }

    /// Curves under the sketch (underlays, and hatch lines when fills aren't stacked): arcs
    /// before lines so line ends draw over arc ends; both are anti-aliased, so that order
    /// is cosmetic.
    fn draw_under(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw_curves(
            pass,
            &self.arc_pipeline,
            &self.under_arcs,
            0..self.under_arcs.count,
        );
        self.draw_curves(
            pass,
            &self.segment_pipeline,
            &self.under_segments,
            0..self.under_segments.count,
        );
    }

    /// Pictures (underlays'), in the order given.
    fn draw_images(&self, pass: &mut wgpu::RenderPass<'_>) {
        if !self.image_draws.is_empty() {
            pass.set_pipeline(&self.image_pipeline);
            pass.set_vertex_buffer(
                0,
                self.image_vertices
                    .buffer
                    .slice(..self.image_vertices.bytes),
            );
            for (key, range) in &self.image_draws {
                if let Some(t) = self.textures.get(key) {
                    pass.set_bind_group(1, &t.bind_group, &[]);
                    pass.draw(range.clone(), 0..1);
                }
            }
        }
    }
}

const PREMULTIPLIED_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// The next mipmap level: half the size, rounded down (at least a pixel), as the GPU sizes
/// mip levels; each pixel the average of those it covers (an odd row or column's last
/// pixel takes in the one left over).
fn halve(data: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let (w, h, nw_, nh_) = (w as usize, h as usize, nw as usize, nh as usize);
    // The source pixels output pixel `i` of `n` covers, of `len`.
    let span =
        |i: usize, n: usize, len: usize| (i * len / n)..((i + 1) * len / n).max(i * len / n + 1);
    let mut out = vec![0u8; nw_ * nh_ * 4];
    for y in 0..nh_ {
        for x in 0..nw_ {
            let mut sum = [0u32; 4];
            let mut n = 0;
            for sy in span(y, nh_, h) {
                for sx in span(x, nw_, w) {
                    let at = (sy * w + sx) * 4;
                    for (s, v) in sum.iter_mut().zip(&data[at..at + 4]) {
                        *s += u32::from(*v);
                    }
                    n += 1;
                }
            }
            let at = (y * nw_ + x) * 4;
            for (o, s) in out[at..at + 4].iter_mut().zip(sum) {
                *o = ((s + n / 2) / n.max(1)) as u8;
            }
        }
    }
    (out, nw, nh)
}

/// A growable GPU vertex buffer of instances.
struct InstanceBuffer {
    label: &'static str,
    buffer: wgpu::Buffer,
    capacity: u64,
    bytes: u64,
    count: u32,
}

impl InstanceBuffer {
    const INITIAL_CAPACITY: u64 = 64 * 1024;

    fn new(device: &wgpu::Device, label: &'static str) -> Self {
        Self {
            label,
            buffer: Self::create(device, label, Self::INITIAL_CAPACITY),
            capacity: Self::INITIAL_CAPACITY,
            bytes: 0,
            count: 0,
        }
    }

    fn create(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn upload<T: bytemuck::Pod>(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[T]) {
        let bytes = bytemuck::cast_slice::<T, u8>(data);
        let len = bytes.len() as u64;
        // Grow to fit; shrink again when a peak (a huge underlay, say) has passed, so its
        // memory isn't held for the rest of the session.
        let roomy = self.capacity > Self::INITIAL_CAPACITY && len < self.capacity / 8;
        if len > self.capacity || roomy {
            self.capacity = len.next_power_of_two().max(Self::INITIAL_CAPACITY);
            self.buffer = Self::create(device, self.label, self.capacity);
        }
        if len > 0 {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
        self.bytes = len;
        self.count = data.len() as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::halve;

    #[test]
    fn mip_levels_are_sized_as_the_gpu_sizes_them() {
        // An odd, wide picture: as many levels as the GPU allows, each `size >> level`.
        let (mut w, mut h) = (3001u32, 1999u32);
        let mut data = vec![128u8; (w * h * 4) as usize];
        let mut levels = 1;
        while w > 1 || h > 1 {
            let (next, nw, nh) = halve(&data, w, h);
            assert_eq!(
                (nw, nh),
                ((3001u32 >> levels).max(1), (1999u32 >> levels).max(1))
            );
            assert_eq!(next.len(), (nw * nh * 4) as usize);
            assert!(next.iter().all(|v| *v == 128));
            (data, w, h) = (next, nw, nh);
            levels += 1;
        }
        // wgpu's limit: floor(log2(3001)) + 1.
        assert_eq!(levels, 12);
    }
}
