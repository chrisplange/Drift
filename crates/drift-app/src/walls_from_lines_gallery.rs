//! The gallery of Walls from lines: plans drawn as face lines, the way imported drawings
//! have them, each turned into walls and checked for what it should make (walls, openings,
//! skipped lines, the poché's area, and no notices). `walls_from_lines_pictures` (ignored)
//! renders each case before and after onto one sheet to look at:
//! `cargo test -p drift-app walls_from_lines_pictures -- --ignored`, then
//! `target/tmp/walls-from-lines.png` (cases top to bottom, before on the left).

use drift_core::{DVec2, Document, EntityId, EntityKind, LineKind};
use drift_geom::walls::{generate, regen, regenerate};

use drift_edit::editor::Editor;

/// 4½″: a 2×4 wall, finished.
const T: f64 = 114.3;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// Something drawn in the plan: a line, or an arc counter-clockwise round a centre.
#[derive(Clone, Copy)]
enum Mark {
    Line(DVec2, DVec2),
    Arc(DVec2, DVec2, DVec2),
}

struct Case {
    name: &'static str,
    marks: Vec<Mark>,
    walls: usize,
    openings: usize,
    /// Lines skipped (any reason).
    skipped: usize,
    /// The poché's area.
    area: Option<f64>,
}

fn lines(pts: &[(DVec2, DVec2)]) -> Vec<Mark> {
    pts.iter().map(|&(a, b)| Mark::Line(a, b)).collect()
}

/// A closed polyline's sides.
fn poly(pts: &[DVec2]) -> Vec<Mark> {
    (0..pts.len())
        .map(|k| Mark::Line(pts[k], pts[(k + 1) % pts.len()]))
        .collect()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Mark> {
    poly(&[v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)])
}

fn case(
    name: &'static str,
    marks: Vec<Mark>,
    walls: usize,
    openings: usize,
    area: Option<f64>,
) -> Case {
    Case {
        name,
        marks,
        walls,
        openings,
        skipped: 0,
        area,
    }
}

fn cases() -> Vec<Case> {
    let (w, h) = (4000.0, 3000.0);
    let ring = w * h - (w - 2.0 * T) * (h - 2.0 * T);
    let (x0, x1) = (2000.0 - T / 2.0, 2000.0 + T / 2.0);
    let mut room_with_door = lines(&[
        // Bottom wall, a 36″ door from x = 1000.
        (v(0.0, 0.0), v(1000.0, 0.0)),
        (v(T, T), v(1000.0, T)),
        (v(1000.0, 0.0), v(1000.0, T)),
        (v(1914.4, 0.0), v(w, 0.0)),
        (v(1914.4, T), v(w - T, T)),
        (v(1914.4, 0.0), v(1914.4, T)),
    ]);
    room_with_door.extend(lines(&[
        (v(w, 0.0), v(w, h)),
        (v(w, h), v(0.0, h)),
        (v(0.0, h), v(0.0, 0.0)),
        (v(w - T, T), v(w - T, h - T)),
        (v(w - T, h - T), v(T, h - T)),
        (v(T, h - T), v(T, T)),
    ]));
    // A room split by a partition teeing into both long walls.
    let mut split = rect(0.0, 0.0, w, h);
    split.extend(lines(&[
        (v(T, T), v(x0, T)),
        (v(x0, T), v(x0, h - T)),
        (v(x0, h - T), v(T, h - T)),
        (v(T, h - T), v(T, T)),
        (v(x1, T), v(w - T, T)),
        (v(w - T, T), v(w - T, h - T)),
        (v(w - T, h - T), v(x1, h - T)),
        (v(x1, h - T), v(x1, T)),
    ]));
    let hh = T / 2.0;
    let quarter = |r: f64| Mark::Arc(v(0.0, 0.0), v(r, 0.0), v(0.0, r));
    let mut curved = vec![quarter(2000.0 - 100.0), quarter(2000.0 + 100.0)];
    curved.extend(lines(&[
        (v(1900.0, 0.0), v(1900.0, -2000.0)),
        (v(2100.0, 0.0), v(2100.0, -2000.0)),
        (v(0.0, 1900.0), v(-2000.0, 1900.0)),
        (v(0.0, 2100.0), v(-2000.0, 2100.0)),
    ]));
    let mut furniture = rect(0.0, 0.0, 60.0 * 25.4, 30.0 * 25.4);
    furniture.extend(rect(2000.0, 0.0, 2000.0 + 36.0 * 25.4, 12.0 * 25.4));
    furniture.extend(lines(
        &(0..8)
            .map(|k| {
                let y = 1500.0 + k as f64 * 100.0;
                (v(0.0, y), v(3000.0, y))
            })
            .collect::<Vec<_>>(),
    ));
    vec![
        case(
            "A straight wall with end caps",
            lines(&[
                (v(0.0, 0.0), v(3000.0, 0.0)),
                (v(0.0, T), v(3000.0, T)),
                (v(0.0, 0.0), v(0.0, T)),
                (v(3000.0, 0.0), v(3000.0, T)),
            ]),
            1,
            0,
            Some(3000.0 * T),
        ),
        case(
            "An L: outer and inner faces",
            lines(&[
                (v(0.0, 0.0), v(3000.0, 0.0)),
                (v(0.0, 0.0), v(0.0, 2000.0)),
                (v(T, T), v(3000.0, T)),
                (v(T, T), v(T, 2000.0)),
                (v(3000.0, 0.0), v(3000.0, T)),
                (v(0.0, 2000.0), v(T, 2000.0)),
            ]),
            1,
            0,
            Some((3000.0 + 2000.0 - T) * T),
        ),
        case(
            "A T: the host's face broken where the stem comes in",
            lines(&[
                (v(0.0, 0.0), v(3000.0, 0.0)),
                (v(0.0, T), v(1500.0 - hh, T)),
                (v(1500.0 + hh, T), v(3000.0, T)),
                (v(1500.0 - hh, T), v(1500.0 - hh, 2000.0)),
                (v(1500.0 + hh, T), v(1500.0 + hh, 2000.0)),
            ]),
            2,
            0,
            Some(3000.0 * T + (2000.0 - T) * T),
        ),
        case(
            "An X: every face broken",
            lines(&[
                (v(-2000.0, -hh), v(-hh, -hh)),
                (v(hh, -hh), v(2000.0, -hh)),
                (v(-2000.0, hh), v(-hh, hh)),
                (v(hh, hh), v(2000.0, hh)),
                (v(-hh, -2000.0), v(-hh, -hh)),
                (v(-hh, hh), v(-hh, 2000.0)),
                (v(hh, -2000.0), v(hh, -hh)),
                (v(hh, hh), v(hh, 2000.0)),
            ]),
            2,
            0,
            Some(2.0 * 4000.0 * T - T * T),
        ),
        case(
            "A room: two rectangles",
            {
                let mut m = rect(0.0, 0.0, w, h);
                m.extend(rect(T, T, w - T, h - T));
                m
            },
            1,
            0,
            Some(ring),
        ),
        case(
            "A room with a door: the walls end at the jambs",
            room_with_door,
            1,
            1,
            Some(ring - 914.4 * T),
        ),
        case(
            "A room split by a partition",
            split,
            2,
            0,
            Some(ring + (h - 2.0 * T) * T),
        ),
        case(
            "An L of two thicknesses",
            lines(&[
                (v(0.0, 0.0), v(3000.0, 0.0)),
                (v(0.0, 0.0), v(0.0, 2000.0)),
                (v(T, 165.1), v(3000.0, 165.1)),
                (v(T, 165.1), v(T, 2000.0)),
            ]),
            2,
            0,
            Some(3000.0 * 165.1 + (2000.0 - 165.1) * T),
        ),
        case(
            "Faces a little out of parallel, and in pieces",
            lines(&[
                (v(0.0, 0.0), v(1200.0, 0.0)),
                (v(1200.0, 0.0), v(4000.0, 0.0)),
                (v(0.0, T), v(4000.0, T + 3.0)),
            ]),
            1,
            0,
            None,
        ),
        case(
            "A curved wall between two straight ones",
            curved,
            1,
            0,
            None,
        ),
        Case {
            skipped: 18,
            ..case(
                "A table, a shelf and a hatch: no walls",
                furniture,
                0,
                0,
                None,
            )
        },
    ]
}

/// The case as a drawing of plain lines, in inches (as a US plan).
fn before(c: &Case) -> Document {
    let mut doc = Document::new();
    doc.set_units(drift_core::Units::architectural());
    for m in &c.marks {
        match *m {
            Mark::Line(a, b) => {
                let (pa, pb) = (doc.add_point(a), doc.add_point(b));
                doc.add_line(pa, pb).unwrap();
            }
            Mark::Arc(center, a, b) => {
                let (pc, pa, pb) = (doc.add_point(center), doc.add_point(a), doc.add_point(b));
                doc.add_arc(pc, pa, pb).unwrap();
            }
        }
    }
    doc
}

/// Walls from lines on every curve of `doc`.
fn after(
    doc: Document,
) -> (
    Editor,
    Result<drift_edit::walls_from_lines::FromLinesReport, String>,
) {
    let mut ed = Editor::new(doc);
    let all: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. } | EntityKind::Arc { .. }))
        .map(|(id, _)| id)
        .collect();
    let r = ed.walls_from_lines(&all);
    (ed, r)
}

#[test]
fn walls_from_lines_gallery_makes_what_it_should() {
    let mut failures = Vec::new();
    for (i, c) in cases().iter().enumerate() {
        let (ed, r) = after(before(c));
        let mut wrong = Vec::new();
        let (walls, openings, skipped) = match &r {
            Ok(r) => (
                r.walls.len(),
                r.openings,
                r.skipped.iter().map(|s| s.2.len()).sum(),
            ),
            Err(_) => (0, 0, c.skipped),
        };
        if (walls, openings, skipped) != (c.walls, c.openings, c.skipped) {
            wrong.push(format!(
                "{walls} walls, {openings} openings, {skipped} skipped, not {}, {} and {} ({r:?})",
                c.walls, c.openings, c.skipped
            ));
        }
        if let Some(n) = &ed.notice {
            wrong.push(format!("notice {n:?}"));
        }
        let (inputs, _) = regen::wall_inputs(&ed.doc);
        let g = generate(&inputs, regen::snap(&ed.doc));
        let area: f64 = g.regions().iter().map(|(_, _, r)| r.area).sum();
        if let Some(want) = c.area
            && (area - want).abs() > 1e-6 * want
        {
            wrong.push(format!("area {area:.1}, not {want:.1}"));
        }
        if c.walls > 0 {
            // The lines it used are gone; walls' faces are all that is solid.
            let loose = ed
                .doc
                .iter()
                .filter(|(id, e)| {
                    !matches!(e.kind, EntityKind::Point(_))
                        && e.style != LineKind::Construction
                        && ed.wall_of(*id).is_none()
                })
                .count();
            if loose != 0 {
                wrong.push(format!("{loose} lines left over"));
            }
            let mut again = ed.doc.clone();
            if regenerate(&mut again).changed {
                wrong.push("regenerating again changed it".into());
            }
        }
        if !wrong.is_empty() {
            failures.push(format!("{} {}: {}", i + 1, c.name, wrong.join("; ")));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Every case before and after onto one sheet (`target/tmp/walls-from-lines.png`).
#[test]
#[ignore = "writes pictures to look at"]
fn walls_from_lines_pictures() {
    const CELL: u32 = 320;
    let all = cases();
    let rows = all.len() as u32;
    let mut sheet = tiny_skia::Pixmap::new(CELL * 2, CELL * rows).unwrap();
    sheet.fill(tiny_skia::Color::WHITE);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
    let _ = std::fs::create_dir_all(&dir);
    let render = |ed: Editor| {
        let mut s = crate::agent::Session::new();
        s.editor = ed;
        s.editor.selection.clear();
        s.editor.sync();
        let out = s.call(
            "render",
            &serde_json::json!({
                "width": CELL - 20,
                "dimensions": false,
                "dof_colors": false,
                "relations": false,
                "points": false,
                "labels": "none",
            }),
        );
        tiny_skia::Pixmap::decode_png(&out.image.expect("a picture")).unwrap()
    };
    for (i, c) in all.iter().enumerate() {
        let pics = [render(Editor::new(before(c))), render(after(before(c)).0)];
        for (k, pic) in pics.iter().enumerate() {
            let (x, y) = (k as u32 * CELL, i as u32 * CELL);
            sheet.draw_pixmap(
                (x + (CELL - pic.width()) / 2) as i32,
                (y + (CELL - pic.height()) / 2) as i32,
                pic.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                tiny_skia::Transform::identity(),
                None,
            );
        }
        println!("{:2}. {}", i + 1, c.name);
    }
    sheet.save_png(dir.join("walls-from-lines.png")).unwrap();
}
