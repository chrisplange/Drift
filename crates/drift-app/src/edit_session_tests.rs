//! Tests of editing that need the assistant's session (scripts, import and export through
//! it) or the app's output, which are still in drift-app: they move to drift-edit with
//! `agent` and `output` (R3 S5, docs/editor-split-design.md). Copied helpers are marked with
//! where they came from.

use std::time::Instant;

use drift_core::{DVec2, Document, EntityId, EntityKind, LayerId, LineKind, Path, Wall};
use drift_edit::editor::{Editor, Pointer};
use drift_edit::tools::{Input, Modifiers, Tool, ToolKind};
use drift_geom::SnapOptions;
use serde_json::json;

use crate::agent::Session;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

// ---------------------------------------------------------------------------------------------
// Walls from lines (drift-edit's walls_from_lines/tests.rs)

const T: f64 = 114.3;

fn curves(doc: &Document) -> Vec<EntityId> {
    doc.iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. } | EntityKind::Arc { .. }))
        .map(|(id, _)| id)
        .collect()
}

/// A grid of `rows` × `cols` rooms 4 m × 3 m, as walls (L, T and X joins), exploded into
/// plain lines: what an imported plan's walls look like.
fn exploded_grid(rows: usize, cols: usize) -> (Editor, f64) {
    let mut doc = Document::new();
    // A US plan: 4½″ is one of its presets.
    doc.set_units(drift_core::Units::architectural());
    let (w, h) = (4000.0, 3000.0);
    let run = |doc: &mut Document, a: DVec2, b: DVec2| {
        let (pa, pb) = (doc.add_point(a), doc.add_point(b));
        let c = doc.add_line(pa, pb).unwrap();
        doc.set_style(c, LineKind::Construction);
        let p = doc
            .add_path(Path {
                curves: vec![c],
                closed: false,
            })
            .unwrap();
        doc.add_wall(Wall::new(p, T, LayerId::DEFAULT)).unwrap();
    };
    for j in 0..=rows {
        let y = j as f64 * h;
        run(&mut doc, v(0.0, y), v(cols as f64 * w, y));
    }
    for i in 0..=cols {
        let x = i as f64 * w;
        run(&mut doc, v(x, 0.0), v(x, rows as f64 * h));
    }
    drift_geom::walls::regenerate(&mut doc);
    let area = poche_area(&doc);
    let mut ed = Editor::new(doc);
    let walls: Vec<_> = ed.doc.walls().map(|(id, _)| id).collect();
    assert!(ed.explode_walls(&walls));
    (ed, area)
}

fn poche_area(doc: &Document) -> f64 {
    let (inputs, _) = drift_geom::walls::regen::wall_inputs(doc);
    let g = drift_geom::walls::generate(&inputs, drift_geom::walls::regen::snap(doc));
    g.regions().iter().map(|(_, _, r)| r.area).sum()
}

#[test]
fn old_script_walls_become_walls() {
    let mut s = Session::new();
    let out = s.call(
        "run_script",
        &json!({ "script": "wall w1 from 0,0 to 6000,0 thick 200\nwall w2 from 3000,0 to 3000,4000 thick 200\n" }),
    );
    assert!(!out.is_error, "{}", out.json);
    let all = curves(&s.editor.doc);
    let r = s.editor.walls_from_lines(&all).unwrap();
    assert_eq!(r.walls.len(), 2, "{}", r.summary());
    assert!(
        s.editor
            .doc
            .walls()
            .all(|(_, w)| (w.thickness - 200.0).abs() < 1e-9)
    );
    // Nothing of the old walls is left loose: faces, caps and centrelines all went.
    let loose: Vec<_> = curves(&s.editor.doc)
        .into_iter()
        .filter(|id| s.editor.wall_of(*id).is_none())
        .collect();
    assert!(loose.is_empty(), "{loose:?}");
}

#[test]
fn an_imported_dxf_plan_becomes_walls() {
    // The exploded grid exported to DXF and imported again, as a plan from elsewhere comes.
    let dir = std::env::temp_dir().join(format!("drift-walls-lines-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (ed, area) = exploded_grid(2, 2);
    let mut s = Session::new().with_root(dir.clone());
    s.editor = ed;
    s.editor.sync();
    let out = s.call("export", &json!({ "path": "plan.dxf" }));
    assert!(!out.is_error, "{}", out.json);
    let mut t = Session::new().with_root(dir.clone());
    t.editor.doc.set_units(drift_core::Units::architectural());
    let out = t.call("import_file", &json!({ "path": "plan.dxf" }));
    assert!(!out.is_error, "{}", out.json);
    let all = curves(&t.editor.doc);
    let r = t.editor.walls_from_lines(&all).unwrap();
    if r.walls.len() != 3 {
        // What came in, to see why.
        let dump: Vec<String> = t
            .editor
            .doc
            .iter()
            .filter_map(|(id, e)| {
                let s = t.editor.doc.shape(id)?;
                Some(format!("{id} {:?} {:?} {s:?}", e.layer, e.style))
            })
            .collect();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/walls-from-lines-dxf.txt");
        let _ = std::fs::write(out, dump.join("\n"));
    }
    assert_eq!(r.walls.len(), 3, "{}", r.summary());
    let got = poche_area(&t.editor.doc);
    assert!((got - area).abs() < 1e-3 * area, "{got} vs {area}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The private sample plans (DWG, DXF), if `DRIFT_PRIVATE_SAMPLES` names their folder: what
/// Walls from lines makes of each and how long it takes. Prints counts only, never names.
#[test]
#[ignore = "needs the private sample files"]
fn walls_from_lines_private_samples() {
    let Ok(dir) = std::env::var("DRIFT_PRIVATE_SAMPLES") else {
        return;
    };
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext.to_ascii_lowercase().as_str(), "dwg" | "dxf" | "pdf") {
            continue;
        }
        n += 1;
        let mut s = Session::new().with_root(path.parent().unwrap().to_path_buf());
        let name = path.file_name().unwrap().to_str().unwrap();
        let out = s.call("import_file", &json!({ "path": name }));
        if out.is_error {
            println!("sample {n} ({ext}): import failed");
            continue;
        }
        let all = curves(&s.editor.doc);
        let lines: Vec<drift_geom::walls::from_lines::LineIn> = all
            .iter()
            .filter_map(|id| {
                let e = s.editor.doc.get(*id)?;
                let EntityKind::Line { a, b } = e.kind else {
                    return None;
                };
                Some(drift_geom::walls::from_lines::LineIn {
                    id: *id,
                    group: e.layer.0,
                    seg: drift_geom::walls::Seg::Line {
                        a: s.editor.doc.point(a).ok()?,
                        b: s.editor.doc.point(b).ok()?,
                    },
                    guide: e.style == LineKind::Construction,
                })
            })
            .collect();
        let settings = drift_geom::walls::from_lines::Settings::new(0.5, vec![T], 1.0);
        let t = Instant::now();
        let found = drift_geom::walls::from_lines::find(&lines, &settings);
        let most = found.walls.iter().map(|w| w.lines.len()).max().unwrap_or(0);
        println!(
            "  finding: {:.0} ms; most lines in one wall {most}",
            t.elapsed().as_secs_f64() * 1e3
        );
        let t = Instant::now();
        let r = s.editor.walls_from_lines(&all);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        match r {
            Ok(r) => println!(
                "sample {n} ({ext}): {} curves; {}; {} kept in part; {ms:.0} ms",
                all.len(),
                r.summary(),
                r.partly.len()
            ),
            Err(e) => println!("sample {n} ({ext}): {} curves; {e}; {ms:.0} ms", all.len()),
        }
        if let Some(notice) = &s.editor.out.notice {
            println!("  notice: {notice}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The Wall tool (drift-edit's tools/wall_tests.rs)

const TOL: f64 = 0.5;

fn ptr(ed: &Editor, at: DVec2) -> Pointer {
    let mut p = Pointer::new(at, TOL);
    p.snap = ed.index.snap(
        at,
        SnapOptions {
            tolerance: TOL,
            grid: None,
            from: None,
        },
        |_| false,
    );
    p
}

fn click(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2) {
    let p = ptr(ed, at);
    tool.handle(Input::Hover(p), ed);
    tool.handle(Input::Click(p, Modifiers::default()), ed);
}

fn editor() -> Editor {
    let mut ed = Editor::default();
    ed.prefs.auto_relations = false;
    ed.prefs.wall_thickness = 10.0;
    ed.show_centrelines = true;
    ed
}

/// Draw a run through `pts` and finish it.
fn run(ed: &mut Editor, pts: &[DVec2]) {
    let mut tool = ToolKind::Wall.create();
    for at in pts {
        click(ed, tool.as_mut(), *at);
    }
    tool.handle(Input::Finish, ed);
}

#[test]
fn walls_print_their_faces_not_their_centrelines_or_butts() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(200.0, 0.0)]);
    // A thinner wall of another poché teed into it: it butts against the host's face.
    // (Another material: faces only, no poché.)
    ed.prefs.wall_thickness = 6.0;
    ed.prefs.wall_poche.on = false;
    run(&mut ed, &[v(100.0, 80.0), v(100.0, 5.0)]);
    assert_eq!(ed.doc.fills().count(), 1, "the poché of the host");
    let ctx = eframe::egui::Context::default();
    let painter = eframe::egui::Painter::new(
        ctx,
        eframe::egui::LayerId::background(),
        eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(800.0, 800.0),
        ),
    );
    let d = crate::output::model_drawing(
        &mut ed,
        &drift_edit::fills::PatternLibrary::default(),
        &painter,
    );
    let lines = d
        .items
        .iter()
        .filter(|i| matches!(i.prim, drift_draft::page::Prim::Line(..)))
        .count();
    // The host's two faces and caps, the branch's two faces and its far cap: not the
    // centrelines (construction) nor the butt edge along the host's face.
    assert_eq!(lines, 7);
}
