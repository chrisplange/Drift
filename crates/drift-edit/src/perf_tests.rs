//! Viewport and editing torture tests: a 10,000-curve sketch through every per-frame and
//! per-edit path. Opt-in (they're slow in debug builds):
//!
//! `cargo test --release -p drift-edit perf -- --ignored --nocapture --test-threads=1`
//!
//! The tests that drive the whole app or build GPU frames are drift-app's `perf_tests`.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use drift_core::{Bounds, Constraint, DVec2, Document, EntityId};
use drift_geom::SnapOptions;

use crate::editor::Editor;
use crate::test_sketches::torture_sketch;

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

/// One timing taken once, as [`time`] gives it.
fn once(ms: f64) -> (f64, f64, f64) {
    (ms, 0.0, ms)
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
fn perf_editor_pipeline() {
    println!();
    let t = Instant::now();
    let (doc, curves) = torture_sketch();
    let entities = doc.len();
    report("build the sketch", once(t.elapsed().as_secs_f64() * 1e3));
    println!(
        "  {curves} curves, {entities} entities, {} relations",
        doc.constraints().count()
    );

    let t = Instant::now();
    let mut ed = Editor::new(doc);
    report(
        "open: solve, analyse, index",
        once(t.elapsed().as_secs_f64() * 1e3),
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

    // Per frame: styled items (the GPU frame at three zooms is drift-app's
    // `perf_editor_pipeline_frames`).
    let mut items = Vec::new();
    report(
        "sketch items (per frame)",
        time(20, |_| items = ed.items_styled(true, true)),
    );
    let bounds = ed.bounds();

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
    let drag = time(10, |_| {
        ed.drag(&some, v(0.5, 0.25), &dragged);
    });
    report("drag 1,000 curves (per mouse move)", drag);
    // (About 250 small parts with relations of their own: each part's solves look only at
    // its own relations, not the drawing's 10,000, #253.)
    assert!(
        drag.0 < 20.0,
        "drag 1,000 curves: {:.1} ms a mouse move (budget 20 ms)",
        drag.0
    );
    let t = Instant::now();
    ed.commit("Drag");
    report(
        "…and let go (commit, analyse)",
        once(t.elapsed().as_secs_f64() * 1e3),
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
                definition: Vec::new(),
                origin: DVec2::ZERO,
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

/// #265: a real PDF (`DRIFT_PDF`) grouped whole and opened for editing: how long the first
/// area lookup takes from the open group's curves (found on the spot, not by the areas
/// worker), against the drawing's own first areas, and the first Fill-tool click inside.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_group_edit_pdf() {
    let Ok(path) = std::env::var("DRIFT_PDF") else {
        return;
    };
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1e3;
    let bytes = std::fs::read(path).unwrap();
    let art = drift_io::pdf_import::read_artwork(&bytes, 1, "sheet").unwrap();
    let options = drift_io::vector::ImportOptions {
        scale: 1.0,
        construction_edges: true,
    };
    let import = drift_io::vector::to_import(&art, &options);
    let mut ed = Editor::new(Document::new());
    ed.doc.begin();
    let r = drift_io::dxf::add_to_document(&mut ed.doc, &import);
    assert!(ed.commit("Import"));
    println!("{} curves", r.curves);
    // (Found afresh, as the areas worker would after an edit.)
    ed.regions = None;
    ed.areas = Default::default();
    let t = Instant::now();
    let regions = ed.regions();
    println!(
        "drawing's areas, found afresh: {:.0} ms ({} areas)",
        ms(t),
        regions.len()
    );
    // A point inside a small area, to fill.
    let at = regions
        .iter()
        .filter(|r| r.area > 1.0)
        .min_by(|a, b| a.area.total_cmp(&b.area))
        .map(|r| crate::editor::interior_point(r, r.centroid))
        .unwrap();
    let t = Instant::now();
    ed.select_all();
    println!("select all: {:.0} ms", ms(t));
    let t = Instant::now();
    let g = ed.make_group().expect("grouped");
    println!("group: {:.0} ms", ms(t));
    // What Group left picked, to the file `DRIFT_DIGEST` names (to check against another
    // build).
    if let Ok(path) = std::env::var("DRIFT_DIGEST") {
        let digest = format!(
            "members {:?}\nselection {:?}\nfills {:?}\n",
            ed.doc.group(g).map(|g| &g.members),
            ed.selection,
            ed.selected_fills
        );
        std::fs::write(path, digest).unwrap();
    }
    let t = Instant::now();
    ed.open_group = Some(g);
    ed.selection.clear();
    ed.edit_scope();
    println!("open, scope found: {:.0} ms", ms(t));
    let t = Instant::now();
    let (scoped, _) = ed.scoped_areas();
    println!(
        "open group's areas, first: {:.0} ms ({} areas)",
        ms(t),
        scoped.len()
    );
    let t = Instant::now();
    ed.scoped_areas();
    println!("open group's areas, again: {:.3} ms", ms(t));
    let t = Instant::now();
    ed.fill_at(at);
    println!("fill_at (hover) in the group: {:.1} ms", ms(t));
    let t = Instant::now();
    ed.add_fill_at(at).expect("filled");
    println!("first Fill click in the group: {:.0} ms", ms(t));
    let t = Instant::now();
    let (scoped, _) = ed.scoped_areas();
    println!(
        "open group's areas after the fill: {:.0} ms ({} areas)",
        ms(t),
        scoped.len()
    );
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

/// f3's sketch (#282): a move of two lines held on an arc, the second held at `at` degrees
/// round it (315: a diameter from the first, a fold where the solve doesn't finish; the
/// arc's far end `fixed`, nothing carries it and every frame is refused); with `chain` bars
/// hanging from the arc's free end, free (`related` false) or each held at its length (in
/// the failing solve then). Returns (editor, the two lines).
fn held_on_an_arc(chain: usize, related: bool, fixed: bool, at: f64) -> (Editor, [EntityId; 2]) {
    let mut doc = Document::new();
    doc.begin();
    let p = [
        v(0.0, 0.0),
        v(8.0, 0.0),
        v(2.3431457505076203, 5.656854249492381),
        v(2.3431457505076203, 5.656854249492381),
        v(-7.316112512383063, 3.0686637984671727),
        v(9.414213562373096, 12.727922061357855),
        v(15.0, 0.0),
        v(8.0, 0.0) + DVec2::from_angle(at.to_radians()) * 8.0,
        v(8.0, 0.0) + DVec2::from_angle(at.to_radians()) * 8.0 + v(14.489, 3.8823),
    ]
    .map(|q| doc.add_point(q));
    let arc = doc.add_arc(p[1], p[2], p[0]).unwrap();
    let a = doc.add_line(p[3], p[4]).unwrap();
    let tangent = doc.add_line(p[2], p[5]).unwrap();
    doc.add_line(p[0], p[6]).unwrap();
    let b = doc.add_line(p[7], p[8]).unwrap();
    for c in [
        Constraint::PointOnCurve {
            point: p[3],
            curve: arc,
        },
        Constraint::Tangent { a: tangent, b: arc },
        Constraint::PointOnCurve {
            point: p[7],
            curve: arc,
        },
    ] {
        doc.add_constraint(c).unwrap();
    }
    if fixed {
        doc.add_constraint(Constraint::Fix {
            point: p[0],
            at: v(0.0, 0.0),
        })
        .unwrap();
    }
    let mut last = p[6];
    for k in 0..chain {
        let next = doc.add_point(v(15.0, -1.0 - k as f64));
        doc.add_line(last, next).unwrap();
        if related {
            doc.add_constraint(Constraint::Distance {
                a: last,
                b: next,
                value: doc.point(last).unwrap().distance(v(15.0, -1.0 - k as f64)),
            })
            .unwrap();
        }
        last = next;
    }
    doc.commit("Never finishes");
    (Editor::new(doc), [a, b])
}

/// The refused frame's cost (#281): a move whose solve doesn't finish at any step size,
/// held still so that every frame tries it again, alone and in a big part; and (#282) the
/// same unpinned, where the arc is carried along, and a move held on the arc that lands,
/// pinned or not (the check a landed frame pays).
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_drag_refused() {
    println!();
    for (chain, related, fixed, at, what) in [
        (0, false, true, 315.0, "refused drag, 0 free bars"),
        (2_000, false, true, 315.0, "refused drag, 2000 free bars"),
        (100, true, true, 315.0, "refused drag, 100 held bars"),
        (0, false, false, 315.0, "carried drag, 0 free bars"),
        (2_000, false, false, 315.0, "carried drag, 2000 free bars"),
        (
            2_000,
            false,
            false,
            250.0,
            "held drag landing, 2000 free bars",
        ),
        (
            2_000,
            false,
            true,
            250.0,
            "held drag landing, pinned, 2000 free bars",
        ),
    ] {
        let (mut ed, ids) = held_on_an_arc(chain, related, fixed, at);
        let dragged: Vec<drift_solver::Param> = ed
            .doc
            .defining_points(ids)
            .into_iter()
            .flat_map(drift_solver::Param::point)
            .collect();
        let delta = v(0.0, -18.003814381075802);
        ed.doc.begin();
        let mut solves = Vec::new();
        let mut refused = 0;
        // (A move that goes is taken out and back, as the pointer would: from where the
        // last frame left it, so a frame that goes doesn't take the next one further.)
        let mut applied = DVec2::ZERO;
        let r = time(20, |k| {
            let target = if k % 2 == 0 || (fixed && at == 315.0) {
                delta
            } else {
                DVec2::ZERO
            };
            if ed.drag(&ids, target - applied, &dragged) {
                applied = target;
            } else {
                refused += 1;
            }
            solves.push(ed.drag_solves);
        });
        ed.doc.rollback();
        report(&format!("{what} (per frame)"), r);
        let most = solves.iter().max().copied().unwrap_or(0);
        println!("  solves per frame: most {most}; {refused} of 20 frames refused");
    }
}

/// The torture sketch plus what makes a drawing heavy without changing often: a large
/// embedded font, blocks with many curves, an underlay with many curves, imported patterns.
fn heavy_document() -> Document {
    use drift_core::{Block, BlockItem, LineKind, Shape, Underlay};
    use drift_core::{EmbeddedFont, FontFile, FontSet};
    let (mut doc, _) = torture_sketch();
    doc.begin();
    let mut fonts = FontSet::default();
    fonts.embedded.insert(
        "Big".into(),
        EmbeddedFont {
            regular: FontFile::new(vec![7; 8 << 20], 0),
            bold: None,
        },
    );
    doc.set_fonts(fonts);
    for b in 0..20 {
        let items = (0..2_000)
            .map(|k| BlockItem {
                shape: Shape::Line {
                    a: v(k as f64, b as f64),
                    b: v(k as f64 + 1.0, b as f64 + 1.0),
                },
                style: LineKind::Solid,
                appearance: Default::default(),
            })
            .collect();
        doc.add_block(Block {
            name: format!("Block {b}"),
            items,
            fills: Vec::new(),
            profile: false,
        });
    }
    let prims = (0..50_000)
        .map(|k| drift_core::RefPrim::Line(v(k as f64, 0.0), v(k as f64, 1.0)))
        .collect();
    doc.add_underlay(Underlay::new("Plan", "plan.pdf", prims));
    doc.set_custom_patterns("*P, pattern\n0, 0,0, 0,1\n".repeat(20_000));
    doc.commit("Heavy");
    doc
}

/// A copy of the document (undo snapshots, options, option ghosts, agent edits) costs about
/// nothing: its parts are shared until one is changed.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_document_clone() {
    println!();
    let doc = heavy_document();
    let mut copies = Vec::new();
    let clone = time(50, |_| copies.push(doc.clone()));
    report("clone a heavy document", clone);
    copies.clear();
    let first = doc.iter().find(|(_, e)| e.kind.is_point()).unwrap().0;
    let edit = time(20, |k| {
        let mut copy = doc.clone();
        copy.begin();
        let _ = copy.set_point(first, v(k as f64, 0.0));
        copy.commit("Move");
    });
    report("clone, then move a point in the copy", edit);
    assert!(clone.0 < 0.05, "cloning took {:.3} ms", clone.0);
    assert_eq!(doc.clone(), doc);
}

/// A floor of 40 × 25 rooms walled along a grid (about 2,000 wall segments in 67 runs, all
/// one cluster: the worst case for re-chaining the outline), with 5,000 furniture lines
/// inside, and the centreline points of the grid.
pub(crate) fn wall_floor() -> (Document, Vec<Vec<EntityId>>) {
    use drift_core::{LayerId, LineKind, Path, Wall};
    let (cols, rows, step) = (40usize, 25usize, 4000.0);
    let mut doc = Document::new();
    let grid: Vec<Vec<EntityId>> = (0..=cols)
        .map(|i| {
            (0..=rows)
                .map(|j| doc.add_point(v(i as f64 * step, j as f64 * step)))
                .collect()
        })
        .collect();
    let run = |doc: &mut Document, pts: Vec<EntityId>| {
        let curves: Vec<EntityId> = pts
            .windows(2)
            .map(|w| {
                let l = doc.add_line(w[0], w[1]).unwrap();
                doc.set_style(l, LineKind::Construction);
                l
            })
            .collect();
        let path = doc
            .add_path(Path {
                curves,
                closed: false,
            })
            .unwrap();
        doc.add_wall(Wall::new(path, 200.0, LayerId::DEFAULT))
            .unwrap();
    };
    for j in 0..=rows {
        run(&mut doc, grid.iter().map(|col| col[j]).collect());
    }
    for col in &grid {
        run(&mut doc, col.clone());
    }
    // Furniture: short lines inside the rooms.
    let mut rng = Lcg(7);
    for _ in 0..5000 {
        let (i, j) = (
            (rng.next() * cols as f64) as usize,
            (rng.next() * rows as f64) as usize,
        );
        let o = v(i as f64 * step + 500.0, j as f64 * step + 500.0);
        let a = doc.add_point(o + v(rng.next() * 2500.0, rng.next() * 2500.0));
        let b = doc.add_point(o + v(rng.next() * 2500.0, rng.next() * 2500.0));
        let _ = doc.add_line(a, b);
    }
    drift_geom::walls::regenerate(&mut doc);
    (doc, grid)
}

/// Walls (docs/walls-design.md §9.4): regenerating a whole floor, a commit of one wall
/// edit, a drag frame of a 4-way node, the cluster's poché, and opening the file.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_walls() {
    println!();
    let t = Instant::now();
    let (doc, grid) = wall_floor();
    report(
        "build and make a 2,000-segment floor",
        once(t.elapsed().as_secs_f64() * 1e3),
    );
    let faces = doc.generated_entities().len();
    println!(
        "  {} walls, {} entities ({faces} made by walls)",
        doc.walls().count(),
        doc.len()
    );
    let mut check = doc.clone();
    report(
        "regenerate every wall (nothing changed)",
        time(5, |_| {
            let r = drift_geom::walls::regenerate(&mut check);
            assert!(!r.changed);
        }),
    );
    report(
        "  of which: read the walls (inputs)",
        time(5, |_| {
            let _ = drift_geom::walls::wall_inputs(&doc);
        }),
    );
    let (inputs, _) = drift_geom::walls::wall_inputs(&doc);
    report(
        "  of which: generate (nodes, faces, outlines)",
        time(5, |_| {
            let _ = drift_geom::walls::generate(&inputs, 0.5);
        }),
    );
    report(
        "  owners index",
        time(5, |_| {
            let _ = doc.wall_owners();
        }),
    );
    let text = drift_io::save(&doc, &drift_io::Metadata::default());
    report(
        "open the file (read, check the walls)",
        time(3, |_| {
            let (d, _) = drift_io::load(&text).unwrap();
            let _ = Editor::new(d);
        }),
    );
    let mut ed = Editor::new(doc);
    ed.prefs.auto_relations = false;
    let node = grid[20][12];
    let at = ed.doc.point(node).unwrap();
    // A furniture point, for the same edits without walls.
    let owners = ed.doc.wall_owners();
    let loose = ed
        .doc
        .iter()
        .find(|(id, e)| e.kind.is_point() && owners.get(*id).is_none())
        .map(|(id, _)| id)
        .unwrap();
    let loose_at = ed.doc.point(loose).unwrap();
    let commit = time(10, |k| {
        ed.doc.begin();
        let _ = ed.doc.set_point(node, at + v(10.0 * (k % 3) as f64, 0.0));
        ed.commit("Move a node");
    });
    report("commit of one wall edit (move a 4-way node)", commit);
    let walls_part = time(10, |k| {
        ed.doc.begin();
        let _ = ed
            .doc
            .set_point(node, at + v(5.0 + 10.0 * (k % 3) as f64, 0.0));
        ed.follow_walls();
        ed.doc.rollback();
    });
    report("  of which the walls' part", walls_part);
    let baseline = time(10, |k| {
        ed.doc.begin();
        let _ = ed
            .doc
            .set_point(loose, loose_at + v(10.0 * (k % 3) as f64, 0.0));
        ed.commit("Move a point");
    });
    report("  the same commit moving a furniture point", baseline);
    let tx_entities = {
        ed.doc.begin();
        let _ = ed.doc.set_point(node, at + v(37.0, 0.0));
        ed.follow_walls();
        let tx = ed.doc.commit("Probe");
        tx.entities().count()
    };
    println!("  one node moved: {tx_entities} entities journalled");
    ed.doc.begin();
    let frame = time(20, |k| {
        let _ = ed.drag(&[node], v(1.0 + (k % 4) as f64, 0.0), &[]);
        ed.sync();
    });
    ed.doc.rollback();
    ed.sync();
    report("drag frame of a 4-way node (with the walls)", frame);
    ed.doc.begin();
    let loose_frame = time(20, |k| {
        let _ = ed.drag(&[loose], v(1.0 + (k % 4) as f64, 0.0), &[]);
        ed.sync();
    });
    ed.doc.rollback();
    ed.sync();
    report("  the same drag frame of a furniture point", loose_frame);
    println!(
        "  walls add {:.2} ms to a commit and {:.2} ms to a drag frame",
        walls_part.0,
        frame.0 - loose_frame.0
    );
    let fill = ed.doc.fills().next().map(|(id, _)| id).unwrap();
    let poche = time(10, |_| {
        let _ = ed.wall_fill_region(fill);
    });
    report("the cluster's poché region", poche);
    // A clear width between two walls' faces near the node (W2): the solve sees it lowered
    // onto their centrelines.
    let owners = ed.doc.wall_owners();
    let faces: Vec<(EntityId, DVec2, DVec2)> = ed
        .doc
        .walls()
        .flat_map(|(_, w)| {
            w.made
                .curves
                .iter()
                .filter(|(k, _)| matches!(k.part, drift_core::Part::Left | drift_core::Part::Right))
                .map(|(_, id)| *id)
                .collect::<Vec<_>>()
        })
        .filter_map(|id| match ed.doc.shape(id)? {
            drift_core::Shape::Line { a, b } => Some((id, a, b)),
            _ => None,
        })
        .collect();
    let near = |(_, a, b): &(EntityId, DVec2, DVec2)| ((*a + *b) / 2.0).distance(at);
    let first = *faces
        .iter()
        .min_by(|x, y| near(x).total_cmp(&near(y)))
        .unwrap();
    let d0 = (first.2 - first.1).normalize();
    let mid = (first.1 + first.2) / 2.0;
    let other = *faces
        .iter()
        .filter(|f| owners.wall(f.0) != owners.wall(first.0))
        .filter(|f| (f.2 - f.1).normalize().perp_dot(d0).abs() < 1e-9)
        .filter(|f| d0.perp_dot((f.1 + f.2) / 2.0 - mid).abs() > 1.0)
        .min_by(|x, y| {
            let off = |f: &(EntityId, DVec2, DVec2)| {
                d0.perp_dot((f.1 + f.2) / 2.0 - mid).abs()
                    + 1e-3 * d0.dot((f.1 + f.2) / 2.0 - mid).abs()
            };
            off(x).total_cmp(&off(y))
        })
        .unwrap();
    let c = drift_core::Constraint::PointLineDistance {
        point: first.0,
        line: other.0,
        value: 0.0,
    };
    let c = c.with_value(drift_solver::measure(&ed.doc, &c).unwrap());
    let dim = ed.add_dimension(c, None).expect("a face dimension");
    assert!(!ed.doc.is_driven(dim), "{:?}", ed.out.notice);
    let value = c.value().unwrap();
    let typed = time(6, |k| {
        let units = ed.doc.units();
        let text = units.format_length(value + 10.0 * (k % 3) as f64);
        ed.set_dimension_text(dim, &text).unwrap();
    });
    report(
        "type a clear width between two walls' faces (commit)",
        typed,
    );
    let face_commit = time(10, |k| {
        ed.doc.begin();
        let _ = ed.doc.set_point(node, at + v(10.0 * (k % 3) as f64, 0.0));
        ed.commit("Move a node");
    });
    report(
        "commit of one wall edit, with a face dimension",
        face_commit,
    );
    let face_walls = time(10, |k| {
        ed.doc.begin();
        let _ = ed
            .doc
            .set_point(node, at + v(5.0 + 10.0 * (k % 3) as f64, 0.0));
        ed.follow_walls();
        ed.doc.rollback();
    });
    report("  of which the walls' part", face_walls);
    let lower = time(10, |_| {
        let _ = drift_geom::walls::lower(&ed.doc);
    });
    report("  lowering the face relations", lower);
    ed.doc.begin();
    let face_frame = time(20, |k| {
        let _ = ed.drag(&[node], v(1.0 + (k % 4) as f64, 0.0), &[]);
        ed.sync();
    });
    ed.doc.rollback();
    ed.sync();
    report("drag frame of the node, with a face dimension", face_frame);
    println!(
        "  with a face dimension: walls add {:.2} ms to a commit and {:.2} ms to a drag frame",
        face_walls.0,
        face_frame.0 - loose_frame.0
    );
    println!("  budgets: commit 5 ms, drag frame +2 ms, poché 2 ms, open +150 ms");
}

/// Openings (docs/walls-design.md §9.4, W3): the floor of [`wall_floor`] with a door in every
/// room's lower wall (1,000 openings with their symbols): a commit and a drag frame moving a
/// 4-way node, sliding an opening (commit and drag frame), and opening the file.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_walls_openings() {
    use drift_core::{Opening, Symbol};
    println!();
    let t = Instant::now();
    let (mut doc, grid) = wall_floor();
    // The rows of walls are the first runs: a door centred on each segment of rows 0..25.
    let rows: Vec<drift_core::WallId> = doc.walls().map(|(id, _)| id).take(25).collect();
    for w in rows {
        for k in 0..40u16 {
            let mut o = Opening::new(w, k, 970.0);
            o.leaf = 900.0;
            o.symbol = Symbol::Library {
                family: "door.single".into(),
                type_name: "900 × 2040".into(),
            };
            doc.add_opening(o).unwrap();
        }
    }
    let mut ed = Editor::new(doc);
    ed.prefs.auto_relations = false;
    report(
        "floor with 1,000 doors (walls, cuts, symbols)",
        once(t.elapsed().as_secs_f64() * 1e3),
    );
    println!(
        "  {} openings, {} entities ({} made by walls and openings)",
        ed.doc.openings().count(),
        ed.doc.len(),
        ed.doc.generated_entities().len()
    );
    let mut check = ed.doc.clone();
    report(
        "regenerate every wall with its cuts",
        time(5, |_| {
            let r = drift_geom::walls::regenerate(&mut check);
            assert!(!r.changed);
        }),
    );
    let node = grid[20][12];
    let at = ed.doc.point(node).unwrap();
    let owners = ed.doc.wall_owners();
    let loose = ed
        .doc
        .iter()
        .find(|(id, e)| e.kind.is_point() && owners.get(*id).is_none())
        .map(|(id, _)| id)
        .unwrap();
    let walls_part = time(10, |k| {
        ed.doc.begin();
        let _ = ed
            .doc
            .set_point(node, at + v(5.0 + 10.0 * (k % 3) as f64, 0.0));
        ed.follow_walls();
        ed.doc.rollback();
    });
    report("walls' part of a commit (move a 4-way node)", walls_part);
    ed.sync();
    ed.doc.begin();
    let frame = time(20, |k| {
        let _ = ed.drag(&[node], v(1.0 + (k % 4) as f64, 0.0), &[]);
        ed.sync();
    });
    ed.doc.rollback();
    ed.sync();
    report("drag frame of a 4-way node (doors beside it)", frame);
    ed.doc.begin();
    let loose_frame = time(20, |k| {
        let _ = ed.drag(&[loose], v(1.0 + (k % 4) as f64, 0.0), &[]);
        ed.sync();
    });
    ed.doc.rollback();
    ed.sync();
    report("  the same drag frame of a furniture point", loose_frame);
    // Slide the door beside the node along its wall.
    let door = ed
        .doc
        .openings()
        .find(|(_, o)| o.seg == 20)
        .map(|(id, _)| id)
        .unwrap();
    let (u0, _) = ed.opening_span(door).unwrap();
    let slide_part = time(10, |k| {
        ed.doc.begin();
        let _ = ed.slide_opening(door, u0 + 10.0 * (k % 3) as f64 + 5.0);
        ed.doc.rollback();
    });
    report("walls' part of sliding a door (cut and symbol)", slide_part);
    ed.sync();
    ed.doc.begin();
    let slide_frame = time(20, |k| {
        let _ = ed.slide_opening(door, u0 + 1.0 + (k % 4) as f64);
        ed.sync();
    });
    ed.doc.rollback();
    ed.sync();
    report("drag frame sliding a door", slide_frame);
    let text = drift_io::save(&ed.doc, &drift_io::Metadata::default());
    report(
        "open the file (read, check walls and openings)",
        time(3, |_| {
            let (d, _) = drift_io::load(&text).unwrap();
            let _ = Editor::new(d);
        }),
    );
    println!(
        "  walls and openings add {:.2} ms to a commit, {:.2} ms to a node's drag frame and \
         {:.2} ms to a door's slide frame",
        walls_part.0,
        frame.0 - loose_frame.0,
        slide_frame.0 - loose_frame.0
    );
    println!("  budgets: commit 5 ms, drag frame +2 ms");
}

/// Drag frames on big drawings (sweep items A1, A2): a frame is the drag, the editor's sync
/// and the fills' shapes, as the canvas asks for them. The tree of the spatial index waits
/// for the end of the drag, and click-fills away from the drag keep their areas.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_drag_frames() {
    println!();
    let frame = |ed: &mut Editor, point: EntityId| {
        ed.doc.begin();
        let r = time(20, |k| {
            let _ = ed.drag(&[point], v(1.0 + (k % 4) as f64, 0.0), &[]);
            ed.sync();
            let _ = ed.fill_shapes();
        });
        ed.doc.rollback();
        ed.sync();
        r
    };

    // A1: the wall floor (26,000 entities), dragging a furniture point.
    let (doc, grid) = wall_floor();
    let mut ed = Editor::new(doc);
    ed.prefs.auto_relations = false;
    let owners = ed.doc.wall_owners();
    let loose = ed
        .doc
        .iter()
        .find(|(id, e)| e.kind.is_point() && owners.get(*id).is_none())
        .map(|(id, _)| id)
        .unwrap();
    let floor = frame(&mut ed, loose);
    report("wall floor: drag frame of a furniture point", floor);
    let node = frame(&mut ed, grid[20][12]);
    report("wall floor: drag frame of a 4-way node", node);
    // Picking right after the drag finds the point where it was let go.
    let at = ed.doc.point(loose).unwrap();
    ed.doc.begin();
    let _ = ed.drag(&[loose], v(300.0, 0.0), &[]);
    ed.sync();
    assert_eq!(
        ed.index.pick(at + v(300.0, 0.0), 1.0, |_| true),
        Some(loose)
    );
    assert!(ed.commit("Drag"));
    assert_eq!(
        ed.index.pick(at + v(300.0, 0.0), 1.0, |_| true),
        Some(loose)
    );
    assert_ne!(ed.index.pick(at, 1.0, |_| true), Some(loose));

    // A2: 10,000 curves, without and with one click-fill away from the dragged point.
    let (doc, _) = torture_sketch();
    let mut ed = Editor::new(doc);
    let far = ed
        .doc
        .iter()
        .find(|(id, e)| e.kind.is_point() && ed.doc.point(*id).unwrap().x > 300.0)
        .map(|(id, _)| id)
        .unwrap();
    let bare = frame(&mut ed, far);
    report("10k curves: drag frame, no fills", bare);
    let fill = ed.add_fill_at(v(10.0, 7.0)).unwrap();
    let _ = ed.fill_shapes();
    let filled = frame(&mut ed, far);
    report("10k curves: drag frame, one click-fill", filled);
    assert!(!ed.fill_shapes()[&fill].lost);

    println!("  budgets: wall floor frame 3 ms (a node 6 ms); a click-fill adds under 5 ms");
    assert!(floor.0 < 3.0, "wall floor drag frame {:.2} ms", floor.0);
    assert!(node.0 < 6.0, "wall node drag frame {:.2} ms", node.0);
    assert!(
        filled.0 < bare.0 + 5.0,
        "a click-fill makes a drag frame {:.2} ms (without: {:.2} ms)",
        filled.0,
        bare.0
    );
}

/// Per-frame item lists and block-copy hover (sweep items A4, A3): the sketch's items no
/// longer sort every entity each frame, and hover tests only the copies whose box is near.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_items_and_copy_hover() {
    use drift_core::blocks::{Block, BlockItem, Instance};
    println!();
    // A4: 10,000 curves, nothing picked and 1,000 picked.
    let (doc, _) = torture_sketch();
    let mut ed = Editor::new(doc);
    let items = time(20, |_| {
        let _ = ed.items_styled(true, true);
    });
    report("10k curves: sketch items (per frame)", items);
    ed.selection = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .take(1000)
        .collect();
    let picked = time(20, |_| {
        let _ = ed.items_styled(true, true);
    });
    report("10k curves: items with 1,000 picked", picked);
    // #259: with View → End points on, the lit points are found too (only centres follow
    // them), where before the toggle skipped that.
    ed.prefs.always_endpoints = true;
    let picked_ends = time(20, |_| {
        let _ = ed.items_styled(true, true);
    });
    report(
        "10k curves: items with 1,000 picked, End points on",
        picked_ends,
    );
    ed.prefs.always_endpoints = false;
    // #259: the select tool's check of a centre under the pointer, at its worst: every
    // other curve picked, so it looks through them all.
    let (circle, centre) = ed
        .doc
        .iter()
        .find_map(|(id, e)| match e.kind {
            drift_core::EntityKind::Circle { center, .. } => Some((id, center)),
            _ => None,
        })
        .expect("a circle");
    ed.selection = ed
        .doc
        .iter()
        .filter(|(id, e)| !e.kind.is_point() && *id != circle)
        .map(|(id, _)| id)
        .collect();
    let picks = ed.selection.len();
    let unseen = time(100, |_| {
        assert!(ed.centre_unseen(centre));
    });
    report(
        &format!("{picks} picked: a hidden centre under the pointer"),
        unseen,
    );
    ed.selection.clear();
    // The same entities spread over 12 layers (as imports come in).
    let layers: Vec<drift_core::LayerId> = (0..12)
        .map(|k| {
            ed.doc
                .add_layer(drift_core::Layer::new(format!("Layer {k}")))
        })
        .collect();
    let all: Vec<EntityId> = ed.doc.iter().map(|(id, _)| id).collect();
    for (k, id) in all.into_iter().enumerate() {
        ed.doc.set_entity_layer(id, layers[k % 12]).unwrap();
    }
    ed.sync();
    let layered = time(20, |_| {
        let _ = ed.items_styled(true, true);
    });
    report("10k curves on 12 layers: sketch items", layered);
    let handles = time(100, |_| {
        let _ = ed.hidden_handles();
    });
    report("10k curves: hidden handles", handles);

    // A3: 120 copies of a 400-line block, hovering at random spots.
    let mut doc = Document::new();
    let lines: Vec<BlockItem> = (0..400)
        .map(|k| {
            let (x, y) = ((k % 20) as f64 * 50.0, (k / 20) as f64 * 50.0);
            BlockItem {
                shape: drift_core::Shape::Line {
                    a: v(x, y),
                    b: v(x + 40.0, y + 10.0),
                },
                style: drift_core::LineKind::Solid,
                appearance: Default::default(),
            }
        })
        .collect();
    let block = doc.add_block(Block {
        name: "Desk".into(),
        items: lines,
        ..Default::default()
    });
    for k in 0..120 {
        let at = v((k % 12) as f64 * 1500.0, (k / 12) as f64 * 1500.0);
        doc.add_instance(Instance::new(block, at)).unwrap();
    }
    let ed = Editor::new(doc);
    let mut rng = Lcg(11);
    let spots: Vec<DVec2> = (0..500)
        .map(|_| v(rng.next() * 18000.0, rng.next() * 15000.0))
        .collect();
    let hover = time(500, |i| {
        let _ = ed.instance_at(spots[i], 2.0);
    });
    report("120 block copies: hover (instance_at)", hover);
    // Still finds a copy by its curve, and by its box.
    let first = ed.doc.instances().next().map(|(id, _)| id).unwrap();
    assert_eq!(ed.instance_at(v(20.0, 5.0), 1.0), Some(first));
    assert_eq!(ed.instance_at(v(45.0, 30.0), 1.0), Some(first));
    assert_eq!(ed.instance_at(v(1200.0, 1200.0), 1.0), None);

    println!(
        "  budgets: items 3.3 ms, on 12 layers 4.5 ms, with 1,000 picked 4.2 ms (medians), \
         handles 0.3 ms, copy hover 0.5 ms"
    );
    // The items budgets: p90 of 8 alternated rounds, 2026-10-30, main and branch alike; the
    // item loop (25k entities, ~100 ns each) is its own task.
    assert!(items.2 < 3.3, "sketch items {:.2} ms (median)", items.2);
    assert!(
        picked.2 < 4.2,
        "items with 1,000 picked {:.2} ms (median)",
        picked.2
    );
    assert!(layered.0 < 4.5, "items on 12 layers {:.2} ms", layered.0);
    assert!(handles.0 < 0.3, "hidden handles {:.3} ms", handles.0);
    assert!(hover.0 < 0.5, "copy hover {:.2} ms", hover.0);
}

/// A hundred rectangle offsets (#247): made again from nothing changed, one source corner
/// moved and committed (the regeneration of the offsets the edit touched), and twenty drag
/// frames of one source point of a pen-curve offset (re-fitted every frame). New lines with
/// this PR: no base to compare with.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_offsets() {
    use drift_core::{Offset, Side};
    println!();
    let mut ed = Editor::default();
    ed.doc.begin();
    let mut corners = Vec::new();
    let mut first_line = None;
    for i in 0..100 {
        let x0 = f64::from(i % 10) * 100.0;
        let y0 = f64::from(i / 10) * 100.0;
        let ps = [
            v(x0, y0),
            v(x0 + 60.0, y0),
            v(x0 + 60.0, y0 + 40.0),
            v(x0, y0 + 40.0),
        ]
        .map(|p| ed.doc.add_point(p));
        let lines: Vec<EntityId> = (0..4)
            .map(|k| ed.doc.add_line(ps[k], ps[(k + 1) % 4]).unwrap())
            .collect();
        first_line.get_or_insert(lines[0]);
        corners.push(ps[1]);
        ed.doc
            .add_offset(Offset::new(lines, Side::Right, 3.0))
            .unwrap();
    }
    // A pen curve offset too, for the drag.
    let arch = [
        v(1200.0, 0.0),
        v(1200.0, 200.0),
        v(1600.0, 200.0),
        v(1600.0, 0.0),
    ]
    .map(|p| ed.doc.add_point(p));
    let curve = ed
        .doc
        .add_bezier(arch[0], arch[1], arch[2], arch[3])
        .unwrap();
    ed.doc
        .add_offset(Offset::new(vec![curve], Side::Left, 20.0))
        .unwrap();
    ed.commit("Offsets");
    println!(
        "  {} offsets, {} entities ({} made by offsets)",
        ed.doc.offsets().count(),
        ed.doc.len(),
        ed.doc.generated_entities().len()
    );
    let mut check = ed.doc.clone();
    report(
        "regenerate every offset (nothing changed)",
        time(8, |_| {
            let r = drift_geom::offsets::regenerate(&mut check);
            assert!(!r.changed);
        }),
    );
    let corner = corners[0];
    let at = ed.doc.point(corner).unwrap();
    report(
        "move one source corner and commit",
        time(8, |i| {
            ed.doc.begin();
            ed.doc
                .set_point(corner, at + v((i % 2) as f64, 0.0))
                .unwrap();
            assert!(ed.commit("Move"));
        }),
    );
    let dragged = drift_solver::Param::point(arch[1]).to_vec();
    ed.doc.begin();
    report(
        "20 drag frames of a pen curve's handle (re-fitted)",
        time(8, |i| {
            for k in 0..20 {
                let d = v(0.0, (i % 2) as f64 + 0.05 * f64::from(k));
                assert!(ed.drag(&[arch[1]], d, &dragged));
            }
        }),
    );
    ed.doc.rollback();
    ed.sync();
    let _ = first_line;
}

/// #265 (review): frames of moving a picked region inside an open group: (a) a block edit of
/// a block-sized drawing, (b) an open group holding the whole drawing of `DRIFT_PDF` (made
/// with `add_group`). A frame is the drag, the editor's sync and the picked region, as the
/// canvas asks for it.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_region_move_in_open_group() {
    use crate::editor::RegionMotion;
    let frames = |ed: &mut Editor, label: &str| {
        let r = ed.region().expect("a picked region");
        let ids = ed.region_curves(&r);
        assert!(!ids.is_empty(), "the region's curves");
        ed.selection = ids.iter().copied().collect();
        let before = ed.regions_found;
        let scoped_before = ed.scoped_regions_found;

        ed.doc.begin();
        let mut times = Vec::new();
        let mut offset = DVec2::ZERO;
        for _ in 0..12 {
            let t = Instant::now();
            let step = v(0.5, 0.25);
            if ed.drag(&ids, step, &[]) {
                offset += step;
                ed.region_motion = Some(RegionMotion::Move(offset));
            }
            ed.sync();
            assert!(ed.region().is_some());
            times.push(t.elapsed().as_secs_f64() * 1e3);
        }

        ed.doc.rollback();
        ed.region_motion = None;
        ed.sync();
        times.sort_by(f64::total_cmp);
        println!(
            "{label:<44} frame median {:>8.2} ms worst {:>8.2} ms  (moved {offset}; areas found: drawing's {}, the group's {})",
            times[times.len() / 2],
            times[times.len() - 1],
            ed.regions_found - before,
            ed.scoped_regions_found - scoped_before
        );
    };
    // (a) A block of a 20 × 10 rectangle with a line across it, edited in place, its left
    // half picked.
    let mut doc = Document::new();
    let p =
        [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)].map(|(x, y)| doc.add_point(v(x, y)));
    let mut lines: BTreeSet<EntityId> = (0..4)
        .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    let (a, b) = (doc.add_point(v(10.0, 0.0)), doc.add_point(v(10.0, 10.0)));
    lines.insert(doc.add_line(a, b).unwrap());
    let mut ed = Editor::new(doc);
    ed.selection = lines;
    let copy = ed.make_block("Box").unwrap();
    ed.edit_block(copy);
    ed.selected_region = Some(v(5.0, 5.0));
    frames(&mut ed, "(a) block edit, region move");
    // (b) The whole PDF in one open group.
    let Ok(path) = std::env::var("DRIFT_PDF") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let art = drift_io::pdf_import::read_artwork(&bytes, 1, "sheet").unwrap();
    let options = drift_io::vector::ImportOptions {
        scale: 1.0,
        construction_edges: true,
    };
    let import = drift_io::vector::to_import(&art, &options);
    let mut ed = Editor::new(Document::new());
    ed.doc.begin();
    let r = drift_io::dxf::add_to_document(&mut ed.doc, &import);
    assert!(ed.commit("Import"));
    let members: BTreeSet<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .collect();
    ed.doc.begin();
    let g = ed
        .doc
        .add_group(drift_core::Group {
            name: "All".into(),
            members,
        })
        .unwrap();
    ed.commit("Group");
    ed.open_group = Some(g);
    let regions = ed.regions();
    let mut small: Vec<_> = regions.iter().filter(|r| r.area > 1.0).collect();
    small.sort_by(|a, b| a.area.total_cmp(&b.area));
    // (The smallest whose outline shows, as a click would pick it.)
    let at = small
        .iter()
        .map(|r| crate::editor::interior_point(r, r.centroid))
        .find(|p| {
            ed.visible_region_at(*p)
                .is_some_and(|r| !ed.region_curves(&r).is_empty())
        })
        .unwrap();
    ed.selected_region = Some(at);
    assert!(ed.region().is_some());
    println!("({} curves)", r.curves);
    frames(&mut ed, "(b) the whole PDF in an open group, region move");
    // (c) #321: the same area picked with nothing open, the drawing's own areas.
    ed.open_group = None;
    ed.selection.clear();
    ed.selected_region = Some(at);
    assert!(ed.region().is_some());
    frames(&mut ed, "(c) the whole PDF, nothing open, region move");
    // The same move landed: the picked area, every fill's shape and the selection, to the
    // file `DRIFT_DIGEST` names (to check against another build).
    if let Ok(path) = std::env::var("DRIFT_DIGEST") {
        let r = ed.region().expect("a picked region");
        let ids = ed.region_curves(&r);
        ed.selection = ids.iter().copied().collect();
        ed.doc.begin();
        let mut offset = DVec2::ZERO;
        for _ in 0..12 {
            let step = v(0.5, 0.25);
            if ed.drag(&ids, step, &[]) {
                offset += step;
                ed.region_motion = Some(RegionMotion::Move(offset));
            }
            ed.sync();
            ed.region();
        }
        assert!(ed.commit("Move"));
        ed.region_motion = None;
        ed.settle_areas();
        let landed = ed.region();
        let mut digest = format!(
            "moved {offset}\narea {landed:?}\nselection {:?}\n",
            ed.selection
        );
        for (id, s) in ed.fill_shapes().iter() {
            digest += &format!(
                "{id:?} {:?} {:?} {} {}\n",
                s.region, s.curves, s.lost, s.crossed
            );
        }
        std::fs::write(path, digest).unwrap();
    }
}

/// #317: grouping a big pick (10,000 loose lines, and the same as 100 groups of 100), and
/// picking the group again, scale with the pick, not its square.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_group_large_pick() {
    let lines = |n: usize| {
        let mut doc = Document::new();
        for k in 0..n {
            let (x, y) = ((k % 100) as f64 * 3.0, (k / 100) as f64 * 3.0);
            let a = doc.add_point(v(x, y));
            let b = doc.add_point(v(x + 2.0, y + 1.0));
            doc.add_line(a, b).unwrap();
        }
        let mut ed = Editor::new(doc);
        ed.select_all();
        ed
    };
    println!();
    let mut ed = lines(10_000);
    let t = Instant::now();
    let g = ed.make_group().expect("grouped");
    let group_ms = t.elapsed().as_secs_f64() * 1e3;
    report("group 10,000 lines", once(group_ms));
    assert_eq!(ed.doc.group(g).unwrap().members.len(), 10_000);
    ed.selection = ed.selection.iter().copied().take(1).collect();
    let t = Instant::now();
    ed.expand_groups();
    let expand_ms = t.elapsed().as_secs_f64() * 1e3;
    report("pick a 10,000-line group whole", once(expand_ms));
    assert_eq!(ed.selection.len(), 10_000);
    // 100 groups of 100, all picked, grouped into one.
    let mut ed = lines(10_000);
    let all: Vec<EntityId> = ed.selection.iter().copied().collect();
    for chunk in all.chunks(100) {
        ed.selection = chunk.iter().copied().collect();
        ed.make_group().expect("grouped");
    }
    ed.select_all();
    let t = Instant::now();
    ed.expand_groups();
    let many_ms = t.elapsed().as_secs_f64() * 1e3;
    report("pick 100 groups of 100 whole", once(many_ms));
    let t = Instant::now();
    ed.make_group().expect("grouped");
    let regroup_ms = t.elapsed().as_secs_f64() * 1e3;
    report("group 100 groups of 100 into one", once(regroup_ms));
    assert_eq!(ed.doc.groups().count(), 1);
    // A click in a drawing of 1,000 groups of 50 lines: the group it picks, widened.
    let mut ed = lines(50_000);
    let all: Vec<EntityId> = ed.selection.iter().copied().collect();
    for chunk in all.chunks(50) {
        ed.doc
            .add_group(drift_core::Group {
                name: String::new(),
                members: chunk.iter().copied().collect(),
            })
            .unwrap();
    }
    let clicked = all[25_025];
    report(
        "click one of 1,000 groups of 50, widened",
        time(20, |_| {
            ed.selection = ed.pick_set(clicked);
            ed.expand_groups();
            assert_eq!(ed.selection.len(), 50);
        }),
    );
    // (Linear work on 10,000 lines takes milliseconds; the square of it, seconds.)
    for (what, ms) in [
        ("group", group_ms),
        ("expand", expand_ms),
        ("expand many", many_ms),
        ("regroup", regroup_ms),
    ] {
        assert!(ms < 250.0, "{what} took {ms:.0} ms");
    }
}

/// #253: the Inspector's relation and dimension offers, asked every frame while things are
/// picked, for `n` picked loose lines: they grow with the pick, not its square (12 s a frame
/// for 66,844 lines before).
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_relation_options_large_pick() {
    println!();
    let mut at_40k = 0.0;
    for n in [2_500usize, 5_000, 10_000, 20_000, 40_000, 66_844] {
        let mut doc = Document::new();
        for k in 0..n {
            let (x, y) = ((k % 300) as f64 * 3.0, (k / 300) as f64 * 3.0);
            let a = doc.add_point(v(x, y));
            let b = doc.add_point(v(x + 2.0, y + 1.0));
            doc.add_line(a, b).unwrap();
        }
        let mut ed = Editor::new(doc);
        ed.selection = ed
            .doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point())
            .map(|(id, _)| id)
            .collect();
        let t = Instant::now();
        let r = ed.relation_options();
        let rel = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let d = ed.dimension_options();
        let dim = t.elapsed().as_secs_f64() * 1e3;
        // (What was offered, every relation of every offer, as one number: to check it
        // against another build.)
        let offered = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            format!("{r:?}").hash(&mut h);
            h.finish()
        };
        println!(
            "{n:>6} lines: relation_options {rel:>9.1} ms ({} offers, {offered:016x}), dimension_options {dim:>7.2} ms ({})",
            r.len(),
            d.len()
        );
        if n == 40_000 {
            at_40k = rel;
        }
    }
    assert!(
        at_40k < 100.0,
        "relation offers for 40,000 lines took {at_40k:.0} ms"
    );
}

/// A drag's first frame (its parts found) where many small parts solve, on a drawing with a
/// locked layer and a wall: the locked geometry and what the walls made are found once for
/// the drag, not once per part (each finding walks the whole drawing).
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_drag_many_solving_parts() {
    let mut doc = Document::new();
    let mut picked = Vec::new();
    for k in 0..2_000 {
        let (x, y) = ((k % 50) as f64 * 30.0, (k / 50) as f64 * 30.0);
        let a = doc.add_point(v(x, y));
        let b = doc.add_point(v(x + 20.0, y));
        let l = doc.add_line(a, b).unwrap();
        doc.add_constraint(Constraint::Horizontal { line: l })
            .unwrap();
        picked.push(l);
    }
    // Locked lines, and a wall.
    let layer = doc.add_layer(drift_core::Layer {
        locked: true,
        ..drift_core::Layer::new("Locked")
    });
    for k in 0..5_000 {
        let (x, y) = ((k % 100) as f64 * 15.0, -100.0 - (k / 100) as f64 * 15.0);
        let a = doc.add_point(v(x, y));
        let b = doc.add_point(v(x + 10.0, y + 5.0));
        let l = doc.add_line(a, b).unwrap();
        doc.set_entity_layer(l, layer).unwrap();
        doc.set_entity_layer(a, layer).unwrap();
        doc.set_entity_layer(b, layer).unwrap();
    }
    let (a, b) = (
        doc.add_point(v(0.0, 2_000.0)),
        doc.add_point(v(1_500.0, 2_000.0)),
    );
    let centre = doc.add_line(a, b).unwrap();
    doc.set_style(centre, drift_core::LineKind::Construction);
    let path = doc
        .add_path(drift_core::Path {
            curves: vec![centre],
            closed: false,
        })
        .unwrap();
    doc.add_wall(drift_core::Wall::new(
        path,
        200.0,
        drift_core::LayerId::DEFAULT,
    ))
    .unwrap();
    drift_geom::walls::regenerate(&mut doc);
    let mut ed = Editor::new(doc);
    ed.sync();
    let dragged: Vec<drift_solver::Param> = ed
        .doc
        .defining_points(picked.iter().copied())
        .into_iter()
        .flat_map(drift_solver::Param::point)
        .collect();
    println!();
    report(
        "first drag frame, 2,000 solving parts",
        time(5, |_| {
            ed.doc.begin();
            assert!(ed.drag(&picked, v(0.5, 0.25), &dragged));
            ed.doc.rollback();
            ed.sync();
        }),
    );
}

/// The Select tool moving picked curves of the 10,000-curve sketch, as the canvas drives it
/// (no drawing): the modal move (G, the pointer in steps, a click to finish) and a mouse
/// drag (pressed on a picked curve, dragged in steps, let go). Per step and the release.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_select_tool_moves() {
    use crate::editor::Pointer;
    use crate::tools::{Input, Modifiers, SelectTool, Tool, ToolKey};
    let (doc, _) = torture_sketch();
    let mut ed = Editor::new(doc);
    ed.sync();
    let lines: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, drift_core::EntityKind::Line { .. }))
        .map(|(id, _)| id)
        .collect();
    let all: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .collect();
    let some: Vec<EntityId> = all.iter().copied().step_by(10).collect();
    println!();
    for (label, picked) in [("1,000 curves", &some), ("all curves", &all)] {
        // A point a third of the way along a picked line: what a press picks up.
        let line = *lines
            .iter()
            .find(|l| picked.contains(l))
            .expect("a picked line");
        let on = match ed.doc.shape(line) {
            Some(drift_core::Shape::Line { a, b }) => a + (b - a) / 3.0,
            _ => unreachable!(),
        };
        let p = |at: DVec2| Pointer::new(at, 1e-3);
        for how in ["G move", "mouse drag"] {
            ed.selection = picked.iter().copied().collect();
            let mut tool = SelectTool::default();
            let mut steps = Vec::new();
            if how == "G move" {
                tool.key(ToolKey::G, Some(&p(on)), &mut ed);
            } else {
                tool.handle(Input::DragStart(p(on), Modifiers::default()), &mut ed);
            }
            for k in 1..=8 {
                let at = on + v(0.7 * k as f64, 0.3 * k as f64);
                let t = Instant::now();
                if how == "G move" {
                    tool.handle(Input::Hover(p(at)), &mut ed);
                } else {
                    tool.handle(Input::Drag(p(at)), &mut ed);
                }
                steps.push(t.elapsed().as_secs_f64() * 1e3);
            }
            let end = on + v(5.6, 2.4);
            let t = Instant::now();
            if how == "G move" {
                tool.handle(Input::Click(p(end), Modifiers::default()), &mut ed);
            } else {
                tool.handle(Input::DragEnd(p(end), Modifiers::default()), &mut ed);
            }
            ed.settle_areas();
            let release = t.elapsed().as_secs_f64() * 1e3;
            steps.sort_by(f64::total_cmp);
            let moved = ed.history.undo_label() == Some("Move");
            report(
                &format!("{how} of {label}: step"),
                (steps.iter().sum::<f64>() / 8.0, steps[7], steps[4]),
            );
            report(&format!("{how} of {label}: release"), once(release));
            assert!(moved, "{how} of {label}: no move made");
            assert!(ed.undo());
            ed.selection.clear();
        }
    }
}

/// A refused drag on a big part costs what it always has (#280, #377): `perf_drag_refused`'s
/// refused drag with a 1000 mm chain of free bars takes at most 7 solves a frame before it is
/// refused. A drag's patience cut whose rows don't hold to a thousandth of the bar is
/// unfinished (fallback (b)); counted as converged, those cuts send the frame down its ladder
/// of retakes (18 solves a frame).
#[test]
fn a_refused_drag_on_a_big_part_takes_at_most_seven_solves_a_frame() {
    let (mut ed, ids) = held_on_an_arc(1_000, false, true, 315.0);
    let dragged: Vec<drift_solver::Param> = ed
        .doc
        .defining_points(ids)
        .into_iter()
        .flat_map(drift_solver::Param::point)
        .collect();
    ed.doc.begin();
    let mut most = 0;
    for _ in 0..3 {
        assert!(!ed.drag(&ids, v(0.0, -18.003814381075802), &dragged));
        most = most.max(ed.drag_solves);
    }
    assert!(most <= 7, "{most} solves a frame");
    ed.doc.rollback();
}

/// The solver bar's tiled sketch (#365, #377; `docs/solver-audit.md`, section 5, item 5): 200
/// and 1,000 copies of a sketch whose drag always refuses (a line dragged as a curve, its
/// midpoint carrying a hanging line held equal to a line the move stretches from a fixed
/// point) in one drawing, every moved line dragged together for 10 frames. Milliseconds per
/// drag call; then the drag is rolled back and every point checked unchanged.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_drag_tiles() {
    println!();
    for n in [200usize, 1_000] {
        let mut doc = Document::new();
        doc.begin();
        let mut moved = Vec::new();
        let side = (n as f64).sqrt().ceil() as usize;
        for t in 0..n {
            let o = v(60.0 * (t % side) as f64, 60.0 * (t / side) as f64);
            let at = |x: f64, y: f64| o + v(x, y);
            let p0 = doc.add_point(at(0.0, 0.0));
            let q = doc.add_point(at(10.0, 0.0));
            let r = doc.add_point(at(10.0, 6.0));
            let m = doc.add_point(at(10.0, 3.0));
            let s = doc.add_point(at(20.0, 3.0));
            let stretched = doc.add_line(p0, q).unwrap();
            let line = doc.add_line(q, r).unwrap();
            let hanging = doc.add_line(m, s).unwrap();
            doc.add_constraint(Constraint::Fix {
                point: p0,
                at: at(0.0, 0.0),
            })
            .unwrap();
            doc.add_constraint(Constraint::Midpoint { point: m, line })
                .unwrap();
            doc.add_constraint(Constraint::Equal {
                a: hanging,
                b: stretched,
            })
            .unwrap();
            moved.push(line);
        }
        doc.commit("Tiles");
        let mut ed = Editor::new(doc);
        let original: Vec<(EntityId, DVec2)> = ed
            .doc
            .iter()
            .filter(|(_, e)| e.kind.is_point())
            .map(|(id, _)| (id, ed.doc.point(id).unwrap()))
            .collect();
        let dragged: Vec<drift_solver::Param> = ed
            .doc
            .defining_points(moved.iter().copied())
            .into_iter()
            .flat_map(drift_solver::Param::point)
            .collect();
        let mut times = Vec::new();
        for _ in 0..3 {
            ed.doc.begin();
            let mut applied = DVec2::ZERO;
            for k in 1..=10 {
                let target = v(4.0, 1.0) * (f64::from(k) / 10.0);
                let t0 = Instant::now();
                if ed.drag(&moved, target - applied, &dragged) {
                    applied = target;
                }
                times.push(t0.elapsed());
            }
            ed.doc.rollback();
            for &(id, at) in &original {
                assert_eq!(ed.doc.point(id).unwrap(), at, "the drawing changed");
            }
        }
        times.sort_unstable();
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        let mean = times.iter().map(|d| ms(*d)).sum::<f64>() / times.len() as f64;
        report(
            &format!("tiled sketch, {n} copies, drag call"),
            (mean, ms(times[times.len() - 1]), ms(times[times.len() / 2])),
        );
    }
}
