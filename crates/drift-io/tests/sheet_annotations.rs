//! A sheet's own annotations in the file (#432, format 45): they save and reload exactly,
//! older files open as they were, a file an older build edited is repaired on reading
//! (`docs/file-format.md` rule 3), and a bad record is refused rather than half-read.
#![cfg(not(feature = "demo"))]

use drift_core::{
    Anchor, ArrowStyle, Bounds, Constraint, DVec2, DimNote, Document, EntityId, Leader, LeaderEnd,
    LeaderShape, NoteFrame, NoteStyle, PortId, Sheet, SheetAnnotation, SheetAnnotationId, SheetId,
    SheetLeader, StyleOverride, TextAlign, Tolerance, View, Viewport,
};
use drift_io::{FORMAT_VERSION, Metadata, READER_VERSION, load, load_with_notes, save};
use proptest::prelude::*;

const V: fn(f64, f64) -> DVec2 = DVec2::new;

/// A rectangle with a circle and an arc, two views of it placed on a sheet (the first
/// twice), and a second sheet.
struct Drawing {
    doc: Document,
    points: Vec<EntityId>,
    lines: Vec<EntityId>,
    circle: EntityId,
    arc: EntityId,
    sheet: SheetId,
    /// The ids of the first sheet's three viewports.
    ports: [PortId; 3],
}

fn drawing() -> Drawing {
    let mut doc = Document::new();
    let points: Vec<EntityId> = [(0.0, 0.0), (4000.0, 0.0), (4000.0, 3000.0), (0.0, 3000.0)]
        .into_iter()
        .map(|(x, y)| doc.add_point(V(x, y)))
        .collect();
    let lines: Vec<EntityId> = (0..4)
        .map(|i| doc.add_line(points[i], points[(i + 1) % 4]).unwrap())
        .collect();
    let c = doc.add_point(V(2000.0, 1500.0));
    let circle = doc.add_circle(c, 400.0).unwrap();
    let (s, e) = (
        doc.add_point(V(1000.0, 500.0)),
        doc.add_point(V(500.0, 1000.0)),
    );
    let centre = doc.add_point(V(500.0, 500.0));
    let arc = doc.add_arc(centre, s, e).unwrap();
    let crop = Bounds {
        min: V(-500.0, -500.0),
        max: V(4500.0, 3500.0),
    };
    let plan = doc.add_view(View::new("Plan", crop, 50.0));
    let detail = doc.add_view(View::new("Detail", crop, 20.0));
    let mut s = Sheet::new("A-101");
    s.viewports.push(Viewport::new(plan, V(120.0, 200.0)));
    s.viewports.push(Viewport::new(detail, V(300.0, 150.0)));
    s.viewports.push(Viewport::new(plan, V(120.0, 80.0)));
    let sheet = doc.add_sheet(s);
    doc.add_sheet(Sheet::new("A-102"));
    let ports = [0, 1, 2].map(|i| doc.tie_viewport(sheet, i).unwrap());
    Drawing {
        doc,
        points,
        lines,
        circle,
        arc,
        sheet,
        ports,
    }
}

impl Drawing {
    fn add(&mut self, a: SheetAnnotation) -> SheetAnnotationId {
        self.doc.add_sheet_annotation(self.sheet, a).unwrap()
    }

    /// Every kind of dimension a sheet can hold.
    fn measures(&self) -> Vec<Constraint> {
        let (a, b) = (self.points[0], self.points[2]);
        vec![
            Constraint::Distance { a, b, value: 0.0 },
            Constraint::HorizontalDistance { a, b, value: 0.0 },
            Constraint::VerticalDistance { a, b, value: 0.0 },
            Constraint::PointLineDistance {
                point: self.points[3],
                line: self.lines[0],
                value: 0.0,
            },
            Constraint::Radius {
                curve: self.arc,
                value: 0.0,
            },
            Constraint::Diameter {
                curve: self.circle,
                value: 0.0,
            },
            Constraint::Angle {
                a: self.lines[0],
                b: self.lines[1],
                degrees: 0.0,
            },
        ]
    }
}

/// Saved and read back: the same drawing, and it saves the same again.
fn round_trip(doc: &Document) -> String {
    let text = save(doc, &Metadata::default());
    let (back, meta, notes) = load_with_notes(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert!(notes.is_empty(), "{notes:?}");
    for ((id, a), (_, b)) in back.sheets().zip(doc.sheets()) {
        for (aid, x) in &a.annotations {
            assert_eq!(Some(x), b.annotations.get(aid), "{id} {aid}");
        }
        assert_eq!(a, b, "{id}");
    }
    for ((id, a), (_, b)) in back.iter().zip(doc.iter()) {
        assert_eq!(a, b, "{id}");
    }
    assert_eq!(&back, doc, "{text}");
    assert_eq!(save(&back, &meta), text);
    text
}

#[test]
fn every_kind_of_sheet_annotation_round_trips() {
    let mut d = drawing();
    // Each kind of dimension, through each viewport in turn, placed and not, with every
    // tolerance and a style of its own.
    let tolerances = [
        Tolerance::None,
        Tolerance::Symmetric(0.5),
        Tolerance::Deviation {
            upper: 0.2,
            lower: 0.1,
        },
        Tolerance::Limits {
            upper: 0.3,
            lower: 0.0,
        },
        Tolerance::Fit("H7".into()),
    ];
    for (i, measure) in d.measures().into_iter().enumerate() {
        let note = DimNote {
            prefix: if i % 2 == 0 {
                "4×".into()
            } else {
                String::new()
            },
            suffix: if i % 3 == 0 {
                "TYP".into()
            } else {
                String::new()
            },
            tolerance: tolerances[i % tolerances.len()].clone(),
            below: if i == 1 {
                "TO FACE".into()
            } else {
                String::new()
            },
            text: if i == 2 { "EQ".into() } else { String::new() },
        };
        let look = StyleOverride {
            text_height: (i % 2 == 0).then_some(2.5),
            arrow: (i % 3 == 0).then_some(ArrowStyle::Tick),
            aligned_text: (i == 4).then_some(false),
            color: (i == 5).then_some(Some(drift_core::Rgb([200, 0, 0]))),
            ..StyleOverride::default()
        };
        d.add(SheetAnnotation::Dimension {
            port: d.ports[i % 3],
            measure,
            label: (i != 3).then(|| V(1.5 * i as f64, -8.25)),
            note,
            look,
        });
    }
    // Notes: on the paper alone; with every kind of leader, shape and end; styled.
    d.add(SheetAnnotation::note(
        "GENERAL NOTES\n1. All dimensions in mm",
        V(350.0, 40.0),
    ));
    let mut bent = SheetLeader::through(
        d.ports[1],
        Anchor::Curve {
            curve: d.lines[2],
            t: 0.375,
        },
    );
    bent.leader.shape = LeaderShape::Bent(V(4.0, 6.0));
    bent.leader.end = LeaderEnd::Dot;
    let mut curved = SheetLeader::through(d.ports[0], Anchor::Point(d.points[1]));
    curved.leader.shape = LeaderShape::Curved(-0.25);
    curved.leader.end = LeaderEnd::OpenArrow;
    curved.leader.shoulder = false;
    let model_point = SheetLeader::through(d.ports[2], Anchor::Free(V(1234.5, 678.25)));
    let mut paper = SheetLeader::on_paper(V(20.0, 30.0));
    paper.leader.end = LeaderEnd::None;
    paper.leader.shoulder_len = 5.0;
    d.add(SheetAnnotation::Note {
        text: "SEE DETAIL 3 \"A\"".into(),
        at: V(200.0, 250.0),
        leaders: vec![bent, curved, model_point, paper],
        style: NoteStyle {
            align: TextAlign::Right,
            wrap: Some(40.0),
            frame: NoteFrame::Rounded,
            mask: true,
            bold: true,
            italic: true,
            height: Some(5.0),
        },
        look: StyleOverride {
            arrow_size: Some(3.0),
            ..StyleOverride::default()
        },
    });
    let text = round_trip(&d.doc);
    assert!(text.contains(&format!("\"version\": {FORMAT_VERSION},")));
    assert!(text.contains(&format!("\"reader\": {READER_VERSION},")));
    // A dimension is written without a value: there is none to keep.
    let line = text
        .lines()
        .find(|l| l.contains("\"annotations\""))
        .unwrap();
    for measure in line.split("\"measure\":{").skip(1) {
        let measure = &measure[..measure.find('}').unwrap()];
        assert!(
            !measure.contains("\"value\"") && !measure.contains("\"degrees\""),
            "{measure}"
        );
    }
    // None of it is in the model's tables.
    let (back, _) = load(&text).unwrap();
    assert_eq!(back.annotations().count(), 0);
    assert_eq!(back.constraints().count(), 0);
    assert_eq!(back.sheet(d.sheet).unwrap().annotations.len(), 9);
    // New ids after loading never collide with the file's.
    let mut back = back;
    let fresh = back
        .add_sheet_annotation(d.sheet, SheetAnnotation::note("x", V(1.0, 1.0)))
        .unwrap();
    let taken: Vec<u32> = d
        .doc
        .sheet(d.sheet)
        .unwrap()
        .annotations
        .keys()
        .map(|a| a.0)
        .chain(d.ports.iter().map(|p| p.0))
        .collect();
    assert!(!taken.contains(&fresh.0), "{fresh} {taken:?}");
    assert!(back.tie_viewport(d.sheet, 0).is_some());
}

#[test]
fn a_drawing_without_sheet_annotations_writes_what_it_always_wrote() {
    // No viewport has an id until something is tied to it, so nothing new is written.
    let mut doc = Document::new();
    let view = doc.add_view(View::new(
        "Plan",
        Bounds {
            min: V(0.0, 0.0),
            max: V(100.0, 100.0),
        },
        1.0,
    ));
    let mut s = Sheet::new("A-101");
    s.viewports.push(Viewport::new(view, V(200.0, 150.0)));
    doc.add_sheet(s);
    let text = round_trip(&doc);
    let line = text.lines().find(|l| l.contains("\"viewports\"")).unwrap();
    assert!(!line.contains("\"annotations\""), "{line}");
    assert!(!line.contains("\"id\":\"r"), "{line}");
}

#[test]
fn an_older_file_opens_unchanged() {
    // The last fixture written before sheets had annotations of their own.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v44.2.drift");
    let old = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    let (doc, meta, notes) = load_with_notes(&old).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    for (_, s) in doc.sheets() {
        assert!(s.annotations.is_empty());
        assert!(s.viewports.iter().all(|p| p.id.is_none()));
    }
    // Saved by this build it is the same file but for the version line.
    let saved = save(&doc, &meta);
    // (The fixture's whole-sketch option is a file of its own, with its own version.)
    let bumped = old
        .replace(
            "\"version\": 44,",
            &format!("\"version\": {FORMAT_VERSION},"),
        )
        .replace("\"version\":44,", &format!("\"version\":{FORMAT_VERSION},"));
    for (now, was) in saved.lines().zip(bumped.lines()) {
        assert_eq!(now, was);
    }
    assert_eq!(saved, bumped);
}

/// A saved drawing with a dimension through the first viewport and a note with a leader
/// through it to a line and one to the paper.
fn tied() -> (Drawing, String) {
    let mut d = drawing();
    d.add(SheetAnnotation::dimension(
        d.ports[0],
        Constraint::Distance {
            a: d.points[0],
            b: d.points[1],
            value: 0.0,
        },
        Some(V(0.0, -8.0)),
    ));
    d.add(SheetAnnotation::Note {
        text: "WALL".into(),
        at: V(60.0, 260.0),
        leaders: vec![
            SheetLeader::through(
                d.ports[0],
                Anchor::Curve {
                    curve: d.lines[1],
                    t: 0.5,
                },
            ),
            SheetLeader::on_paper(V(70.0, 240.0)),
        ],
        style: NoteStyle::default(),
        look: StyleOverride::default(),
    });
    let text = save(&d.doc, &Metadata::default());
    (d, text)
}

fn sheet_notes(doc: &Document, sheet: SheetId) -> Vec<SheetAnnotation> {
    doc.sheet(sheet)
        .unwrap()
        .annotations
        .values()
        .cloned()
        .collect()
}

#[test]
fn what_an_older_build_left_dangling_is_repaired_and_said() {
    let (d, text) = tied();
    let port = d.ports[0].to_string();
    // An older build passes the annotations over and keeps them. It took the view off the
    // sheet (here: the viewport's id is gone with it): the dimension goes, the note keeps
    // its text and its paper leader, and the file says so when it opens.
    let without_port = text.replacen(&format!(",\"id\":\"{port}\""), "", 1);
    assert_ne!(without_port, text);
    let (doc, _, notes) = load_with_notes(&without_port).unwrap();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("A-101") && notes[0].contains("2 annotations"),
        "{notes:?}"
    );
    let left = sheet_notes(&doc, d.sheet);
    assert_eq!(left.len(), 1);
    assert!(matches!(
        &left[0],
        SheetAnnotation::Note { text, at, leaders, .. }
            if text == "WALL" && *at == V(60.0, 260.0)
                && leaders[..] == [SheetLeader::on_paper(V(70.0, 240.0))]
    ));
    // Repaired, it saves and opens clean: the repair is written, not carried along as
    // something of the file's this build doesn't know.
    let (doc, meta, _) = load_with_notes(&without_port).unwrap();
    assert!(!meta.carried.any());
    let saved = save(&doc, &meta);
    assert!(!saved.contains("\"type\":\"dimension\""), "{saved}");
    let (again, _, notes) = load_with_notes(&saved).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(again, doc);

    // Every annotation of a sheet gone at once (the sheet then writes no `annotations` at
    // all): written as repaired too.
    let only_dimension = {
        let mut d = drawing();
        d.add(SheetAnnotation::dimension(
            d.ports[0],
            Constraint::Distance {
                a: d.points[0],
                b: d.points[1],
                value: 0.0,
            },
            None,
        ));
        save(&d.doc, &Metadata::default())
    };
    let dangling = only_dimension.replacen(&format!(",\"id\":\"{port}\""), "", 1);
    assert_ne!(dangling, only_dimension);
    let (doc, meta, notes) = load_with_notes(&dangling).unwrap();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(!meta.carried.any());
    let saved = save(&doc, &meta);
    assert!(!saved.contains("\"annotations\""), "{saved}");
    let (again, _, notes) = load_with_notes(&saved).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(again, doc);

    // It deleted the line the leader was on: the same, for that leader alone.
    let line = d.lines[1].to_string();
    let gone = format!("\"curve\":\"{line}\"");
    assert!(text.contains(&gone));
    let dangling = text.replace(&gone, "\"curve\":\"e9999\"");
    let (doc, _, notes) = load_with_notes(&dangling).unwrap();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("1 annotation "), "{notes:?}");
    let left = sheet_notes(&doc, d.sheet);
    assert_eq!(left.len(), 2, "the dimension is on other geometry");
    assert!(left.iter().any(|a| matches!(
        a,
        SheetAnnotation::Note { leaders, .. } if leaders.len() == 1 && leaders[0].port.is_none()
    )));
}

#[test]
fn a_bad_sheet_annotation_is_refused() {
    let (d, text) = tied();
    let edit = |from: &str, to: &str| {
        assert!(text.contains(from), "{from} in {text}");
        text.replacen(from, to, 1)
    };
    let (a, line) = (d.points[0].to_string(), d.lines[0].to_string());
    let bad = [
        // Not a kind of dimension, or a relation rather than one.
        edit(
            "\"measure\":{\"type\":\"distance\"",
            "\"measure\":{\"type\":\"sideways\"",
        ),
        edit(
            "\"measure\":{\"type\":\"distance\"",
            "\"measure\":{\"type\":\"coincident\"",
        ),
        // A radius of a point: the wrong kind of entity for it.
        edit(
            &format!("\"measure\":{{\"type\":\"distance\",\"a\":\"{a}\""),
            &format!("\"measure\":{{\"type\":\"radius\",\"curve\":\"{a}\",\"a\":\"{a}\""),
        ),
        // Missing what it measures; ids that aren't ids; numbers that aren't numbers.
        edit(&format!("\"a\":\"{a}\",\"b\""), "\"b\""),
        edit(
            "\"type\":\"dimension\",\"id\":\"a",
            "\"type\":\"dimension\",\"id\":\"n",
        ),
        edit("\"port\":\"r", "\"port\":\"w"),
        edit("\"label\":[0.0,-8.0]", "\"label\":[0.0,1e999]"),
        edit("\"at\":[60.0,260.0]", "\"at\":[60.0,null]"),
        edit("\"type\":\"note\"", "\"type\":\"cloud\""),
        // A leader to geometry with no viewport to see it through.
        edit(
            "{\"leader\":{\"to\":{\"at\":[70.0,240.0]}}}",
            &format!("{{\"leader\":{{\"to\":{{\"point\":\"{a}\"}}}}}}"),
        ),
    ];
    for file in bad {
        assert!(load(&file).is_err(), "{file}");
    }
    let _ = line;
    // The same id twice, on one sheet or across two, is refused too.
    let line = text
        .lines()
        .find(|l| l.contains("\"annotations\""))
        .unwrap();
    let start = line.find("{\"type\":\"dimension\"").unwrap();
    let end = line[start..].find("},{\"type\":\"note\"").unwrap() + start + 1;
    let record = &line[start..end];
    let twice = text.replacen(record, &format!("{record},{record}"), 1);
    assert!(load(&twice).is_err());
    let other = text.replacen(
        "\"name\":\"A-102\"",
        &format!("\"name\":\"A-102\",\"annotations\":[{{\"type\":\"note\",\"id\":\"{}\",\"text\":\"x\",\"at\":[1.0,1.0]}}]",
            d.doc.sheet(d.sheet).unwrap().annotations.keys().next().unwrap()),
        1,
    );
    assert_ne!(other, text);
    assert!(load(&other).is_err());
}

// ----- Properties ------------------------------------------------------------------------

fn coordinate() -> impl Strategy<Value = f64> {
    prop_oneof![
        -500.0..500.0f64,
        Just(0.0),
        Just(-0.0),
        Just(1e-9),
        Just(123_456.789),
    ]
}

fn point() -> impl Strategy<Value = DVec2> {
    (coordinate(), coordinate()).prop_map(|(x, y)| V(x, y))
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        "[ -~]{0,24}",
        Just("Ø25 ±0.1\n“quoted” \\ / \u{1F600}".to_owned()),
    ]
}

/// What a generated leader is made from: its viewport (or the paper), what it points at
/// (a model point, a sketch point or a spot on a line) and which, a point and a number for
/// those, its shape and end, and whether it has a shoulder.
type LeaderSeed = (Option<usize>, u8, usize, DVec2, f64, u8, u8, bool);

/// A leader: to the paper, or through viewport `port` (0–2) to a model point, a sketch
/// point (0–3) or a spot on a line (0–3).
fn leader() -> impl Strategy<Value = LeaderSeed> {
    (
        prop::option::of(0..3usize),
        0..3u8,
        0..4usize,
        point(),
        -2.0..2.0f64,
        0..3u8,
        0..4u8,
        any::<bool>(),
    )
}

#[derive(Clone, Debug)]
enum Made {
    Note {
        text: String,
        at: DVec2,
        leaders: Vec<LeaderSeed>,
        bold: bool,
        frame: u8,
        height: Option<f64>,
    },
    Dimension {
        port: usize,
        measure: usize,
        label: Option<DVec2>,
        prefix: String,
        height: Option<f64>,
    },
}

fn made() -> impl Strategy<Value = Made> {
    prop_oneof![
        (
            text(),
            point(),
            prop::collection::vec(leader(), 0..4),
            any::<bool>(),
            0..3u8,
            prop::option::of(0.5..20.0f64),
        )
            .prop_map(|(text, at, leaders, bold, frame, height)| Made::Note {
                text,
                at,
                leaders,
                bold,
                frame,
                height,
            }),
        (
            0..3usize,
            0..7usize,
            prop::option::of(point()),
            text(),
            prop::option::of(0.5..20.0f64),
        )
            .prop_map(|(port, measure, label, prefix, height)| Made::Dimension {
                port,
                measure,
                label,
                prefix,
                height,
            }),
    ]
}

fn build(d: &Drawing, m: &Made) -> SheetAnnotation {
    match m {
        Made::Note {
            text,
            at,
            leaders,
            bold,
            frame,
            height,
        } => SheetAnnotation::Note {
            text: text.clone(),
            at: *at,
            leaders: leaders
                .iter()
                .map(|(port, kind, which, p, t, shape, end, shoulder)| {
                    let to = match (port, kind) {
                        (None, _) | (Some(_), 0) => Anchor::Free(*p),
                        (Some(_), 1) => Anchor::Point(d.points[*which]),
                        (Some(_), _) => Anchor::Curve {
                            curve: d.lines[*which],
                            t: *t,
                        },
                    };
                    SheetLeader {
                        leader: Leader {
                            to,
                            end: LeaderEnd::ALL[usize::from(*end)],
                            shape: match shape {
                                0 => LeaderShape::Straight,
                                1 => LeaderShape::Bent(*p),
                                _ => LeaderShape::Curved(t.clamp(-1.0, 1.0)),
                            },
                            shoulder: *shoulder,
                            shoulder_len: if *shoulder { Leader::SHOULDER } else { 4.0 },
                        },
                        port: port.map(|i| d.ports[i]),
                    }
                })
                .collect(),
            style: NoteStyle {
                bold: *bold,
                frame: NoteFrame::ALL[usize::from(*frame)],
                height: *height,
                ..NoteStyle::default()
            },
            look: StyleOverride::default(),
        },
        Made::Dimension {
            port,
            measure,
            label,
            prefix,
            height,
        } => SheetAnnotation::Dimension {
            port: d.ports[*port],
            measure: d.measures()[*measure],
            label: *label,
            note: DimNote {
                prefix: prefix.clone(),
                ..DimNote::default()
            },
            look: StyleOverride {
                text_height: *height,
                ..StyleOverride::default()
            },
        },
    }
}

/// What every sheet of a loaded drawing must be: nothing on it names a viewport it doesn't
/// have, a view or an entity that isn't there.
fn settled(doc: &Document) {
    for (_, s) in doc.sheets() {
        for a in s.annotations.values() {
            for p in a.ports() {
                assert!(
                    doc.port_view(s, p).is_some(),
                    "{a:?} names a missing viewport"
                );
            }
            for e in a.entities() {
                assert!(doc.get(e).is_some(), "{a:?} names a missing entity");
            }
            assert!(a.is_valid(), "{a:?}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 96,
        ..ProptestConfig::default()
    })]

    /// Any annotations the document accepts save and reload exactly.
    #[test]
    fn whatever_is_put_on_a_sheet_round_trips(all in prop::collection::vec(made(), 0..12)) {
        let mut d = drawing();
        for m in &all {
            let a = build(&d, m);
            d.add(a);
        }
        round_trip(&d.doc);
        settled(&d.doc);
    }

    /// A file with its sheets' records damaged never panics the reader: it is refused, or
    /// it opens as a drawing with nothing dangling that saves and opens again the same.
    #[test]
    fn a_damaged_file_is_refused_or_opens_sound(
        all in prop::collection::vec(made(), 1..6),
        edits in prop::collection::vec((any::<prop::sample::Index>(), 0..6u8, any::<u8>()), 1..6),
    ) {
        let mut d = drawing();
        for m in &all {
            let a = build(&d, m);
            d.add(a);
        }
        let text = save(&d.doc, &Metadata::default());
        // The sheets' lines, damaged a few characters at a time: a digit changed, an id
        // renamed, a character dropped or doubled, a quote or bracket turned.
        let (from, to) = {
            let from = text.find("\"sheets\": [").unwrap();
            (from, from + text[from..].find("\n  ]").unwrap())
        };
        let mut bytes = text.clone().into_bytes();
        for (at, how, with) in edits {
            let i = from + at.index(to - from);
            if !bytes[i].is_ascii() {
                continue;
            }
            match how {
                0 => bytes[i] = b'0' + with % 10,
                1 => bytes[i] = b"era\"[]{},:.-"[usize::from(with) % 12],
                2 => {
                    bytes.remove(i);
                }
                3 => bytes.insert(i, bytes[i]),
                4 if bytes[i].is_ascii_digit() => bytes[i] = b'9',
                _ => bytes[i] = with % 0x60 + 0x20,
            }
        }
        let Ok(damaged) = String::from_utf8(bytes) else {
            return Ok(());
        };
        if let Ok((doc, meta, _)) = load_with_notes(&damaged) {
            settled(&doc);
            let saved = save(&doc, &meta);
            let (again, _, notes) = load_with_notes(&saved)
                .map_err(|e| TestCaseError::fail(format!("saved file doesn't open: {e}")))?;
            // What was repaired on reading is written repaired: it opens clean after.
            prop_assert!(notes.is_empty(), "{:?}", notes);
            prop_assert_eq!(again, doc);
        }
    }
}
