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

#[test]
fn layers_fills_and_text() {
    let mut s = Session::new();
    rough_room(&mut s);
    let l = ok(
        &mut s,
        "layers",
        json!({ "create": [{ "name": "Walls", "color": "#aa0000", "weight_mm": 0.5 }] }),
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
