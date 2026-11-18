use serde_json::{Value, json};

use super::*;

/// Call a tool that must succeed.
fn ok(s: &mut Session, name: &str, args: Value) -> Value {
    let out = s.call(name, &args);
    assert!(!out.is_error, "{name} failed: {}", out.json);
    out.json
}

fn err(s: &mut Session, name: &str, args: Value) -> String {
    let out = s.call(name, &args);
    assert!(out.is_error, "{name} should have failed: {}", out.json);
    out.json["error"].as_str().unwrap_or_default().to_owned()
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> &'a str {
    let mut v = v;
    for k in path {
        v = &v[*k];
    }
    v.as_str().unwrap_or_else(|| panic!("no {path:?} in {v}"))
}

/// A rough 3600 × 3000 room, as a model might sketch it.
fn rough_room(s: &mut Session) -> Value {
    ok(
        s,
        "add_geometry",
        json!({ "items": [{ "type": "rectangle", "corner": [0, 0], "width": 3550, "height": 3080 }] }),
    )
}

#[test]
fn a_room_is_drawn_constrained_and_measured() {
    let mut s = Session::new();
    let made = rough_room(&mut s);
    let room = &made["created"][0];
    assert_eq!(made["relations_added"], 4, "{made}");
    let bottom = str_at(room, &["sides", "bottom"]).to_owned();
    let left = str_at(room, &["sides", "left"]).to_owned();
    let corner = str_at(room, &["corners", "bottom_left"]).to_owned();
    let v = ok(
        &mut s,
        "add_dimensions",
        json!({ "dimensions": [
            { "type": "length", "line": bottom, "value": 3600 },
            { "type": "length", "line": left, "value": "3 m" }
        ]}),
    );
    assert_eq!(v["added"].as_array().map(Vec::len), Some(2), "{v}");
    ok(
        &mut s,
        "add_relations",
        json!({ "relations": [{ "type": "fix", "point": corner, "at": [0, 0] }] }),
    );
    let a = ok(&mut s, "analyze", json!({}));
    assert_eq!(a["dof"], 0, "{a}");
    assert_eq!(a["fully_defined"], true);
    let regions = a["regions"].as_array().expect("regions");
    assert_eq!(regions.len(), 1, "{a}");
    let area = regions[0]["area_m2"].as_f64().expect("area");
    assert!((area - 10.8).abs() < 1e-6, "area {area}");
    assert!(a["open_ends"].as_array().is_some_and(Vec::is_empty));
    // Each change was one step: undo the fix, the dimensions, the rectangle.
    assert_eq!(s.editor.history.undo_depth(), 3);
    let m = ok(&mut s, "measure", json!({ "a": bottom }));
    assert!(
        (m["length"]["mm"].as_f64().unwrap() - 3600.0).abs() < 1e-6,
        "{m}"
    );
    ok(
        &mut s,
        "add_fill",
        json!({ "at": [1800, 1500], "kind": "hatch", "pattern": "ANSI31", "scale": 20 }),
    );
    ok(
        &mut s,
        "add_text",
        json!({ "text": "LIVING", "at": [1800, 1500], "align": "center", "height": 150 }),
    );
    let out = s.call("render", &json!({ "width": 800 }));
    save_png("agent_render_room_defined.png", out.image.as_deref());
}

/// Keep a render to look at (in target/tmp).
fn save_png(name: &str, png: Option<&[u8]>) {
    let tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
    let _ = std::fs::create_dir_all(&tmp);
    if let Some(png) = png {
        let _ = std::fs::write(tmp.join(name), png);
    }
}

#[test]
fn a_failed_change_leaves_the_drawing_as_it_was() {
    let mut s = Session::new();
    let made = rough_room(&mut s);
    let bottom = str_at(&made["created"][0], &["sides", "bottom"]).to_owned();
    let top = str_at(&made["created"][0], &["sides", "top"]).to_owned();
    let before = s.editor.doc.clone();
    let depth = s.editor.history.undo_depth();
    // The second dimension conflicts with the first (opposite sides of a rectangle).
    let e = err(
        &mut s,
        "add_dimensions",
        json!({ "dimensions": [
            { "type": "length", "line": bottom, "value": 3600 },
            { "type": "length", "line": top, "value": 4000 }
        ]}),
    );
    assert!(e.contains("dimensions[1]"), "{e}");
    assert_eq!(s.editor.history.undo_depth(), depth);
    assert_eq!(s.editor.doc.constraint_count(), before.constraint_count());
    let a = ok(&mut s, "analyze", json!({}));
    assert!(a["conflicts"].as_array().is_some_and(Vec::is_empty));
}

#[test]
fn relations_and_variables_drive_the_sketch() {
    let mut s = Session::new();
    // Two lines, roughly perpendicular and sharing a point.
    let made = ok(
        &mut s,
        "add_geometry",
        json!({ "auto_relations": false, "items": [
            { "type": "line", "from": [0, 0], "to": [1000, 30] },
            { "type": "line", "from": [0, 0], "to": [-20, 800] }
        ]}),
    );
    let a = str_at(&made["created"][0], &["id"]).to_owned();
    let b = str_at(&made["created"][1], &["id"]).to_owned();
    assert_eq!(
        made["created"][0]["a"], made["created"][1]["a"],
        "lines share their start"
    );
    ok(
        &mut s,
        "add_relations",
        json!({ "relations": [{ "type": "horizontal", "line": a }, { "type": "perpendicular", "a": a, "b": b }] }),
    );
    ok(
        &mut s,
        "set_variables",
        json!({ "set": [{ "name": "W", "value": 2400 }] }),
    );
    let d = ok(
        &mut s,
        "add_dimensions",
        json!({ "dimensions": [{ "type": "length", "line": a, "value": "W" }, { "type": "length", "line": b, "value": "W/2" }] }),
    );
    assert_eq!(d["added"][0]["expression"], "W", "{d}");
    ok(
        &mut s,
        "set_variables",
        json!({ "set": [{ "name": "W", "value": "3 m" }] }),
    );
    let m = ok(&mut s, "measure", json!({ "a": b }));
    assert!(
        (m["length"]["mm"].as_f64().unwrap() - 1500.0).abs() < 1e-6,
        "{m}"
    );
    let m = ok(&mut s, "measure", json!({ "a": a, "b": b }));
    assert!((m["angle"].as_f64().unwrap() - 90.0).abs() < 1e-6, "{m}");
    // A relation that is already true is skipped, not refused.
    let r = ok(
        &mut s,
        "add_relations",
        json!({ "relations": [{ "type": "horizontal", "line": a }] }),
    );
    assert_eq!(r["skipped"].as_array().map(Vec::len), Some(1), "{r}");
}

#[test]
fn render_returns_a_png_with_ink_where_the_drawing_is() {
    let mut s = Session::new();
    rough_room(&mut s);
    let out = s.call("render", &json!({ "width": 400 }));
    assert!(!out.is_error, "{}", out.json);
    let png = out.image.expect("an image");
    save_png("agent_render_room.png", Some(&png));
    assert_eq!(&png[1..4], b"PNG");
    let img = tiny_skia::Pixmap::decode_png(&png).expect("decodes");
    assert_eq!(img.width().max(img.height()), 400);
    let px = |x: u32, y: u32| img.pixel(x, y).expect("in the picture");
    // The room's outline is drawn (blue: it can still move) and its middle is blank.
    let mpp = out.json["mm_per_pixel"].as_f64().unwrap();
    let r = &out.json["region"];
    let to_px = |x: f64, y: f64| {
        (
            ((x - r[0].as_f64().unwrap()) / mpp) as u32,
            ((r[3].as_f64().unwrap() - y) / mpp) as u32,
        )
    };
    let (cx, cy) = to_px(1775.0, 1540.0);
    let mid = px(cx, cy);
    assert!(mid.red() > 240 && mid.green() > 240, "middle is paper");
    let (lx, ly) = to_px(0.0, 1000.0);
    let ink = (lx.saturating_sub(2)..=lx + 2).any(|x| {
        let p = px(x, ly);
        i32::from(p.blue()) > i32::from(p.red()) + 60
    });
    assert!(ink, "the left wall is drawn in blue");
}

#[test]
fn modify_tools_work_through_the_session() {
    let mut s = Session::new();
    let made = rough_room(&mut s);
    let room = &made["created"][0];
    let right = str_at(room, &["sides", "right"]).to_owned();
    let corner = str_at(room, &["corners", "top_right"]).to_owned();
    ok(
        &mut s,
        "fillet_chamfer",
        json!({ "corner": corner, "radius": 300 }),
    );
    let arcs = ok(&mut s, "list_entities", json!({ "kind": "arc" }));
    assert_eq!(arcs["count"], 1, "{arcs}");
    let off = ok(
        &mut s,
        "offset",
        json!({ "id": right, "distance": 200, "side": [5000, 1500] }),
    );
    assert!(
        off["created"].as_array().is_some_and(|c| c.len() >= 4),
        "{off}"
    );
    let a = ok(&mut s, "analyze", json!({}));
    assert_eq!(
        a["regions"].as_array().map(Vec::len),
        Some(2),
        "a wall: the room and its ring"
    );
    ok(&mut s, "undo", json!({ "steps": 2 }));
    let arcs = ok(&mut s, "list_entities", json!({ "kind": "arc" }));
    assert_eq!(arcs["count"], 0);
    ok(&mut s, "redo", json!({}));
    let arcs = ok(&mut s, "list_entities", json!({ "kind": "arc" }));
    assert_eq!(arcs["count"], 1);
}

/// Assigning a filled shape's outline to a layer takes its fill along (#266).
#[test]
fn layers_assign_takes_a_shapes_fill() {
    let mut s = Session::new();
    // A filled rectangle, as the drawing tools make one.
    let ed = &mut s.editor;
    ed.prefs.fill_new_shapes = true;
    ed.doc.begin();
    let corners = [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)];
    let p: Vec<_> = corners
        .iter()
        .map(|(x, y)| ed.doc.add_point(drift_core::DVec2::new(*x, *y)))
        .collect();
    let sides: Vec<_> = (0..4)
        .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    let fill = ed.fill_new_shape(&sides).expect("a shape's fill");
    ed.commit("Rectangle");
    let ids: Vec<Value> = sides.iter().map(|c| json!(c.to_string())).collect();
    let l = ok(&mut s, "layers", json!({ "create": [{ "name": "Walls" }] }));
    let walls = l["created"][0].as_str().unwrap().to_owned();
    ok(
        &mut s,
        "layers",
        json!({ "assign": { "layer": "Walls", "ids": ids } }),
    );
    let fill = s.editor.doc.fill(fill).unwrap();
    assert_eq!(fill.layer.to_string(), walls);
}

#[test]
fn layers_fills_and_text() {
    let mut s = Session::new();
    rough_room(&mut s);
    let l = ok(
        &mut s,
        "layers",
        json!({ "create": [
            { "name": "Walls", "color": "#aa0000", "weight_mm": 0.5 },
            { "name": "Fine", "weight_mm": 0.09 }
        ] }),
    );
    let walls = l["created"][0].as_str().unwrap().to_owned();
    let lines = ok(&mut s, "list_entities", json!({ "kind": "line" }));
    let ids: Vec<Value> = lines["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].clone())
        .collect();
    ok(
        &mut s,
        "layers",
        json!({ "assign": { "layer": "Walls", "ids": ids } }),
    );
    let lines = ok(&mut s, "list_entities", json!({ "layer": walls }));
    assert_eq!(lines["count"], 4);
    let f = ok(
        &mut s,
        "add_fill",
        json!({ "at": [1000, 1000], "kind": "hatch", "pattern": "ANSI31" }),
    );
    assert!(
        (f["area_mm2"].as_f64().unwrap() - 3550.0 * 3080.0).abs() < 1e-3,
        "{f}"
    );
    err(&mut s, "add_fill", json!({ "at": [9000, 9000] }));
    let t = ok(
        &mut s,
        "add_text",
        json!({ "text": "LIVING", "at": [1500, 1500], "height": 200 }),
    );
    assert!(t["id"].as_str().unwrap().starts_with('t'));
    let info = ok(&mut s, "drawing_info", json!({}));
    assert_eq!(info["fills"], 1);
    assert_eq!(info["texts"], 1);
    // A weight on AutoCAD's list stays as given, as from an imported file (#255).
    let fine = info["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "Fine")
        .unwrap();
    assert_eq!(fine["weight_mm"], 0.09);
}

#[test]
fn batch_is_one_step_and_all_or_nothing() {
    let mut s = Session::new();
    let v = ok(
        &mut s,
        "batch",
        json!({ "calls": [
            { "tool": "add_geometry", "args": { "items": [{ "type": "circle", "center": [0, 0], "radius": 500 }] } },
            { "tool": "add_geometry", "args": { "items": [{ "type": "circle", "center": [0, 0], "radius": 800 }] } }
        ]}),
    );
    assert_eq!(v["results"].as_array().map(Vec::len), Some(2));
    assert_eq!(s.editor.history.undo_depth(), 1);
    let e = err(
        &mut s,
        "batch",
        json!({ "calls": [
            { "tool": "add_geometry", "args": { "items": [{ "type": "line", "from": [0, 0], "to": [10, 10] }] } },
            { "tool": "delete", "args": { "ids": ["e999"] } }
        ]}),
    );
    assert!(e.contains("calls[1]"), "{e}");
    assert_eq!(s.editor.history.undo_depth(), 1);
    let lines = ok(&mut s, "list_entities", json!({ "kind": "line" }));
    assert_eq!(lines["count"], 0);
}

#[test]
fn saving_over_things_from_a_newer_drift_needs_anyway() {
    let dir = std::env::temp_dir().join(format!("drift-agent-newer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::new().with_root(dir.clone());
    // A newer Drift's file, with a field this build doesn't know on the point e1.
    let plain = drift_io::save(&drift_core::demo::mounting_plate(), &Default::default());
    let text = plain.replacen(
        r#"{"type":"point","id":"e1","#,
        r#"{"type":"point","id":"e1","weld":"fillet","#,
        1,
    );
    assert_ne!(text, plain);
    let path = dir.join("newer.drift");
    std::fs::write(&path, &text).unwrap();
    ok(&mut s, "open_drawing", json!({ "path": "newer.drift" }));
    // Kept through an edit elsewhere and a save.
    ok(&mut s, "save_drawing", json!({}));
    assert!(std::fs::read_to_string(&path).unwrap().contains("weld"));
    // Deleting the point would lose it: refused, the file untouched.
    ok(&mut s, "delete", json!({ "ids": ["e1"] }));
    let before = std::fs::read_to_string(&path).unwrap();
    let e = err(&mut s, "save_drawing", json!({}));
    assert!(e.contains("newer Drift") && e.contains("anyway"), "{e}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    ok(&mut s, "save_drawing", json!({ "anyway": true }));
    assert!(!std::fs::read_to_string(&path).unwrap().contains("weld"));
    // Nothing more to lose: saved without asking.
    ok(&mut s, "save_drawing", json!({}));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_round_trip_and_stay_in_the_root() {
    let dir = std::env::temp_dir().join(format!("drift-agent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::new().with_root(dir.clone());
    rough_room(&mut s);
    ok(&mut s, "save_drawing", json!({ "path": "room.drift" }));
    assert!(dir.join("room.drift").exists());
    let e = err(
        &mut s,
        "save_drawing",
        json!({ "path": "../outside.drift" }),
    );
    assert!(e.contains("outside"), "{e}");
    for (file, magic) in [
        ("room.pdf", &b"%PDF"[..]),
        ("room.svg", b"<?xm"),
        ("room.png", b"\x89PNG"),
    ] {
        ok(&mut s, "export", json!({ "path": file }));
        let bytes = std::fs::read(dir.join(file)).unwrap();
        assert!(bytes.starts_with(magic), "{file}");
    }
    ok(&mut s, "export", json!({ "path": "room.dxf" }));
    ok(&mut s, "export", json!({ "path": "room.dwg" }));
    ok(
        &mut s,
        "export",
        json!({ "path": "room-r12.dxf", "format": "dxf-r12" }),
    );
    assert!(
        std::fs::read(dir.join("room.dwg"))
            .unwrap()
            .starts_with(b"AC1032")
    );
    let text = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
    assert!(text("room.dxf").contains("AC1032") && text("room-r12.dxf").contains("AC1009"));
    let mut t = Session::new().with_root(dir.clone());
    let info = ok(&mut t, "open_drawing", json!({ "path": "room.drift" }));
    assert_eq!(info["entities"]["line"], 4, "{info}");
    // The SVG we exported comes back in as geometry.
    let imp = ok(
        &mut t,
        "import_file",
        json!({ "path": "room.svg", "scale": "1:1", "offset": [10000, 0] }),
    );
    assert!(imp["curves"].as_u64().unwrap() >= 4, "{imp}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #245: the export tool's `placement` for a PDF with no sheets: `fit` (the default, as before)
/// fits the drawing to an A3 sheet; `true-scale` prints it at 1 : 1 on a page of its size.
#[test]
fn a_pdf_with_no_sheets_is_placed_as_asked() {
    let dir = std::env::temp_dir().join(format!("drift-agent-245-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::new().with_root(dir.clone());
    ok(
        &mut s,
        "add_geometry",
        json!({ "items": [{ "type": "rectangle", "corner": [0, 0], "width": 120, "height": 80 }] }),
    );
    let read =
        |f: &str| crate::output::pdf_page_and_longest_line(&std::fs::read(dir.join(f)).unwrap());
    ok(
        &mut s,
        "export",
        json!({ "path": "plate.pdf", "placement": "true-scale" }),
    );
    let (page, longest) = read("plate.pdf");
    assert!(
        (longest - 120.0).abs() < 0.01,
        "the 120 mm side: {longest} mm"
    );
    assert!(
        (page.x - 130.0).abs() < 1.0 && (page.y - 90.0).abs() < 1.0,
        "{page}"
    );
    // Fit, by default or by name: today's A3 sheet, the same file either way.
    ok(&mut s, "export", json!({ "path": "default.pdf" }));
    ok(
        &mut s,
        "export",
        json!({ "path": "fit.pdf", "placement": "fit" }),
    );
    let (page, _) = read("default.pdf");
    assert!(
        (page.x - 420.0).abs() < 0.1 && (page.y - 297.0).abs() < 0.1,
        "A3: {page}"
    );
    let bytes = |f: &str| std::fs::read(dir.join(f)).unwrap();
    assert_eq!(bytes("default.pdf"), bytes("fit.pdf"));
    // A drawing set to 1:50: at 1:1 its dimension text is paper size, as a drawing set to 1:1
    // prints it, not 50 times that.
    let mut text_at = |n: u32| {
        ok(&mut s, "new_drawing", json!({ "scale": n }));
        let made = ok(
            &mut s,
            "add_geometry",
            json!({ "items": [{ "type": "rectangle", "corner": [0, 0], "width": 120, "height": 80 }] }),
        );
        let bottom = str_at(&made["created"][0], &["sides", "bottom"]).to_owned();
        ok(
            &mut s,
            "add_dimensions",
            json!({ "dimensions": [{ "type": "length", "line": bottom, "value": 120 }] }),
        );
        let file = format!("plate{n}.pdf");
        ok(
            &mut s,
            "export",
            json!({ "path": file, "placement": "true-scale" }),
        );
        let pdf = std::fs::read(dir.join(&file)).unwrap();
        let (_, longest) = crate::output::pdf_page_and_longest_line(&pdf);
        assert!((longest - 120.0).abs() < 0.01, "1:{n}: {longest}");
        let art = drift_io::pdf_import::read_artwork(&pdf, 1, "page").unwrap();
        let sizes: Vec<f64> = art.texts.iter().map(|t| t.size).collect();
        assert_eq!(sizes.len(), 1, "the dimension's text: {sizes:?}");
        (sizes[0], s.editor.doc.draft_style().text_height)
    };
    let (at_1, paper) = text_at(1);
    let (at_50, _) = text_at(50);
    assert!(
        (at_50 - at_1).abs() < 0.01,
        "1:50 prints its text as 1:1 does ({at_1} mm), not at {at_50} mm"
    );
    // (The reader gives the font's em, a little more than the style's cap height.)
    assert!(
        at_1 > paper && at_1 < 2.0 * paper,
        "{at_1} mm for {paper} mm text"
    );
    // Only a PDF takes it.
    let e = err(
        &mut s,
        "export",
        json!({ "path": "plate.svg", "placement": "true-scale" }),
    );
    assert!(e.contains("placement is for a PDF"), "{e}");
    let e = err(
        &mut s,
        "export",
        json!({ "path": "x.pdf", "placement": "1:1" }),
    );
    assert!(e.contains("fit or true-scale"), "{e}");
    let e = err(
        &mut s,
        "export",
        json!({ "path": "x.pdf", "placement": "huge" }),
    );
    assert!(e.contains("fit or true-scale"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drawings_rules_travel_with_it_and_the_assistants_picking_does_not() {
    let dir = std::env::temp_dir().join(format!("drift-agent-rules-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::new().with_root(dir.clone());
    rough_room(&mut s);
    // The assistant picks empty areas, but that isn't the drawing's rule.
    assert!(!s.editor.regions_need_fill());
    assert!(s.editor.doc.rules().regions_need_fill);
    s.editor.change_rules(|r| {
        r.stacked_fills = false;
        r.gap_tolerance = 2.0;
    });
    ok(&mut s, "save_drawing", json!({ "path": "rules.drift" }));
    let text = std::fs::read_to_string(dir.join("rules.drift")).unwrap();
    assert!(!text.contains("pick_filled_only"), "{text}");
    let mut t = Session::new().with_root(dir.clone());
    ok(&mut t, "open_drawing", json!({ "path": "rules.drift" }));
    let rules = t.editor.doc.rules();
    assert!(!rules.stacked_fills && rules.regions_need_fill);
    assert_eq!(rules.gap_tolerance, 2.0);
    assert!(!t.editor.regions_need_fill(), "still picks empty areas");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_drawings_and_older_files_get_the_rules_drift_gives_them() {
    let dir = std::env::temp_dir().join(format!("drift-agent-new-rules-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mine = drift_core::DrawingRules {
        regions_by_layer: true,
        gap_tolerance: 1.25,
        ..Default::default()
    };
    let mut s = Session::new().with_root(dir.clone()).with_new_rules(mine);
    ok(&mut s, "new_drawing", json!({}));
    assert_eq!(s.editor.doc.rules(), mine);
    // A file from before drawings kept their rules gets them too, and the notes say so.
    let plate = drift_core::demo::mounting_plate();
    let current = drift_io::save(&plate, &drift_io::Metadata::default());
    let older = current.replace(
        &format!(r#""version": {}"#, drift_io::FORMAT_VERSION),
        r#""version": 38"#,
    );
    assert_ne!(older, current);
    std::fs::write(dir.join("old.drift"), older).unwrap();
    let v = ok(&mut s, "open_drawing", json!({ "path": "old.drift" }));
    assert_eq!(s.editor.doc.rules(), mine);
    assert!(v["notes"].to_string().contains("save to keep them"), "{v}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_tool_has_an_object_schema_and_is_dispatched() {
    let mut s = Session::new();
    for t in tools() {
        assert_eq!(t.schema["type"], "object", "{}", t.name);
        let out = s.call(t.name, &json!({ "__probe": true }));
        let msg = out.json["error"].as_str().unwrap_or_default();
        assert!(
            !msg.starts_with("unknown tool"),
            "{} isn't dispatched",
            t.name
        );
    }
}

#[test]
fn a_walled_room_can_be_fully_defined() {
    let mut s = Session::new();
    ok(&mut s, "new_drawing", json!({ "scale": 50 }));
    let made = ok(
        &mut s,
        "add_geometry",
        json!({ "items": [{ "type": "rectangle", "corner": [0, 0], "width": 4100, "height": 3700 }] }),
    );
    let room = &made["created"][0];
    let bottom = str_at(room, &["sides", "bottom"]).to_owned();
    let left = str_at(room, &["sides", "left"]).to_owned();
    let corner = str_at(room, &["corners", "bottom_left"]).to_owned();
    let off = ok(
        &mut s,
        "offset",
        json!({ "id": bottom, "distance": 200, "side": [-1000, -1000] }),
    );
    assert_eq!(
        off["created"].as_array().map(Vec::len),
        Some(4),
        "the whole outline: {off}"
    );
    ok(
        &mut s,
        "add_dimensions",
        json!({ "dimensions": [
            { "type": "length", "line": bottom, "value": 4200 },
            { "type": "length", "line": left, "value": 3600 }
        ]}),
    );
    ok(
        &mut s,
        "add_relations",
        json!({ "relations": [{ "type": "fix", "point": corner, "at": [0, 0] }] }),
    );
    let a = ok(&mut s, "analyze", json!({}));
    assert_eq!(
        a["fully_defined"], true,
        "the offset holds the walls 200 off: {a}"
    );
    let areas: Vec<f64> = a["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["area_m2"].as_f64().unwrap())
        .collect();
    assert_eq!(areas.len(), 2, "the room and its walls");
    assert!(
        (areas[0] - 15.12).abs() < 1e-6 && (areas[1] - 3.28).abs() < 1e-6,
        "{areas:?}"
    );
    let out = s.call("render", &json!({ "width": 900 }));
    save_png("agent_render_walls.png", out.image.as_deref());
}

#[test]
fn a_batch_after_other_changes_is_its_own_undo_step() {
    // Tools that record several steps inside a batch once made it count them twice: undoing
    // the batch took the change before it too, and a failing batch undid it.
    let mut s = Session::new();
    rough_room(&mut s);
    let made = ok(
        &mut s,
        "batch",
        json!({ "calls": [
            { "tool": "add_geometry", "args": { "items": [{ "type": "line", "from": [0, 5000], "to": [3000, 5000] }] } },
            { "tool": "add_dimensions", "args": { "dimensions": [{ "type": "length", "line": "e11", "value": "3 m" }] } }
        ]}),
    );
    assert_eq!(made["sketch"]["undo_steps"], 2, "{made}");
    let before = s.editor.doc.clone();
    err(
        &mut s,
        "batch",
        json!({ "calls": [
            { "tool": "add_dimensions", "args": { "dimensions": [{ "type": "length", "line": "e5", "value": "3.6 m" }] } },
            { "tool": "delete", "args": { "ids": ["e999"] } }
        ]}),
    );
    assert_eq!(
        s.editor.doc, before,
        "a failed batch leaves the earlier steps alone"
    );
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.editor.history.undo_depth(), 1, "the room is still there");
    assert!(!s.editor.doc.is_empty());
}

#[test]
fn split_returns_both_pieces() {
    let mut s = Session::new();
    ok(
        &mut s,
        "add_geometry",
        json!({ "items": [{ "type": "line", "from": [0, 0], "to": [1000, 0] }] }),
    );
    let v = ok(
        &mut s,
        "trim_extend_split",
        json!({ "id": "e3", "at": [400, 0], "mode": "split" }),
    );
    // The second piece is the new line (not the point at the cut).
    let rest = v["pieces"][1].as_str().expect("two pieces").to_owned();
    let lines = ok(&mut s, "list_entities", json!({ "ids": [rest] }));
    assert_eq!(lines["entities"][0]["kind"], "line", "{v} {lines}");
    assert_eq!(lines["entities"][0]["from"], json!([400.0, 0.0]), "{lines}");
    assert!(v["point"].is_string(), "{v}");
}

#[test]
fn a_curve_ending_on_an_arc_closes_an_area() {
    // The lower half of a circle, its diameter, and a Bézier from the arc's end back onto
    // its interior at (60, -80): the lock between them and the half disc are both areas,
    // without splitting the arc by hand (#100).
    let mut s = Session::new();
    ok(
        &mut s,
        "add_geometry",
        json!({ "items": [
            { "type": "arc", "center": [0, 0], "start": [-100, 0], "end": [100, 0] },
            { "type": "line", "from": [-100, 0], "to": [100, 0] },
            { "type": "bezier", "from": [100, 0], "c1": [160, -20], "c2": [150, -70], "to": [60, -80] },
        ] }),
    );
    let lock = ok(&mut s, "measure", json!({ "inside": [120, -40] }));
    let area = lock["area_mm2"].as_f64().expect("area");
    assert!((area - 2715.9).abs() < 0.1, "{lock}");
    let disc = ok(&mut s, "measure", json!({ "inside": [0, -50] }));
    let area = disc["area_mm2"].as_f64().expect("area");
    assert!((area - 15707.96).abs() < 0.01, "{disc}");
}

#[test]
fn add_fill_in_a_filled_area_says_it_restyled_the_fill() {
    let mut s = Session::new();
    ok(
        &mut s,
        "add_geometry",
        json!({ "items": [{ "type": "rectangle", "corner": [0, 0], "width": 100, "height": 50 }] }),
    );
    let first = ok(&mut s, "add_fill", json!({ "at": [10, 10] }));
    assert_eq!(first.get("restyled"), None, "{first}");
    let again = ok(
        &mut s,
        "add_fill",
        json!({ "at": [90, 40], "kind": "solid", "color": "#336699" }),
    );
    assert_eq!(again["id"], first["id"], "{again}");
    assert_eq!(again["restyled"], true, "{again}");
    assert!(again["note"].is_string(), "{again}");
    let info = ok(&mut s, "drawing_info", json!({}));
    assert_eq!(info["fills"], 1);
}

#[test]
fn a_line_drawn_across_a_fill_is_reported() {
    let mut s = Session::new();
    ok(
        &mut s,
        "add_geometry",
        // Lines running past the corners: an area fill, not a shape (#202).
        json!({ "items": [
            { "type": "line", "from": [-5, 0], "to": [105, 0] },
            { "type": "line", "from": [-5, 50], "to": [105, 50] },
            { "type": "line", "from": [0, -5], "to": [0, 55] },
            { "type": "line", "from": [100, -5], "to": [100, 55] },
        ] }),
    );
    let fill = ok(&mut s, "add_fill", json!({ "at": [10, 10] }));
    let v = ok(
        &mut s,
        "add_geometry",
        json!({ "items": [{ "type": "line", "from": [40, -10], "to": [40, 60] }] }),
    );
    let fills = v["fills"].as_array().expect("fill changes");
    assert_eq!(fills.len(), 1, "{v}");
    assert_eq!(fills[0]["id"], fill["id"], "{v}");
    assert_eq!(fills[0]["change"], "divided", "{v}");
    assert_eq!(fills[0]["areas"], 2, "{v}");
    // A step that changes no fill says nothing about fills.
    let v = ok(
        &mut s,
        "add_geometry",
        json!({ "items": [{ "type": "point", "at": [500, 500] }] }),
    );
    assert!(v.get("fills").is_none(), "{v}");
}

#[test]
fn underlays_are_drawn_at_the_scale_given_whatever_the_format() {
    let dir = std::env::temp_dir().join(format!("drift-agent-scale-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A 50 mm line on a 100 mm page, and a DXF in millimetres with a 100 mm line.
    std::fs::write(
        dir.join("plan.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm"
 viewBox="0 0 100 100"><path d="M10 50 L60 50" stroke="black" fill="none"/></svg>"#,
    )
    .unwrap();
    let dxf = [
        "0",
        "SECTION",
        "2",
        "HEADER",
        "9",
        "$INSUNITS",
        "70",
        "4",
        "0",
        "ENDSEC",
        "0",
        "SECTION",
        "2",
        "ENTITIES",
        "0",
        "LINE",
        "8",
        "0",
        "10",
        "0",
        "20",
        "0",
        "11",
        "100",
        "21",
        "0",
        "0",
        "ENDSEC",
        "0",
        "EOF",
    ]
    .join("\n");
    std::fs::write(dir.join("plan.dxf"), dxf).unwrap();
    let mut s = Session::new().with_root(dir.clone());
    let underlay_width = |s: &Session, id: &str| {
        let (_, u) = s
            .editor
            .doc
            .underlays()
            .find(|(i, _)| i.to_string() == id)
            .expect("the underlay");
        (u.scale, u.bounds().size().x)
    };
    for (file, scale, n, width) in [
        ("plan.svg", json!("1:50"), 50.0, 2500.0),
        ("plan.dxf", json!("1:50"), 50.0, 5000.0),
        ("plan.dxf", json!(20), 20.0, 2000.0),
    ] {
        let got = ok(
            &mut s,
            "import_file",
            json!({ "path": file, "as": "underlay", "scale": scale }),
        );
        let id = str_at(&got, &["underlay"]).to_owned();
        let (got_n, w) = underlay_width(&s, &id);
        assert!(
            (w - width).abs() < 1e-6 && (got_n - n).abs() < 1e-9,
            "{file} at {scale}: scale {got_n}, {w} wide"
        );
    }
    // As geometry, a DXF drawn at 1 : 50 comes in 50 times larger.
    let got = ok(
        &mut s,
        "import_file",
        json!({ "path": "plan.dxf", "scale": "1:50" }),
    );
    assert_eq!(got["curves"], 1, "{got}");
    let long = s
        .editor
        .doc
        .iter()
        .filter_map(|(id, _)| match s.editor.doc.shape(id) {
            Some(drift_core::Shape::Line { a, b }) => Some(a.distance(b)),
            _ => None,
        })
        .fold(0.0, f64::max);
    assert!((long - 5000.0).abs() < 1e-6, "{long}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn trimming_against_given_curves_only() {
    let mut s = Session::new();
    let made = ok(
        &mut s,
        "add_geometry",
        json!({ "items": [
            { "type": "line", "from": [0, 0], "to": [40, 0] },
            { "type": "line", "from": [10, -5], "to": [10, 5] },
            { "type": "line", "from": [20, -5], "to": [20, 5] },
        ] }),
    );
    let ids: Vec<String> = made["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| str_at(c, &["id"]).to_owned())
        .collect();
    // Cut by the second upright only: the bar loses all of its first 20 mm.
    ok(
        &mut s,
        "trim_extend_split",
        json!({ "id": ids[0], "at": [15, 0], "against": [ids[2]] }),
    );
    let bar = ok(&mut s, "list_entities", json!({ "ids": [ids[0]] }));
    assert_eq!(bar["entities"][0]["from"], json!([20.0, 0.0]), "{bar}");
    assert!(
        err(
            &mut s,
            "trim_extend_split",
            json!({ "id": ids[1], "at": [10, 3], "against": [] })
        )
        .contains("against")
    );
}

#[test]
fn a_union_by_trimming_is_one_call_and_one_step() {
    let mut s = Session::new();
    let made = ok(
        &mut s,
        "add_geometry",
        json!({ "items": [
            { "type": "rectangle", "corner": [0, 0], "width": 60, "height": 40 },
            { "type": "circle", "center": [60, 20], "radius": 12 },
        ] }),
    );
    let right = str_at(&made["created"][0], &["sides", "right"]).to_owned();
    let circle = str_at(&made["created"][1], &["id"]).to_owned();
    let regions = |s: &mut Session| {
        ok(s, "analyze", json!({}))["regions"]
            .as_array()
            .map(Vec::len)
    };
    assert_eq!(regions(&mut s), Some(3));
    let v = ok(
        &mut s,
        "trim_extend_split",
        json!({ "id": right, "remove": "inside", "by": [circle], "and_remove": "inside" }),
    );
    assert_eq!(v["trimmed"], 2, "{v}");
    assert_eq!(regions(&mut s), Some(1), "one outline: {v}");
    ok(&mut s, "undo", json!({}));
    assert_eq!(regions(&mut s), Some(3), "one step");
    // A side of the rectangle stands for all of it, but a lone line bounds nothing.
    let line = ok(
        &mut s,
        "add_geometry",
        json!({ "items": [{ "type": "line", "from": [-10, -10], "to": [80, -10] }] }),
    );
    let line = str_at(&line["created"][0], &["id"]).to_owned();
    let e = err(
        &mut s,
        "trim_extend_split",
        json!({ "id": circle, "remove": "inside", "by": [line] }),
    );
    assert!(e.contains("closed"), "{e}");
}

/// A tool that panics part way through an edit: the session survives, the edit is undone,
/// and the undo history is as it was (#216).
#[test]
fn a_panicking_tool_is_undone_and_reported() {
    let mut s = Session::new();
    rough_room(&mut s);
    let before = s.editor.doc.clone();
    let revision = s.editor.doc.revision();
    let e = err(&mut s, "__panic", json!({}));
    assert!(e.contains("bug in Drift"), "{e}");
    assert!(!s.editor.doc.in_transaction());
    assert_eq!(s.editor.doc.iter().count(), before.iter().count());
    assert!(s.editor.doc.revision() >= revision);
    // The room is still one undo step away.
    ok(&mut s, "undo", json!({}));
    assert!(s.editor.doc.is_empty());
    ok(&mut s, "redo", json!({}));
    assert_eq!(s.editor.doc.iter().count(), before.iter().count());
}

/// #238 over the AI link: a fully defined plate whose width follows a variable, with a point
/// at the middle of its bottom side, filleted at two corners in one call.
#[test]
fn a_fillet_keeps_the_plate_parametric_over_the_link() {
    let mut s = Session::new();
    let made = ok(
        &mut s,
        "run_script",
        json!({ "script": "let S = 40 mm\nrect plate about 100 x 50 at 0,0 fixed\n\
            dim plate.sw to plate.se = S + 60 mm\ndim plate.west = 50\n\
            point m at 50,0\nmidpoint m of plate.south" }),
    );
    let name = |n: &str| str_at(&made, &["names", n]).to_owned();
    let (se, ne, m) = (name("plate.se"), name("plate.ne"), name("m"));
    let before = ok(&mut s, "list_constraints", json!({}));
    let ids = |v: &Value| -> Vec<String> {
        v["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_owned())
            .collect()
    };
    let undo_steps = s.editor.history.recorded();
    let v = ok(
        &mut s,
        "fillet_chamfer",
        json!({ "corner": [se, ne], "radius": 8 }),
    );
    assert_eq!(v["created"].as_array().map(Vec::len), Some(2), "{v}");
    assert_eq!(v["sketch"]["dof"], 0, "{v}");
    assert_eq!(s.editor.history.recorded(), undo_steps + 1, "one undo step");
    // Every constraint there was is still there: the width on the kept corner, the
    // midpoint from corner to corner.
    let after = ok(&mut s, "list_constraints", json!({}));
    let kept = ids(&after);
    for id in ids(&before) {
        assert!(kept.contains(&id), "{id} went: {after}");
    }
    let types: Vec<&str> = after["constraints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["type"].as_str().unwrap())
        .collect();
    assert!(
        types.contains(&"corner") && types.contains(&"midpoint_of"),
        "{after}"
    );
    // The plate follows S.
    ok(
        &mut s,
        "set_variables",
        json!({ "set": [{ "name": "S", "value": "50 mm" }] }),
    );
    let at = |s: &Session, id: &str| {
        let id = drift_core::EntityId(id[1..].parse().unwrap());
        s.editor.doc.point(id).unwrap()
    };
    assert!(at(&s, &se).distance(glam::DVec2::new(110.0, 0.0)) < 1e-6);
    assert!((at(&s, &m).x - 55.0).abs() < 1e-6);
    // A list with a corner that isn't one changes nothing.
    let count = s.editor.doc.len();
    let e = err(
        &mut s,
        "fillet_chamfer",
        json!({ "corner": [name("plate.nw"), se], "chamfer": 2 }),
    );
    assert!(e.contains(&se), "{e}");
    assert_eq!(s.editor.doc.len(), count);
}

/// What an edit takes off comes back as its `notice`.
#[test]
fn a_relation_a_trim_takes_off_is_reported() {
    let mut s = Session::new();
    let made = ok(
        &mut s,
        "run_script",
        json!({ "script": "line a from 0,0 to 30,0\nline b from 0,10 to 30,10\n\
            line c from 10,-5 to 10,15\nequal a b" }),
    );
    let a = str_at(&made, &["names", "a"]).to_owned();
    let v = ok(
        &mut s,
        "trim_extend_split",
        json!({ "id": a, "at": [5, 0] }),
    );
    let notice = v["notice"].as_str().unwrap_or_default();
    assert!(notice.contains("Equal"), "{v}");
}

/// A 100 × 50 rectangle with its lower-left corner at the origin; its four sides.
fn desk(s: &mut Session) -> Vec<String> {
    let made = ok(
        s,
        "add_geometry",
        json!({ "items": [{ "type": "rectangle", "corner": [0, 0], "width": 100, "height": 50 }] }),
    );
    ["bottom", "right", "top", "left"]
        .iter()
        .map(|side| str_at(&made["created"][0], &["sides", side]).to_owned())
        .collect()
}

/// The desk as a block called Desk with its base point at the origin: (block, copy).
fn desk_block(s: &mut Session) -> (String, String) {
    let sides = desk(s);
    let v = ok(
        s,
        "blocks",
        json!({ "op": "make", "ids": sides, "name": "Desk", "base": [0, 0] }),
    );
    (
        str_at(&v, &["block"]).to_owned(),
        str_at(&v, &["copy", "id"]).to_owned(),
    )
}

fn numbers(v: &Value) -> Vec<f64> {
    v.as_array()
        .unwrap_or_else(|| panic!("not a list: {v}"))
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

/// #231: curves become a named block with a base point, in one undo step.
#[test]
fn blocks_make_turns_curves_into_a_named_block_and_a_copy() {
    let mut s = Session::new();
    let sides = desk(&mut s);
    let steps = s.editor.history.undo_depth();
    let v = ok(
        &mut s,
        "blocks",
        json!({ "op": "make", "ids": sides, "name": "Desk", "base": [0, 0] }),
    );
    assert_eq!(v["name"], "Desk", "{v}");
    assert_eq!(v["curves"], 4, "{v}");
    assert_eq!(numbers(&v["copy"]["at"]), [0.0, 0.0], "{v}");
    assert_eq!(numbers(&v["copy"]["bounds"]), [0.0, 0.0, 100.0, 50.0]);
    // The curves went into the block: one copy stands where they were.
    assert_eq!(s.editor.doc.iter().count(), 0);
    assert_eq!(s.editor.history.undo_depth(), steps + 1);
    let info = ok(&mut s, "drawing_info", json!({}));
    assert_eq!(info["blocks"][0]["name"], "Desk", "{info}");
    assert_eq!(info["blocks"][0]["id"], v["block"]);
    assert_eq!(info["blocks"][0]["copies"], 1);
    assert_eq!(info["block_copies"], 1);
    assert_eq!(info["undo"], "Assistant: make block");
    // A name that is taken fails, and changes nothing.
    let more = desk(&mut s);
    let before = s.editor.doc.clone();
    let steps = s.editor.history.undo_depth();
    let e = err(
        &mut s,
        "blocks",
        json!({ "op": "make", "ids": more, "name": "Desk" }),
    );
    assert!(e.contains("already a block called Desk"), "{e}");
    assert!(e.contains("nothing was changed"), "{e}");
    assert_eq!(s.editor.doc, before);
    assert_eq!(s.editor.history.undo_depth(), steps);
    // Undo (the second desk, then the block) brings the first desk's lines back.
    ok(&mut s, "undo", json!({ "steps": 2 }));
    assert_eq!(s.editor.doc.blocks().count(), 0);
    assert_eq!(s.editor.doc.instances().count(), 0);
    let lines = ok(&mut s, "list_entities", json!({ "kind": "line" }));
    assert_eq!(lines["count"], 4, "{lines}");
}

/// #231: a copy is placed by its base point, turned, mirrored and scaled, in one undo step.
#[test]
fn blocks_place_puts_a_copy_turned_mirrored_and_scaled() {
    let mut s = Session::new();
    let (block, _) = desk_block(&mut s);
    let steps = s.editor.history.undo_depth();
    // By id: turned a quarter and twice the size.
    let v = ok(
        &mut s,
        "blocks",
        json!({ "op": "place", "block": block, "at": [1000, 0], "angle": 90, "scale": 2 }),
    );
    let copy = &v["copy"];
    assert_eq!(copy["block"], block, "{v}");
    assert_eq!(copy["angle"], 90.0);
    assert_eq!(copy["scale"], 2.0);
    assert_eq!(copy["mirror"], false);
    assert_eq!(numbers(&copy["bounds"]), [900.0, 0.0, 1000.0, 200.0], "{v}");
    assert_eq!(s.editor.history.undo_depth(), steps + 1, "one step");
    // By name, on a layer: mirrored left to right, then turned.
    ok(
        &mut s,
        "layers",
        json!({ "create": [{ "name": "Furniture" }] }),
    );
    let v = ok(
        &mut s,
        "blocks",
        json!({ "op": "place", "block": "Desk", "at": [2000, 0], "angle": 90, "mirror": true,
                "scale": 2, "layer": "Furniture" }),
    );
    assert_eq!(v["copy"]["mirror"], true, "{v}");
    assert_eq!(
        numbers(&v["copy"]["bounds"]),
        [1900.0, -200.0, 2000.0, 0.0],
        "{v}"
    );
    assert!(str_at(&v, &["copy", "layer"]).contains("Furniture"), "{v}");
    assert_eq!(s.editor.doc.instances().count(), 3);
    // Undo takes the whole copy away (placing and sizing it were one step).
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.editor.doc.instances().count(), 2);
    // Bad calls change nothing.
    let before = s.editor.doc.clone();
    let e = err(
        &mut s,
        "blocks",
        json!({ "op": "place", "block": "Chair", "at": [0, 0] }),
    );
    assert!(e.contains("no block"), "{e}");
    let e = err(
        &mut s,
        "blocks",
        json!({ "op": "place", "block": "Desk", "at": [0, 0], "scale": 0 }),
    );
    assert!(e.contains("`scale` must be positive"), "{e}");
    // In a batch, a copy placed before a call that fails is not kept.
    let e = err(
        &mut s,
        "batch",
        json!({ "calls": [
            { "tool": "blocks", "args": { "op": "place", "block": "Desk", "at": [5000, 0] } },
            { "tool": "blocks", "args": { "op": "place", "block": "Chair", "at": [0, 0] } }
        ]}),
    );
    assert!(e.contains("calls[1]"), "{e}");
    assert_eq!(s.editor.doc, before);
}

/// #231: blocks show in drawing_info, their copies in list_entities.
#[test]
fn blocks_and_their_copies_are_listed() {
    let mut s = Session::new();
    let (block, first) = desk_block(&mut s);
    ok(
        &mut s,
        "layers",
        json!({ "create": [{ "name": "Furniture" }] }),
    );
    let v = ok(
        &mut s,
        "blocks",
        json!({ "op": "place", "block": "Desk", "at": [1000, 0], "layer": "Furniture" }),
    );
    let second = str_at(&v, &["copy", "id"]).to_owned();
    ok(
        &mut s,
        "add_geometry",
        json!({ "items": [{ "type": "line", "from": [0, 500], "to": [300, 500] }] }),
    );
    let info = ok(&mut s, "drawing_info", json!({}));
    assert_eq!(
        info["blocks"],
        json!([{ "id": block, "name": "Desk", "curves": 4, "fills": 0, "copies": 2 }]),
        "{info}"
    );
    let all = ok(&mut s, "list_entities", json!({}));
    assert_eq!(all["count"], 1, "the line: {all}");
    assert_eq!(all["block_copy_count"], 2, "{all}");
    let copy = &all["block_copies"][1];
    assert_eq!(copy["id"], second, "{all}");
    assert_eq!(copy["block"], block);
    assert_eq!(copy["name"], "Desk");
    assert_eq!(numbers(&copy["at"]), [1000.0, 0.0]);
    assert_eq!(numbers(&copy["bounds"]), [1000.0, 0.0, 1100.0, 50.0]);
    // They follow the layer and the region, and stay out of a list of curves.
    let on = ok(&mut s, "list_entities", json!({ "layer": "Furniture" }));
    assert_eq!(on["block_copy_count"], 1, "{on}");
    assert_eq!(on["block_copies"][0]["id"], second);
    let within = ok(
        &mut s,
        "list_entities",
        json!({ "within": [-10, -10, 200, 100] }),
    );
    assert_eq!(within["block_copy_count"], 1, "{within}");
    assert_eq!(within["block_copies"][0]["id"], first);
    let lines = ok(&mut s, "list_entities", json!({ "kind": "line" }));
    assert!(lines.get("block_copies").is_none(), "{lines}");
    let only = ok(&mut s, "list_entities", json!({ "kind": "block_copy" }));
    assert_eq!(only["count"], 0, "{only}");
    assert_eq!(only["block_copy_count"], 2);
    // A copy is deleted by its id, in one step.
    ok(&mut s, "delete", json!({ "ids": [second] }));
    assert_eq!(s.editor.doc.instances().count(), 1);
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.editor.doc.instances().count(), 2);
}

/// #231: a copy explodes into plain curves where it stood, in one undo step.
#[test]
fn blocks_explode_turns_a_copy_back_into_curves() {
    let mut s = Session::new();
    let (block, first) = desk_block(&mut s);
    let v = ok(
        &mut s,
        "blocks",
        json!({ "op": "place", "block": block, "at": [1000, 0], "angle": 90, "scale": 2 }),
    );
    let second = str_at(&v, &["copy", "id"]).to_owned();
    let steps = s.editor.history.undo_depth();
    let v = ok(
        &mut s,
        "blocks",
        json!({ "op": "explode", "copies": [second] }),
    );
    let created = v["created"].as_array().expect("created").clone();
    assert_eq!(created.len(), 4, "{v}");
    assert_eq!(s.editor.history.undo_depth(), steps + 1);
    // The other copy and the block are still there.
    assert_eq!(s.editor.doc.instances().count(), 1);
    assert_eq!(s.editor.doc.blocks().count(), 1);
    // The curves are where the copy was, turned and scaled.
    let listed = ok(&mut s, "list_entities", json!({ "ids": created }));
    assert_eq!(listed["count"], 4, "{listed}");
    let curves: Vec<_> = created
        .iter()
        .map(|id| args::entity(&s.editor.doc, id).unwrap())
        .collect();
    let b = s.editor.bounds_of(&curves).expect("bounds");
    assert!((b.min - drift_core::DVec2::new(900.0, 0.0)).length() < 1e-9);
    assert!((b.max - drift_core::DVec2::new(1000.0, 200.0)).length() < 1e-9);
    // An id that names no copy fails and changes nothing.
    let before = s.editor.doc.clone();
    let e = err(
        &mut s,
        "blocks",
        json!({ "op": "explode", "copies": [first, "i999"] }),
    );
    assert!(e.contains("i999 does not exist"), "{e}");
    assert_eq!(s.editor.doc, before);
    // Undo puts the copy back.
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.editor.doc.instances().count(), 2);
    assert_eq!(s.editor.doc.iter().count(), 0);
}

/// #231: a copy becomes a group named after its block, and a group becomes a block.
#[test]
fn a_block_copy_becomes_a_group_and_a_group_a_block() {
    let mut s = Session::new();
    let (_, copy) = desk_block(&mut s);
    let steps = s.editor.history.undo_depth();
    let v = ok(
        &mut s,
        "blocks",
        json!({ "op": "explode", "copies": [copy], "to_group": true }),
    );
    assert_eq!(s.editor.history.undo_depth(), steps + 1);
    let group = v["groups"][0]["id"].as_str().expect("a group").to_owned();
    assert_eq!(v["groups"][0]["name"], "Desk", "{v}");
    assert_eq!(
        v["groups"][0]["members"].as_array().map(Vec::len),
        Some(4),
        "{v}"
    );
    assert_eq!(s.editor.doc.instances().count(), 0);
    let info = ok(&mut s, "drawing_info", json!({}));
    assert_eq!(
        info["groups"],
        json!([{ "id": group, "name": "Desk", "members": 4 }]),
        "{info}"
    );
    let lines = ok(&mut s, "list_entities", json!({ "kind": "line" }));
    assert_eq!(lines["entities"][0]["group"], group, "{lines}");
    // The group back into a block: the block Desk is still there, so it takes a free name.
    let steps = s.editor.history.undo_depth();
    let v = ok(
        &mut s,
        "blocks",
        json!({ "op": "from_group", "group": group }),
    );
    assert_eq!(v["name"], "Desk 2", "{v}");
    assert_eq!(v["curves"], 4);
    assert_eq!(
        numbers(&v["copy"]["bounds"]),
        [0.0, 0.0, 100.0, 50.0],
        "{v}"
    );
    assert_eq!(s.editor.history.undo_depth(), steps + 1);
    assert_eq!(s.editor.doc.groups().count(), 0);
    assert_eq!(s.editor.doc.iter().count(), 0);
    // A group that is gone fails and changes nothing.
    let before = s.editor.doc.clone();
    let e = err(
        &mut s,
        "blocks",
        json!({ "op": "from_group", "group": group }),
    );
    assert!(e.contains("no group"), "{e}");
    assert_eq!(s.editor.doc, before);
    // Undo: the group and its lines are back.
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.editor.doc.groups().count(), 1);
    assert_eq!(s.editor.doc.instances().count(), 0);
}

/// #231, on the drawing open in Drift: a block made through the link is one step in Drift's
/// own history with its copy picked, and nothing is done to blocks while the user is
/// editing one in place.
#[test]
fn blocks_work_over_the_link_and_wait_for_a_block_edit() {
    let mut ed = Editor::new(drift_core::Document::new());
    let live = |ed: &mut Editor, tool: &str, args: Value| {
        let doc = LiveDoc {
            editor: ed,
            path: None,
            meta: Default::default(),
            dirty: true,
            ctx: eframe::egui::Context::default(),
            new_rules: Default::default(),
        };
        call_live(doc, tool, &args, None).0
    };
    let made = live(
        &mut ed,
        "add_geometry",
        json!({ "items": [{ "type": "rectangle", "corner": [0, 0], "width": 100, "height": 50 }] }),
    );
    assert!(!made.is_error, "{}", made.json);
    let sides: Vec<&str> = ["bottom", "right", "top", "left"]
        .iter()
        .map(|side| str_at(&made.json["created"][0], &["sides", side]))
        .collect();
    let steps = ed.history.undo_depth();
    let out = live(
        &mut ed,
        "blocks",
        json!({ "op": "make", "ids": sides, "name": "Desk" }),
    );
    assert!(!out.is_error, "{}", out.json);
    assert_eq!(ed.history.undo_depth(), steps + 1);
    assert_eq!(ed.history.undo_label(), Some("Assistant: make block"));
    let copy = *ed.selected_instances.first().expect("the copy is picked");
    assert_eq!(out.json["copy"]["id"], copy.to_string());
    // The user opens the block to edit it in place: the assistant's block calls wait.
    ed.edit_block(copy);
    let before = ed.doc.clone();
    for args in [
        json!({ "op": "place", "block": "Desk", "at": [500, 0] }),
        json!({ "op": "explode", "copies": [copy.to_string()] }),
    ] {
        let out = live(&mut ed, "blocks", args);
        assert!(out.is_error, "{}", out.json);
        let e = out.json["error"].as_str().unwrap_or_default();
        assert!(e.contains("being edited in place"), "{e}");
    }
    assert_eq!(ed.doc, before);
    assert!(ed.block_edit.is_some(), "the edit goes on");
}

fn eid_of(id: &str) -> drift_core::EntityId {
    drift_core::EntityId(id[1..].parse().expect("an entity id"))
}

fn eids(v: &Value) -> Vec<drift_core::EntityId> {
    v.as_array()
        .unwrap_or_else(|| panic!("no list in {v}"))
        .iter()
        .map(|id| eid_of(id.as_str().expect("an id")))
        .collect()
}

/// #476: two rectangles one script draws side by side, sharing two corners, are two
/// figures: Tab on a side of either picks that rectangle's four sides.
#[test]
fn rectangles_a_script_draws_sharing_corners_chain_apart() {
    let mut s = Session::new();
    let made = ok(
        &mut s,
        "run_script",
        json!({ "script": "rect a about 100 x 50 at 0,0\nrect b about 100 x 50 at 100,0" }),
    );
    let sides = |r: &str| -> std::collections::BTreeSet<drift_core::EntityId> {
        ["south", "east", "north", "west"]
            .iter()
            .map(|k| eid_of(str_at(&made, &["names", &format!("{r}.{k}")])))
            .collect()
    };
    let (a, b) = (sides("a"), sides("b"));
    assert_eq!(
        eid_of(str_at(&made, &["names", "a.se"])),
        eid_of(str_at(&made, &["names", "b.sw"])),
        "the corners are shared: {made}"
    );
    for r in [&a, &b] {
        for side in r {
            assert_eq!(&s.editor.chain_of(*side), r, "{made}");
        }
    }
}

/// #476: one `add_geometry` call with two closed polylines sharing a corner makes each
/// its own figure.
#[test]
fn closed_polylines_one_call_draws_sharing_a_corner_are_separate_figures() {
    let mut s = Session::new();
    let made = ok(
        &mut s,
        "add_geometry",
        json!({ "items": [
            { "type": "polyline", "points": [[0, 0], [10, 0], [10, 10], [0, 10]], "closed": true },
            { "type": "polyline", "points": [[10, 10], [20, 10], [20, 20], [10, 20]], "closed": true }
        ]}),
    );
    let (p, q) = (
        eids(&made["created"][0]["lines"]),
        eids(&made["created"][1]["lines"]),
    );
    let doc = &s.editor.doc;
    let figure = |lines: &[drift_core::EntityId]| {
        let f = doc.figure_of(lines[0]);
        assert!(f.is_some());
        assert!(lines.iter().all(|l| doc.figure_of(*l) == f), "{made}");
        f
    };
    assert_ne!(figure(&p), figure(&q), "{made}");
    assert_eq!(s.editor.chain_of(p[0]), p.iter().copied().collect());
    assert_eq!(s.editor.chain_of(q[0]), q.iter().copied().collect());
}

/// #476: a polyline, open or closed, is one figure; one a script starts on an open end of
/// an existing path joins that path, as drawing on from a path's end does. One started
/// on a closed shape's corner stays apart.
#[test]
fn a_polyline_is_one_figure_and_continues_a_path_from_its_open_end() {
    let mut s = Session::new();
    let made = ok(
        &mut s,
        "add_geometry",
        json!({ "items": [
            { "type": "polyline", "points": [[0, 0], [10, 0], [10, 10]] },
            { "type": "polyline", "points": [[50, 0], [60, 0], [60, 10]], "closed": true }
        ]}),
    );
    let (open, closed) = (
        eids(&made["created"][0]["lines"]),
        eids(&made["created"][1]["lines"]),
    );
    {
        let doc = &s.editor.doc;
        for lines in [&open, &closed] {
            let f = doc.figure_of(lines[0]);
            assert!(f.is_some());
            assert!(lines.iter().all(|l| doc.figure_of(*l) == f), "{made}");
        }
        assert_ne!(doc.figure_of(open[0]), doc.figure_of(closed[0]));
    }
    let more = ok(
        &mut s,
        "run_script",
        json!({ "script": "polyline on (10,10) (0,10) (0,5)\npolyline off (60,10) (70,10)" }),
    );
    let names = |n: &str| -> Vec<drift_core::EntityId> {
        (1..)
            .map_while(|k| more["names"].get(format!("{n}[{k}]")))
            .map(|v| eid_of(v.as_str().unwrap()))
            .collect()
    };
    let (on, off) = (names("on"), names("off"));
    assert_eq!(on.len(), 2, "{more}");
    assert_eq!(off.len(), 1, "{more}");
    let doc = &s.editor.doc;
    let path = doc.figure_of(open[0]);
    assert!(on.iter().all(|l| doc.figure_of(*l) == path), "{more}");
    assert_ne!(doc.figure_of(off[0]), doc.figure_of(closed[0]), "{more}");
    let whole: std::collections::BTreeSet<_> = open.iter().chain(&on).copied().collect();
    assert_eq!(s.editor.chain_of(open[0]), whole);
    assert_eq!(
        s.editor.chain_of(closed[0]),
        closed.iter().copied().collect()
    );
}
