//! Bridges the egui UI and [`drift_render::CanvasRenderer`] through an egui-wgpu paint callback.

use drift_core::{DVec2, SketchItem};
use drift_render::{Camera, CanvasRenderer, Frame, Palette, Viewport, build_frame};
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};

/// Creates per-frame canvas callbacks. The GPU renderer itself lives in egui-wgpu's callback
/// resources so it can be reached from the render thread.
/// How the canvas is drawn this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct CanvasOptions {
    pub dark_mode: bool,
    pub imperial_grid: bool,
    pub thin_lines: bool,
}

pub struct CanvasPainter {
    srgb_target: bool,
}

impl CanvasPainter {
    pub fn install(render_state: &egui_wgpu::RenderState) -> Self {
        let renderer = CanvasRenderer::new(&render_state.device, render_state.target_format);
        render_state
            .renderer
            .write()
            .callback_resources
            .insert(renderer);
        Self {
            srgb_target: render_state.target_format.is_srgb(),
        }
    }

    pub fn callback(
        &self,
        rect: egui::Rect,
        pixels_per_point: f64,
        items: &[SketchItem],
        camera: &Camera,
        options: CanvasOptions,
    ) -> egui::PaintCallback {
        let palette = if options.dark_mode {
            Palette::dark()
        } else {
            Palette::light()
        };
        let viewport = Viewport {
            size_px: DVec2::new(f64::from(rect.width()), f64::from(rect.height()))
                * pixels_per_point,
            pixels_per_point,
            srgb_target: self.srgb_target,
            imperial_grid: options.imperial_grid,
            thin_lines: options.thin_lines,
        };
        let frame = build_frame(items, camera, viewport, &palette);
        egui_wgpu::Callback::new_paint_callback(rect, CanvasCallback { frame })
    }
}

struct CanvasCallback {
    frame: Frame,
}

impl egui_wgpu::CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<CanvasRenderer>() {
            renderer.prepare(device, queue, &self.frame);
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(renderer) = resources.get::<CanvasRenderer>() else {
            return;
        };
        // egui sets an integer-rounded viewport. Replace it with the exact fractional canvas
        // rectangle so pixel positions computed on the CPU land exactly where intended.
        let ppp = info.pixels_per_point;
        let [screen_w, screen_h] = info.screen_size_px.map(|v| v as f32);
        let x = (info.viewport.min.x * ppp).clamp(0.0, screen_w);
        let y = (info.viewport.min.y * ppp).clamp(0.0, screen_h);
        let w = (info.viewport.width() * ppp).min(screen_w - x);
        let h = (info.viewport.height() * ppp).min(screen_h - y);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        renderer.paint(pass);
    }
}
