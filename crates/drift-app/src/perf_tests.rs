//! Viewport torture tests: the 10,000-curve sketch through the app's frames and the GPU frame
//! build. Opt-in (they're slow in debug builds):
//!
//! `cargo test --release -p drift-app perf -- --ignored --nocapture --test-threads=1`
//!
//! The editing ones (`perf_editor_pipeline` and the rest) are drift-edit's `perf_tests`.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use drift_core::{Bounds, DVec2, Document, EntityId};
use drift_render::{Camera, Palette, Viewport, build_frame};

use drift_edit::editor::Editor;
use drift_edit::test_sketches::torture_sketch;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// Run `f` `n` times; mean, worst and median, in milliseconds. (The median is what one
/// call costs when the machine doesn't get in the way: a single descheduled call moves the
/// mean of a few, not it.)
fn time(n: usize, mut f: impl FnMut(usize)) -> (f64, f64, f64) {
    let mut each = Vec::with_capacity(n);
    let start = Instant::now();
    for i in 0..n {
        let t = Instant::now();
        f(i);
        each.push(t.elapsed());
    }
    each.sort_unstable();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    (
        start.elapsed().as_secs_f64() * 1e3 / n as f64,
        each.last().map_or(0.0, |d| ms(*d)),
        each.get(n / 2).map_or(0.0, |d| ms(*d)),
    )
}

fn report(label: &str, (mean, worst, median): (f64, f64, f64)) {
    let flag = if worst > 16.7 {
        "  ⚠ over a 60 fps frame"
    } else {
        ""
    };
    println!(
        "{label:<46} mean {mean:>9.2} ms   median {median:>9.2} ms   worst {worst:>9.2} ms{flag}"
    );
}

#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_editor_pipeline_frames() {
    println!();
    let (doc, _) = torture_sketch();
    let ed = Editor::new(doc);
    let items = ed.items_styled(true, true);
    // The GPU frame at three zooms (the rest of the pipeline is drift-edit's
    // `perf_editor_pipeline`).
    let viewport = Viewport {
        size_px: v(2560.0, 1440.0),
        pixels_per_point: 2.0,
        srgb_target: false,
        imperial_grid: false,
        thin_lines: false,
        show_grid: true,
        show_axes: true,
    };
    let palette = Palette::dark();
    let bounds = ed.bounds();
    for (label, zoom) in [
        ("fit", 1.0),
        ("zoomed in ×10", 10.0),
        ("zoomed in ×100", 100.0),
    ] {
        let mut camera = Camera::default();
        camera.fit(bounds, viewport.size_px / viewport.pixels_per_point, 20.0);
        camera.zoom_at(DVec2::ZERO, zoom);
        let mut instances = 0;
        let r = time(20, |k| {
            camera.pan(v(f64::from(k as u32 % 5), 1.0));
            let f = build_frame(&items, &camera, viewport, &palette);
            instances = f.segments.len() + f.arcs.len();
        });
        report(
            &format!("GPU frame build, {label} ({instances} instances)"),
            r,
        );
    }
}

#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_app_frames() {
    use eframe::egui::{self, Pos2};
    use egui_kittest::Harness;

    if !crate::test_gpu::available() {
        return;
    }
    let mut h = Harness::builder()
        .with_size([1920.0, 1080.0])
        .wgpu_setup(crate::test_gpu::setup())
        .build_eframe(|cc| crate::DriftApp::new(cc));
    h.run();
    let (doc, curves) = torture_sketch();
    h.state_mut().editor = Editor::new(doc);
    let fit = h.state().fit_bounds();
    h.state_mut().fit_target = Some(fit);
    h.run();
    // Fitted; let the camera moves below stick (as a mouse pan or zoom would).
    h.state_mut().fit_target = None;
    println!("\n{curves} curves in the full app at 1920 × 1080:");

    let frame = |h: &mut Harness<'_, crate::DriftApp>| {
        let t = Instant::now();
        h.step();
        t.elapsed()
    };
    let phase = |label: &str,
                 h: &mut Harness<'_, crate::DriftApp>,
                 f: &mut dyn FnMut(&mut Harness<'_, crate::DriftApp>, usize)| {
        let mut times = Vec::new();
        for k in 0..60 {
            f(h, k);
            times.push(frame(h));
        }
        times.sort();
        let mean = times.iter().sum::<Duration>().as_secs_f64() * 1e3 / times.len() as f64;
        let p95 = times[times.len() * 95 / 100].as_secs_f64() * 1e3;
        let worst = times.last().unwrap().as_secs_f64() * 1e3;
        let flag = if p95 > 16.7 {
            "  ⚠ p95 over a 60 fps frame"
        } else {
            ""
        };
        println!(
            "{label:<40} mean {mean:>7.2} ms   p95 {p95:>7.2} ms   worst {worst:>7.2} ms{flag}"
        );
    };
    let centre = Pos2::new(1000.0, 540.0);
    phase("idle, whole sketch in view", &mut h, &mut |_, _| {});
    phase("hovering across the sketch", &mut h, &mut |h, k| {
        h.hover_at(Pos2::new(
            300.0 + (k as f32 * 23.0) % 1300.0,
            200.0 + (k as f32 * 17.0) % 700.0,
        ));
    });
    phase("panning, whole sketch", &mut h, &mut |h, _| {
        h.state_mut()
            .camera
            .pan(egui::emath::Vec2::new(4.0, 2.0).into_dvec());
    });
    phase("zooming in", &mut h, &mut |h, _| {
        let c = h.state().camera;
        let _ = c;
        h.state_mut().camera.zoom_at(DVec2::ZERO, 1.05);
    });
    println!("  scale after zooming in: {}", h.state().camera.scale);
    phase("panning, zoomed in", &mut h, &mut |h, _| {
        h.state_mut().camera.pan(v(6.0, 3.0));
    });
    phase("zooming out past everything", &mut h, &mut |h, _| {
        h.state_mut().camera.zoom_at(DVec2::ZERO, 1.0 / 1.12);
    });
    // A thousand curves picked: emphasis, handles, combs, inspector.
    let some: BTreeSet<EntityId> = h
        .state()
        .editor
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .take(1000)
        .collect();
    let fit = h.state().fit_bounds();
    h.state_mut().fit_target = Some(fit);
    h.run();
    h.state_mut().fit_target = None;
    h.state_mut().editor.selection = some;
    phase("1,000 picked, hovering", &mut h, &mut |h, k| {
        h.hover_at(Pos2::new(400.0 + (k as f32 * 31.0) % 1200.0, centre.y));
    });
    let mut gpu = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let _ = h.render();
        gpu.push(t.elapsed().as_secs_f64() * 1e3);
    }
    println!(
        "{:<40} {:.1} / {:.1} / {:.1} ms (incl. reading the image back)",
        "full GPU render", gpu[0], gpu[1], gpu[2]
    );
}

trait IntoDVec {
    fn into_dvec(self) -> DVec2;
}

impl IntoDVec for eframe::egui::emath::Vec2 {
    fn into_dvec(self) -> DVec2 {
        DVec2::new(f64::from(self.x), f64::from(self.y))
    }
}

/// The drawing of issue #150 (49 solid fills, 39 block copies, notes in an embedded font, a
/// view's crop), from `DRIFT_ZOOM_FIXTURE` or `tests/fixtures`; without it, a stand-in with
/// what made zooming in slow: fills, a group and a view's dashed crop.
fn zoom_drawing() -> (Document, drift_io::Metadata) {
    let path = std::env::var("DRIFT_ZOOM_FIXTURE").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/athenian_bar_shelves.drift"
        )
        .into()
    });
    if let Ok(text) = std::fs::read_to_string(&path) {
        println!("zoom drawing: {path}");
        return drift_io::load(&text).expect("the zoom drawing loads");
    }
    println!("zoom drawing: a stand-in ({path} isn't there)");
    let mut doc = Document::new();
    doc.begin();
    let mut first = Vec::new();
    for k in 0..40 {
        let o = v(f64::from(k % 8) * 300.0, f64::from(k / 8) * 200.0);
        let p = [(0.0, 0.0), (250.0, 0.0), (250.0, 150.0), (0.0, 150.0)]
            .map(|(x, y)| doc.add_point(o + v(x, y)));
        let lines: Vec<EntityId> = (0..4)
            .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        let fill = drift_core::Fill::new(
            lines.clone(),
            o + v(125.0, 75.0),
            drift_core::FillKind::Solid,
        );
        doc.add_fill(fill);
        if k == 0 {
            first = lines;
        }
    }
    doc.add_group(drift_core::Group {
        name: "Group".into(),
        members: first.into_iter().collect(),
    })
    .unwrap();
    doc.add_view(drift_core::View::new(
        "Plan",
        Bounds {
            min: v(-50.0, -50.0),
            max: v(2450.0, 1050.0),
        },
        50.0,
    ));
    doc.commit("Zoom drawing");
    (doc, drift_io::Metadata::default())
}

/// Issue #150: zoomed in on the same spot, a frame must cost about what it does at 100%
/// (it was 2 ms at 100% and 60 ms at 10,000%: dashed frames were cut into dashes across
/// their whole length on screen, not just the part that shows).
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_zoom_frame_cost() {
    use egui_kittest::Harness;

    if !crate::test_gpu::available() {
        return;
    }
    let (doc, meta) = zoom_drawing();
    let mut h = Harness::builder()
        .with_size([1600.0, 1000.0])
        .wgpu_setup(crate::test_gpu::setup())
        .build_eframe(|cc| crate::DriftApp::new(cc));
    h.run();
    h.state_mut()
        .replace_document(doc, "zoom".into(), None, meta, true);
    h.state_mut().show_view_crops = true;
    h.run();
    h.state_mut().fit_target = None;
    let fitted = h.state().camera;
    // About the middle, and about a fill's seed (something is on screen at every zoom).
    let seeds: Vec<DVec2> = h.state().editor.doc.fills().map(|(_, f)| f.seed).collect();
    let anchors = [fitted.center, seeds[seeds.len() / 2]];
    for anchor in anchors {
        println!("\nzoomed in about {:.1}, {:.1}:", anchor.x, anchor.y);
        let mut at_100 = 0.0;
        for percent in [100.0, 1000.0, 3000.0, 10000.0] {
            h.state_mut().camera = drift_render::Camera {
                center: anchor,
                // Zoom as the status bar shows it: real size on a 96 dpi screen.
                scale: percent / 100.0 * 96.0 / 25.4,
            };
            h.step();
            let (step, worst, _) = time(20, |_| h.step());
            let (render, _, _) = time(3, |_| {
                let _ = h.render();
            });
            println!(
                "{percent:>7}%   frame {step:>7.2} ms (worst {worst:>6.2})   GPU render and read back {render:>7.2} ms"
            );
            if percent == 100.0 {
                at_100 = step;
            } else {
                assert!(
                    step < (at_100 * 3.0).max(at_100 + 3.0),
                    "a frame at {percent}% took {step:.2} ms, {at_100:.2} ms at 100%"
                );
            }
        }
    }
}
