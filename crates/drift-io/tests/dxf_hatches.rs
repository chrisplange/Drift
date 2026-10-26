//! Hatches and solid fills from DXF and DWG come in as AutoCAD draws them:
//! - patterns at their real size, from the file's units (not Drift's drawing scale);
//! - the pattern each HATCH defines for itself, so AutoCAD's and the office's own patterns
//!   come in exactly (Drift's own pattern where it is the same one);
//! - hatches inside blocks, as the block's fills;
//! - SOLID and TRACE as solid fills;
//! - solids opaque, or as transparent as the file says; a gradient in its first colour;
//! - the pattern starting where the file's does.

use drift_core::{DVec2, Document, FillKind, Hatch, Rgb};
use drift_draft::{PatLine, parse_pat};
use drift_io::dxf::{DxfImport, add_to_document, read};

/// A DXF file from group codes and values.
fn dxf(pairs: &[(i32, String)]) -> String {
    pairs
        .iter()
        .map(|(code, value)| format!("{code:>3}\n{value}\n"))
        .collect()
}

fn g(code: i32, v: impl ToString) -> (i32, String) {
    (code, v.to_string())
}

/// One family of a pattern, as a .pat file gives it: angle (degrees), a point it passes
/// through, its step (along, across) and dashes, at scale 1.
struct Family {
    angle: f64,
    base: DVec2,
    step: DVec2,
    dashes: Vec<f64>,
}

fn family(angle: f64, base: (f64, f64), step: (f64, f64), dashes: &[f64]) -> Family {
    Family {
        angle,
        base: DVec2::new(base.0, base.1),
        step: DVec2::new(step.0, step.1),
        dashes: dashes.to_vec(),
    }
}

/// A HATCH over the rectangle `min`–`max`, its pattern written as AutoCAD writes it: each
/// family scaled by `scale` and turned by `angle`, the step in drawing coordinates, the
/// pattern starting at `origin`.
#[allow(clippy::too_many_arguments)]
fn hatch(
    pattern: &str,
    scale: f64,
    angle: f64,
    origin: DVec2,
    families: &[Family],
    min: (f64, f64),
    max: (f64, f64),
    extra: &[(i32, String)],
) -> Vec<(i32, String)> {
    let mut out = vec![g(0, "HATCH"), g(8, "HATCH"), g(2, pattern)];
    out.extend_from_slice(extra);
    out.extend([
        g(70, if pattern == "SOLID" { 1 } else { 0 }),
        g(91, 1),
        g(92, 3),
        g(72, 0),
        g(73, 1),
        g(93, 4),
    ]);
    for (x, y) in [min, (max.0, min.1), max, (min.0, max.1)] {
        out.extend([g(10, x), g(20, y)]);
    }
    out.extend([g(97, 0), g(75, 0), g(76, 1)]);
    if pattern != "SOLID" {
        out.extend([g(52, angle), g(41, scale), g(77, 0), g(78, families.len())]);
    }
    let turn = DVec2::from_angle(angle.to_radians());
    for f in families {
        let base = origin + turn.rotate(f.base * scale);
        let step = DVec2::from_angle((f.angle + angle).to_radians()).rotate(f.step * scale);
        out.extend([
            g(53, f.angle + angle),
            g(43, base.x),
            g(44, base.y),
            g(45, step.x),
            g(46, step.y),
            g(79, f.dashes.len()),
        ]);
        for d in &f.dashes {
            out.push(g(49, d * scale));
        }
    }
    out.push(g(98, 0));
    out
}

/// A file in `insunits` (and `measurement`, when given) with these entities.
fn file(insunits: i32, measurement: Option<i32>, entities: &[(i32, String)]) -> String {
    let mut all = vec![
        g(0, "SECTION"),
        g(2, "HEADER"),
        g(9, "$INSUNITS"),
        g(70, insunits),
    ];
    if let Some(m) = measurement {
        all.extend([g(9, "$MEASUREMENT"), g(70, m)]);
    }
    all.extend([g(0, "ENDSEC"), g(0, "SECTION"), g(2, "ENTITIES")]);
    all.extend_from_slice(entities);
    all.extend([g(0, "ENDSEC"), g(0, "EOF")]);
    dxf(&all)
}

fn imported(import: &DxfImport) -> Document {
    let mut doc = Document::new();
    doc.begin();
    add_to_document(&mut doc, import);
    doc.commit("Import");
    doc
}

/// The drawing's hatches' patterns.
fn hatches(doc: &Document) -> Vec<Hatch> {
    doc.fills()
        .filter_map(|(_, f)| match &f.kind {
            FillKind::Hatch(h) => Some(h.clone()),
            FillKind::Solid => None,
        })
        .collect()
}

fn ansi31() -> Vec<Family> {
    // acad.pat (inches).
    vec![family(45.0, (0.0, 0.0), (0.0, 0.125), &[])]
}

#[test]
fn ansi31_in_an_inch_drawing_has_its_real_spacing() {
    // ANSI31 at 16 in an inch drawing: lines 2" (50.8 mm) apart, whatever Drift's own
    // drawing scale is.
    let text = file(
        1,
        None,
        &hatch(
            "ANSI31",
            16.0,
            0.0,
            DVec2::ZERO,
            &ansi31(),
            (0.0, 0.0),
            (100.0, 60.0),
            &[],
        ),
    );
    let doc = imported(&read(&text, 1.0).unwrap());
    let h = &hatches(&doc)[0];
    assert_eq!(h.pattern, "ANSI31", "Drift's own ANSI31");
    assert!(h.model, "at real size");
    // Drift's ANSI31 is 3.175 mm apart at scale 1.
    let spacing = drift_draft::find("ANSI31").unwrap().min_spacing() * h.scale;
    assert!((spacing - 50.8).abs() < 1e-6, "{spacing}");
    assert!(doc.custom_patterns().is_empty(), "nothing added");
}

#[test]
fn a_pattern_drift_lacks_comes_in_as_the_file_defines_it() {
    // AR-BRSTD (acadiso.pat, millimetres): bricks 203 × 67.7.
    let bricks = vec![
        family(0.0, (0.0, 0.0), (0.0, 67.7), &[]),
        family(90.0, (0.0, 0.0), (67.7, 101.6), &[67.7, -67.7]),
    ];
    let text = file(
        4,
        None,
        &hatch(
            "AR-BRSTD",
            0.5,
            30.0,
            DVec2::new(5.0, 7.0),
            &bricks,
            (0.0, 0.0),
            (1000.0, 600.0),
            &[],
        ),
    );
    let back = read(&text, 1.0).unwrap();
    assert!(
        back.notes.iter().all(|n| !n.contains("ANSI31")),
        "{:?}",
        back.notes
    );
    let doc = imported(&back);
    let h = &hatches(&doc)[0];
    assert_eq!((h.pattern.as_str(), h.model), ("AR-BRSTD", true));
    assert!((h.scale - 0.5).abs() < 1e-9 && (h.angle - 30.0).abs() < 1e-9);
    assert!(
        h.origin.distance(DVec2::new(5.0, 7.0)) < 1e-9,
        "starts where the file's does"
    );
    // The drawing has the pattern now, at scale 1 and angle 0.
    let library = parse_pat(doc.custom_patterns()).unwrap();
    let p = library.iter().find(|p| p.name == "AR-BRSTD").unwrap();
    assert!(p.model);
    let near = |a: &PatLine, f: &Family| {
        (a.angle - f.angle).abs() < 1e-6
            && a.origin.distance(f.base) < 1e-6
            && a.delta.distance(f.step) < 1e-6
            && a.dashes.len() == f.dashes.len()
            && a.dashes
                .iter()
                .zip(&f.dashes)
                .all(|(x, y)| (x - y).abs() < 1e-6)
    };
    assert!(
        p.lines.iter().zip(&bricks).all(|(a, f)| near(a, f)),
        "{:?}",
        p.lines
    );
}

#[test]
fn patterns_by_the_same_name_are_shared_when_the_same_and_kept_apart_when_not() {
    let a = vec![family(0.0, (0.0, 0.0), (0.0, 10.0), &[5.0, -5.0])];
    let b = vec![family(0.0, (0.0, 0.0), (0.0, 10.0), &[2.0, -8.0])];
    let mut entities = hatch(
        "OFFICE",
        1.0,
        0.0,
        DVec2::ZERO,
        &a,
        (0.0, 0.0),
        (100.0, 100.0),
        &[],
    );
    entities.extend(hatch(
        "OFFICE",
        2.0,
        0.0,
        DVec2::ZERO,
        &a,
        (200.0, 0.0),
        (300.0, 100.0),
        &[],
    ));
    entities.extend(hatch(
        "OFFICE",
        1.0,
        0.0,
        DVec2::ZERO,
        &b,
        (400.0, 0.0),
        (500.0, 100.0),
        &[],
    ));
    let doc = imported(&read(&file(4, None, &entities), 1.0).unwrap());
    let mut names: Vec<String> = hatches(&doc).into_iter().map(|h| h.pattern).collect();
    names.sort();
    assert_eq!(names, ["OFFICE", "OFFICE", "OFFICE (2)"]);
    let library = parse_pat(doc.custom_patterns()).unwrap();
    assert_eq!(library.len(), 2);
}

#[test]
fn a_named_but_undefined_pattern_uses_autocads_library_for_the_drawing() {
    // No definition in the file: ANSI31 from acad.pat (inches) in an imperial drawing, from
    // acadiso.pat (millimetres) in a metric one.
    let undefined = hatch(
        "ANSI31",
        2.0,
        0.0,
        DVec2::ZERO,
        &[],
        (0.0, 0.0),
        (100.0, 100.0),
        &[],
    );
    let imperial = imported(&read(&file(1, Some(0), &undefined), 1.0).unwrap());
    let metric = imported(&read(&file(4, Some(1), &undefined), 1.0).unwrap());
    let spacing =
        |doc: &Document| drift_draft::find("ANSI31").unwrap().min_spacing() * hatches(doc)[0].scale;
    // 0.125" × 2 = 6.35 mm; 3.175 mm × 2.
    assert!(
        (spacing(&imperial) - 6.35).abs() < 1e-6,
        "{}",
        spacing(&imperial)
    );
    assert!(
        (spacing(&metric) - 6.35).abs() < 1e-6,
        "{}",
        spacing(&metric)
    );
    // A pattern neither defined nor Drift's: ANSI31 in its place, and a note.
    let unknown = hatch(
        "MYSTERY",
        1.0,
        0.0,
        DVec2::ZERO,
        &[],
        (0.0, 0.0),
        (100.0, 100.0),
        &[],
    );
    let back = read(&file(4, None, &unknown), 1.0).unwrap();
    let mut doc = Document::new();
    doc.begin();
    let report = add_to_document(&mut doc, &back);
    doc.commit("Import");
    assert_eq!(hatches(&doc)[0].pattern, "ANSI31");
    assert!(
        report.notes.iter().any(|n| n.contains("MYSTERY")),
        "{:?}",
        report.notes
    );
}

#[test]
fn a_turned_hatch_of_drifts_own_pattern_is_turned() {
    let text = file(
        1,
        None,
        &hatch(
            "ANSI31",
            1.0,
            30.0,
            DVec2::ZERO,
            &ansi31(),
            (0.0, 0.0),
            (100.0, 100.0),
            &[],
        ),
    );
    let doc = imported(&read(&text, 1.0).unwrap());
    let h = &hatches(&doc)[0];
    assert_eq!(h.pattern, "ANSI31");
    assert!((h.angle - 30.0).abs() < 1e-6 && (h.scale - 1.0).abs() < 1e-9);
}

#[test]
fn solids_are_filled_opaque_or_as_transparent_as_the_file_says() {
    let mut entities = vec![
        // A SOLID: corners 1, 2, 4, 3.
        g(0, "SOLID"),
        g(8, "POCHE"),
        g(62, 1),
        g(10, 0),
        g(20, 0),
        g(11, 100),
        g(21, 0),
        g(12, 0),
        g(22, 50),
        g(13, 100),
        g(23, 50),
        // A SOLID with no area (all on one line): its edges.
        g(0, "SOLID"),
        g(8, "POCHE"),
        g(10, 0),
        g(20, 200),
        g(11, 50),
        g(21, 200),
        g(12, 100),
        g(22, 200),
    ];
    // A solid hatch half transparent (alpha 127), and one on a layer that is.
    entities.extend(hatch(
        "SOLID",
        1.0,
        0.0,
        DVec2::ZERO,
        &[],
        (300.0, 0.0),
        (400.0, 100.0),
        &[g(440, 0x0200_007F)],
    ));
    let mut on_layer = hatch(
        "SOLID",
        1.0,
        0.0,
        DVec2::ZERO,
        &[],
        (500.0, 0.0),
        (600.0, 100.0),
        &[],
    );
    on_layer[1] = g(8, "TINT");
    entities.extend(on_layer);
    // A gradient, red to blue.
    entities.extend(hatch(
        "SOLID",
        1.0,
        0.0,
        DVec2::ZERO,
        &[],
        (700.0, 0.0),
        (800.0, 100.0),
        &[
            g(450, 1),
            g(453, 2),
            g(463, 0),
            g(63, 1),
            g(421, 0xFF0000),
            g(463, 1),
            g(63, 5),
            g(421, 0x0000FF),
        ],
    ));
    let mut all = vec![
        g(0, "SECTION"),
        g(2, "TABLES"),
        g(0, "TABLE"),
        g(2, "LAYER"),
        g(0, "LAYER"),
        g(2, "TINT"),
        g(70, 0),
        g(62, 3),
        g(1001, "AcCmTransparency"),
        g(1071, 0x0200_0033),
        g(0, "ENDTAB"),
        g(0, "ENDSEC"),
        g(0, "SECTION"),
        g(2, "ENTITIES"),
    ];
    all.extend(entities);
    all.extend([g(0, "ENDSEC"), g(0, "EOF")]);
    let back = read(&dxf(&all), 1.0).unwrap();
    // The flat SOLID is its edges; the real one has no outline of its own.
    assert_eq!(back.entities.len(), 3, "{:?}", back.entities);
    assert!(back.entities.iter().all(|e| e.shape.bounds().min.y > 150.0));
    assert_eq!(back.hatches.len(), 4);
    let doc = imported(&back);
    let solids: Vec<drift_core::Fill> = doc
        .fills()
        .map(|(_, f)| f.clone())
        .filter(|f| f.kind == FillKind::Solid)
        .collect();
    assert_eq!(solids.len(), 4);
    let at = |x: f64| {
        solids
            .iter()
            .find(|f| f.seed.x > x && f.seed.x < x + 100.0)
            .unwrap()
    };
    assert_eq!(at(0.0).color, Some(Rgb([255, 0, 0])));
    assert!((at(0.0).opacity - 1.0).abs() < 1e-9, "opaque");
    assert!((at(300.0).opacity - 127.0 / 255.0).abs() < 1e-9);
    assert!(
        (at(500.0).opacity - 0x33 as f64 / 255.0).abs() < 1e-9,
        "the layer's"
    );
    assert_eq!(
        at(700.0).color,
        Some(Rgb([255, 0, 0])),
        "the gradient's first colour"
    );
}

#[test]
fn hatches_inside_blocks_are_the_blocks_fills() {
    // A table: a square top hatched with ANSI31, a solid round leg mark, placed twice.
    let mut block = vec![
        g(0, "SECTION"),
        g(2, "BLOCKS"),
        g(0, "BLOCK"),
        g(2, "TABLE"),
        g(10, 0),
        g(20, 0),
        g(0, "LWPOLYLINE"),
        g(8, "0"),
        g(90, 4),
        g(70, 1),
    ];
    for (x, y) in [(0, 0), (800, 0), (800, 800), (0, 800)] {
        block.extend([g(10, x), g(20, y)]);
    }
    let mut top = hatch(
        "ANSI31",
        4.0,
        0.0,
        DVec2::ZERO,
        &ansi31(),
        (0.0, 0.0),
        (800.0, 800.0),
        &[],
    );
    top[1] = g(8, "0");
    block.extend(top);
    // A SOLID inside it too.
    block.extend([
        g(0, "SOLID"),
        g(8, "0"),
        g(10, 350),
        g(20, 350),
        g(11, 450),
        g(21, 350),
        g(12, 350),
        g(22, 450),
        g(13, 450),
        g(23, 450),
    ]);
    block.extend([g(0, "ENDBLK"), g(0, "ENDSEC")]);
    let mut all = vec![
        g(0, "SECTION"),
        g(2, "HEADER"),
        g(9, "$INSUNITS"),
        g(70, 1),
        g(0, "ENDSEC"),
    ];
    all.extend(block);
    all.extend([g(0, "SECTION"), g(2, "ENTITIES")]);
    for x in [0, 2000] {
        all.extend([
            g(0, "INSERT"),
            g(8, "FURN"),
            g(2, "TABLE"),
            g(10, x),
            g(20, 0),
            g(50, 90),
        ]);
    }
    all.extend([g(0, "ENDSEC"), g(0, "EOF")]);
    let back = read(&dxf(&all), 1.0).unwrap();
    assert!(
        back.notes.iter().all(|n| !n.contains("left out")),
        "{:?}",
        back.notes
    );
    assert_eq!(back.blocks.len(), 1);
    assert_eq!(back.blocks[0].hatches.len(), 2);
    let doc = imported(&back);
    assert_eq!(
        doc.fills().count(),
        0,
        "the fills are the block's, not loose"
    );
    let (_, block) = doc.blocks().next().unwrap();
    assert_eq!(block.fills.len(), 2);
    let hatch = block
        .fills
        .iter()
        .find_map(|f| match &f.kind {
            FillKind::Hatch(h) => Some((f.seed, h.clone())),
            FillKind::Solid => None,
        })
        .unwrap();
    // In the block's own coordinates (inches → mm): inside the top, outside the leg mark.
    let (seed, h) = hatch;
    assert!(seed.x > 0.0 && seed.x < 800.0 * 25.4 && seed.y > 0.0 && seed.y < 800.0 * 25.4);
    assert_eq!((h.pattern.as_str(), h.model), ("ANSI31", true));
    assert!((h.scale - 4.0).abs() < 1e-9);
    // The leg mark's square is among the block's curves now, to bound its fill.
    assert!(block.items.len() >= 8, "{}", block.items.len());
    assert_eq!(doc.instances().count(), 2);
}

#[test]
fn a_hatch_in_a_mirrored_copy_that_is_expanded_is_mirrored() {
    // A block placed mirrored in y (expanded into curves): its hatch comes along, its
    // pattern turned the mirrored way.
    let text = {
        let mut all = vec![
            g(0, "SECTION"),
            g(2, "BLOCKS"),
            g(0, "BLOCK"),
            g(2, "B"),
            g(10, 0),
            g(20, 0),
        ];
        let mut h = hatch(
            "ANSI31",
            1.0,
            0.0,
            DVec2::ZERO,
            &ansi31(),
            (0.0, 0.0),
            (10.0, 10.0),
            &[],
        );
        h[1] = g(8, "0");
        all.extend(h);
        all.extend([
            g(0, "ENDBLK"),
            g(0, "ENDSEC"),
            g(0, "SECTION"),
            g(2, "ENTITIES"),
        ]);
        all.extend([g(0, "INSERT"), g(2, "B"), g(10, 0), g(20, 0), g(42, -1)]);
        all.extend([g(0, "ENDSEC"), g(0, "EOF")]);
        dxf(&all)
    };
    let back = read(&text, 1.0).unwrap();
    let h = &back.hatches[0];
    // Mirrored in y, a 45° family runs at −45°.
    let world = (h.angle + h.definition[0].angle).rem_euclid(360.0);
    assert!(
        (world - 315.0).abs() < 1e-6 || (world - 135.0).abs() < 1e-6,
        "{world}"
    );
}
