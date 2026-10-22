//! Viewport and editing torture tests: a 10,000-curve sketch through every per-frame and
//! per-edit path. Opt-in (they're slow in debug builds):
//!
//! `cargo test --release -p drift-app perf -- --ignored --nocapture --test-threads=1`

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use drift_core::{Bounds, Constraint, DVec2, Document, EntityId};
use drift_geom::SnapOptions;
use drift_render::{Camera, Palette, Viewport, build_frame};

use crate::editor::Editor;

const TARGET_CURVES: usize = 10_000;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A deterministic pseudo-random sequence (no dependencies, same every run).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// About 10,000 curves in a grid of cells, each one of: a rectangle (shared corners, H/V
/// relations) with a circle; a rectangle with a corner arc; a path of four Bézier curves with
/// smooth points; an ellipse with a cross of lines.
pub(crate) fn torture_sketch() -> (Document, usize) {
    let mut doc = Document::new();
    doc.begin();
    let mut curves = 0;
    let cols = 45;
    let mut k = 0;
    while curves < TARGET_CURVES {
        let (i, j) = (k % cols, k / cols);
        let o = v(i as f64 * 30.0, j as f64 * 30.0);
        match k % 4 {
            0 | 1 => {
                let c = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 14.0), v(0.0, 14.0)]
                    .map(|p| doc.add_point(o + p));
                let l: Vec<EntityId> = (0..4)
                    .map(|n| doc.add_line(c[n], c[(n + 1) % 4]).unwrap())
                    .collect();
                for (n, line) in l.iter().enumerate() {
                    let rel = if n % 2 == 0 {
                        Constraint::Horizontal { line: *line }
                    } else {
                        Constraint::Vertical { line: *line }
                    };
                    doc.add_constraint(rel).unwrap();
                }
                curves += 4;
                if k % 4 == 0 {
                    let centre = doc.add_point(o + v(10.0, 7.0));
                    doc.add_circle(centre, 4.0).unwrap();
                } else {
                    let centre = doc.add_point(o + v(10.0, 7.0));
                    let s = doc.add_point(o + v(15.0, 7.0));
                    let e = doc.add_point(o + v(10.0, 12.0));
                    doc.add_arc(centre, s, e).unwrap();
                }
                curves += 1;
            }
            2 => {
                let pts: Vec<EntityId> = (0..13)
                    .map(|n| {
                        let x = n as f64 * 20.0 / 12.0;
                        let y = 7.0 + 5.0 * (n as f64 * 0.9).sin();
                        doc.add_point(o + v(x, y))
                    })
                    .collect();
                for seg in 0..4 {
                    let b = seg * 3;
                    doc.add_bezier(pts[b], pts[b + 1], pts[b + 2], pts[b + 3])
                        .unwrap();
                    curves += 1;
                    if seg > 0 {
                        doc.add_constraint(Constraint::Smooth {
                            point: pts[b],
                            a: pts[b - 1],
                            b: pts[b + 1],
                        })
                        .ok();
                    }
                }
            }
            _ => {
                let [c, m, n] =
                    [v(10.0, 7.0), v(19.0, 7.0), v(10.0, 12.0)].map(|p| doc.add_point(o + p));
                doc.add_ellipse(c, m, n).unwrap();
                let [a, b, p, q] = [v(0.0, 7.0), v(20.0, 7.0), v(10.0, 0.0), v(10.0, 14.0)]
                    .map(|p| doc.add_point(o + p));
                doc.add_line(a, b).unwrap();
                doc.add_line(p, q).unwrap();
                curves += 3;
            }
        }
        k += 1;
    }
    doc.commit("Torture");
    (doc, curves)
}

/// Run `f` `n` times; mean and worst, in milliseconds.
fn time(n: usize, mut f: impl FnMut(usize)) -> (f64, f64) {
    let mut worst = Duration::ZERO;
    let start = Instant::now();
    for i in 0..n {
        let t = Instant::now();
        f(i);
        worst = worst.max(t.elapsed());
    }
    (
        start.elapsed().as_secs_f64() * 1e3 / n as f64,
        worst.as_secs_f64() * 1e3,
    )
}

fn report(label: &str, (mean, worst): (f64, f64)) {
    let flag = if worst > 16.7 {
        "  ⚠ over a 60 fps frame"
    } else {
        ""
    };
    println!("{label:<46} mean {mean:>9.2} ms   worst {worst:>9.2} ms{flag}");
}

#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_editor_pipeline() {
    println!();
    let t = Instant::now();
    let (doc, curves) = torture_sketch();
    let entities = doc.len();
    report("build the sketch", (t.elapsed().as_secs_f64() * 1e3, 0.0));
    println!(
        "  {curves} curves, {entities} entities, {} relations",
        doc.constraints().count()
    );

    let t = Instant::now();
    let mut ed = Editor::new(doc);
    report(
        "open: solve, analyse, index",
        (t.elapsed().as_secs_f64() * 1e3, 0.0),
    );
    println!("  DOF {:?}", ed.analysis().dof);

    report(
        "re-index (after an edit)",
        time(5, |_| ed.index = drift_geom::SceneIndex::build(&ed.doc)),
    );
    report(
        "freedom analysis (after an edit)",
        time(3, |_| {
            let _ = drift_solver::analyze(&ed.doc);
        }),
    );

    // Per frame: styled items and the GPU frame at three zooms.
    let mut items = Vec::new();
    report(
        "sketch items (per frame)",
        time(20, |_| items = ed.items_styled(true, true)),
    );
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

    // Pointer work: snapping and picking at random spots.
    let mut rng = Lcg(7);
    let span = bounds.max - bounds.min;
    let mut spot = || bounds.min + v(rng.next() * span.x, rng.next() * span.y);
    let spots: Vec<DVec2> = (0..1000).map(|_| spot()).collect();
    let options = SnapOptions {
        tolerance: 1.0,
        grid: Some(1.0),
        from: None,
    };
    report(
        "snap (per pointer move)",
        time(1000, |i| {
            let _ = ed.snap(spots[i], options, |_| false);
        }),
    );
    report(
        "pick (per hover)",
        time(1000, |i| {
            let hidden = ed.hidden_handles();
            let _ = ed.index.pick(spots[i], 1.0, |id| ed.pickable(id, &hidden));
        }),
    );
    report(
        "box-select everything",
        time(5, |_| {
            let _ = ed.index.select_in_rect(&bounds, true);
        }),
    );

    // Editing: drag and rotate a thousand curves.
    let some: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .take(1000)
        .collect();
    ed.selection = some.iter().copied().collect();
    report(
        "items with 1,000 picked (per frame)",
        time(10, |_| items = ed.items_styled(true, true)),
    );
    // (Their points are the dragged, as the Grab tool passes them.)
    let dragged: Vec<drift_solver::Param> = ed
        .doc
        .defining_points(some.iter().copied())
        .into_iter()
        .flat_map(drift_solver::Param::point)
        .collect();
    ed.doc.begin();
    report(
        "drag 1,000 curves (per mouse move)",
        time(10, |_| {
            ed.drag(&some, v(0.5, 0.25), &dragged);
        }),
    );
    let t = Instant::now();
    ed.commit("Drag");
    report(
        "…and let go (commit, analyse)",
        (t.elapsed().as_secs_f64() * 1e3, 0.0),
    );
    let points: BTreeSet<EntityId> = ed.doc.defining_points(some.iter().copied());
    let pivot = points
        .iter()
        .filter_map(|p| ed.doc.point(*p).ok())
        .map(Bounds::from_point)
        .collect::<Bounds>()
        .center();
    ed.doc.begin();
    report(
        "rotate 1,000 curves (per mouse move)",
        time(10, |k| {
            ed.rotate_geometry(&some, &points, pivot, 3.0 * k as f64);
        }),
    );
    ed.doc.rollback();
    ed.sync();
    ed.selection.clear();

    // Regions (fills, region picking) over the whole sketch.
    report(
        "find closed regions (once per edit)",
        time(1, |_| {
            // Nothing has asked for regions yet, so this is a cold build.
            let _ = ed.regions();
        }),
    );

    report(
        "undo",
        time(1, |_| {
            ed.undo();
        }),
    );
    report(
        "redo",
        time(1, |_| {
            ed.redo();
        }),
    );
    let mut text = String::new();
    report(
        "save (.drift)",
        time(3, |_| {
            text = drift_io::save(&ed.doc, &drift_io::Metadata::default())
        }),
    );
    println!("  {:.1} MB", text.len() as f64 / 1e6);
    report(
        "load (.drift)",
        time(3, |_| {
            let _ = drift_io::load(&text).unwrap();
        }),
    );
}

#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_drag_breakdown() {
    let (doc, _) = torture_sketch();
    let mut ed = Editor::new(doc);
    let some: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .take(1000)
        .collect();
    ed.doc.begin();
    println!();
    report(
        "translate points",
        time(10, |_| ed.doc.translate(some.iter().copied(), v(0.5, 0.25))),
    );
    report(
        "solve whole sketch",
        time(10, |_| {
            let _ = drift_solver::solve(&mut ed.doc, &Default::default());
        }),
    );
    report(
        "degenerate check",
        time(10, |_| {
            let _ = drift_solver::degenerate_entities(&ed.doc);
        }),
    );
    report(
        "index update (1,000 moved)",
        time(10, |_| {
            ed.doc.translate(some.iter().copied(), v(0.5, 0.25));
            ed.index.update(&ed.doc);
        }),
    );
}

#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_app_frames() {
    use eframe::egui::{self, Pos2};
    use egui_kittest::Harness;

    let instance = eframe::egui_wgpu::wgpu::Instance::default();
    if pollster::block_on(
        instance.request_adapter(&eframe::egui_wgpu::wgpu::RequestAdapterOptions::default()),
    )
    .is_err()
    {
        eprintln!("no GPU adapter; skipping");
        return;
    }
    let mut h = Harness::builder()
        .with_size([1920.0, 1080.0])
        .wgpu()
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

#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_selection_breakdown() {
    let (doc, _) = torture_sketch();
    let mut ed = Editor::new(doc);
    ed.selection = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .take(1000)
        .collect();
    println!();
    report(
        "curves showing handles",
        time(10, |_| {
            let _ = ed.curves_showing_handles();
        }),
    );
    report(
        "hidden handles",
        time(10, |_| {
            let _ = ed.hidden_handles();
        }),
    );
    report(
        "handle lines",
        time(10, |_| {
            let _ = ed.handle_lines([0, 0, 0]);
        }),
    );
    report(
        "curvature combs",
        time(10, |_| {
            let _ = ed.curvature_combs(1.0, [0, 0, 0]);
        }),
    );
    report(
        "touched points",
        time(10, |_| {
            let _ = ed.touched_points();
        }),
    );
    report(
        "items styled",
        time(10, |_| {
            let _ = ed.items_styled(true, true);
        }),
    );
}

/// A synthetic import like a dense architectural PDF sheet: `cells` rooms in a grid, each
/// drawn as four lines with a few short lines inside (furniture, hatching), and a solid
/// fill on every `fill_every`-th room, plus a few long grid lines across the whole sheet.
/// No relations, like any import.
pub(crate) fn sheet_import(cells: usize, fill_every: usize) -> drift_io::dxf::DxfImport {
    use drift_core::Shape;
    use drift_io::dxf::{DxfEntity, DxfHatch, DxfImport};
    let mut import = DxfImport::default();
    let line = |a: DVec2, b: DVec2| DxfEntity {
        layer: "0".into(),
        shape: Shape::Line { a, b },
        line_type: None,
        color: None,
        weight: None,
    };
    let cols = (cells as f64).sqrt().ceil() as usize;
    let mut rng = Lcg(7);
    for k in 0..cells {
        let o = v((k % cols) as f64 * 30.0, (k / cols) as f64 * 30.0);
        let c = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 20.0), v(0.0, 20.0)].map(|p| o + p);
        for n in 0..4 {
            import.entities.push(line(c[n], c[(n + 1) % 4]));
        }
        for _ in 0..6 {
            let a = o + v(2.0 + 16.0 * rng.next(), 2.0 + 16.0 * rng.next());
            let b = o + v(2.0 + 16.0 * rng.next(), 2.0 + 16.0 * rng.next());
            import.entities.push(line(a, b));
        }
        if k % fill_every == 0 {
            import.hatches.push(DxfHatch {
                layer: "0".into(),
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
                color: None,
                opacity: None,
                seeds: Vec::new(),
            });
        }
    }
    let size = cols as f64 * 30.0;
    for n in 0..10 {
        let t = 25.0 + n as f64 * size / 10.0;
        import.entities.push(line(v(t, -10.0), v(t, size)));
        import.entities.push(line(v(-10.0, t), v(size, t)));
    }
    import
}

#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_import_commit() {
    let cells: usize = std::env::var("DRIFT_CELLS").map_or(2_000, |s| s.parse().unwrap());
    let import = sheet_import(cells, 2);
    let mut ed = Editor::new(Document::new());
    ed.doc.begin();
    let t = Instant::now();
    let r = drift_io::dxf::add_to_document(&mut ed.doc, &import);
    println!(
        "add_to_document {:.2} s: {} curves, {} fills",
        t.elapsed().as_secs_f64(),
        r.curves,
        ed.doc.fills().count()
    );
    let t = Instant::now();
    let _ = ed.solve(&[]);
    println!("solve {:.2} s", t.elapsed().as_secs_f64());
    let t = Instant::now();
    let _ = drift_solver::analyze(&ed.doc);
    println!("analyze {:.2} s", t.elapsed().as_secs_f64());
    let t = Instant::now();
    let n = ed.regions().len();
    println!("regions {:.2} s: {n}", t.elapsed().as_secs_f64());
    let t = Instant::now();
    ed.follow_fills();
    println!("follow_fills {:.2} s", t.elapsed().as_secs_f64());
    let t = Instant::now();
    ed.doc.commit("Import");
    println!("doc.commit {:.2} s", t.elapsed().as_secs_f64());
    let t = Instant::now();
    ed.sync();
    println!("sync {:.2} s", t.elapsed().as_secs_f64());
    let t = Instant::now();
    let n = ed.fill_shapes().len();
    println!("fill_shapes {:.2} s: {n}", t.elapsed().as_secs_f64());
    write_digest(&mut ed);
}

/// A real PDF through the import path (page 1, as drawn): `DRIFT_PDF` names the file.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_import_pdf() {
    let Ok(path) = std::env::var("DRIFT_PDF") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let t = Instant::now();
    let art = drift_io::pdf_import::read_artwork(&bytes, 1, "sheet").unwrap();
    let options = drift_io::vector::ImportOptions {
        scale: 1.0,
        construction_edges: true,
    };
    let import = drift_io::vector::to_import(&art, &options);
    println!("read {:.2} s", t.elapsed().as_secs_f64());
    let mut ed = Editor::new(Document::new());
    ed.doc.begin();
    let t = Instant::now();
    let r = drift_io::dxf::add_to_document(&mut ed.doc, &import);
    println!(
        "add_to_document {:.2} s: {} curves, {} fills",
        t.elapsed().as_secs_f64(),
        r.curves,
        ed.doc.fills().count()
    );
    let t = Instant::now();
    assert!(ed.commit("Import"));
    println!("commit {:.2} s", t.elapsed().as_secs_f64());
    let t = Instant::now();
    let shapes = ed.fill_shapes();
    println!(
        "fill_shapes {:.2} s: {} ({} lost)",
        t.elapsed().as_secs_f64(),
        shapes.len(),
        shapes.values().filter(|s| s.lost).count()
    );
    write_digest(&mut ed);
}

/// Every fill and its area, as text, to the file `DRIFT_DIGEST` names (if any): for checking
/// a change finds the same areas.
fn write_digest(ed: &mut Editor) {
    let Ok(path) = std::env::var("DRIFT_DIGEST") else {
        return;
    };
    let mut digest = String::new();
    for (id, s) in ed.fill_shapes().iter() {
        let f = ed.doc.fill(*id).unwrap();
        digest += &format!(
            "{id:?} {:?} {:.6} {:.6} | {:.6} {:.6} {:.6} {} {:?}
",
            f.boundary,
            f.seed.x,
            f.seed.y,
            s.region.area,
            s.region.centroid.x,
            s.region.centroid.y,
            s.lost,
            s.curves
        );
    }
    std::fs::write(path, digest).unwrap();
}

/// A chain of `bars` bars of fixed length from a fixed point, lying along x, its far end
/// dragged round to the other side: a drag whose one big solve mispredicts round the fold
/// (#103), so frames go in steps.
fn folding_chain(bars: usize) -> (Editor, EntityId) {
    let mut doc = Document::new();
    doc.begin();
    let points: Vec<EntityId> = (0..=bars)
        .map(|k| doc.add_point(v(k as f64 * 10.0, 0.0)))
        .collect();
    doc.add_constraint(Constraint::Fix {
        point: points[0],
        at: DVec2::ZERO,
    })
    .unwrap();
    for pair in points.windows(2) {
        doc.add_line(pair[0], pair[1]).unwrap();
        doc.add_constraint(Constraint::Distance {
            a: pair[0],
            b: pair[1],
            value: 10.0,
        })
        .unwrap();
    }
    doc.commit("Chain");
    (Editor::new(doc), points[bars])
}

#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_drag_wandering() {
    println!();
    // Within reach, and (the last) out of it: a move that can't be made at all.
    for (bars, out) in [(3, 0.8), (12, 0.8), (48, 0.8), (12, 1.5)] {
        let (mut ed, tip) = folding_chain(bars);
        let reach = bars as f64 * 10.0;
        let dragged = drift_solver::Param::point(tip);
        ed.doc.begin();
        // Round a half circle to the other side, in 60 frames.
        let frames = 60;
        let mut at = v(reach, 0.0);
        let mut solves = Vec::new();
        let mut refused = 0;
        let r = time(frames, |k| {
            let t = (k + 1) as f64 / frames as f64 * std::f64::consts::PI;
            let to = v(t.cos(), t.sin()) * reach * out;
            refused += usize::from(!ed.drag(&[tip], to - at, &dragged));
            at = ed.doc.point(tip).unwrap();
            solves.push(ed.drag_solves);
        });
        ed.doc.rollback();
        report(
            &format!("folding drag, {bars} bars to {out} × (per frame)"),
            r,
        );
        let most = solves.iter().max().copied().unwrap_or(0);
        let all: u32 = solves.iter().sum();
        println!("  solves per frame: most {most}, {all} in all; {refused} frames refused");
    }
}

#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_drag_pushing_an_arc() {
    // A line, an arc ending at its start, and a line from there tangent to the arc; the first
    // line dragged across, squeezing the arc until it is pushed along (as in drag_props).
    println!();
    let mut doc = Document::new();
    doc.begin();
    let p = [
        v(0.0, 0.0),
        v(2.5881904510252074, 9.659258262890683),
        v(2.0705523608201624, -7.727406610312547),
        v(6.070552360820166, -0.7992033800370404),
        v(9.659258262890685, 2.588190451025203),
    ]
    .map(|q| doc.add_point(q));
    let a = doc.add_line(p[0], p[1]).unwrap();
    let arc = doc.add_arc(p[2], p[3], p[0]).unwrap();
    let b = doc.add_line(p[0], p[4]).unwrap();
    doc.add_constraint(Constraint::Tangent { a: b, b: arc })
        .unwrap();
    doc.commit("Shape");
    let mut ed = Editor::new(doc);
    ed.doc.begin();
    let frames = 60;
    let mut solves = Vec::new();
    let mut refused = 0;
    let r = time(frames, |_| {
        refused += usize::from(!ed.drag(&[a], v(1.0, 0.0), &[]));
        solves.push(ed.drag_solves);
    });
    ed.doc.rollback();
    report("pushing a squeezed arc (per frame)", r);
    let most = solves.iter().max().copied().unwrap_or(0);
    let all: u32 = solves.iter().sum();
    println!("  solves per frame: most {most}, {all} in all; {refused} frames refused");
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

    let instance = eframe::egui_wgpu::wgpu::Instance::default();
    if pollster::block_on(
        instance.request_adapter(&eframe::egui_wgpu::wgpu::RequestAdapterOptions::default()),
    )
    .is_err()
    {
        eprintln!("no GPU adapter; skipping");
        return;
    }
    let (doc, meta) = zoom_drawing();
    let mut h = Harness::builder()
        .with_size([1600.0, 1000.0])
        .wgpu()
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
            let (step, worst) = time(20, |_| h.step());
            let (render, _) = time(3, |_| {
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
