//! Drift script: intent written once, by name, instead of many tool calls.
//!
//! ```text
//! let W = 3.6 m
//! room bed W x 1.25*W at 0,0 walls 200 fixed
//! door d1 900 in bed.south centred
//! ```
//!
//! A script is parsed and its names checked before anything changes. Then each statement is
//! lowered to the ordinary tools (`add_geometry`, `add_relations`, `add_dimensions`, `offset`,
//! …) and run in order, the ids they return bound to the script's names. The whole script is
//! one undo step, and all or nothing, exactly as a `batch` of those calls would be;
//! `explain` returns that batch.

mod errors;
mod lex;
mod names;
mod parse;
mod run;

use serde_json::{Value, json};

use super::{Session, ToolOutput};

/// The tool's description: the language in brief.
pub const DESCRIPTION: &str = "\
Run a Drift script: geometry, relations and dimensions written by name, as one undo step (all or nothing). \
Prefer it to separate calls for building or changing a drawing. One statement per line; `#` comments; `\\` continues a line.
Values: 3.6 m, 360 cm, 3600 mm, 12'6\", 6 1/2\", 30°, or expressions of variables (W/2 + 100). A plain number is in the drawing's \
unit, as when typed into Drift: mm in a metric drawing, inches in a feet-and-inches one (see drawing_info), also when added to a length; \
write units when in doubt. Dimensions keep the text, so they follow variables. \
Positions are rough (`0,0` or `(W, 0)`) or a named point (`bed.ne`); the solver makes them exact.
  let W = 3.6 m                               variable (add `number` or `angle` for other kinds)
  layer Walls color #333333 weight 0.5 line center   |   on Walls   |   any statement ending `on Walls`
  point p at 0,0 [fixed]
  [construction] line l from p to 4200,0 [length 4.2 m] [fixed]
  [construction] polyline site (0,0) (30 m,0) (30 m,20 m) closed
  [construction] rect r [about] 4200 x 3600 at 0,0 [fixed]    (`centred at` for the centre; sizes become dimensions unless `about`)
  [construction] circle c r 225 at 0,0 [fixed]    (or `d 450`)
  [construction] arc a centre 0,0 from 1000,0 to 0,1000
  room bed W x D at 0,0 [walls 200 [outside|inside|centre]] [fixed]   inside size W x D
  room bath 2400 x D right of bed walls 200    (left of / above / below: shares the wall between them)
  wall w1 from 0,0 to 8 m,0 thick 200 [centre|left|right] [length 8 m] [fixed]
  door d1 900 in bed.south [centred | offset 600 from bed.sw]    (window, opening: the same)
  grid cols A..E every 6 m length 30 m [at 0,0] [fixed]    (rows 1..4; `spacing 6 m, 7.5 m` for uneven)
  array cols of col 5 x 6 m [, 4 x 7.2 m] [up]   |   array a of x 4 by 1000,500   |   array ring of x 8 around 0,0 [angle 180]
  parallel a b | perpendicular a b | equal a b [c…] | collinear a b | tangent a b | concentric a b | coincident p q
  horizontal l (or p q) | vertical l | symmetric p q about axis | midpoint p of l | on p curve | fix p [at 0,0]
  dim bed.south = 3.6 m | dim a to b = 1200 [horizontal|vertical] | dim r c = 225 | dim d c = 450 | dim angle a b = 30° | dim x ref
  label bed \"Bedroom 1\" [height 250]   |   text \"N\" at 0,12 m [height 250] [align centre] [angle 90]
  hatch bed.walls ANSI31 [scale 2] [angle 45]   |   fill bed #f0f0f0 [opacity 0.5]
  use e12 as north_wall    (existing geometry; raw ids like e12 also work wherever a name does)
Parts: line .start .end; rect/room .south .east .north .west (.bottom .right .top .left), .sw .se .ne .nw, .width .depth, .inside; \
room with walls also .outer.<side or corner>, .walls; wall .left .right .start .end (.axis when centred); door .start .end (jambs); \
grid .A .3; circle .center; arc .center .start .end; array [1] … [n] (then .center); polyline [k] .start .end. \
After an opening, a face is in pieces: bed.south[2]. y is up: north is +y.
The result lists every name's id, each line's reading, skipped relations, warnings (such as a door only millimetres wide), room areas and the sketch's degrees of freedom. \
Errors give the line, what it was read as, and change nothing.";

/// The tool's arguments.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "script": { "type": "string", "description": "The Drift script" },
            "check": { "type": "boolean", "description": "Run it, report, and keep nothing (a dry run)" },
            "explain": { "type": "boolean", "description": "Also return the tool calls it made, as a batch takes them" },
            "render": { "type": "boolean", "description": "Attach a render of the result" }
        },
        "required": ["script"]
    })
}

fn failed(json: Value) -> ToolOutput {
    ToolOutput {
        json,
        image: None,
        is_error: true,
    }
}

/// `run_script`.
pub(super) fn call(s: &mut Session, a: &Value) -> ToolOutput {
    let Some(text) = a.get("script").and_then(Value::as_str) else {
        return failed(json!({ "error": "`script` (the script's text) is required" }));
    };
    let flag = |k: &str| a.get(k).and_then(Value::as_bool).unwrap_or(false);
    let (check, explain, render) = (flag("check"), flag("explain"), flag("render"));
    let statements = match parse::parse(text) {
        Ok(st) => st,
        Err(e) => return failed(e.json()),
    };
    if let Err(e) = names::check(&statements, &s.editor.doc) {
        return failed(e.json());
    }
    let before = s.editor.history.recorded();
    let result = {
        let mut r = run::Runner::new(s);
        r.sources = statements
            .iter()
            .map(|st| (st.line, st.source.clone()))
            .collect();
        let mut outcome = Ok(());
        for st in &statements {
            if let Err(m) = r.statement(st) {
                outcome = Err(r.error(st, m));
                break;
            }
            r.did
                .push(json!({ "line": st.line, "did": run::meant(&st.stmt) }));
        }
        outcome.map(|()| {
            let mut v = json!({
                "names": r.names(),
                "lines": r.did,
                "rooms": r.rooms(),
            });
            if !r.skipped.is_empty() {
                v["skipped"] = json!(r.skipped);
            }
            if !r.warnings.is_empty() {
                v["warnings"] = json!(r.warnings);
            }
            if explain {
                v["calls"] = json!(r.calls);
            }
            v
        })
    };
    let label = "Assistant: script";
    let keep = result.is_ok() && !check;
    if s.editor.doc.in_transaction() {
        s.editor.doc.rollback();
    }
    let n = (s.editor.history.recorded() - before) as usize;
    let status = s.status();
    {
        let ed = &mut s.editor;
        if keep {
            ed.history.squash_last(n, label);
        } else {
            ed.history.discard_last(n, &mut ed.doc);
            ed.notice = None;
        }
        ed.sync();
    }
    match result {
        Ok(mut v) => {
            if check {
                v["checked"] = json!("it runs; nothing was kept");
                v["sketch"] = status;
            } else {
                v["sketch"] = s.status();
            }
            let mut image = None;
            if render && !check {
                let out = s.call("render", &json!({}));
                image = out.image;
            }
            ToolOutput {
                json: v,
                image,
                is_error: false,
            }
        }
        Err(e) => failed(e.json()),
    }
}

#[cfg(test)]
mod tests;
