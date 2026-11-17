use serde_json::{Value, json};

use super::parse::{Along, ArrayKind, DimKind, Part, Place, Pos, Side, Spacing, Stmt, WallSide};
use super::*;

fn stmt(line: &str) -> Stmt {
    match parse::parse(line) {
        Ok(mut v) => v.remove(0).stmt,
        Err(e) => panic!("{line}: {}", e.text()),
    }
}

fn parse_err(script: &str) -> errors::ScriptError {
    match parse::parse(script) {
        Ok(v) => panic!("{script} should not parse: {v:?}"),
        Err(e) => e,
    }
}

/// The text of each value in a statement written as `let x = <value>`.
fn value(v: &str) -> String {
    match stmt(&format!("let x = {v}")) {
        Stmt::Let { value, .. } => value.text,
        other => panic!("{other:?}"),
    }
}

fn run(s: &mut Session, script: &str) -> Value {
    let out = s.call("run_script", &json!({ "script": script }));
    assert!(
        !out.is_error,
        "the script failed:\n{}",
        out.json["error"].as_str().unwrap_or(&out.json.to_string())
    );
    out.json
}

fn run_err(s: &mut Session, script: &str) -> Value {
    let out = s.call("run_script", &json!({ "script": script }));
    assert!(out.is_error, "the script should have failed: {}", out.json);
    out.json
}

fn ok(s: &mut Session, name: &str, args: Value) -> Value {
    let out = s.call(name, &args);
    assert!(!out.is_error, "{name} failed: {}", out.json);
    out.json
}

fn id(v: &Value, name: &str) -> String {
    v["names"][name]
        .as_str()
        .unwrap_or_else(|| panic!("no name {name} in {}", v["names"]))
        .to_owned()
}

/// Where point `p` is.
fn at(s: &Session, p: &str) -> drift_core::DVec2 {
    let id = drift_core::EntityId(p[1..].parse().unwrap());
    s.editor.doc.point(id).unwrap()
}

fn line_len(s: &Session, l: &str) -> f64 {
    let id = drift_core::EntityId(l[1..].parse().unwrap());
    match s.editor.doc.get(id).map(|e| e.kind) {
        Some(drift_core::EntityKind::Line { a, b }) => s
            .editor
            .doc
            .point(a)
            .unwrap()
            .distance(s.editor.doc.point(b).unwrap()),
        other => panic!("{l} is {other:?}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

// Parsing.

#[test]
fn values_keep_their_text_with_units_and_feet_and_inches() {
    for v in [
        "3600",
        "3.6 m",
        "3.6m",
        "360 cm",
        "12'6\"",
        "12' 6\"",
        "12'-6\"",
        "6 1/2\"",
        "150\"",
        "1'-6 1/2\"",
        "30°",
        "W/2 + 100",
        "1.25 * W",
        "(W - 2*T) / 3",
        "sqrt(W^2 + D^2)",
        "-W",
        "12 ft",
        "6 in",
    ] {
        assert_eq!(value(v), v, "{v}");
    }
}

#[test]
fn inches_and_text_are_told_apart() {
    match stmt("label bed \"Bed 12\" height 250") {
        Stmt::Label { text, height, .. } => {
            assert_eq!(text, "Bed 12");
            assert_eq!(height.unwrap().text, "250");
        }
        other => panic!("{other:?}"),
    }
    match stmt("rect r 12'6\" x 10' at 0,0") {
        Stmt::Rect { w, h, .. } => {
            assert_eq!(w.text, "12'6\"");
            assert_eq!(h.text, "10'");
        }
        other => panic!("{other:?}"),
    }
    // `in` is inches only where it can't start `in <name>`.
    match stmt("door d1 36 in in w1") {
        Stmt::Opening { width, host, .. } => {
            assert_eq!(width.text, "36 in");
            assert_eq!(host.root, "w1");
        }
        other => panic!("{other:?}"),
    }
    match stmt("door d1 900 in bed.south") {
        Stmt::Opening {
            width, host, along, ..
        } => {
            assert_eq!(width.text, "900");
            assert_eq!(host.to_string(), "bed.south");
            assert_eq!(along, Along::Centred);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn statements_parse() {
    match stmt("room bed W x 1.25*W at 0,0 walls 200 inside fixed") {
        Stmt::Room {
            name,
            w,
            h,
            place,
            walls,
            fixed,
        } => {
            assert_eq!(
                (name.as_str(), w.text.as_str(), h.text.as_str()),
                ("bed", "W", "1.25*W")
            );
            assert!(matches!(place, Place::At(Pos::At(..))));
            assert_eq!(
                walls.map(|(t, s)| (t.text, s)),
                Some(("200".into(), WallSide::Inside))
            );
            assert!(fixed);
        }
        other => panic!("{other:?}"),
    }
    match stmt("room bath 2400 x D right of bed walls 200") {
        Stmt::Room {
            place: Place::Beside(Side::Right, other),
            ..
        } => assert_eq!(other.root, "bed"),
        other => panic!("{other:?}"),
    }
    match stmt("window w1 1200 in bed.north offset 600 from bed.nw") {
        Stmt::Opening {
            along: Along::Offset(by, from),
            ..
        } => {
            assert_eq!(by.text, "600");
            assert_eq!(from.to_string(), "bed.nw");
        }
        other => panic!("{other:?}"),
    }
    match stmt("grid cols A..E every 6 m length 30 m at 0,0") {
        Stmt::Grid {
            name,
            labels,
            spacing,
            ..
        } => {
            assert_eq!(name, "grid");
            assert_eq!(labels, ["A", "B", "C", "D", "E"]);
            assert!(matches!(spacing, Spacing::Every(e) if e.text == "6 m"));
        }
        other => panic!("{other:?}"),
    }
    match stmt("grid s rows 1..3 spacing 6 m, 7.5 m length 20 m") {
        Stmt::Grid {
            name,
            labels,
            spacing: Spacing::List(l),
            ..
        } => {
            assert_eq!(name, "s");
            assert_eq!(labels, ["1", "2", "3"]);
            assert_eq!(l.len(), 2);
        }
        other => panic!("{other:?}"),
    }
    match stmt("array bay of col 5 x 6 m, 4 x 7.2 m") {
        Stmt::Array {
            kind:
                ArrayKind::Grid {
                    first,
                    second: Some(second),
                    up: false,
                },
            ..
        } => {
            assert_eq!((first.0.text.as_str(), first.1.text.as_str()), ("5", "6 m"));
            assert_eq!(
                (second.0.text.as_str(), second.1.text.as_str()),
                ("4", "7.2 m")
            );
        }
        other => panic!("{other:?}"),
    }
    match stmt("dim bed.sw to hall.se = 8 m horizontal") {
        Stmt::Dim {
            kind: DimKind::Auto,
            a,
            b: Some(b),
            dir: Some(_),
            value: Some(v),
            reference: false,
        } => {
            assert_eq!(
                (a.to_string(), b.to_string(), v.text),
                ("bed.sw".into(), "hall.se".into(), "8 m".into())
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        stmt("dim r col = 225"),
        Stmt::Dim {
            kind: DimKind::Radius,
            ..
        }
    ));
    assert!(matches!(
        stmt("dim angle w1 w2 = 30°"),
        Stmt::Dim {
            kind: DimKind::Angle,
            b: Some(_),
            ..
        }
    ));
    assert!(matches!(
        stmt("dim bed.north ref"),
        Stmt::Dim {
            reference: true,
            value: None,
            ..
        }
    ));
    match stmt("line l from p to cols[3].center") {
        Stmt::Line {
            from: Pos::Ref(p),
            to: Pos::Ref(q),
            ..
        } => {
            assert_eq!(p.root, "p");
            assert_eq!(q.parts, vec![Part::Index(3), Part::Field("center".into())]);
        }
        other => panic!("{other:?}"),
    }
    match stmt("polyline site (0,0) (30 m,0) (30 m, 22 m) closed") {
        Stmt::Polyline { points, closed, .. } => assert_eq!((points.len(), closed), (3, true)),
        other => panic!("{other:?}"),
    }
    match stmt("layer Walls color #333333 weight 0.5 line center") {
        Stmt::Layer { color, line, .. } => assert_eq!(
            (color.unwrap(), line.unwrap()),
            ("#333333".into(), "center".into())
        ),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        stmt("construction line axis from 0,0 to 0,10 m"),
        Stmt::Line {
            construction: true,
            ..
        }
    ));
    assert!(matches!(
        stmt("point p at W, 0"),
        Stmt::Point {
            at: Pos::At(..),
            ..
        }
    ));
    assert!(matches!(
        stmt("point p at (W/2, D)"),
        Stmt::Point {
            at: Pos::At(..),
            ..
        }
    ));
}

#[test]
fn comments_continuations_and_layer_suffixes() {
    let st = parse::parse(
        "# a flat\n\nlet W = 3.6 m   # width\nroom bed W x 4 m \\\n  at 0,0 walls 200 on Walls\nfill bed #eeeeee",
    )
    .unwrap();
    assert_eq!(st.len(), 3);
    assert_eq!(
        st[1].line, 4,
        "a continued statement has its first line's number"
    );
    assert_eq!(st[1].layer.as_deref(), Some("Walls"));
    assert!(matches!(&st[2].stmt, Stmt::Fill { color: Some(c), .. } if c == "#eeeeee"));
}

#[test]
fn parse_errors_say_where_and_how_to_write_it() {
    let e = parse_err("let W = 3.6 m\ndoor d1 in bed.south 900 centred");
    assert_eq!(e.line, 2);
    assert_eq!(e.column(), Some(9));
    let text = e.text();
    assert!(text.contains("expected the door's width"), "{text}");
    assert!(
        text.contains("2 | door d1 in bed.south 900 centred"),
        "{text}"
    );
    assert!(
        text.contains("        ^^"),
        "the marker is under `in`:\n{text}"
    );
    assert!(text.ends_with("Nothing was changed."), "{text}");

    let e = parse_err("rom bed 3 m x 4 m at 0,0");
    assert!(e.text().contains("unknown statement `rom`"), "{}", e.text());
    assert!(e.text().contains("did you mean `room`?"), "{}", e.text());

    let e = parse_err("room bed 3 m x 4 m walls 200");
    assert!(e.text().contains("a room needs a place"), "{}", e.text());

    let e = parse_err("dim bed.south");
    assert!(e.text().contains("`= <value>`, or `ref`"), "{}", e.text());

    let e = parse_err("text \"N at 0,0");
    assert!(e.text().contains("no closing"), "{}", e.text());

    let e = parse_err("rect r 3 m x 2 m at 0,0 fixd");
    assert!(e.text().contains("unexpected `fixd`"), "{}", e.text());

    let e = parse_err("let W = 3 +");
    assert!(e.text().contains("expected a value"), "{}", e.text());

    let e = parse_err("grid rows 1..3 spacing 6 m length 20 m");
    assert!(
        e.text().contains("3 axes need 2 spacings, not 1"),
        "{}",
        e.text()
    );
}

// Names, checked before anything runs.

#[test]
fn unknown_names_parts_and_variables_change_nothing() {
    let mut s = Session::new();
    let e = run_err(
        &mut s,
        "room bed 3 m x 4 m at 0,0 walls 200\nwindow w1 1200 in bed.sout offset 600 from bed.nw",
    );
    let text = e["error"].as_str().unwrap();
    assert_eq!(e["line"], 2, "{text}");
    assert!(
        text.contains("room bed has no part `sout` (did you mean `south`?)"),
        "{text}"
    );
    assert!(text.contains("Its parts: south, east"), "{text}");
    assert!(s.editor.doc.is_empty(), "nothing ran");

    let e = run_err(&mut s, "room bed W x 4 m at 0,0");
    assert!(
        e["error"]
            .as_str()
            .unwrap()
            .contains("unknown variable `W`"),
        "{e}"
    );

    let e = run_err(
        &mut s,
        "room bed 3 m x 4 m at 0,0\nroom bed 3 m x 4 m at 5 m,0",
    );
    assert!(
        e["error"]
            .as_str()
            .unwrap()
            .contains("`bed` is already the room on line 1"),
        "{e}"
    );

    let e = run_err(&mut s, "rect r 3 m x 4 m at 0,0\nhatch r.walls");
    assert!(
        e["error"]
            .as_str()
            .unwrap()
            .contains("rect r has no part `walls`"),
        "{e}"
    );

    let e = run_err(&mut s, "door d 900 in bedroom.south");
    assert!(
        e["error"]
            .as_str()
            .unwrap()
            .contains("`bedroom` isn't defined"),
        "{e}"
    );

    let e = run_err(&mut s, "use e999 as x");
    assert!(
        e["error"].as_str().unwrap().contains("e999 doesn't exist"),
        "{e}"
    );
    assert_eq!(s.editor.history.undo_depth(), 0);
}

// Running.

const FLAT: &str = "\
let W = 3.6 m
let D = 1.25 * W
let T = 200
room bed W x D at 0,0 walls T fixed
door d1 900 in bed.south centred
window w1 1200 in bed.north offset 600 from bed.nw
";

#[test]
fn a_room_with_walls_a_door_and_a_window_is_fully_defined() {
    let mut s = Session::new();
    let v = run(&mut s, FLAT);
    assert_eq!(v["sketch"]["fully_defined"], true, "{}", v["sketch"]);
    assert_eq!(v["sketch"]["undo_steps"], 1);
    assert!(
        close(v["rooms"][0]["area_m2"].as_f64().unwrap(), 3.6 * 4.5),
        "{}",
        v["rooms"]
    );
    // The door is 900 and centred: equal pieces of wall either side.
    let (l, r) = (id(&v, "bed.south[1]"), id(&v, "bed.south[2]"));
    assert!(
        close(line_len(&s, &l), (3600.0 - 900.0) / 2.0),
        "{}",
        line_len(&s, &l)
    );
    assert!(close(line_len(&s, &r), (3600.0 - 900.0) / 2.0));
    // The jambs run through the wall.
    let j = id(&v, "d1.start");
    assert!(close(line_len(&s, &j), 200.0));
    // The window starts 600 from the north-west corner (its jamb nearer that corner).
    let nw = at(&s, &id(&v, "bed.nw"));
    let near = ["w1.start", "w1.end"]
        .iter()
        .flat_map(|j| {
            let j = id(&v, j);
            match s
                .editor
                .doc
                .get(drift_core::EntityId(j[1..].parse().unwrap()))
                .map(|e| e.kind)
            {
                Some(drift_core::EntityKind::Line { a, b }) => [a, b],
                other => panic!("{other:?}"),
            }
        })
        .map(|p| s.editor.doc.point(p).unwrap().distance(nw))
        .fold(f64::MAX, f64::min);
    assert!(close(near, 600.0), "{near}");

    // A second script changes the width; the door stays centred.
    run(&mut s, "let W = 4 m");
    assert!(
        close(line_len(&s, &l), (4000.0 - 900.0) / 2.0),
        "{}",
        line_len(&s, &l)
    );
    assert!(close(line_len(&s, &r), (4000.0 - 900.0) / 2.0));
    let a = ok(&mut s, "analyze", json!({}));
    assert_eq!(a["fully_defined"], true, "{a}");
}

#[test]
fn a_script_is_one_undo_step() {
    let mut s = Session::new();
    run(&mut s, FLAT);
    assert!(!s.editor.doc.is_empty());
    let u = ok(&mut s, "undo", json!({}));
    assert_eq!(u["undone"], json!(["Assistant: script"]));
    assert!(s.editor.doc.is_empty(), "one undo removes the whole script");
}

#[test]
fn a_failure_part_way_changes_nothing_and_names_the_lines() {
    let mut s = Session::new();
    run(&mut s, "rect keep 1 m x 1 m at -5 m,0 fixed");
    let before = s.editor.doc.clone();
    let e = run_err(
        &mut s,
        "let W = 3.6 m\nroom bed W x 4 m at 0,0 walls 200 fixed\ndim bed.south = 4 m",
    );
    let text = e["error"].as_str().unwrap();
    assert_eq!(e["line"], 3, "{text}");
    assert_eq!(e["related_lines"], json!([2]), "{text}");
    assert!(
        text.contains("2 | room bed W x 4 m at 0,0 walls 200 fixed"),
        "{text}"
    );
    assert!(
        text.contains("meant: make the length of bed.south 4 m"),
        "{text}"
    );
    assert!(text.contains("add `ref`"), "{text}");
    assert!(text.ends_with("Nothing was changed."), "{text}");
    assert_eq!(s.editor.doc, before, "the drawing is as it was");
    assert_eq!(s.editor.history.undo_depth(), 1);
}

#[test]
fn check_keeps_nothing_and_explain_replays_as_a_batch() {
    let mut s = Session::new();
    let out = s.call("run_script", &json!({ "script": FLAT, "check": true }));
    assert!(!out.is_error, "{}", out.json);
    assert_eq!(
        out.json["sketch"]["fully_defined"], true,
        "check reports the result"
    );
    assert!(s.editor.doc.is_empty(), "and keeps nothing");
    assert_eq!(s.editor.history.undo_depth(), 0);

    let mut s = Session::new();
    let out = s.call("run_script", &json!({ "script": FLAT, "explain": true }));
    let calls = out.json["calls"].clone();
    assert!(calls.as_array().unwrap().len() > 10, "{calls}");
    let mut replay = Session::new();
    let b = replay.call("batch", &json!({ "calls": calls }));
    assert!(!b.is_error, "{}", b.json);
    assert_eq!(
        replay.editor.doc, s.editor.doc,
        "the same drawing, call for call"
    );
}

#[test]
fn rooms_side_by_side_share_a_wall() {
    let mut s = Session::new();
    let v = run(
        &mut s,
        "room bed 3600 x 4500 at 0,0 walls 200 fixed\nroom bath 2400 x 3000 right of bed walls 200\nlabel bath \"Bath\"\nhatch bed.walls ANSI31\nhatch bath.walls ANSI31",
    );
    assert_eq!(v["sketch"]["fully_defined"], true, "{}", v["sketch"]);
    // bath's inside west face is bed's outside east face.
    assert_eq!(id(&v, "bath.west"), id(&v, "bed.outer.east"));
    let sw = at(&s, &id(&v, "bath.sw"));
    assert!(close(sw.x, 3800.0) && close(sw.y, 0.0), "{sw}");
    let rooms = &v["rooms"];
    assert!(
        close(rooms[1]["area_m2"].as_f64().unwrap(), 2.4 * 3.0),
        "{rooms}"
    );
}

#[test]
fn walls_grids_and_arrays() {
    let mut s = Session::new();
    let v = run(
        &mut s,
        "\
let S = 6 m
grid cols A..C every S length 14 m at 0,-1 m fixed
grid rows 1..3 every 6 m length 14 m at -1 m,0 fixed
circle col r 200 at 0,0 fixed
array cols of col 3 x S, 3 x 6 m
wall w1 from 0,0 to 12 m,0 thick 300 centre fixed length 12 m
door d1 1000 in w1.left offset 1 m from grid.B
",
    );
    assert_eq!(v["sketch"]["fully_defined"], true, "{}", v["sketch"]);
    // The last column copy sits on C/3.
    let c9 = &v["names"]["cols[9]"];
    assert!(c9.is_string(), "{}", v["names"]);
    let center = match s
        .editor
        .doc
        .get(drift_core::EntityId(
            c9.as_str().unwrap()[1..].parse().unwrap(),
        ))
        .map(|e| e.kind)
    {
        Some(drift_core::EntityKind::Circle { center, .. }) => s.editor.doc.point(center).unwrap(),
        other => panic!("{other:?}"),
    };
    assert!(
        close(center.x, 12000.0) && close(center.y, 12000.0),
        "{center}"
    );
    // Grid lines are centre lines: they print but don't cut areas.
    let a = id(&v, "grid.A");
    let e = s
        .editor
        .doc
        .get(drift_core::EntityId(a[1..].parse().unwrap()))
        .unwrap();
    assert_eq!(e.style, drift_core::LineKind::Center);
    // A spacing change moves the grid and the columns with it.
    run(&mut s, "let S = 7 m");
    let c9 = c9.as_str().unwrap();
    let center = match s
        .editor
        .doc
        .get(drift_core::EntityId(c9[1..].parse().unwrap()))
        .map(|e| e.kind)
    {
        Some(drift_core::EntityKind::Circle { center, .. }) => s.editor.doc.point(center).unwrap(),
        other => panic!("{other:?}"),
    };
    assert!(close(center.x, 14000.0), "the columns follow S: {center}");
    let a = ok(&mut s, "analyze", json!({}));
    assert_eq!(a["fully_defined"], true, "{a}");
}

#[test]
fn plain_geometry_relations_and_dimensions() {
    let mut s = Session::new();
    let v = run(
        &mut s,
        "\
point o at 0,0 fixed
line a from o to 3000,100
line b from 0,1000 to 2900,1200
horizontal a
parallel a b
dim a = 3 m
dim b = 3 m
dim a to b = 1200
vertical a.start b.start
",
    );
    assert_eq!(v["sketch"]["fully_defined"], true, "{}", v["sketch"]);
    let b0 = at(&s, &id(&v, "b.start"));
    assert!(close(b0.x, 0.0) && close(b0.y, 1200.0), "{b0}");
    assert!(close(line_len(&s, &id(&v, "b")), 3000.0));
}

#[test]
fn run_script_cannot_nest_in_a_batch() {
    let mut s = Session::new();
    let out = s.call(
        "batch",
        &json!({ "calls": [{ "tool": "run_script", "args": { "script": "point p at 0,0" } }] }),
    );
    assert!(out.is_error);
    assert!(
        out.json["error"]
            .as_str()
            .unwrap()
            .contains("can't be used inside a batch"),
        "{}",
        out.json
    );
}

#[test]
fn the_tool_is_listed_and_preferred() {
    let t = crate::agent::tools();
    assert!(t.iter().any(|t| t.name == "run_script" && !t.read_only));
    assert!(crate::agent::INSTRUCTIONS.contains("run_script"));
}

#[test]
fn every_statement_runs() {
    let mut s = Session::new();
    run(&mut s, "rect old 1 m x 1 m at -5 m,0");
    let v = run(
        &mut s,
        r#"
let N = 4 number
let A = 30°
layer Walls color #333333 weight 0.5
layer Grid color #c03030 line center
on Walls
polyline site (0,0) (30 m,0) (30 m,20 m) (0,20 m) closed on Grid
construction line axis from 15 m,0 to 15 m,20 m
point p at 10 m,5 m
point q at 20 m,5 m
symmetric p q about axis
horizontal p q
line chord from p to q
point m at 15 m,6 m
midpoint m of chord
circle hole d 450 at 5 m,15 m
on m hole
arc bend centre 25 m,15 m from 26 m,15 m to 25 m,16 m
rect slab about 2 m x 1 m centred at 15 m,10 m
equal slab.south slab.north slab.width
collinear site[1] slab.south
dim angle site[1] chord ref
array posts of hole N by 1 m,0
array ring of hole 6 around 5 m,12 m angle 180
text "N" at 0,22 m height 250 align centre
fill slab #eeeeee opacity 0.5
hatch site.inside ANSI31 scale 2
label slab "Slab"
use e5 as old_bottom
dim old_bottom ref
"#,
    );
    assert!(v["names"]["posts[4]"].is_string(), "{}", v["names"]);
    assert!(
        v["skipped"].is_array(),
        "the implied equal or collinear is reported: {v}"
    );
    let layers = ok(&mut s, "drawing_info", json!({}));
    assert!(layers.to_string().contains("Walls"), "{layers}");
}

#[test]
fn feet_and_inches_dimensions_keep_their_text() {
    let mut s = Session::new();
    let v = run(&mut s, "rect r 12'6\" x 10' at 0,0 fixed");
    assert_eq!(v["sketch"]["fully_defined"], true);
    let b = id(&v, "r.south");
    assert!(close(line_len(&s, &b), 3810.0), "{}", line_len(&s, &b));
    assert!(close(line_len(&s, &id(&v, "r.west")), 3048.0));
}

/// The examples in docs/guide/ai/script.md run as written.
#[test]
fn the_documented_examples_run() {
    let mut s = Session::new();
    let v = run(
        &mut s,
        r#"
let W = 3.6 m
let D = 1.25 * W
room bed W x D at 0,0 walls 200 fixed
door d1 900 in bed.south centred
window w1 1200 in bed.north offset 600 from bed.nw
label bed "Bedroom"
hatch bed.walls ANSI31
"#,
    );
    assert_eq!(v["sketch"]["fully_defined"], true, "{}", v["sketch"]);
    run(&mut s, "let W = 4 m");

    let mut s = Session::new();
    run(
        &mut s,
        "\
grid cols A..E every 6 m length 30 m at 0,0 fixed
grid rows 1..4 spacing 6 m, 7.5 m, 6 m length 24 m at 0,0
circle col r 200 at 0,0 fixed
array cols of col 5 x 6 m, 4 x 6 m
array posts of col 4 by 1 m,0
array ring of col 8 around 0,0 angle 180
wall w1 from 0,0 to 8 m,0 thick 200
wall w2 from 0,0 to 0,6 m thick 200 left
opening o1 2 m in w1.left offset 1 m from grid.B
dim angle w1.left w2.left ref
point p at 1 m,3 m
dim p to grid.A = 1200
",
    );

    let mut s = Session::new();
    let v = run(
        &mut s,
        "\
line seat from 0,0 to 500,0
line back from 0,0 to 0,800
block chair of seat, back base seat.start   # Make block
place c1 chair at 2 m,0 angle 90            # a copy, turned 90°
place c2 chair at 4 m,0 mirror scale 0.5    # mirrored left to right, half size
explode c1                                  # c1 is curves again: c1[1], c1[2]
dim c1[1] ref                               # later lines can use them
",
    );
    let block = id(&v, "chair");
    for script in [
        "place c3 chair at 0,0".to_owned(),
        format!("place c3 {block} at 0,0"),
        format!("explode {} to group", id(&v, "chair.copy")),
    ] {
        run(&mut s, &script);
    }
}

/// The rest of docs/guide/ai/script.md's statements, each on geometry it fits (the page's
/// cheat sheets reuse names, so each runs in a drawing of its own).
#[test]
fn the_documented_statements_run() {
    for script in [
        "let T = 200\nroom bed 3.6 m x 3 m at 0,0 walls T centre fixed",
        "circle col r 200 at 0,0 fixed\narray studs of col 4 x 1 m up",
        "wall w3 from 8 m,0 to 8 m,6 m thick 200 length 6 m fixed",
        // (w2 drawn off square, so the angle has something to drive.)
        "wall w1 from 0,0 to 8 m,0 thick 200\nwall w2 from 0,0 to 600,6 m thick 200 left\n\
         dim angle w1.left w2.left = 90°",
        // (An arc drawn by its ends has no size of its own to clash with.)
        "arc col centre 0,0 from 200,0 to 0,200\ndim r col = 225",
        "arc col centre 0,0 from 200,0 to 0,200\ndim d col = 450",
    ] {
        run(&mut Session::new(), script);
    }
    let lines = "line a from 0,0 to 4 m,400\nline b from 0,1 m to 4 m,1.45 m\n\
                 line c from 0,2 m to 3 m,2.3 m\n";
    for relation in [
        "parallel a b",
        "perpendicular a b",
        "equal a b c",
        "collinear a b",
    ] {
        run(&mut Session::new(), &format!("{lines}{relation}"));
    }
    let shapes = "line a from 0,0 to 4 m,0\ncircle c r 500 at 2 m,520\n\
                  circle c1 r 500 at 6 m,0\ncircle c2 r 300 at 6.02 m,10\n\
                  line l from 0,3 m to 4 m,3.4 m\nline axis from 2 m,2 m to 2 m,6 m\n\
                  point p at 1 m,4 m\npoint q at 3.05 m,4.02 m\n";
    for relation in [
        "tangent a c",
        "concentric c1 c2",
        "horizontal l",
        "vertical axis",
        "horizontal p q          # two points level",
        "symmetric p q about axis",
        "fix p at 0,0",
    ] {
        run(&mut Session::new(), &format!("{shapes}{relation}"));
    }
    for (setup, relation) in [
        ("point p at 0,0\npoint q at 5,5\n", "coincident p q"),
        (
            "line l from 0,0 to 4 m,0\npoint p at 2 m,50\n",
            "midpoint p of l",
        ),
        ("circle c r 500 at 0,0\npoint p at 510,0\n", "on p c"),
    ] {
        run(&mut Session::new(), &format!("{setup}{relation}"));
    }
}

#[test]
fn plain_numbers_added_to_lengths_are_millimetres() {
    assert_eq!(
        lex::with_unit("W/2 + 100", "mm").as_deref(),
        Some("W/2 + 100 mm")
    );
    assert_eq!(
        lex::with_unit("2400 + T", "mm").as_deref(),
        Some("2400 mm + T")
    );
    assert_eq!(
        lex::with_unit("(W - 200) / 2", "mm").as_deref(),
        Some("(W - 200 mm) / 2")
    );
    assert_eq!(lex::with_unit("(W - 2*T) / 3", "mm"), None);
    assert_eq!(lex::with_unit("2 * W", "mm"), None);
    let mut s = Session::new();
    let v = run(
        &mut s,
        "let T = 200
let W = 3 m
let D = 2400 + T
rect r W/2 + 100 x D at 0,0 fixed",
    );
    assert!(close(line_len(&s, &id(&v, "r.south")), 1600.0));
    assert!(close(line_len(&s, &id(&v, "r.west")), 2600.0));
    // Still tied to the variables.
    run(&mut s, "let W = 4 m");
    assert!(close(line_len(&s, &id(&v, "r.south")), 2100.0));
}

#[test]
fn bare_numbers_are_in_the_drawings_units() {
    // Feet and inches: a plain number is inches, as when typed into Drift.
    let mut s = Session::new();
    ok(&mut s, "new_drawing", json!({ "units": "ft-in" }));
    let v = run(
        &mut s,
        "let T = 6\nroom bed 12' x 10' at 0,0 walls T fixed\ndoor d1 36 in bed.south centred\n\
         rect r 10 x 20 at 30', 0 fixed\nlet H = 4'\nline l from 50',0 to 60',0 length H/2 + 6",
    );
    assert_eq!(
        v["sketch"]["fully_defined"], false,
        "the line can still move"
    );
    assert!(v.get("warnings").is_none(), "{v}");
    assert_eq!(v["plain_numbers_in"], "in");
    let inch = 25.4;
    assert!(
        close(line_len(&s, &id(&v, "d1.start")), 6.0 * inch),
        "6\" walls"
    );
    assert!(
        close(line_len(&s, &id(&v, "bed.south[1]")), 54.0 * inch),
        "a 36\" door, centred"
    );
    assert!(close(line_len(&s, &id(&v, "r.south")), 10.0 * inch));
    assert!(
        close(line_len(&s, &id(&v, "l")), 30.0 * inch),
        "H/2 + 6 is 30\""
    );
    // Changing a variable keeps reading plain numbers in inches.
    run(&mut s, "let T = 8");
    assert!(close(line_len(&s, &id(&v, "d1.start")), 8.0 * inch));

    // Millimetres: a 36 mm door runs, with a warning.
    let mut s = Session::new();
    let v = run(
        &mut s,
        "room bed 3 m x 3 m at 0,0 walls 200 fixed\ndoor d1 36 in bed.south",
    );
    let w = v["warnings"][0]["warning"].as_str().unwrap_or_default();
    assert_eq!(v["warnings"][0]["line"], 2, "{v}");
    assert_eq!(v["plain_numbers_in"], "mm");
    assert!(w.contains("d1 is only 36"), "{w}");
    assert!(w.contains("write 36\""), "{w}");
}

#[test]
fn trim_parses_what_where_and_by_what() {
    match stmt("trim slab inside bay") {
        Stmt::Trim { what, inside, by } => {
            assert_eq!(
                (what.root.as_str(), inside, by.root.as_str()),
                ("slab", true, "bay")
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        stmt("trim bay outside slab"),
        Stmt::Trim { inside: false, .. }
    ));
    assert!(parse_err("trim slab bay").text().contains("inside"));
}

/// The areas of the drawing's closed regions, largest first.
fn region_areas(s: &mut Session) -> Vec<f64> {
    let mut a: Vec<f64> = s.editor.regions().iter().map(|r| r.area).collect();
    a.sort_by(|x, y| y.total_cmp(x));
    a
}

#[test]
fn two_trims_make_a_union_a_notch_or_the_overlap() {
    // A 6 × 4 m slab and a bay of radius 1.2 m centred on its east side.
    let shapes = "rect slab about 6 m x 4 m at 0,0\ncircle bay r 1.2 m at 6 m,2 m\n";
    let half = std::f64::consts::PI * 1200.0 * 1200.0 / 2.0;
    let slab = 6000.0 * 4000.0;
    for (lines, area) in [
        ("trim slab inside bay\ntrim bay inside slab", slab + half),
        ("trim slab inside bay\ntrim bay outside slab", slab - half),
        ("trim slab outside bay\ntrim bay outside slab", half),
    ] {
        let mut s = Session::new();
        let depth = s.editor.history.undo_depth();
        let v = run(&mut s, &format!("{shapes}{lines}"));
        let found = region_areas(&mut s);
        assert_eq!(found.len(), 1, "{lines}: {found:?} {v}");
        assert!(
            (found[0] - area).abs() < 1.0,
            "{lines}: {found:?} vs {area}"
        );
        assert!(v.get("warnings").is_none(), "{v}");
        assert_eq!(s.editor.history.undo_depth(), depth + 1, "one step");
    }
}

#[test]
fn trim_needs_a_closed_shape_and_warns_when_nothing_goes() {
    let mut s = Session::new();
    let e = run_err(
        &mut s,
        "rect slab about 6 m x 4 m at 0,0\nline l from 0,0 to 8 m,6 m\ntrim slab inside l",
    );
    assert!(
        e["error"]
            .as_str()
            .unwrap_or_default()
            .contains("isn't closed"),
        "{e}"
    );
    let v = run(
        &mut s,
        "rect slab about 6 m x 4 m at 0,0\ncircle round r 10 m at 3 m,2 m\ntrim slab outside round",
    );
    let w = v["warnings"][0]["warning"].as_str().unwrap_or_default();
    assert!(w.contains("nothing outside round"), "{v}");
}

// Blocks (#456).

/// A seat and a back, as two named lines.
const CHAIR: &str = "line seat from 0,0 to 500,0\nline back from 0,0 to 0,800\n";

fn instance(s: &Session, id: &str) -> drift_core::Instance {
    let id = drift_core::InstanceId(id[1..].parse().unwrap());
    *s.editor.doc.instance(id).unwrap()
}

fn curve_count(s: &Session) -> usize {
    s.editor
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .count()
}

#[test]
fn block_place_and_explode_parse() {
    match stmt("block chair of seat, back base seat.start") {
        Stmt::Block { name, of, base } => {
            assert_eq!(name, "chair");
            assert_eq!(
                of.iter().map(ToString::to_string).collect::<Vec<_>>(),
                ["seat", "back"]
            );
            assert!(matches!(base, Some(Pos::Ref(p)) if p.to_string() == "seat.start"));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        stmt("block chair of seat back"),
        Stmt::Block { of, base: None, .. } if of.len() == 2
    ));
    match stmt("place c1 chair at 2 m,0 angle 90 mirror scale 0.5") {
        Stmt::Place {
            name,
            block,
            at,
            angle,
            mirror,
            scale,
        } => {
            assert_eq!((name.as_str(), block.root.as_str()), ("c1", "chair"));
            assert!(matches!(at, Pos::At(x, y) if x.text == "2 m" && y.text == "0"));
            assert_eq!(angle.unwrap().text, "90");
            assert!(mirror);
            assert_eq!(scale.unwrap().text, "0.5");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        stmt("place c2 chair at p"),
        Stmt::Place {
            mirror: false,
            angle: None,
            scale: None,
            at: Pos::Ref(_),
            ..
        }
    ));
    assert!(matches!(
        stmt("explode c1"),
        Stmt::Explode {
            to_group: false,
            ..
        }
    ));
    assert!(matches!(
        stmt("explode c1 to group"),
        Stmt::Explode { to_group: true, .. }
    ));
    let e = parse_err("explode c1 to grup");
    assert!(e.text().contains("`group`"), "{}", e.text());
    let e = parse_err("place c1 chair 2 m,0");
    assert!(e.text().contains("`at`"), "{}", e.text());
}

#[test]
fn a_script_makes_a_block_from_named_geometry() {
    let mut s = Session::new();
    let v = run(
        &mut s,
        &format!("{CHAIR}block chair of seat, back base seat.start"),
    );
    assert!(id(&v, "chair").starts_with('b'), "{}", v["names"]);
    let (_, block) = s.editor.doc.blocks().next().expect("a block");
    assert_eq!(block.name, "chair");
    assert_eq!(block.items.len(), 2, "the seat and the back");
    // A copy takes the shapes' place, by the base point.
    let i = instance(&s, &id(&v, "chair.copy"));
    assert!(i.at.length() < 1e-9, "{:?}", i.at);
    assert!(
        v["names"].get("seat").is_none(),
        "seat is gone: {}",
        v["names"]
    );
    assert_eq!(curve_count(&s), 0, "the curves became the block");
}

#[test]
fn a_script_places_copies_turned_mirrored_and_scaled() {
    let mut s = Session::new();
    let v = run(
        &mut s,
        &format!(
            "{CHAIR}block chair of seat, back base seat.start\n\
             let A = 30°\n\
             place c1 chair at 2 m,1 m angle A mirror scale 0.5\n\
             point p at 4 m,0\n\
             place c2 chair at p"
        ),
    );
    let c1 = instance(&s, &id(&v, "c1"));
    assert!((c1.at - drift_core::DVec2::new(2000.0, 1000.0)).length() < 1e-9);
    assert!(close(c1.angle, 30.0), "{}", c1.angle);
    assert!(c1.mirror);
    assert!(close(c1.scale, 0.5), "{}", c1.scale);
    let c2 = instance(&s, &id(&v, "c2"));
    assert!((c2.at - drift_core::DVec2::new(4000.0, 0.0)).length() < 1e-9);
    assert!(close(c2.angle, 0.0) && !c2.mirror && close(c2.scale, 1.0));
    assert_eq!(s.editor.doc.instances().count(), 3, "chair.copy, c1 and c2");
}

#[test]
fn a_script_explodes_a_copy_into_named_curves() {
    let mut s = Session::new();
    let v = run(
        &mut s,
        &format!(
            "{CHAIR}block chair of seat, back base seat.start\n\
             place c1 chair at 2 m,0\n\
             explode c1\n\
             dim c1[1] ref\n\
             explode chair.copy"
        ),
    );
    assert!(id(&v, "c1[1]").starts_with('e'), "{}", v["names"]);
    assert!(id(&v, "c1[2]").starts_with('e'), "{}", v["names"]);
    assert!(v["names"].get("chair.copy").is_none(), "{}", v["names"]);
    assert_eq!(s.editor.doc.instances().count(), 0, "both copies exploded");
    assert_eq!(s.editor.doc.blocks().count(), 1, "the block stays");
    assert_eq!(curve_count(&s), 4);

    let mut s = Session::new();
    run(
        &mut s,
        &format!("{CHAIR}block chair of seat, back\nplace c1 chair at 2 m,0\nexplode c1 to group"),
    );
    let groups: Vec<_> = s.editor.doc.groups().collect();
    assert_eq!(groups.len(), 1, "one group");
    assert_eq!(groups[0].1.members.len(), 2);
}

#[test]
fn blocks_are_one_undo_step_and_all_or_nothing() {
    let mut s = Session::new();
    run(
        &mut s,
        &format!("{CHAIR}block chair of seat, back\nplace c1 chair at 2 m,0 angle 45\nexplode c1"),
    );
    let u = ok(&mut s, "undo", json!({}));
    assert_eq!(u["undone"], json!(["Assistant: script"]));
    assert!(s.editor.doc.is_empty(), "one undo removes it all");
    assert_eq!(s.editor.doc.blocks().count(), 0, "the block too");

    // A failure after the block is made keeps nothing, the block included.
    let before = s.editor.doc.clone();
    let e = run_err(
        &mut s,
        &format!("{CHAIR}block chair of seat, back\nplace c1 chair at 2 m,0\ndim c1 ref"),
    );
    let text = e["error"].as_str().unwrap();
    assert_eq!(e["line"], 5, "{text}");
    assert!(text.contains("explode it"), "{text}");
    assert_eq!(s.editor.doc, before);
    assert_eq!(s.editor.doc.blocks().count(), 0);
}

#[test]
fn block_names_are_checked_before_anything_runs() {
    let mut s = Session::new();
    for (script, says) in [
        (
            format!("{CHAIR}block chair of seat, back\ndim seat ref"),
            "became part of block chair on line 3",
        ),
        (
            format!("{CHAIR}place c1 seat at 0,0"),
            "is the line on line 1, not a block",
        ),
        ("place c1 sofa at 0,0".to_owned(), "isn't a block"),
        (
            format!("{CHAIR}block chair of seat, back\nexplode chair"),
            "explode a copy of it",
        ),
        (
            format!(
                "{CHAIR}block chair of seat, back\nplace c1 chair at 0,0\nexplode c1\nexplode c1"
            ),
            "already exploded on line 5",
        ),
        (
            format!("{CHAIR}block chair of seat, back\nplace c1 chair at 0,0\nblock two of c1"),
            "not of blocks",
        ),
        ("explode i9".to_owned(), "isn't a block copy"),
    ] {
        let e = run_err(&mut s, &script);
        let text = e["error"].as_str().unwrap();
        assert!(text.contains(says), "{script}\n{text}");
    }
    assert!(s.editor.doc.is_empty());
}

#[test]
fn a_later_script_places_and_explodes_by_the_drawings_names_and_ids() {
    let mut s = Session::new();
    let v = run(&mut s, &format!("{CHAIR}block chair of seat, back"));
    let copy = id(&v, "chair.copy");
    let v = run(&mut s, "place c1 chair at 3 m,0 scale 2");
    assert!(close(instance(&s, &id(&v, "c1")).scale, 2.0));
    run(&mut s, &format!("explode {copy}"));
    assert_eq!(s.editor.doc.instances().count(), 1, "c1 is left");
}
