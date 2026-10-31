//! Big imported drawings: where an edit's and a frame's time goes on tens of thousands of
//! curves. Opt-in (slow in debug builds):
//!
//! `cargo test --release -p drift-app perf_big -- --ignored --nocapture --test-threads=1`
//!
//! `perf_big` runs the whole app (with a GPU) on the
//! file `DRIFT_BIG` names, imported as `DRIFT_MODE` (`geometry`, the default, or
//! `underlay`); `DRIFT_BIG=synthetic:8000` imports [`big_drawing`] of 8,000 rooms instead.
//! `perf_big_drag` drags one curve, 1000 scattered curves, every curve and every point of
//! the `DRIFT_BIG` file (as geometry), calling `Editor::drag` and then with the Select tool
//! (`DRIFT_CASE` picks one by the start of its name), with the frame after each step.
//!
//! The ones without a window (`perf_big_synthetic`, `fixtures_save`) are drift-edit's
//! `perf_big`.

use std::collections::BTreeSet;
use std::time::Instant;

use drift_core::{DVec2, Document, EntityId};
use eframe::egui::Pos2;
use egui_kittest::Harness;

use crate::app::import_ui::ImportAs;
use drift_edit::editor::Editor;
use drift_edit::test_sketches::big_drawing;

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1e3
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
        let ed = &mut h.state_mut().drawing.editor;
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
        let ed = &h.state().drawing.editor;
        let curves = ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count();
        println!(
            "{curves} curves, {} fills, {} underlays",
            ed.doc.fills().count(),
            ed.doc.underlays().count()
        );
    }
    let fit = h.state().fit_bounds();
    h.state_mut().drawing.view.fit_target = Some(fit);
    let t = Instant::now();
    h.step();
    println!("first frame after import {:.0} ms", ms(t));
    h.run();
    h.state_mut().drawing.view.fit_target = None;
    let fitted = h.state().drawing.view.camera;

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
        h.state_mut().drawing.view.camera.pan(DVec2::new(4.0, 2.0));
    });
    {
        let c = &mut h.state_mut().drawing.view.camera;
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
        h.state_mut().drawing.view.camera.pan(DVec2::new(6.0, 3.0));
    });
    h.state_mut().drawing.view.camera = fitted;
    h.step();
    let one = {
        let ed = &h.state().drawing.editor;
        ed.doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point())
            .map(|(id, _)| id)
            .nth(1000)
    };
    if let Some(one) = one {
        h.state_mut().drawing.editor.selection = [one].into_iter().collect();
        phase("one curve picked, hover, fit", &mut h, 20, &mut |h, k| {
            h.hover_at(Pos2::new(500.0 + (k as f32 * 37.0) % 1100.0, 300.0));
        });
        h.state_mut().drawing.editor.selection.clear();
        h.step();
    }
    if mode != ImportAs::Geometry {
        return;
    }
    let curves: Vec<EntityId> = h
        .state()
        .drawing
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
        while h.state().drawing.editor.areas_pending() && frames < 2000 {
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
        app.drawing.editor.selection = [mid].into_iter().collect();
        assert_eq!(app.drawing.editor.nudge(DVec2::new(1.0, 0.0)), Some(true));
    });
    edit("undo", &mut h, &mut |app| {
        assert!(app.drawing.editor.undo())
    });
    let hundred: BTreeSet<EntityId> = curves
        .iter()
        .step_by(curves.len() / 100)
        .take(100)
        .copied()
        .collect();
    edit("delete 100 curves", &mut h, &mut |app| {
        app.drawing.editor.selection = hundred.clone();
        app.drawing.editor.delete_selection();
    });
    edit("undo delete", &mut h, &mut |app| {
        assert!(app.drawing.editor.undo())
    });
    edit("redo delete", &mut h, &mut |app| {
        assert!(app.drawing.editor.redo())
    });
    // The crossed fills' areas as remembered are those a fresh look finds.
    let ed = &mut h.state_mut().drawing.editor;
    ed.settle_areas();
    let kept = ed.fill_shapes();
    let (fills, bytes) = ed.crossed_fill_memo();
    println!(
        "crossed-fill memo: {} fills, {:.2} MB",
        fills,
        bytes as f64 / 1e6
    );
    ed.forget_fill_shapes();
    let fresh = ed.fill_shapes();
    assert_eq!(kept.len(), fresh.len());
    for (id, k) in kept.iter() {
        let f = &fresh[id];
        assert_eq!(
            format!("{:?}", k.region),
            format!("{:?}", f.region),
            "fill {id:?}"
        );
        assert_eq!(
            (&k.curves, k.crossed, k.lost),
            (&f.curves, f.crossed, f.lost)
        );
    }
    println!("fill shapes as kept = found afresh: {} fills", kept.len());
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
    h.state_mut().drawing.view.fit_target = Some(fit);
    h.run();
    h.state_mut().drawing.view.fit_target = None;
    let (curves, points) = {
        let ed = &h.state().drawing.editor;
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
        h.state().drawing.editor.doc.fills().count(),
        h.state().drawing.editor.doc.constraints().count()
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
            let ed = &mut h.state_mut().drawing.editor;
            ed.doc.begin();
            let scope = ed.scope_of_drag(&ids);
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
            .drawing
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
                .drawing
                .editor
                .drag(&ids, DVec2::new(2.0, 1.0), &dragged);
            let call = ms(t);
            let solves = h.state().drawing.editor.drag_solves();
            let t = Instant::now();
            h.step();
            let frame = ms(t);
            println!(
                "  step {k}: ok {ok}, drag call {call:.1} ms, {solves} solves, frame {frame:.1} ms"
            );
        }
        {
            let ed = &mut h.state_mut().drawing.editor;
            ed.doc.rollback();
            ed.sync();
        }
        h.step();
        h.step();
        // The same with the Select tool: its move of the picked curves (G), a few pixels a
        // frame, then a click to finish (the same moves as a drag of them).
        if ids.len() > 1 && label.contains("curves") {
            let first = ids[0];
            let was = h
                .state()
                .drawing
                .editor
                .doc
                .shape(first)
                .map(|s| s.bounds().min);
            // (The import's report closes first: with a window open the canvas takes no keys.)
            for _ in 0..3 {
                h.key_press(eframe::egui::Key::Escape);
                h.step();
            }
            assert!(!h.state().any_dialog_open(), "a window is open");
            h.state_mut().drawing.editor.selection = ids.iter().copied().collect();
            h.step();
            h.step();
            let at = h.state().drawing.view.canvas_rect.center();
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
                let now = h
                    .state()
                    .drawing
                    .editor
                    .doc
                    .shape(first)
                    .map(|s| s.bounds().min);
                println!(
                    "  select tool step {k}: frame {frame:.1} ms, {} solves, moved {:.1} mm",
                    h.state().drawing.editor.drag_solves(),
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
            while h.state().drawing.editor.areas_pending() {
                h.step();
            }
            assert!(h.state_mut().drawing.editor.undo());
            h.state_mut().drawing.editor.selection.clear();
            h.step();
            h.step();
        }
    }
}

/// The median of `n` timed runs of `f`, in ms.
fn median(n: usize, mut f: impl FnMut()) -> f64 {
    let mut times: Vec<f64> = (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            ms(t)
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[n / 2]
}

/// Saving and opening a big drawing (#185): `DRIFT_BIG` as for [`perf_big`] (`synthetic:N`
/// too). Times a save, an open (which writes the drawing again to look for things from a
/// newer Drift), and the same with an unknown table and field in the file (opened, saved
/// unchanged, saved after an edit). `DRIFT_SAVE_OUT` names a file for the saved text, to
/// compare writers byte for byte.
#[test]
#[ignore = "big-drawing probe: needs DRIFT_BIG and a GPU"]
fn perf_big_save() {
    let Ok(path) = std::env::var("DRIFT_BIG") else {
        return;
    };
    let mode = match std::env::var("DRIFT_MODE").as_deref() {
        Ok("underlay") => ImportAs::Underlay,
        _ => ImportAs::Geometry,
    };
    let reps: usize = std::env::var("DRIFT_REPS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(5);
    let synthetic = path
        .strip_prefix("synthetic")
        .map(|n| n.trim_start_matches(':').parse().unwrap_or(8_000));
    let doc = if let Some(cells) = synthetic {
        let mut ed = Editor::new(Document::new());
        ed.doc.begin();
        drift_io::dxf::add_to_document(&mut ed.doc, &big_drawing(cells));
        ed.commit_import("Import");
        ed.doc
    } else {
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
        h.state_mut().begin_import(&name, bytes, Some(mode), 0.0);
        let mut pending = h.state_mut().pending_import.take().expect("pending");
        h.state_mut().finish_import(&mut pending, 0.0);
        h.state().drawing.editor.doc.clone()
    };
    println!();
    println!(
        "{} curves, {} fills, {} underlays",
        doc.iter().filter(|(_, e)| !e.kind.is_point()).count(),
        doc.fills().count(),
        doc.underlays().count()
    );
    let meta = drift_io::Metadata {
        generator: Some("probe".into()),
        ..Default::default()
    };
    let mut text = String::new();
    let save = median(reps, || text = drift_io::save(&doc, &meta));
    let open = median(reps, || {
        let (_, m, _) = drift_io::load_with_notes(&text).unwrap();
        assert!(!m.carried.any());
    });
    println!(
        "save {save:.0} ms ({:.1} MB), open {open:.0} ms",
        text.len() as f64 / 1e6
    );
    // From a newer Drift: an unknown table (naming e1) and an unknown field on the first entity.
    let reader = format!("  \"reader\": {},\n", drift_io::READER_VERSION);
    let mut newer = text.replacen(
        &reader,
        &format!("{reader}  \"zz_newer\": {{\"a\":[1,2],\"on\":\"e1\"}},\n"),
        1,
    );
    newer = newer.replacen(
        "\"entities\": [\n    {",
        "\"entities\": [\n    {\"zz\":1,",
        1,
    );
    let mut opened = None;
    let open_newer = median(reps, || {
        opened = Some(drift_io::load_with_notes(&newer).unwrap())
    });
    let (mut again, again_meta, _) = opened.unwrap();
    assert!(again_meta.carried.any());
    let mut same = String::new();
    let unchanged = median(reps, || same = drift_io::save(&again, &again_meta));
    assert_eq!(same, newer, "an unchanged newer file saves as it was");
    // The same open again, and as a 42 file (no look for things from a newer Drift).
    let open2 = median(reps, || drop(drift_io::load_with_notes(&text).unwrap()));
    let old = text.replacen(
        &format!("\"version\": {},", drift_io::FORMAT_VERSION),
        "\"version\": 42,",
        1,
    );
    let old = old.replacen(
        &reader,
        "  \"reader\": 42,
",
        1,
    );
    let open42 = median(reps, || drop(drift_io::load_with_notes(&old).unwrap()));
    println!("open again {open2:.0} ms, open as 42 (no carry check) {open42:.0} ms");
    let point = again
        .iter()
        .find(|(_, e)| e.kind.is_point())
        .map(|(id, _)| id);
    if let Some(point) = point {
        let at = again.point(point).unwrap();
        again.begin();
        again.set_point(point, at + DVec2::new(1.0, 0.0)).unwrap();
        again.commit("Move");
    } else {
        again.begin();
        again.set_logo("<svg/>".into());
        again.commit("Logo");
    }
    let mut edited = drift_io::Saved {
        text: String::new(),
        loses_newer_data: true,
        carried: Default::default(),
    };
    let after_edit = median(reps, || {
        edited = drift_io::save_checked(&again, &again_meta)
    });
    assert!(!edited.loses_newer_data);
    assert!(edited.text.contains("\"zz_newer\""));
    assert_eq!(edited.text.contains("\"zz\":1"), newer.contains("\"zz\":1"));
    // Only an unknown field on the last entity: found only after looking through them all.
    let end = "}\n  ],\n  \"constraints\"";
    if text.contains(end) {
        let late = text.replacen(end, ",\"zz\":2}\n  ],\n  \"constraints\"", 1);
        let mut opened = None;
        let open_late = median(reps, || {
            opened = Some(drift_io::load_with_notes(&late).unwrap())
        });
        let (late_doc, late_meta, _) = opened.unwrap();
        assert!(late_meta.carried.any());
        let late_same = median(reps, || {
            assert_eq!(drift_io::save(&late_doc, &late_meta), late)
        });
        println!(
            "newer field on the last entity only: open {open_late:.0} ms, save unchanged \
             {late_same:.0} ms"
        );
    }
    // Whether the drawing read back writes the same again.
    let (read, read_meta, _) = drift_io::load_with_notes(&text).unwrap();
    let second = drift_io::save(&read, &read_meta);
    let differ = text
        .lines()
        .zip(second.lines())
        .filter(|(a, b)| a != b)
        .count();
    println!(
        "second save same: {} ({differ} lines differ, {} vs {} bytes)",
        second == text,
        text.len(),
        second.len()
    );
    println!(
        "newer file: open {open_newer:.0} ms, save unchanged {unchanged:.0} ms, \
         save after an edit {after_edit:.0} ms"
    );
    if let Ok(out) = std::env::var("DRIFT_SAVE_OUT") {
        std::fs::write(&out, &text).unwrap();
        std::fs::write(format!("{out}.edited"), &edited.text).unwrap();
        std::fs::write(format!("{out}.second"), &second).unwrap();
    }
}
