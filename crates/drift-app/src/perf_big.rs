//! Big imported drawings: where an edit's and a frame's time goes on tens of thousands of
//! curves. Opt-in (slow in debug builds):
//!
//! `cargo test --release -p drift-app perf_big -- --ignored --nocapture --test-threads=1`
//!
//! `perf_big_synthetic` needs no files. `perf_big` runs the whole app (with a GPU) on the
//! file `DRIFT_BIG` names, imported as `DRIFT_MODE` (`geometry`, the default, or
//! `underlay`); `DRIFT_BIG=synthetic:8000` imports [`big_drawing`] of 8,000 rooms instead.

use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Instant;

use drift_core::{DVec2, Document, EntityId};
use eframe::egui::Pos2;
use egui_kittest::Harness;

use crate::app::import_ui::ImportAs;
use crate::editor::Editor;

/// A made-up big CAD drawing: a grid of `cells` rooms, each a closed outline with a door
/// swing (an arc), a column (a circle) and loose lines across it; every third room hatched
/// solid; long grid lines across the sheet. About 10 curves a room.
pub(crate) fn big_drawing(cells: usize) -> drift_io::dxf::DxfImport {
    use drift_core::Shape;
    use drift_io::dxf::{DxfEntity, DxfHatch, DxfImport};
    let mut import = DxfImport::default();
    let curve = |layer: &str, shape: Shape| DxfEntity {
        layer: layer.into(),
        shape,
        line_type: None,
        color: None,
        weight: None,
    };
    let v = DVec2::new;
    let cols = (cells as f64).sqrt().ceil() as usize;
    let mut seed = 11u64;
    let mut rnd = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for k in 0..cells {
        let o = v((k % cols) as f64 * 3000.0, (k / cols) as f64 * 3000.0);
        let c = [
            v(0.0, 0.0),
            v(2400.0, 0.0),
            v(2400.0, 2000.0),
            v(0.0, 2000.0),
        ]
        .map(|p| o + p);
        for n in 0..4 {
            import.entities.push(curve(
                "walls",
                Shape::Line {
                    a: c[n],
                    b: c[(n + 1) % 4],
                },
            ));
        }
        import.entities.push(curve(
            "doors",
            Shape::Arc {
                center: o + v(300.0, 0.0),
                radius: 800.0,
                start: 0.0,
                sweep: std::f64::consts::FRAC_PI_2,
            },
        ));
        import.entities.push(curve(
            "columns",
            Shape::Circle {
                center: o + v(1800.0 + 200.0 * rnd(), 1400.0 + 200.0 * rnd()),
                radius: 150.0,
            },
        ));
        for _ in 0..4 {
            let a = o + v(200.0 + 2000.0 * rnd(), 200.0 + 1600.0 * rnd());
            let b = o + v(200.0 + 2000.0 * rnd(), 200.0 + 1600.0 * rnd());
            import
                .entities
                .push(curve("furniture", Shape::Line { a, b }));
        }
        if k % 3 == 0 {
            import.hatches.push(DxfHatch {
                layer: "fills".into(),
                loops: vec![
                    (0..4)
                        .map(|n| Shape::Line {
                            a: c[n],
                            b: c[(n + 1) % 4],
                        })
                        .collect(),
                ],
                solid: true,
                pattern: "SOLID".into(),
                scale: 1.0,
                angle: 0.0,
                definition: Vec::new(),
                origin: DVec2::ZERO,
                color: None,
                opacity: None,
                seeds: Vec::new(),
            });
        }
    }
    let size = cols as f64 * 3000.0;
    for n in 0..20 {
        let t = 2700.0 + n as f64 * size / 20.0;
        import.entities.push(curve(
            "grid",
            Shape::Line {
                a: v(t, -1000.0),
                b: v(t, size),
            },
        ));
        import.entities.push(curve(
            "grid",
            Shape::Line {
                a: v(-1000.0, t),
                b: v(size, t),
            },
        ));
    }
    import
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1e3
}

/// The editor side of a big drawing (no window): an edit finds the closed areas once, the
/// index follows it without a rebuild, and kept canvas items cost next to nothing.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_big_synthetic() {
    println!();
    let mut ed = Editor::new(Document::new());
    ed.doc.begin();
    drift_io::dxf::add_to_document(&mut ed.doc, &big_drawing(4000));
    let t = Instant::now();
    assert!(ed.commit_import("Import"));
    let _ = ed.fill_shapes();
    println!("import commit + first fill shapes: {:.0} ms", ms(t));
    let curves: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .collect();
    println!("{} curves, {} fills", curves.len(), ed.doc.fills().count());

    // Moving one curve: the areas are found once (for the fills to follow), not again after.
    ed.selection = [curves[curves.len() / 2]].into_iter().collect();
    let t = Instant::now();
    assert_eq!(ed.nudge(DVec2::new(1.0, 0.0)), Some(true));
    let edit = ms(t);
    let found = ed
        .regions
        .as_ref()
        .map(|(_, r)| r.clone())
        .expect("areas found");
    let t = Instant::now();
    let _ = ed.fill_shapes();
    let next = ms(t);
    assert!(
        Rc::ptr_eq(&found, &ed.regions.as_ref().unwrap().1),
        "the areas were found again after the edit"
    );
    println!("move one curve: edit {edit:.0} ms, fill shapes after {next:.0} ms");

    // The index follows a few changes without rebuilding.
    let point = ed.doc.iter().find(|(_, e)| e.kind.is_point()).unwrap().0;
    ed.doc.begin();
    ed.doc.set_point(point, DVec2::new(-500.0, -500.0)).unwrap();
    ed.doc.commit("Move");
    let t = Instant::now();
    ed.index.update(&ed.doc);
    let index = ms(t);
    assert_eq!(
        ed.index.pick(DVec2::new(-500.0, -500.0), 1.0, |_| true),
        Some(point)
    );
    println!("index update after a move: {index:.2} ms");

    // Deleting 100 curves.
    ed.selection = curves
        .iter()
        .step_by(curves.len() / 100)
        .take(100)
        .copied()
        .collect();
    let t = Instant::now();
    ed.delete_selection();
    println!("delete 100 curves: {:.0} ms", ms(t));

    // Canvas items: made once, then kept frame to frame (hovering patches a few in).
    ed.sync();
    let mut items = crate::frame_items::ItemsCache::default();
    let stack = ed.stack();
    let t = Instant::now();
    let _ = items.prepare(&ed, true, false, Some(&stack));
    items.restore();
    let made = ms(t);
    let hover: Vec<EntityId> = curves.iter().step_by(997).copied().collect();
    let t = Instant::now();
    for &h in &hover {
        ed.hovered = Some(h);
        let _ = items.prepare(&ed, true, false, Some(&stack));
        items.restore();
    }
    let kept = ms(t) / hover.len() as f64;
    println!("canvas items: made {made:.1} ms, kept (hovering) {kept:.2} ms a frame");

    println!("  budgets: index update 5 ms, kept items 5 ms a frame");
    assert!(index < 5.0, "index update {index:.2} ms");
    assert!(kept < 5.0, "kept items {kept:.2} ms a frame");
}

fn phase(
    label: &str,
    h: &mut Harness<'_, crate::DriftApp>,
    n: usize,
    f: &mut dyn FnMut(&mut Harness<'_, crate::DriftApp>, usize),
) {
    let mut times = Vec::new();
    for k in 0..n {
        f(h, k);
        let t = Instant::now();
        h.step();
        times.push(ms(t));
    }
    let total: f64 = times.iter().sum();
    times.sort_by(f64::total_cmp);
    println!(
        "{label:<34} frame mean {:>7.2} p95 {:>7.2} worst {:>7.2} ms",
        total / n as f64,
        times[n * 95 / 100],
        times[n - 1]
    );
}

/// The whole app on a big drawing: import, frames at fit and zoomed in (idle, hover, pan),
/// with a curve picked, and (as geometry) edits and undo with the frame after each.
#[test]
#[ignore = "big-drawing probe: needs DRIFT_BIG and a GPU"]
fn perf_big() {
    let Ok(path) = std::env::var("DRIFT_BIG") else {
        return;
    };
    let mode = match std::env::var("DRIFT_MODE").as_deref() {
        Ok("underlay") => ImportAs::Underlay,
        _ => ImportAs::Geometry,
    };
    let synthetic = path
        .strip_prefix("synthetic")
        .map(|n| n.trim_start_matches(':').parse().unwrap_or(8_000));
    let mut h = Harness::builder()
        .with_size([1920.0, 1080.0])
        .wgpu()
        .build_eframe(|cc| crate::DriftApp::new(cc));
    h.run();
    println!();
    let t = Instant::now();
    if let Some(cells) = synthetic {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        drift_io::dxf::add_to_document(&mut ed.doc, &big_drawing(cells));
        ed.commit_import("Import");
    } else {
        let name = std::path::Path::new(&path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let bytes = std::fs::read(&path).unwrap();
        h.state_mut().begin_import(&name, bytes, Some(mode), 0.0);
        let mut pending = h.state_mut().pending_import.take().expect("pending");
        h.state_mut().finish_import(&mut pending, 0.0);
    }
    println!("import {:.0} ms", ms(t));
    {
        let ed = &h.state().editor;
        let curves = ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count();
        println!(
            "{curves} curves, {} fills, {} underlays",
            ed.doc.fills().count(),
            ed.doc.underlays().count()
        );
    }
    let fit = h.state().fit_bounds();
    h.state_mut().fit_target = Some(fit);
    let t = Instant::now();
    h.step();
    println!("first frame after import {:.0} ms", ms(t));
    h.run();
    h.state_mut().fit_target = None;
    let fitted = h.state().camera;

    let spot = |k: usize| {
        Pos2::new(
            500.0 + (k as f32 * 37.0) % 1100.0,
            200.0 + (k as f32 * 23.0) % 700.0,
        )
    };
    phase("idle, fit", &mut h, 30, &mut |_, _| {});
    phase("hover across, fit", &mut h, 40, &mut |h, k| {
        h.hover_at(spot(k))
    });
    phase("pan, fit", &mut h, 30, &mut |h, _| {
        h.state_mut().camera.pan(DVec2::new(4.0, 2.0));
    });
    {
        let c = &mut h.state_mut().camera;
        *c = fitted;
        c.center = fit.min + (fit.max - fit.min) * 0.15;
        c.scale = fitted.scale * 12.0;
    }
    h.step();
    phase("idle, zoomed x12", &mut h, 30, &mut |_, _| {});
    phase("hover, zoomed x12", &mut h, 40, &mut |h, k| {
        h.hover_at(spot(k))
    });
    phase("pan, zoomed x12", &mut h, 30, &mut |h, _| {
        h.state_mut().camera.pan(DVec2::new(6.0, 3.0));
    });
    h.state_mut().camera = fitted;
    h.step();
    let one = {
        let ed = &h.state().editor;
        ed.doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point())
            .map(|(id, _)| id)
            .nth(1000)
    };
    if let Some(one) = one {
        h.state_mut().editor.selection = [one].into_iter().collect();
        phase("one curve picked, hover, fit", &mut h, 20, &mut |h, k| {
            h.hover_at(Pos2::new(500.0 + (k as f32 * 37.0) % 1100.0, 300.0));
        });
        h.state_mut().editor.selection.clear();
        h.step();
    }
    if mode != ImportAs::Geometry {
        return;
    }
    let curves: Vec<EntityId> = h
        .state()
        .editor
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .collect();
    let mid = curves[curves.len() / 2];
    let edit = |label: &str,
                h: &mut Harness<'_, crate::DriftApp>,
                f: &mut dyn FnMut(&mut crate::DriftApp)| {
        let t = Instant::now();
        f(h.state_mut());
        let e = ms(t);
        let t = Instant::now();
        h.step();
        println!("{label:<34} edit {e:>6.1} ms, next frame {:>6.1} ms", ms(t));
        h.step();
    };
    edit("move one curve (nudge)", &mut h, &mut |app| {
        app.editor.selection = [mid].into_iter().collect();
        assert_eq!(app.editor.nudge(DVec2::new(1.0, 0.0)), Some(true));
    });
    edit("undo", &mut h, &mut |app| assert!(app.editor.undo()));
    let hundred: BTreeSet<EntityId> = curves
        .iter()
        .step_by(curves.len() / 100)
        .take(100)
        .copied()
        .collect();
    edit("delete 100 curves", &mut h, &mut |app| {
        app.editor.selection = hundred.clone();
        app.editor.delete_selection();
    });
    edit("undo delete", &mut h, &mut |app| assert!(app.editor.undo()));
}
