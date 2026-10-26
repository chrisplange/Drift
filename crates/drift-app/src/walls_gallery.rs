//! The gallery of wall joins: the standard ways walls meet, drawn the way people draw them
//! (on the mark, a little short, a little past, off the centreline, near a free end), and
//! openings in them (near a corner, either side of a T, in a T's branch, in an arc wall,
//! through a room), each checked for what it should make: one outline or several, its holes,
//! its area, and no notices. `walls_gallery_pictures` (ignored) renders them all onto one sheet to look at:
//! `cargo test -p drift-app walls_gallery_pictures -- --ignored`, then
//! `target/tmp/wall-gallery.png` (cases left to right, top to bottom, in the order below).

use std::collections::HashMap;

use drift_core::{
    DVec2, Document, EntityId, FillStyle, LayerId, LineKind, Opening, OpeningAt, Path, Symbol, Wall,
};
use drift_geom::walls::{Seg, regen, regenerate};

const T: f64 = 200.0;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A run of wall to draw.
#[derive(Clone)]
struct Run {
    pts: Vec<DVec2>,
    closed: bool,
    t: f64,
    /// The first segment is an arc through this point.
    through: Option<DVec2>,
    /// A hatched poché (another material) instead of solid.
    hatch: bool,
    /// Whether each end joins what it lands on.
    join: [bool; 2],
}

fn run(pts: &[DVec2]) -> Run {
    Run {
        pts: pts.to_vec(),
        closed: false,
        t: T,
        through: None,
        hatch: false,
        join: [true, true],
    }
}

struct Case {
    name: &'static str,
    runs: Vec<Run>,
    /// Runs share a point where their points are at the same place (as the Wall tool draws
    /// them when a click lands on a point); otherwise every run has points of its own and
    /// joins only by where its ends are.
    share: bool,
    outers: usize,
    holes: usize,
    area: Option<f64>,
    /// Openings: (run, segment, first jamb along it, width) — doors, drawn with their
    /// symbols in the pictures.
    openings: Vec<(usize, u16, f64, f64)>,
}

fn case(
    name: &'static str,
    runs: Vec<Run>,
    outers: usize,
    holes: usize,
    area: Option<f64>,
) -> Case {
    Case {
        name,
        runs,
        share: false,
        outers,
        holes,
        area,
        openings: Vec::new(),
    }
}

fn polar(deg: f64, r: f64) -> DVec2 {
    DVec2::from_angle(deg.to_radians()) * r
}

fn cases() -> Vec<Case> {
    let host = || run(&[v(0.0, 0.0), v(3000.0, 0.0)]);
    vec![
        Case {
            share: true,
            ..case(
                "L on the mark",
                vec![run(&[v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, 2000.0)])],
                1,
                0,
                Some(5000.0 * T),
            )
        },
        case(
            "L, separate points on the mark",
            vec![
                run(&[v(0.0, 0.0), v(3000.0, 0.0)]),
                run(&[v(3000.0, 0.0), v(3000.0, 2000.0)]),
            ],
            1,
            0,
            Some(5000.0 * T),
        ),
        case(
            "L, a little short",
            vec![
                run(&[v(0.0, 0.0), v(2940.0, 0.0)]),
                run(&[v(3000.0, 0.0), v(3000.0, 2000.0)]),
            ],
            1,
            0,
            Some(5000.0 * T),
        ),
        case(
            "L, a little past",
            vec![
                run(&[v(0.0, 0.0), v(3080.0, 0.0)]),
                run(&[v(3000.0, 0.0), v(3000.0, 2000.0)]),
            ],
            1,
            0,
            Some(5000.0 * T),
        ),
        case(
            "L, landing near a free end (a short stub is taken back)",
            vec![host(), run(&[v(150.0, -2000.0), v(150.0, 0.0)])],
            1,
            0,
            Some(2850.0 * T + 2000.0 * T),
        ),
        case(
            "T on the centreline",
            vec![host(), run(&[v(1500.0, 0.0), v(1500.0, 2000.0)])],
            1,
            0,
            Some(3000.0 * T + 1900.0 * T),
        ),
        case(
            "T, off the centreline",
            vec![host(), run(&[v(1500.0, 60.0), v(1500.0, 2000.0)])],
            1,
            0,
            Some(3000.0 * T + 1900.0 * T),
        ),
        case(
            "T, past the centreline",
            vec![host(), run(&[v(1500.0, -90.0), v(1500.0, 2000.0)])],
            1,
            0,
            Some(3000.0 * T + 1900.0 * T),
        ),
        case(
            "T near an end (a long stub stays)",
            vec![host(), run(&[v(600.0, 0.0), v(600.0, 2000.0)])],
            1,
            0,
            Some(3000.0 * T + 1900.0 * T),
        ),
        case(
            "X",
            vec![host(), run(&[v(1500.0, -1500.0), v(1500.0, 1500.0)])],
            1,
            0,
            Some(3000.0 * T + 3000.0 * T - T * T),
        ),
        case(
            "Straight on across a gap",
            vec![
                run(&[v(0.0, 0.0), v(1500.0, 0.0)]),
                run(&[v(1550.0, 0.0), v(3000.0, 0.0)]),
            ],
            1,
            0,
            Some(3000.0 * T),
        ),
        case(
            "Straight on, thinner",
            vec![
                run(&[v(0.0, 0.0), v(1500.0, 0.0)]),
                Run {
                    t: 100.0,
                    ..run(&[v(1500.0, 0.0), v(3000.0, 0.0)])
                },
            ],
            1,
            0,
            Some(1500.0 * T + 1500.0 * 100.0),
        ),
        case(
            "Room closed a little off its start",
            vec![run(&[
                v(0.0, 0.0),
                v(3000.0, 0.0),
                v(3000.0, 2000.0),
                v(0.0, 2000.0),
                v(40.0, 60.0),
            ])],
            1,
            1,
            None,
        ),
        case(
            "Room closed on its own first wall",
            vec![run(&[
                v(0.0, 0.0),
                v(3000.0, 0.0),
                v(3000.0, -2000.0),
                v(150.0, -2000.0),
                v(150.0, 0.0),
            ])],
            1,
            1,
            None,
        ),
        case(
            "P: a run teeing into itself",
            vec![run(&[
                v(0.0, 0.0),
                v(0.0, 3000.0),
                v(1500.0, 3000.0),
                v(1500.0, 1500.0),
                v(0.0, 1500.0),
            ])],
            1,
            1,
            None,
        ),
        case(
            "U teeing both ends into one wall",
            vec![
                host(),
                run(&[
                    v(1000.0, 0.0),
                    v(1000.0, 1500.0),
                    v(2000.0, 1500.0),
                    v(2000.0, 0.0),
                ]),
            ],
            1,
            1,
            None,
        ),
        case(
            "Y: three ends at one place",
            vec![
                run(&[v(0.0, 0.0), polar(90.0, 1500.0)]),
                run(&[v(0.0, 0.0), polar(210.0, 1500.0)]),
                run(&[v(0.0, 0.0), polar(330.0, 1500.0)]),
            ],
            1,
            0,
            None,
        ),
        case(
            "Sharp corner (cut square past the mitre limit)",
            vec![run(&[
                v(0.0, 0.0),
                v(3000.0, 0.0),
                v(3000.0, 0.0) + polar(160.0, 2500.0),
            ])],
            1,
            0,
            None,
        ),
        case(
            "T into a curved wall",
            vec![
                Run {
                    through: Some(v(1500.0, 800.0)),
                    ..run(&[v(0.0, 0.0), v(3000.0, 0.0)])
                },
                run(&[v(1500.0, 800.0), v(1500.0, 2500.0)]),
            ],
            1,
            0,
            None,
        ),
        case(
            "Straight wall meeting a curved one's end",
            vec![
                run(&[v(0.0, 0.0), v(2000.0, 0.0)]),
                Run {
                    through: Some(v(2700.0, 700.0)),
                    ..run(&[v(2000.0, 0.0), v(2000.0, 1400.0)])
                },
            ],
            1,
            0,
            None,
        ),
        case(
            "Apart: too far to join",
            vec![
                run(&[v(0.0, 0.0), v(2000.0, 0.0)]),
                run(&[v(2400.0, 0.0), v(4000.0, 0.0)]),
            ],
            2,
            0,
            Some(3600.0 * T),
        ),
        case(
            "T of another material (it butts)",
            vec![
                Run {
                    hatch: true,
                    ..host()
                },
                Run {
                    t: 100.0,
                    ..run(&[v(1500.0, 0.0), v(1500.0, 2000.0)])
                },
            ],
            2,
            0,
            Some(3000.0 * T + 1900.0 * 100.0),
        ),
        case(
            "Set not to join: overlaps, capped",
            vec![
                host(),
                Run {
                    join: [false, true],
                    ..run(&[v(1500.0, 0.0), v(1500.0, 2000.0)])
                },
            ],
            2,
            0,
            Some(3000.0 * T + 2000.0 * T),
        ),
        case(
            "Four ends near one place",
            vec![
                run(&[v(40.0, 0.0), v(1500.0, 0.0)]),
                run(&[v(0.0, 40.0), v(0.0, 1500.0)]),
                run(&[v(-40.0, 0.0), v(-1500.0, 0.0)]),
                run(&[v(0.0, -40.0), v(0.0, -1500.0)]),
            ],
            1,
            0,
            None,
        ),
        case(
            "End into a wall's corner",
            vec![
                run(&[v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, 2000.0)]),
                run(&[v(5000.0, 0.0), v(3050.0, 0.0)]),
            ],
            1,
            0,
            None,
        ),
        case(
            "T at an angle",
            vec![
                host(),
                run(&[v(1500.0, 50.0), v(1500.0, 50.0) + polar(30.0, 2000.0)]),
            ],
            1,
            0,
            None,
        ),
        // Openings (W3).
        Case {
            share: true,
            openings: vec![(0, 0, 1950.0, 900.0)],
            ..case(
                "Door near an L's corner (the stub to its free end apart)",
                vec![run(&[v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, 2000.0)])],
                2,
                0,
                Some(5000.0 * T - 900.0 * T),
            )
        },
        Case {
            openings: vec![(0, 0, 300.0, 900.0), (0, 0, 1800.0, 900.0)],
            ..case(
                "Doors either side of a T",
                vec![host(), run(&[v(1500.0, 0.0), v(1500.0, 2000.0)])],
                3,
                0,
                Some(3000.0 * T + 1900.0 * T - 1800.0 * T),
            )
        },
        Case {
            openings: vec![(1, 0, 200.0, 800.0)],
            ..case(
                "Door in a T's branch, near its host",
                vec![host(), run(&[v(1500.0, 0.0), v(1500.0, 2000.0)])],
                2,
                0,
                Some(3000.0 * T + 1900.0 * T - 800.0 * T),
            )
        },
        Case {
            openings: vec![(0, 0, 1200.0, 900.0)],
            ..case(
                "Door in an arc wall",
                vec![Run {
                    through: Some(v(1500.0, 800.0)),
                    ..run(&[v(0.0, 0.0), v(3000.0, 0.0)])
                }],
                2,
                0,
                None,
            )
        },
        Case {
            share: true,
            openings: vec![(0, 0, 1550.0, 900.0)],
            ..case(
                "Door through a room's wall",
                vec![Run {
                    closed: true,
                    ..run(&[
                        v(0.0, 0.0),
                        v(4000.0, 0.0),
                        v(4000.0, 3000.0),
                        v(0.0, 3000.0),
                    ])
                }],
                1,
                0,
                Some((4200.0 * 3200.0 - 3800.0 * 2800.0) - 900.0 * T),
            )
        },
    ]
}

/// The case as a drawing, regenerated, and its notices.
fn drawing(c: &Case) -> (Document, Vec<String>) {
    let mut doc = Document::new();
    let mut shared: HashMap<(u64, u64), EntityId> = HashMap::new();
    let mut point = |doc: &mut Document, p: DVec2| {
        if c.share {
            *shared
                .entry((p.x.to_bits(), p.y.to_bits()))
                .or_insert_with(|| doc.add_point(p))
        } else {
            doc.add_point(p)
        }
    };
    let mut walls = Vec::new();
    for r in &c.runs {
        let pts: Vec<EntityId> = r.pts.iter().map(|p| point(&mut doc, *p)).collect();
        let n = if r.closed { pts.len() } else { pts.len() - 1 };
        let mut curves = Vec::new();
        for k in 0..n {
            let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
            let curve = match r.through.filter(|_| k == 0) {
                Some(m) => {
                    let Some(Seg::Arc { center, ccw, .. }) =
                        Seg::arc_through(r.pts[k], m, r.pts[(k + 1) % r.pts.len()])
                    else {
                        panic!("{}: not an arc", c.name)
                    };
                    let cp = doc.add_point(center);
                    if ccw {
                        doc.add_arc(cp, a, b)
                    } else {
                        doc.add_arc(cp, b, a)
                    }
                }
                None => doc.add_line(a, b),
            }
            .unwrap();
            doc.set_style(curve, LineKind::Construction);
            curves.push(curve);
        }
        let path = doc
            .add_path(Path {
                curves,
                closed: r.closed,
            })
            .unwrap();
        let mut w = Wall::new(path, r.t, LayerId::DEFAULT);
        if r.hatch {
            w.poche = Some(FillStyle::default());
        }
        w.ends[0].join = r.join[0];
        w.ends[1].join = r.join[1];
        walls.push(doc.add_wall(w).unwrap());
    }
    for &(run, seg, u0, width) in &c.openings {
        let mut o = Opening::new(walls[run], seg, width);
        o.at = OpeningAt::from_start(u0);
        o.leaf = width - 50.0;
        o.symbol = Symbol::Library {
            family: "door.single".into(),
            type_name: String::new(),
        };
        doc.add_opening(o).unwrap();
    }
    let report = regenerate(&mut doc);
    (doc, report.notices)
}

#[test]
fn walls_gallery_joins_make_what_they_should() {
    let mut failures = Vec::new();
    for (i, c) in cases().iter().enumerate() {
        let (doc, notices) = drawing(c);
        let (inputs, _) = regen::wall_inputs(&doc);
        let openings = drift_geom::walls::opening_inputs(&doc, &inputs);
        let g = drift_geom::walls::generate_cut(&inputs, &openings, regen::snap(&doc));
        let outers = g.loops.iter().filter(|l| l.key.hole == 0).count();
        let holes = g.loops.len() - outers;
        let area: f64 = g.regions().iter().map(|(_, _, r)| r.area).sum();
        let mut wrong = Vec::new();
        if !notices.is_empty() {
            wrong.push(format!("notices {notices:?}"));
        }
        if (outers, holes) != (c.outers, c.holes) {
            wrong.push(format!(
                "{outers} outlines and {holes} holes, not {} and {}",
                c.outers, c.holes
            ));
        }
        if let Some(want) = c.area
            && (area - want).abs() > 1e-6 * want
        {
            wrong.push(format!("area {area:.1}, not {want:.1}"));
        }
        // Regenerating again changes nothing.
        let mut again = doc.clone();
        if regenerate(&mut again).changed {
            wrong.push("regenerating again changed it".into());
        }
        if !wrong.is_empty() {
            failures.push(format!("{} {}: {}", i + 1, c.name, wrong.join("; ")));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Every case onto one sheet (`target/tmp/wall-gallery.png`), and each on its own beside it.
#[test]
#[ignore = "writes pictures to look at"]
fn walls_gallery_pictures() {
    const CELL: u32 = 320;
    const COLS: u32 = 5;
    let all = cases();
    let rows = (all.len() as u32).div_ceil(COLS);
    let mut sheet = tiny_skia::Pixmap::new(CELL * COLS, CELL * rows).unwrap();
    sheet.fill(tiny_skia::Color::WHITE);
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/wall-gallery");
    let _ = std::fs::create_dir_all(&dir);
    for (i, c) in all.iter().enumerate() {
        let (doc, _) = drawing(c);
        let mut s = crate::agent::Session::new();
        // (Through an editor: the openings draw their symbols.)
        s.editor = crate::editor::Editor::new(doc);
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
        let png = out.image.expect("a picture");
        let _ = std::fs::write(dir.join(format!("{:02}.png", i + 1)), &png);
        let pic = tiny_skia::Pixmap::decode_png(&png).unwrap();
        let (x, y) = ((i as u32 % COLS) * CELL, (i as u32 / COLS) * CELL);
        sheet.draw_pixmap(
            (x + (CELL - pic.width()) / 2) as i32,
            (y + (CELL - pic.height()) / 2) as i32,
            pic.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
        println!("{:2}. {}", i + 1, c.name);
    }
    sheet.save_png(dir.join("../wall-gallery.png")).unwrap();
}
