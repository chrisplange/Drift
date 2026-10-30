//! Big drawings without a window: where an edit's time goes on tens of thousands of curves,
//! and the writer on every fixture. Opt-in (slow in debug builds):
//!
//! `cargo test --release -p drift-edit perf_big -- --ignored --nocapture --test-threads=1`
//!
//! `perf_big_synthetic` needs no files; it imports [`big_drawing`] of 4,000 rooms. The probes
//! that run the whole app on a big drawing (`perf_big`, `perf_big_drag`, `perf_big_save`) are
//! drift-app's `perf_big`.

use std::time::Instant;

use drift_core::{DVec2, Document, EntityId};

use crate::editor::Editor;
use crate::test_sketches::big_drawing;

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

/// Every fixture opened and saved (#185): whether it saves as it was read, whether a second
/// save is the same, and (with `DRIFT_SAVE_OUT`, a folder) the saved text, to compare writers
/// byte for byte.
#[test]
#[ignore = "writer probe: run with --ignored --nocapture"]
fn fixtures_save() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../drift-io/tests/fixtures");
    let out = std::env::var("DRIFT_SAVE_OUT").ok();
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    println!();
    for path in names {
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (doc, meta, _) = drift_io::load_with_notes(&text).unwrap();
        let saved = drift_io::save(&doc, &meta);
        let (again, again_meta, _) = drift_io::load_with_notes(&saved).unwrap();
        let twice = drift_io::save(&again, &again_meta);
        println!(
            "{name:<14} {:>7} bytes  saves as read: {:<5}  second save same: {}",
            text.len(),
            saved == text,
            twice == saved
        );
        if let Some(out) = &out {
            std::fs::create_dir_all(out).unwrap();
            std::fs::write(std::path::Path::new(out).join(&name), &saved).unwrap();
            // A newer build's table laid out another way, merged after an edit.
            let reader = format!(
                "  \"reader\": {},
",
                drift_io::READER_VERSION
            );
            if saved.contains(&reader) {
                let newer = saved.replacen(
                    &reader,
                    &format!(
                        "{reader}  \"marks\":[
	{{ \"id\": \"m1\" }} ,

   {{\"x\":[1,
 2]}}
 ] ,
"
                    ),
                    1,
                );
                let (mut d, m, _) = drift_io::load_with_notes(&newer).unwrap();
                let p = d.iter().find(|(_, e)| e.kind.is_point()).unwrap().0;
                let at = d.point(p).unwrap();
                d.set_point(p, at + DVec2::X).unwrap();
                std::fs::write(
                    std::path::Path::new(out).join(format!("{name}.edited")),
                    drift_io::save(&d, &m),
                )
                .unwrap();
            }
        }
    }
    // The demo drawing, and a fragment as copy writes it.
    if let Some(out) = &out {
        let plate = drift_core::demo::mounting_plate();
        std::fs::write(
            std::path::Path::new(out).join("mounting_plate.drift"),
            drift_io::save(&plate, &Default::default()),
        )
        .unwrap();
    }
}
