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
    ed.auto_relations = false;
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
    assert!(!ed.doc.is_driven(dim), "{:?}", ed.notice);
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
    ed.auto_relations = false;
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
    ed.auto_relations = false;
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
