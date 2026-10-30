use std::time::Instant;

use drift_core::{DVec2, Document, EntityId, EntityKind, LayerId, LineKind, Path, Wall};
use serde_json::json;

use super::*;
use crate::agent::Session;

const T: f64 = 114.3;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn curves(doc: &Document) -> Vec<EntityId> {
    doc.iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. } | EntityKind::Arc { .. }))
        .map(|(id, _)| id)
        .collect()
}

/// An L drawn as face lines.
fn l_plan() -> Document {
    let mut doc = Document::new();
    // A US plan: 4½″ is one of its presets.
    doc.set_units(drift_core::Units::architectural());
    for (a, b) in [
        (v(0.0, 0.0), v(3000.0, 0.0)),
        (v(0.0, 0.0), v(0.0, 2000.0)),
        (v(T, T), v(3000.0, T)),
        (v(T, T), v(T, 2000.0)),
        (v(3000.0, 0.0), v(3000.0, T)),
        (v(0.0, 2000.0), v(T, 2000.0)),
    ] {
        let (pa, pb) = (doc.add_point(a), doc.add_point(b));
        doc.add_line(pa, pb).unwrap();
    }
    doc
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
fn walls_from_lines_is_one_undo_step_and_the_lines_go() {
    let mut ed = Editor::new(l_plan());
    let before = ed.doc.len();
    let all = curves(&ed.doc);
    let r = ed.walls_from_lines(&all).unwrap();
    assert_eq!(r.walls.len(), 1);
    assert_eq!(r.used, 6);
    assert!(r.skipped.is_empty() && r.partly.is_empty());
    assert!(all.iter().all(|id| !ed.doc.contains(*id)));
    assert_eq!(ed.doc.walls().count(), 1);
    assert!(ed.notice.is_none(), "{:?}", ed.notice);
    // The new walls are picked, and the report is current.
    assert_eq!(ed.selected_walls().len(), 1);
    assert!(ed.from_lines_now().is_some());
    assert!(
        (poche_area(&ed.doc) - (3000.0 + 2000.0 - T) * T).abs() < 1e-6,
        "{}",
        poche_area(&ed.doc)
    );
    assert!(ed.undo());
    assert_eq!(ed.doc.len(), before);
    assert_eq!(ed.doc.walls().count(), 0);
    assert!(ed.from_lines_now().is_none());
}

#[test]
fn the_lines_used_can_be_kept_or_put_on_a_layer() {
    let mut ed = Editor::new(l_plan());
    ed.wall_lines = UsedLines::Keep;
    let all = curves(&ed.doc);
    ed.walls_from_lines(&all).unwrap();
    assert!(all.iter().all(|id| ed.doc.contains(*id)));
    // Changing it does the command again that way, still one step.
    let r = ed.redo_walls_from_lines(UsedLines::Layer).unwrap();
    assert_eq!(r.lines, UsedLines::Layer);
    assert_eq!(ed.doc.walls().count(), 1);
    let (layer, l) = ed
        .doc
        .layers()
        .find(|(_, l)| l.name == USED_LINES_LAYER)
        .expect("the layer");
    assert!(!l.visible);
    assert!(all.iter().all(|id| ed.doc.get(*id).unwrap().layer == layer));
    assert!(ed.undo());
    assert_eq!(ed.doc.walls().count(), 0);
    assert!(
        all.iter()
            .all(|id| ed.doc.get(*id).unwrap().layer == LayerId::DEFAULT)
    );
}

#[test]
fn walls_and_their_parts_are_left_alone() {
    let mut ed = Editor::new(l_plan());
    let all = curves(&ed.doc);
    ed.walls_from_lines(&all).unwrap();
    let again = curves(&ed.doc);
    assert!(ed.walls_from_lines(&again).is_err());
    assert_eq!(ed.doc.walls().count(), 1);
}

#[test]
fn nothing_wall_like_says_so() {
    let mut doc = Document::new();
    let (a, b) = (doc.add_point(v(0.0, 0.0)), doc.add_point(v(1000.0, 0.0)));
    let l = doc.add_line(a, b).unwrap();
    let mut ed = Editor::new(doc);
    let e = ed.walls_from_lines(&[l]).unwrap_err();
    assert!(e.starts_with("No walls found"), "{e}");
    assert!(ed.walls_from_lines(&[]).is_err());
}

#[test]
fn exploded_walls_come_back_as_the_same_walls() {
    let (mut ed, area) = exploded_grid(2, 3);
    let all = curves(&ed.doc);
    let r = ed.walls_from_lines(&all).unwrap();
    // The outside is one ring, and each inner grid line one wall through its crossings.
    assert_eq!(r.walls.len(), 1 + 1 + 2, "{}", r.summary());
    assert_eq!(r.openings, 0);
    assert!(r.skipped.is_empty(), "{}", r.summary());
    let got = poche_area(&ed.doc);
    assert!((got - area).abs() < 1e-6 * area, "{got} vs {area}");
    assert!(ed.notice.is_none(), "{:?}", ed.notice);
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
        if let Some(notice) = &s.editor.notice {
            println!("  notice: {notice}");
        }
    }
}

/// Walls from lines on a large plan: a 30 × 30 grid of rooms, exploded (about 7,000 lines).
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_walls_from_lines() {
    let (ed, area) = exploded_grid(30, 30);
    let all = curves(&ed.doc);
    println!("\n  {} lines", all.len());
    let lines: Vec<drift_geom::walls::from_lines::LineIn> = all
        .iter()
        .map(|id| {
            let EntityKind::Line { a, b } = ed.doc.get(*id).unwrap().kind else {
                unreachable!()
            };
            drift_geom::walls::from_lines::LineIn {
                id: *id,
                group: 0,
                seg: drift_geom::walls::Seg::Line {
                    a: ed.doc.point(a).unwrap(),
                    b: ed.doc.point(b).unwrap(),
                },
                guide: false,
            }
        })
        .collect();
    let settings = drift_geom::walls::from_lines::Settings::new(0.5, vec![T], 1.0);
    let t = Instant::now();
    let found = drift_geom::walls::from_lines::find(&lines, &settings);
    let find_ms = t.elapsed().as_secs_f64() * 1e3;
    println!(
        "  find the walls: {find_ms:.1} ms ({} walls)",
        found.walls.len()
    );
    let mut ed = ed;
    let t = Instant::now();
    let r = ed.walls_from_lines(&all).unwrap();
    let all_ms = t.elapsed().as_secs_f64() * 1e3;
    println!("  the whole command (find, build, regenerate, commit): {all_ms:.1} ms");
    assert_eq!(r.walls.len(), 1 + 29 + 29);
    let got = poche_area(&ed.doc);
    assert!((got - area).abs() < 1e-6 * area, "{got} vs {area}");
    // Budgets (release): finding stays well under a second on a big plan.
    assert!(find_ms < 500.0, "finding took {find_ms:.0} ms");
}
