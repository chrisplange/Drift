use std::borrow::Cow;
use std::mem::size_of;
use std::num::NonZeroU64;

use crate::frame::{ArcInstance, FillVertex, Frame, Globals, SegmentInstance};

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

        let pipeline = |label: &str,
                        vs: &str,
                        fs: &str,
                        topology: wgpu::PrimitiveTopology,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
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
        }
    }

    /// Upload a frame's data to the GPU.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&frame.globals));
        self.segments.upload(device, queue, &frame.segments);
        self.arcs.upload(device, queue, &frame.arcs);
        self.under_segments
            .upload(device, queue, &frame.under_segments);
        self.under_arcs.upload(device, queue, &frame.under_arcs);
        self.fills.upload(device, queue, &frame.fills);
    }

    /// Record draw calls. The render pass viewport must already cover the canvas.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind_group, &[]);

        pass.set_pipeline(&self.grid_pipeline);
        pass.draw(0..3, 0..1);

        if self.fills.count > 0 {
            pass.set_pipeline(&self.fill_pipeline);
            pass.set_vertex_buffer(0, self.fills.buffer.slice(..self.fills.bytes));
            pass.draw(0..self.fills.count, 0..1);
        }

        // Curves under the sketch (hatch lines, underlays) first. Then arcs before lines so
        // line ends draw over arc ends; both are anti-aliased, so that order is cosmetic.
        for (pipeline, buffer) in [
            (&self.arc_pipeline, &self.under_arcs),
            (&self.segment_pipeline, &self.under_segments),
            (&self.arc_pipeline, &self.arcs),
            (&self.segment_pipeline, &self.segments),
        ] {
            if buffer.count > 0 {
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(0, buffer.buffer.slice(..buffer.bytes));
                pass.draw(0..4, 0..buffer.count);
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
        if len > self.capacity {
            self.capacity = len.next_power_of_two();
            self.buffer = Self::create(device, self.label, self.capacity);
        }
        if len > 0 {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
        self.bytes = len;
        self.count = data.len() as u32;
    }
}
