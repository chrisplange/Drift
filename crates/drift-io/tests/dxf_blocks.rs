//! Blocks from DXF and DWG come in as AutoCAD shows them:
//! - an insert's Y scale is 1 where the file leaves it out (AutoCAD, ODA and acadrust do, so
//!   a door mirrored as −1, 1 came in turned 180°);
//! - "by block" colours, line types and weights take each copy's own;
//! - a block keeps to its own drawing: one of the same name already in the drawing is used
//!   only if it is the same;
//! - a dynamic block's copies are named after it, not `*U12`;
//! - an external reference is named in a note instead of vanishing.

use drift_core::{DVec2, Document, LineType, Rgb, Shape};
use drift_io::dxf::{DxfImport, add_to_document, read};

/// A DXF file from group codes and values.
fn dxf(pairs: &[(i32, &str)]) -> String {
    pairs
        .iter()
        .map(|(code, value)| format!("{code:>3}\n{value}\n"))
        .collect()
}

/// A file with these blocks and these model-space entities (millimetres).
fn file(blocks: &[(i32, &str)], entities: &[(i32, &str)]) -> String {
    let mut all = vec![
        (0, "SECTION"),
        (2, "HEADER"),
        (9, "$INSUNITS"),
        (70, "4"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "BLOCKS"),
    ];
    all.extend_from_slice(blocks);
    all.extend_from_slice(&[(0, "ENDSEC"), (0, "SECTION"), (2, "ENTITIES")]);
    all.extend_from_slice(entities);
    all.extend_from_slice(&[(0, "ENDSEC"), (0, "EOF")]);
    dxf(&all)
}

/// A door: a leaf along +y from the hinge and its swing arc out to +x.
const DOOR: &[(i32, &str)] = &[
    (0, "BLOCK"),
    (2, "DOOR"),
    (10, "0"),
    (20, "0"),
    (0, "LINE"),
    (8, "0"),
    (10, "0"),
    (20, "0"),
    (11, "0"),
    (21, "900"),
    (0, "ARC"),
    (8, "0"),
    (10, "0"),
    (20, "0"),
    (40, "900"),
    (50, "0"),
    (51, "90"),
    (0, "ENDBLK"),
];

fn import(doc: &mut Document, import: &DxfImport) {
    doc.begin();
    add_to_document(doc, import);
    doc.commit("Import");
}

/// Every curve the copies of blocks put in the drawing, in world coordinates.
fn placed(doc: &Document) -> Vec<Shape> {
    doc.instances()
        .flat_map(|(id, _)| doc.instance_items(id))
        .map(|item| item.shape)
        .collect()
}

#[test]
fn a_door_mirrored_without_a_y_scale_is_mirrored_not_turned() {
    // 41 = −1 and no 42, as AutoCAD writes a mirrored door.
    let text = file(
        DOOR,
        &[
            (0, "INSERT"),
            (8, "DOORS"),
            (2, "DOOR"),
            (10, "1000"),
            (20, "0"),
            (41, "-1"),
        ],
    );
    let back = read(&text, 1.0).unwrap();
    assert_eq!(back.inserts.len(), 1, "kept as a copy of the block");
    let copy = &back.inserts[0];
    assert!(copy.mirror, "mirrored");
    assert!(copy.angle.abs() < 1e-9, "not turned: {}", copy.angle);
    let mut doc = Document::new();
    import(&mut doc, &back);
    // The leaf still runs up from the hinge (turned 180° it ran down), and the swing goes
    // out to −x.
    let shapes = placed(&doc);
    assert!(shapes.iter().any(|s| match *s {
        Shape::Line { a, b } => {
            let (lo, hi) = if a.y < b.y { (a, b) } else { (b, a) };
            lo.distance(DVec2::new(1000.0, 0.0)) < 1e-6
                && hi.distance(DVec2::new(1000.0, 900.0)) < 1e-6
        }
        _ => false,
    }));
    let swing = shapes
        .iter()
        .find_map(|s| match *s {
            Shape::Arc { .. } => Some(s.bounds()),
            _ => None,
        })
        .unwrap();
    assert!(swing.min.x < 1000.0 - 800.0 && swing.max.x <= 1000.0 + 1e-6);
    assert!(swing.min.y >= -1e-6, "the swing stays above the hinge");
}

#[test]
fn a_copy_stretched_in_x_only_keeps_its_height() {
    let circle = [
        (0, "BLOCK"),
        (2, "PIPE"),
        (10, "0"),
        (20, "0"),
        (0, "CIRCLE"),
        (8, "0"),
        (10, "0"),
        (20, "0"),
        (40, "10"),
        (0, "ENDBLK"),
    ];
    let text = file(
        &circle,
        &[(0, "INSERT"), (2, "PIPE"), (10, "0"), (20, "0"), (41, "2")],
    );
    let back = read(&text, 1.0).unwrap();
    let e = back
        .entities
        .iter()
        .find(|e| matches!(e.shape, Shape::Ellipse { .. }));
    let Some(Shape::Ellipse { major, minor, .. }) = e.map(|e| e.shape) else {
        panic!("a 2 × 1 stretch makes an ellipse: {:?}", back.entities);
    };
    assert!((major.length() - 20.0).abs() < 1e-9 && (minor - 10.0).abs() < 1e-9);
}

#[test]
fn by_block_colours_line_types_and_weights_follow_each_copy() {
    // A chair whose outline is "by block" in all three, and a seat mark in its own red.
    let chair = [
        (0, "BLOCK"),
        (2, "CHAIR"),
        (10, "0"),
        (20, "0"),
        (0, "LINE"),
        (8, "0"),
        (62, "0"),
        (6, "BYBLOCK"),
        (370, "-2"),
        (10, "0"),
        (20, "0"),
        (11, "450"),
        (21, "0"),
        (0, "CIRCLE"),
        (8, "0"),
        (62, "1"),
        (10, "225"),
        (20, "225"),
        (40, "50"),
        (0, "ENDBLK"),
    ];
    let text = file(
        &chair,
        &[
            // Blue, dashed, 0.5 mm.
            (0, "INSERT"),
            (8, "FURN"),
            (2, "CHAIR"),
            (10, "0"),
            (20, "0"),
            (62, "5"),
            (6, "DASHED"),
            (370, "50"),
            // Green.
            (0, "INSERT"),
            (8, "FURN"),
            (2, "CHAIR"),
            (10, "1000"),
            (20, "0"),
            (62, "3"),
            // By layer, twice: the block as drawn.
            (0, "INSERT"),
            (8, "FURN"),
            (2, "CHAIR"),
            (10, "2000"),
            (20, "0"),
            (0, "INSERT"),
            (8, "FURN"),
            (2, "CHAIR"),
            (10, "3000"),
            (20, "0"),
        ],
    );
    let back = read(&text, 1.0).unwrap();
    assert_eq!(back.inserts.len(), 4, "all kept as copies");
    let block_of = |k: usize| {
        back.blocks
            .iter()
            .find(|b| b.name == back.inserts[k].block)
            .unwrap()
    };
    let outline = |k: usize| {
        block_of(k)
            .entities
            .iter()
            .find(|e| matches!(e.shape, Shape::Line { .. }))
            .unwrap()
            .clone()
    };
    assert_eq!(outline(0).color, Some(Rgb([0, 0, 255])));
    assert_eq!(outline(0).line_type, Some(LineType::Dashed));
    assert!(
        outline(0)
            .weight
            .is_some_and(|w| (w.mm() - 0.5).abs() < 1e-9)
    );
    assert_eq!(outline(1).color, Some(Rgb([0, 255, 0])));
    assert_eq!((outline(2).color, outline(2).line_type), (None, None));
    // The two by-layer copies share one block; every block shows as CHAIR.
    assert_eq!(back.inserts[2].block, back.inserts[3].block);
    assert_eq!(back.blocks.len(), 3);
    assert!(back.blocks.iter().all(|b| b.label == "CHAIR"));
    // The seat mark keeps its own red in every look.
    for k in 0..3 {
        assert!(
            block_of(k)
                .entities
                .iter()
                .any(|e| matches!(e.shape, Shape::Circle { .. })
                    && e.color == Some(Rgb([255, 0, 0])))
        );
    }
    // In the drawing: three blocks of their own, named CHAIR, CHAIR (2) and CHAIR (3).
    let mut doc = Document::new();
    import(&mut doc, &back);
    let mut names: Vec<String> = doc.blocks().map(|(_, b)| b.name.clone()).collect();
    names.sort();
    assert_eq!(names, ["CHAIR", "CHAIR (2)", "CHAIR (3)"]);
    assert_eq!(doc.instances().count(), 4);
}

#[test]
fn a_second_file_with_a_different_block_of_the_same_name_keeps_its_own() {
    let one = file(DOOR, &[(0, "INSERT"), (2, "DOOR"), (10, "0"), (20, "0")]);
    // The same name, a narrower door.
    let narrow: Vec<(i32, &str)> = DOOR
        .iter()
        .map(|&(c, v)| match (c, v) {
            (21, "900") => (21, "700"),
            (40, "900") => (40, "700"),
            other => other,
        })
        .collect();
    let two = file(
        &narrow,
        &[(0, "INSERT"), (2, "DOOR"), (10, "5000"), (20, "0")],
    );
    let mut doc = Document::new();
    import(&mut doc, &read(&one, 1.0).unwrap());
    // The same file again: its door is the same block, used again.
    import(&mut doc, &read(&one, 1.0).unwrap());
    assert_eq!(doc.blocks().count(), 1);
    assert_eq!(doc.instances().count(), 2);
    // A different door by the same name: a block of its own, and its copy places it.
    import(&mut doc, &read(&two, 1.0).unwrap());
    let mut names: Vec<String> = doc.blocks().map(|(_, b)| b.name.clone()).collect();
    names.sort();
    assert_eq!(names, ["DOOR", "DOOR (2)"]);
    let narrow_copy = doc
        .instances()
        .find(|(_, i)| i.at == DVec2::new(5000.0, 0.0))
        .map(|(id, _)| id)
        .unwrap();
    let leaf_top = doc
        .instance_items(narrow_copy)
        .iter()
        .filter_map(|item| match item.shape {
            Shape::Line { a, b } => Some(a.y.max(b.y)),
            _ => None,
        })
        .fold(0.0, f64::max);
    assert!(
        (leaf_top - 700.0).abs() < 1e-6,
        "the narrow door: {leaf_top}"
    );
}

#[test]
fn dynamic_block_copies_are_named_after_the_dynamic_block() {
    // As AutoCAD writes one: the copy *U2's block record points (AcDbBlockRepBTag, 1005)
    // at the record of the dynamic block "Door - Single".
    let text = dxf(&[
        (0, "SECTION"),
        (2, "TABLES"),
        (0, "TABLE"),
        (2, "BLOCK_RECORD"),
        (0, "BLOCK_RECORD"),
        (5, "251"),
        (2, "Door - Single"),
        (0, "BLOCK_RECORD"),
        (5, "274"),
        (2, "*U2"),
        (1001, "AcDbBlockRepBTag"),
        (1070, "1"),
        (1005, "251"),
        (0, "ENDTAB"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "BLOCKS"),
        (0, "BLOCK"),
        (2, "Door - Single"),
        (70, "0"),
        (10, "0"),
        (20, "0"),
        (0, "LINE"),
        (10, "0"),
        (20, "0"),
        (11, "0"),
        (21, "900"),
        (0, "ENDBLK"),
        (0, "BLOCK"),
        (2, "*U2"),
        (70, "1"),
        (10, "0"),
        (20, "0"),
        (0, "LINE"),
        (10, "0"),
        (20, "0"),
        (11, "0"),
        (21, "800"),
        (0, "ENDBLK"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "ENTITIES"),
        (0, "INSERT"),
        (2, "*U2"),
        (10, "0"),
        (20, "0"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ]);
    let back = read(&text, 1.0).unwrap();
    assert_eq!(back.blocks.len(), 1);
    assert_eq!(back.blocks[0].label, "Door - Single");
    let mut doc = Document::new();
    import(&mut doc, &back);
    let names: Vec<String> = doc.blocks().map(|(_, b)| b.name.clone()).collect();
    assert_eq!(names, ["Door - Single"]);
}

#[test]
fn an_external_reference_is_named_in_a_note() {
    let text = file(
        &[
            (0, "BLOCK"),
            (2, "SITE"),
            (70, "4"),
            (10, "0"),
            (20, "0"),
            (1, "..\\survey\\site.dwg"),
            (0, "ENDBLK"),
        ],
        &[
            (0, "INSERT"),
            (2, "SITE"),
            (10, "0"),
            (20, "0"),
            (0, "LINE"),
            (10, "0"),
            (20, "0"),
            (11, "10"),
            (21, "0"),
        ],
    );
    let back = read(&text, 1.0).unwrap();
    assert!(
        back.inserts.is_empty() && back.blocks.is_empty(),
        "no empty block"
    );
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
    assert!(
        back.notes
            .iter()
            .any(|n| n.contains("SITE (..\\survey\\site.dwg)") && n.contains("Bind")),
        "{:?}",
        back.notes
    );
    assert_eq!(back.entities.len(), 1, "the rest still comes in");
}

/// A DWG saved by acadrust (which leaves the Y scale out when it is 1, as AutoCAD does) with
/// a door block placed mirrored.
#[cfg(feature = "dwg")]
#[test]
fn a_mirrored_door_in_a_dwg_is_mirrored() {
    use acadrust::entities::{Arc, EntityType, Insert, Line};
    use acadrust::types::{DxfVersion, Vector3};
    use acadrust::{BlockRecord, CadDocument, DwgWriter, TableEntry};

    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut record = BlockRecord::new("DOOR");
    let handle = doc.allocate_handle();
    record.set_handle(handle);
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    doc.block_records.add(record).unwrap();
    let mut leaf = EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 0.0, 900.0, 0.0));
    leaf.common_mut().owner_handle = handle;
    doc.add_entity(leaf).unwrap();
    let mut swing = EntityType::Arc(Arc::from_coords(
        0.0,
        0.0,
        0.0,
        900.0,
        0.0,
        std::f64::consts::FRAC_PI_2,
    ));
    swing.common_mut().owner_handle = handle;
    doc.add_entity(swing).unwrap();
    let door = Insert::new("DOOR", Vector3::new(1000.0, 0.0, 0.0)).with_scale(-1.0, 1.0, 1.0);
    doc.add_entity(EntityType::Insert(door)).unwrap();
    let bytes = DwgWriter::write_to_vec(&doc).unwrap();

    let back = drift_io::dwg_import::read(&bytes, 1.0).unwrap();
    assert_eq!(back.inserts.len(), 1, "{:?}", back.skipped);
    assert!(back.inserts[0].mirror, "mirrored");
    assert!(
        back.inserts[0].angle.abs() < 1e-9,
        "not turned: {}",
        back.inserts[0].angle
    );
}
