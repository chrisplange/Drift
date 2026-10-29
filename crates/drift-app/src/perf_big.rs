//! Big imported drawings: where an edit's and a frame's time goes on tens of thousands of
//! curves. Opt-in (slow in debug builds):
//!
//! `cargo test --release -p drift-app perf_big -- --ignored --nocapture --test-threads=1`
//!
//! `perf_big_synthetic` needs no files. `perf_big` runs the whole app (with a GPU) on the
//! file `DRIFT_BIG` names, imported as `DRIFT_MODE` (`geometry`, the default, or
//! `underlay`); `DRIFT_BIG=synthetic:8000` imports [`big_drawing`] of 8,000 rooms instead.
//! `perf_big_drag` drags one curve, 1000 scattered curves, every curve and every point of
//! the `DRIFT_BIG` file (as geometry), calling `Editor::drag` and then with the Select tool
//! (`DRIFT_CASE` picks one by the start of its name), with the frame after each step.

use std::collections::BTreeSet;
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
    // (Its fills follow once the areas are found on a thread: as they were until then.)
    let t = Instant::now();
    let _ = ed.fill_shapes();
    let stale = ms(t);
    let t = Instant::now();
    ed.settle_areas();
    let settle = ms(t);
    let found = ed
        .regions
        .as_ref()
        .map(|(_, r)| r.clone())
        .expect("areas found");
    let t = Instant::now();
    let _ = ed.fill_shapes();
    let next = ms(t);
    assert!(
        std::sync::Arc::ptr_eq(&found, &ed.regions.as_ref().unwrap().1),
        "the areas were found again after the edit"
    );
    println!(
        "move one curve: edit {edit:.0} ms, fill shapes while its areas are found \
         {stale:.1} ms, fills followed {settle:.0} ms, fill shapes after {next:.0} ms"
    );

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

    // Dragging every curve (#253): each part moves on its own (rounds moved whole have
    // nothing to solve), the index keeps searches quick, the fills move along, and the
    // overlapping curves are looked for again only where things changed.
    let all: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .collect();
    let dragged: Vec<drift_solver::Param> = ed
        .doc
        .defining_points(all.iter().copied())
        .into_iter()
        .flat_map(drift_solver::Param::point)
        .collect();
    // (As the app does when a drag begins: the last edit's fills follow it first.)
    ed.settle_areas();
    ed.sync();
    let _ = ed.fill_shapes();
    let _ = ed.overlapping();
    ed.doc.begin();
    let (mut first, mut drag, mut fills, mut over) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for k in 1..=4 {
        let t = Instant::now();
        assert!(ed.drag(&all, DVec2::new(2.0 * k as f64, 1.0), &dragged));
        // (The first call works out the drag's parts.)
        if k == 1 {
            first = ms(t);
        } else {
            drag = drag.max(ms(t));
        }
        assert_eq!(ed.drag_solves, 0, "rounds moved whole were solved");
        ed.sync();
        let t = Instant::now();
        let _ = ed.fill_shapes();
        fills = fills.max(ms(t));
        let t = Instant::now();
        let _ = ed.overlapping();
        over = over.max(ms(t));
    }
    assert!(ed.commit("Move"));
    println!(
        "drag every curve: first call {first:.0} ms, then {drag:.0} ms; fill shapes \
         {fills:.1} ms, overlapping {over:.1} ms (the worst)"
    );

    println!(
        "  budgets: index update 5 ms, kept items 5 ms a frame; dragging every curve: \
         first call 400 ms, then 150 ms, fills 60 ms, overlapping 80 ms"
    );
    assert!(index < 5.0, "index update {index:.2} ms");
    assert!(kept < 5.0, "kept items {kept:.2} ms a frame");
    assert!(
        first < 400.0,
        "first call dragging every curve {first:.0} ms"
    );
    assert!(drag < 150.0, "dragging every curve {drag:.0} ms a call");
    assert!(
        fills < 60.0,
        "fills while dragging every curve {fills:.1} ms"
    );
    assert!(
        over < 80.0,
        "overlapping while dragging every curve {over:.1} ms"
    );
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
        let next = ms(t);
        // Frames until its fills have followed it (their areas found on a thread).
        let t = Instant::now();
        let mut frames = 0;
        while h.state().editor.areas.pending.is_some() && frames < 2000 {
            h.step();
            frames += 1;
        }
        let settled = ms(t);
        println!(
            "{label:<34} edit {e:>6.1} ms, next frame {next:>6.1} ms, fills followed \
             {settled:>6.1} ms later ({frames} frames)"
        );
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
    edit("redo delete", &mut h, &mut |app| assert!(app.editor.redo()));
}

/// #253 probe: what a drag of many curves solves, and the frame after it.
#[test]
#[ignore = "big-drawing probe: needs DRIFT_BIG and a GPU"]
fn perf_big_drag() {
    let Ok(path) = std::env::var("DRIFT_BIG") else {
        return;
    };
    let mut h = Harness::builder()
        .with_size([1920.0, 1080.0])
        .wgpu()
        .build_eframe(|cc| crate::DriftApp::new(cc));
    h.run();
    let name = std::path::Path::new(&path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let bytes = std::fs::read(&path).unwrap();
    h.state_mut()
        .begin_import(&name, bytes, Some(ImportAs::Geometry), 0.0);
    let mut pending = h.state_mut().pending_import.take().expect("pending");
    h.state_mut().finish_import(&mut pending, 0.0);
    let fit = h.state().fit_bounds();
    h.state_mut().fit_target = Some(fit);
    h.run();
    h.state_mut().fit_target = None;
    let (curves, points) = {
        let ed = &h.state().editor;
        let mut c = Vec::new();
        let mut p = Vec::new();
        for (id, e) in ed.doc.iter() {
            if e.kind.is_point() {
                p.push(id)
            } else {
                c.push(id)
            }
        }
        (c, p)
    };
    println!(
        "\n{} curves, {} points, {} fills, {} relations",
        curves.len(),
        points.len(),
        h.state().editor.doc.fills().count(),
        h.state().editor.doc.constraints().count()
    );
    let only = std::env::var("DRIFT_CASE").ok();
    let cases: Vec<(&str, Vec<EntityId>)> = vec![
        ("one curve", vec![curves[curves.len().min(1001) - 1]]),
        (
            "1000 scattered curves",
            curves
                .iter()
                .step_by(curves.len() / 1000)
                .take(1000)
                .copied()
                .collect(),
        ),
        ("all curves", curves.clone()),
        ("all points", points.clone()),
    ];
    for (label, ids) in cases {
        if only.as_deref().is_some_and(|o| !label.starts_with(o)) {
            continue;
        }
        {
            let ed = &mut h.state_mut().editor;
            ed.doc.begin();
            let scope = ed.drag_scope(&ids);
            let lowering = ed.lowering();
            let needs = drift_solver::needs_solve_with(&ed.doc, &scope, lowering.as_deref());
            let relations = ed
                .doc
                .constraints()
                .filter(|(_, c)| c.entities().any(|e| scope.contains(&e)))
                .count();
            let mut kinds = std::collections::BTreeMap::<&str, usize>::new();
            for &id in scope.iter() {
                let k = match ed.doc.get(id).map(|e| e.kind) {
                    Some(drift_core::EntityKind::Point(_)) => "point",
                    Some(drift_core::EntityKind::Line { .. }) => "line",
                    Some(drift_core::EntityKind::Arc { .. }) => "arc",
                    Some(drift_core::EntityKind::Circle { .. }) => "circle",
                    _ => "other",
                };
                *kinds.entry(k).or_default() += 1;
            }
            // Components: entities joined by shared points.
            let ids_in: Vec<EntityId> = scope.iter().copied().collect();
            let at: std::collections::HashMap<EntityId, usize> =
                ids_in.iter().enumerate().map(|(i, &id)| (id, i)).collect();
            let mut parent: Vec<usize> = (0..ids_in.len()).collect();
            fn find(p: &mut [usize], mut i: usize) -> usize {
                while p[i] != i {
                    p[i] = p[p[i]];
                    i = p[i];
                }
                i
            }
            for (i, &id) in ids_in.iter().enumerate() {
                if let Some(e) = ed.doc.get(id) {
                    for q in e.kind.points() {
                        if let Some(&j) = at.get(&q) {
                            let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                            parent[a] = b;
                        }
                    }
                }
            }
            let comps: BTreeSet<usize> = (0..ids_in.len()).map(|i| find(&mut parent, i)).collect();
            println!(
                "\n== {label}: {} ids; scope {} of {} entities, {} components; kinds {kinds:?}; \
                 needs_solve {needs}; relations in scope {relations}; locked {}",
                ids.len(),
                scope.len(),
                ed.doc.iter().count(),
                comps.len(),
                ed.doc.locked_entities().len()
            );
        }
        let dragged: Vec<drift_solver::Param> = h
            .state()
            .editor
            .doc
            .defining_points(ids.iter().copied())
            .iter()
            .copied()
            .flat_map(drift_solver::Param::point)
            .collect();
        for k in 0..3 {
            let t = Instant::now();
            let ok = h
                .state_mut()
                .editor
                .drag(&ids, DVec2::new(2.0, 1.0), &dragged);
            let call = ms(t);
            let solves = h.state().editor.drag_solves;
            let t = Instant::now();
            h.step();
            let frame = ms(t);
            println!(
                "  step {k}: ok {ok}, drag call {call:.1} ms, {solves} solves, frame {frame:.1} ms"
            );
        }
        {
            let ed = &mut h.state_mut().editor;
            ed.doc.rollback();
            ed.sync();
        }
        h.step();
        h.step();
        // The same with the Select tool: its move of the picked curves (G), a few pixels a
        // frame, then a click to finish (the same moves as a drag of them).
        if ids.len() > 1 && label.contains("curves") {
            let first = ids[0];
            let was = h.state().editor.doc.shape(first).map(|s| s.bounds().min);
            // (The import's report closes first: with a window open the canvas takes no keys.)
            for _ in 0..3 {
                h.key_press(eframe::egui::Key::Escape);
                h.step();
            }
            assert!(!h.state().any_dialog_open(), "a window is open");
            h.state_mut().editor.selection = ids.iter().copied().collect();
            h.step();
            h.step();
            let at = h.state().canvas_rect.center();
            let press = |h: &mut Harness<'_, crate::DriftApp>, pos: Pos2, pressed: bool| {
                h.event(eframe::egui::Event::PointerButton {
                    pos,
                    button: eframe::egui::PointerButton::Primary,
                    pressed,
                    modifiers: eframe::egui::Modifiers::NONE,
                });
            };
            h.hover_at(at);
            h.step();
            h.key_press(eframe::egui::Key::G);
            h.step();
            for k in 1..=5 {
                h.hover_at(at + eframe::egui::vec2(4.0 * k as f32, 2.0 * k as f32));
                let t = Instant::now();
                h.step();
                let frame = ms(t);
                let now = h.state().editor.doc.shape(first).map(|s| s.bounds().min);
                println!(
                    "  select tool step {k}: frame {frame:.1} ms, {} solves, moved {:.1} mm",
                    h.state().editor.drag_solves,
                    now.zip(was).map_or(0.0, |(a, b)| a.distance(b))
                );
            }
            press(&mut h, at + eframe::egui::vec2(20.0, 10.0), true);
            press(&mut h, at + eframe::egui::vec2(20.0, 10.0), false);
            let t = Instant::now();
            h.step();
            println!("  select tool release frame {:.1} ms", ms(t));
            let t = Instant::now();
            h.step();
            println!("  frame after {:.1} ms", ms(t));
            while h.state().editor.areas.pending.is_some() {
                h.step();
            }
            assert!(h.state_mut().editor.undo());
            h.state_mut().editor.selection.clear();
            h.step();
            h.step();
        }
    }
}
