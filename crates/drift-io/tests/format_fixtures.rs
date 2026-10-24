//! Every `.drift` format version Drift has written still opens, keeps what it held, and saves
//! as the current version. See `docs/file-format.md` for the policy these tests guard.
//!
//! `tests/fixtures/vNN.drift` is one small drawing as Drift wrote it at format NN. The newest is
//! made by [`write_current_fixture`]; older ones were written by the Drift builds of their day
//! (each loaded the next newer fixture, with that version's additions taken out, and saved it).
#![cfg(not(feature = "demo"))]

use std::path::PathBuf;

use drift_core::{
    Annotation, Constraint, DVec2, Document, DrawingRules, EntityId, Fill, FillKind, Hatch, Kind,
    Layer, LineWeight, Rgb, Variable, Variant,
};
use drift_io::{FORMAT_VERSION, Metadata, load_with_notes, save};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture, oldest first, with the version its name gives.
fn fixtures() -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = std::fs::read_dir(fixtures_dir())
        .expect("tests/fixtures exists")
        .filter_map(|e| {
            let path = e.ok()?.path();
            let name = path.file_name()?.to_str()?;
            let version = name
                .strip_prefix('v')?
                .strip_suffix(".drift")?
                .parse()
                .ok()?;
            Some((
                version,
                std::fs::read_to_string(&path).expect("fixture reads"),
            ))
        })
        .collect();
    out.sort_by_key(|(v, _)| *v);
    out
}

#[test]
fn the_current_format_version_has_a_fixture() {
    let path = fixtures_dir().join(format!("v{FORMAT_VERSION}.drift"));
    assert!(
        path.exists(),
        "FORMAT_VERSION is {FORMAT_VERSION} but there is no {}.\n\
         Every format bump needs a fixture and a migration test (docs/file-format.md):\n\
         extend `fixture_drawing` in this file with what the new version adds, then run\n\
         `cargo test -p drift-io --test format_fixtures write_current_fixture -- --ignored`\n\
         and commit the new file. Leave the older fixtures as they are.",
        path.display()
    );
}

#[test]
fn every_fixture_says_the_version_its_name_does() {
    for (version, text) in fixtures() {
        let value: serde_json::Value = serde_json::from_str(&text).expect("fixture is JSON");
        assert_eq!(value["format"], "drift", "v{version}");
        assert_eq!(value["version"], version, "v{version}.drift");
        assert!(
            version <= FORMAT_VERSION,
            "v{version} is newer than this build"
        );
    }
}

/// What every fixture holds, whatever its version (counts from [`fixture_drawing`]).
const ENTITIES: usize = 24;
const CONSTRAINTS: usize = 8;
const FILLS: usize = 2;
const LAYERS: usize = 3;
const VARIABLES: usize = 2;
const ANNOTATIONS: usize = 2;
const TEXTS: usize = 1;
const GROUPS: usize = 1;
const BLOCKS: usize = 1;
const INSTANCES: usize = 2;
const SHEETS: usize = 1;
const VIEWS: usize = 1;
const UNDERLAYS: usize = 1;
const MEASUREMENTS: usize = 1;

#[test]
fn every_fixture_opens_keeps_its_contents_and_saves_as_current() {
    let all = fixtures();
    assert!(
        all.len() >= 4,
        "expected fixtures for several versions, found {}",
        all.len()
    );
    for (version, text) in all {
        let at = |what: &str| format!("v{version}: {what}");
        let (doc, meta, notes) =
            load_with_notes(&text).unwrap_or_else(|e| panic!("v{version} doesn't open: {e}"));
        assert!(notes.is_empty(), "{}", at(&format!("notes {notes:?}")));
        assert_eq!(meta.title.as_deref(), Some("Fixture"), "{}", at("title"));

        assert_eq!(doc.iter().count(), ENTITIES, "{}", at("entities"));
        assert_eq!(doc.constraint_count(), CONSTRAINTS, "{}", at("constraints"));
        assert_eq!(doc.layers().count(), LAYERS, "{}", at("layers"));
        assert_eq!(doc.variables().count(), VARIABLES, "{}", at("variables"));
        assert_eq!(doc.fills().count(), FILLS, "{}", at("fills"));
        assert_eq!(
            doc.annotations().count(),
            ANNOTATIONS,
            "{}",
            at("annotations")
        );
        assert_eq!(doc.texts().count(), TEXTS, "{}", at("texts"));
        assert_eq!(doc.groups().count(), GROUPS, "{}", at("groups"));
        assert_eq!(doc.blocks().count(), BLOCKS, "{}", at("blocks"));
        assert_eq!(doc.instances().count(), INSTANCES, "{}", at("instances"));
        assert_eq!(doc.sheets().count(), SHEETS, "{}", at("sheets"));
        assert_eq!(doc.views().count(), VIEWS, "{}", at("views"));
        assert_eq!(doc.underlays().count(), UNDERLAYS, "{}", at("underlays"));
        assert_eq!(
            doc.measurements().count(),
            MEASUREMENTS,
            "{}",
            at("measurements")
        );
        assert_eq!(doc.fonts().embedded.len(), 1, "{}", at("embedded font"));
        assert_eq!(
            doc.fonts().drawing.as_deref(),
            Some("Fixture Sans"),
            "{}",
            at("font")
        );
        let project = if version >= 30 { "Fixture project" } else { "" };
        assert_eq!(doc.project().name, project, "{}", at("project"));

        // Expressions drive the dimensions.
        let exprs = doc
            .constraints()
            .filter(|(id, _)| doc.expression(*id).is_some())
            .count();
        assert_eq!(exprs, 2, "{}", at("dimension expressions"));
        // The hatch kept its pattern and the solid its colour.
        assert!(
            doc.fills()
                .any(|(_, f)| matches!(&f.kind, FillKind::Hatch(h) if h.pattern == "ANSI31")),
            "{}",
            at("hatch")
        );
        assert!(
            doc.fills()
                .any(|(_, f)| f.color == Some(Rgb([0xd0, 0xc0, 0xa0]))),
            "{}",
            at("solid fill colour")
        );
        let block = doc.blocks().next().unwrap().1;
        assert_eq!(block.items.len(), 4, "{}", at("block items"));
        assert_eq!(block.fills.len(), 1, "{}", at("block fills"));

        // What each version added.
        let paths = if version >= 38 { 2 } else { 0 };
        assert_eq!(doc.paths().count(), paths, "{}", at("paths"));
        let fills_on_paths = doc.fills().filter(|(_, f)| f.path.is_some()).count();
        assert_eq!(fills_on_paths, paths, "{}", at("fills on paths"));
        if version >= 39 {
            assert_eq!(doc.rules(), fixture_rules(), "{}", at("rules"));
            assert!(!meta.older_rules, "{}", at("older rules"));
        } else {
            assert_eq!(doc.rules(), DrawingRules::default(), "{}", at("rules"));
            assert!(meta.older_rules, "{}", at("older rules"));
        }
        let stacked = doc.iter().filter(|(_, e)| e.order.is_some()).count();
        assert_eq!(stacked, usize::from(version >= 40), "{}", at("stack"));
        let underlay = doc.underlays().next().unwrap().1;
        assert_eq!(
            underlay.layer.is_some(),
            version >= 40,
            "{}",
            at("underlay layer")
        );
        let images = if version >= 33 { 1 } else { 0 };
        assert_eq!(underlay.images.len(), images, "{}", at("underlay pictures"));
        let options = if version >= 41 { 2 } else { 1 };
        assert_eq!(doc.variants().count(), options, "{}", at("options"));
        let sketches = doc.variants().filter(|(_, v)| v.is_sketch()).count();
        assert_eq!(sketches, options - 1, "{}", at("whole-sketch options"));
        let shape_fills = block.fills.iter().filter(|f| f.path.is_some()).count();
        assert_eq!(
            shape_fills,
            usize::from(version >= 41),
            "{}",
            at("block shape fill")
        );

        // Saved, it is a current file that reads back the same.
        let saved = save(&doc, &meta);
        assert!(
            saved.contains(&format!("\"version\": {FORMAT_VERSION},")),
            "{}",
            at("saved version")
        );
        let (again, again_meta, _) = load_with_notes(&saved).unwrap();
        assert_eq!(again, doc, "{}", at("round trip"));
        assert_eq!(
            save(&again, &again_meta),
            saved,
            "{}",
            at("deterministic save")
        );
    }
}

#[test]
fn the_current_fixture_is_what_this_build_writes() {
    let path = fixtures_dir().join(format!("v{FORMAT_VERSION}.drift"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return; // `the_current_format_version_has_a_fixture` says what to do.
    };
    let (doc, meta) = drift_io::load(&text).unwrap();
    assert_eq!(
        save(&doc, &meta),
        text.replace("\r\n", "\n"),
        "the current fixture is no longer what Drift writes for the same drawing; if that is \
         intended (and additive), regenerate it with write_current_fixture"
    );
}

/// Writes `tests/fixtures/v{FORMAT_VERSION}.drift`. Run it once per format bump:
/// `cargo test -p drift-io --test format_fixtures write_current_fixture -- --ignored`.
#[test]
#[ignore = "writes the fixture for the current format version"]
fn write_current_fixture() {
    let meta = Metadata {
        title: Some("Fixture".into()),
        ..Default::default()
    };
    let text = save(&fixture_drawing(), &meta);
    let path = fixtures_dir().join(format!("v{FORMAT_VERSION}.drift"));
    std::fs::write(&path, &text).unwrap();
    // It must open the same.
    let (doc, _) = drift_io::load(&text).unwrap();
    assert_eq!(doc, fixture_drawing());
}

fn fixture_rules() -> DrawingRules {
    DrawingRules {
        regions_by_layer: true,
        regions_need_fill: false,
        stacked_fills: true,
        follow_divided_fills: true,
        gap_tolerance: 0.25,
    }
}

fn line(doc: &mut Document, a: EntityId, b: EntityId) -> EntityId {
    doc.add_line(a, b).unwrap()
}

/// A small room plan: walls drawn as one closed shape and filled, a column hatched, a door
/// swing, furniture as a block, dimensions driven by variables, a note, a text, a sheet with a
/// view, a traced PDF underlay with a picture, and one design option of each kind.
fn fixture_drawing() -> Document {
    let mut doc = Document::new();
    let walls = doc.add_layer(Layer {
        color: Some(Rgb([0x30, 0x30, 0x30])),
        fill_color: Some(Rgb([0xd0, 0xc0, 0xa0])),
        weight: LineWeight(50),
        ..Layer::new("Walls")
    });
    let furniture = doc.add_layer(Layer {
        color: Some(Rgb([0x20, 0x60, 0xc0])),
        ..Layer::new("Furniture")
    });
    let w = doc
        .add_variable(Variable {
            name: "w".into(),
            expr: "4200 mm".into(),
            kind: Kind::Length,
        })
        .unwrap();
    doc.add_variable(Variable {
        name: "d".into(),
        expr: "3000 mm".into(),
        kind: Kind::Length,
    })
    .unwrap();

    // The room: four walls drawn as one closed shape.
    let corners = [(0.0, 0.0), (4200.0, 0.0), (4200.0, 3000.0), (0.0, 3000.0)]
        .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
    let sides: Vec<EntityId> = (0..4)
        .map(|i| line(&mut doc, corners[i], corners[(i + 1) % 4]))
        .collect();
    for &s in &sides {
        doc.set_entity_layer(s, walls).unwrap();
    }
    for c in [
        Constraint::Fix {
            point: corners[0],
            at: DVec2::ZERO,
        },
        Constraint::Horizontal { line: sides[0] },
        Constraint::Vertical { line: sides[1] },
        Constraint::Horizontal { line: sides[2] },
        Constraint::Vertical { line: sides[3] },
    ] {
        doc.add_constraint(c).unwrap();
    }
    let width = doc
        .add_constraint(Constraint::Distance {
            a: corners[0],
            b: corners[1],
            value: 4200.0,
        })
        .unwrap();
    doc.set_expression(width, Some("w".into()));
    doc.set_placement(width, DVec2::new(0.0, -400.0)).unwrap();
    let depth = doc
        .add_constraint(Constraint::Distance {
            a: corners[1],
            b: corners[2],
            value: 3000.0,
        })
        .unwrap();
    doc.set_expression(depth, Some("d".into()));
    let room = doc
        .add_path(drift_core::Path {
            curves: sides.clone(),
            closed: true,
        })
        .unwrap();
    let mut floor = Fill::new(sides.clone(), DVec2::new(2100.0, 1500.0), FillKind::Solid);
    floor.color = Some(Rgb([0xd0, 0xc0, 0xa0]));
    floor.opacity = 0.5;
    floor.layer = walls;
    floor.path = Some(room);
    doc.add_fill(floor);
    // The back wall stacks under the floor fill.
    doc.set_order(sides[2], Some(-1)).unwrap();

    // A round column, hatched.
    let column_centre = doc.add_point(DVec2::new(3600.0, 2400.0));
    let column = doc.add_circle(column_centre, 150.0).unwrap();
    doc.add_constraint(Constraint::Radius {
        curve: column,
        value: 150.0,
    })
    .unwrap();
    let column_path = doc
        .add_path(drift_core::Path {
            curves: vec![column],
            closed: true,
        })
        .unwrap();
    let mut hatch = Hatch::new("ANSI31");
    hatch.scale = 20.0;
    hatch.angle = 45.0;
    let mut column_fill = Fill::new(
        vec![column],
        DVec2::new(3600.0, 2400.0),
        FillKind::Hatch(hatch),
    );
    column_fill.weight = Some(LineWeight(18));
    column_fill.path = Some(column_path);
    doc.add_fill(column_fill);
    doc.add_group(drift_core::Group {
        name: "Column".into(),
        members: [column_centre, column].into(),
    })
    .unwrap();

    // A door swing (an arc) and a construction line along the opening.
    let hinge = doc.add_point(DVec2::new(600.0, 0.0));
    let leaf = doc.add_point(DVec2::new(1500.0, 0.0));
    let open = doc.add_point(DVec2::new(600.0, 900.0));
    doc.add_arc(hinge, leaf, open).unwrap();
    let opening = line(&mut doc, hinge, leaf);
    doc.set_style(opening, drift_core::LineKind::Construction);

    // A rug (an ellipse) and a curved bench (a Bézier).
    let rug = [(2100.0, 1500.0), (2700.0, 1500.0), (2100.0, 1900.0)]
        .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
    doc.add_ellipse(rug[0], rug[1], rug[2]).unwrap();
    let bench = [
        (300.0, 2600.0),
        (900.0, 2900.0),
        (1500.0, 2900.0),
        (2100.0, 2600.0),
    ]
    .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
    let bench = doc
        .add_bezier(bench[0], bench[1], bench[2], bench[3])
        .unwrap();
    doc.set_entity_layer(bench, furniture).unwrap();
    doc.add_measurement(Constraint::Distance {
        a: corners[0],
        b: corners[2],
        value: 1.0,
    })
    .unwrap();

    // A chair: a block with a filled seat, placed twice.
    let seat = [(0.0, 0.0), (450.0, 0.0), (450.0, 450.0), (0.0, 450.0)].map(DVec2::from);
    let items = (0..4)
        .map(|i| drift_core::BlockItem {
            shape: drift_core::Shape::Line {
                a: seat[i],
                b: seat[(i + 1) % 4],
            },
            style: Default::default(),
            appearance: Default::default(),
        })
        .collect();
    let mut seat_fill = Fill::new(Vec::new(), DVec2::new(225.0, 225.0), FillKind::Solid);
    seat_fill.path = Some(drift_core::blocks::SHAPE_FILL);
    let chair = doc.add_block(drift_core::Block {
        name: "Chair".into(),
        items,
        fills: vec![seat_fill],
        profile: false,
    });
    for at in [(1000.0, 1000.0), (1600.0, 1000.0)] {
        doc.add_instance(drift_core::Instance::new(chair, DVec2::from(at)))
            .unwrap();
    }

    // Words: a note with a leader, a centre mark, a text, and the font they are set in.
    doc.add_annotation(Annotation::Note {
        text: "Oak floor".into(),
        at: DVec2::new(2500.0, 3400.0),
        leaders: vec![drift_core::Leader {
            to: drift_core::Anchor::Free(DVec2::new(2100.0, 1500.0)),
            end: drift_core::LeaderEnd::Arrow,
            shape: drift_core::LeaderShape::Straight,
            shoulder: true,
            shoulder_len: drift_core::Leader::SHOULDER,
        }],
        style: Default::default(),
    })
    .unwrap();
    doc.add_annotation(Annotation::CentreMark { curve: column })
        .unwrap();
    let mut label = drift_core::SketchText::new("LIVING", DVec2::new(1800.0, 1200.0), 200.0);
    label.layer = furniture;
    doc.add_text(label);
    doc.set_fonts(drift_core::FontSet {
        embedded: [(
            "Fixture Sans".to_owned(),
            drift_core::EmbeddedFont {
                // Not a real font: the file only carries the bytes.
                regular: drift_core::FontFile::new(b"not a real font".to_vec(), 0),
                bold: None,
            },
        )]
        .into(),
        drawing: Some("Fixture Sans".into()),
    });

    // A sheet showing the plan.
    let plan = doc.add_view(drift_core::View::new(
        "Plan",
        drift_core::Bounds {
            min: DVec2::new(-500.0, -500.0),
            max: DVec2::new(4700.0, 3500.0),
        },
        50.0,
    ));
    let mut sheet = drift_core::Sheet::new("A-101");
    sheet
        .viewports
        .push(drift_core::Viewport::new(plan, DVec2::new(200.0, 150.0)));
    sheet.title_block = drift_core::TitleBlock::Modern;
    doc.add_sheet(sheet);
    doc.set_project(drift_core::Project {
        name: "Fixture project".into(),
        number: "F-001".into(),
        ..Default::default()
    });

    // A traced PDF on the walls layer, with a 2×2 picture.
    let mut underlay = drift_core::Underlay::new(
        "survey.pdf",
        "survey.pdf",
        vec![
            drift_core::RefPrim::Line(DVec2::ZERO, DVec2::new(4200.0, 0.0)),
            drift_core::RefPrim::Circle {
                center: DVec2::new(3600.0, 2400.0),
                radius: 150.0,
            },
        ],
    );
    underlay.images = std::sync::Arc::new(vec![drift_core::UnderlayImage {
        width: 2,
        height: 2,
        rgba: std::sync::Arc::new(vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ]),
        at: DVec2::new(0.0, 3000.0),
        across: DVec2::new(100.0, 0.0),
        down: DVec2::new(0.0, -100.0),
    }]);
    underlay.layer = Some(walls);
    underlay.order = -2;
    doc.add_underlay(underlay);

    doc.set_rules(fixture_rules());
    // Options: narrower values, and the whole sketch as it was.
    doc.add_variant(Variant::values(
        "Narrow",
        [(w, "3600 mm".to_owned())].into(),
    ));
    let before = Variant::sketch("As found", &doc);
    doc.add_variant(before);
    doc
}
