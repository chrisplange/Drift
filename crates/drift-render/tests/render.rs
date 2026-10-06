//! Renders the demo drawing offscreen on a real GPU adapter and checks pixels.
//!
//! The rendered image is written to `target/tmp/` for visual inspection. If no adapter is
//! available (e.g. a headless CI machine without a software rasteriser) the test is skipped.

use drift_core::{DVec2, Shape, SketchItem, demo};
use drift_render::{Camera, CanvasRenderer, Frame, Palette, Viewport, build_frame};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 640;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn gpu() -> Option<Gpu> {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
    eprintln!("render test adapter: {:?}", adapter.get_info());
    Some(Gpu { device, queue })
}

/// Render `frame` and return tightly packed RGBA8 pixels.
fn render(gpu: &Gpu, frame: &Frame) -> Vec<u8> {
    let Gpu { device, queue } = gpu;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("render_test_target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bytes_per_row = WIDTH * 4; // 3840, a multiple of 256
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render_test_readback"),
        size: u64::from(bytes_per_row * HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = CanvasRenderer::new(device, FORMAT);
    renderer.prepare(device, queue, frame);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render_test_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        renderer.paint(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll failed");
    slice.get_mapped_range().expect("mapped range").to_vec()
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * WIDTH + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn close(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
    a.iter()
        .zip(b)
        .take(3)
        .all(|(x, y)| x.abs_diff(y) <= tolerance)
}

fn save_png(name: &str, pixels: &[u8]) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(name);
    let file = std::fs::File::create(&path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), WIDTH, HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(pixels))
        .expect("write png");
    eprintln!("wrote {}", path.display());
}

fn viewport() -> Viewport {
    Viewport {
        size_px: DVec2::new(f64::from(WIDTH), f64::from(HEIGHT)),
        pixels_per_point: 1.0,
        srgb_target: false,
    }
}

/// Pixel coordinates (top-left origin) of a world point.
fn to_pixel(camera: &Camera, p: DVec2) -> (u32, u32) {
    let v = camera.world_to_view(p) + DVec2::new(f64::from(WIDTH), f64::from(HEIGHT)) * 0.5;
    (v.x.floor() as u32, v.y.floor() as u32)
}

#[test]
fn renders_demo_plate() {
    let Some(gpu) = gpu() else {
        eprintln!("no GPU adapter available; skipping render test");
        return;
    };
    let items = demo::mounting_plate();
    let bounds = items.iter().map(|i| i.shape.bounds()).collect();
    let mut camera = Camera::default();
    camera.fit(bounds, viewport().size_px, 40.0);

    let palette = Palette::dark();
    let pixels = render(&gpu, &build_frame(&items, &camera, viewport(), &palette));
    save_png("drift_demo_plate.png", &pixels);

    // The bottom edge of the plate at x = 60 mm is a solid stroke.
    let (x, y) = to_pixel(&camera, DVec2::new(60.0, 0.0));
    let on_edge =
        (y.saturating_sub(1)..=y + 1).any(|y| close(pixel(&pixels, x, y), palette.solid, 40));
    assert!(on_edge, "expected stroke at ({x}, {y})");

    // The middle of the bore at a non-grid position is empty canvas.
    let (x, y) = to_pixel(&camera, DVec2::new(73.3, 43.7));
    let inside = pixel(&pixels, x, y);
    assert!(
        close(inside, palette.background, 24),
        "expected background at ({x}, {y}), got {inside:?}"
    );

    // The bore centre is drawn as a point.
    let (x, y) = to_pixel(&camera, DVec2::new(70.0, 40.0));
    assert!(close(pixel(&pixels, x, y), palette.point, 40));
}

#[test]
fn renders_precisely_far_from_origin() {
    let Some(gpu) = gpu() else {
        eprintln!("no GPU adapter available; skipping render test");
        return;
    };
    // A 10 µm-radius circle one kilometre from the origin, zoomed so it is 200 px across.
    let center = DVec2::new(1.0e6, 1.0e6);
    let radius = 0.01;
    let items = [SketchItem::new(Shape::Circle { center, radius })];
    let camera = Camera {
        center,
        scale: 10_000.0,
    };
    let palette = Palette::dark();
    let pixels = render(&gpu, &build_frame(&items, &camera, viewport(), &palette));
    save_png("drift_far_circle.png", &pixels);

    // Sample around the circle: every point at exactly the radius must be stroked, and points
    // 3 px inside must not be. Any f32 precision loss would smear or shift the circle.
    for i in 0..16 {
        let angle = f64::from(i) * std::f64::consts::TAU / 16.0;
        let dir = DVec2::from_angle(angle);
        let on = camera.world_to_view(center + dir * radius)
            + DVec2::new(f64::from(WIDTH), f64::from(HEIGHT)) * 0.5;
        let inside = on - DVec2::new(dir.x, -dir.y) * 3.0;
        let on_px = pixel(&pixels, on.x.floor() as u32, on.y.floor() as u32);
        let in_px = pixel(&pixels, inside.x.floor() as u32, inside.y.floor() as u32);
        assert!(
            !close(on_px, palette.background, 40),
            "angle {angle}: no stroke on the circle, got {on_px:?}"
        );
        assert!(
            !close(in_px, palette.solid, 60),
            "angle {angle}: stroke leaked inside the circle, got {in_px:?}"
        );
    }
}
