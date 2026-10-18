//! Intent: relations, dimensions and variables.

use drift_core::{Constraint, ConstraintId, DVec2, Document, EntityId, EntityKind, Kind};
use serde_json::{Value, json};

use super::args;
use super::{Res, Session};
use crate::editor::EditError;

fn line_ends(doc: &Document, line: EntityId) -> Result<(EntityId, EntityId), String> {
    match doc.get(line).map(|e| e.kind) {
        Some(EntityKind::Line { a, b }) => Ok((a, b)),
        _ => Err(format!("{line} is not a line")),
    }
}

/// A relation from its JSON description.
fn relation(doc: &Document, r: &Value) -> Result<Constraint, String> {
    let t = args::str_(r, "type")?;
    let e = |k: &str| args::entity_at(doc, r, k);
    let p = |k: &str| args::point_at(doc, r, k);
    Ok(match t {
        "coincident" => Constraint::Coincident {
            a: p("a")?,
            b: p("b")?,
        },
        "horizontal" | "vertical" => {
            let h = t == "horizontal";
            if r.get("line").is_some() {
                let line = e("line")?;
                line_ends(doc, line)?;
                if h {
                    Constraint::Horizontal { line }
                } else {
                    Constraint::Vertical { line }
                }
            } else {
                let (a, b) = (p("a")?, p("b")?);
                if h {
                    Constraint::HorizontalPoints { a, b }
                } else {
                    Constraint::VerticalPoints { a, b }
                }
            }
        }
        "parallel" => Constraint::Parallel {
            a: e("a")?,
            b: e("b")?,
        },
        "perpendicular" => Constraint::Perpendicular {
            a: e("a")?,
            b: e("b")?,
        },
        "equal" => Constraint::Equal {
            a: e("a")?,
            b: e("b")?,
        },
        "tangent" => Constraint::Tangent {
            a: e("a")?,
            b: e("b")?,
        },
        "concentric" => Constraint::Concentric {
            a: e("a")?,
            b: e("b")?,
        },
        "coradial" => Constraint::Coradial {
            a: e("a")?,
            b: e("b")?,
        },
        "collinear" => Constraint::Collinear {
            a: e("a")?,
            b: e("b")?,
        },
        "on_curve" => Constraint::PointOnCurve {
            point: p("point")?,
            curve: e("curve")?,
        },
        "symmetric" => Constraint::Symmetric {
            a: p("a")?,
            b: p("b")?,
            axis: e("axis")?,
        },
        "midpoint" => Constraint::Midpoint {
            point: p("point")?,
            line: e("line")?,
        },
        "fix" => {
            let point = p("point")?;
            let at = match args::opt_vec2(r, "at")? {
                Some(at) => at,
                None => doc.point(point).map_err(|e| e.to_string())?,
            };
            Constraint::Fix { point, at }
        }
        other => return Err(format!("unknown relation `{other}`")),
    })
}

pub(super) fn relations(s: &mut Session, a: &Value) -> Res {
    let list = args::array(a, "relations")?;
    let mut added = Vec::new();
    let mut skipped = Vec::new();
    for (i, r) in list.iter().enumerate() {
        let c = relation(&s.editor.doc, r).map_err(|e| format!("relations[{i}]: {e}"))?;
        match s.editor.add_constraints(c.type_name(), &[c]) {
            Ok(ids) => added.extend(ids.iter().map(|id| json!(id.to_string()))),
            // Already true: nothing to add, and no harm in asking.
            Err(EditError::Redundant) => skipped.push(json!({
                "index": i, "reason": "already implied by existing constraints"
            })),
            Err(e) => {
                return Err(format!(
                    "relations[{i}] ({}): {e}",
                    r.get("type").and_then(Value::as_str).unwrap_or("?")
                ));
            }
        }
    }
    let mut v = json!({ "added": added });
    if !skipped.is_empty() {
        v["skipped"] = json!(skipped);
    }
    Ok(v)
}

/// A dimension from its JSON description, with a placeholder value.
fn dimension(doc: &Document, d: &Value) -> Result<Constraint, String> {
    let t = args::str_(d, "type")?;
    let e = |k: &str| args::entity_at(doc, d, k);
    // Ends of a distance: points, or lines (their midpoints); `line` alone measures its
    // ends.
    let ends = || -> Result<(EntityId, EntityId), String> {
        if d.get("line").is_some() && d.get("a").is_none() {
            line_ends(doc, e("line")?)
        } else {
            Ok((e("a")?, e("b")?))
        }
    };
    Ok(match t {
        "distance" | "length" => {
            let (a, b) = ends()?;
            let is_line =
                |id: EntityId| matches!(doc.get(id).map(|e| e.kind), Some(EntityKind::Line { .. }));
            // A point and a line: the distance to the line, not to its midpoint.
            if t == "distance" && d.get("line").is_none() {
                match (is_line(a), is_line(b)) {
                    (false, true) => Constraint::PointLineDistance {
                        point: a,
                        line: b,
                        value: 1.0,
                    },
                    (true, false) => Constraint::PointLineDistance {
                        point: b,
                        line: a,
                        value: 1.0,
                    },
                    (true, true) => {
                        return Err("for the distance between two parallel lines use type `point_line` from an end of one to the other".into());
                    }
                    _ => Constraint::Distance { a, b, value: 1.0 },
                }
            } else {
                Constraint::Distance { a, b, value: 1.0 }
            }
        }
        "horizontal" => {
            let (a, b) = ends()?;
            Constraint::HorizontalDistance { a, b, value: 1.0 }
        }
        "vertical" => {
            let (a, b) = ends()?;
            Constraint::VerticalDistance { a, b, value: 1.0 }
        }
        "point_line" => Constraint::PointLineDistance {
            point: e("point")?,
            line: e("line")?,
            value: 1.0,
        },
        "radius" => Constraint::Radius {
            curve: e("curve")?,
            value: 1.0,
        },
        "diameter" => Constraint::Diameter {
            curve: e("curve")?,
            value: 1.0,
        },
        "angle" => Constraint::Angle {
            a: e("a")?,
            b: e("b")?,
            degrees: 1.0,
        },
        other => return Err(format!("unknown dimension `{other}`")),
    })
}

/// A dimension's wanted value: a number (mm or degrees) or typed text (units, variables,
/// expressions, as typed in Drift).
enum Wanted {
    Number(f64),
    Text(String),
}

fn wanted(v: Option<&Value>) -> Result<Option<Wanted>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(Some(Wanted::Number(n.as_f64().unwrap_or_default()))),
        Some(Value::String(s)) => Ok(Some(Wanted::Text(s.clone()))),
        Some(other) => Err(format!("`value` must be a number or text, got {other}")),
    }
}

/// Set a driving dimension to a value, unlinking it from any expression.
fn set_value(s: &mut Session, id: ConstraintId, w: &Wanted) -> Result<(), String> {
    let ed = &mut s.editor;
    match w {
        Wanted::Text(t) => ed.set_dimension_text(id, t).map_err(|e| e.to_string()),
        Wanted::Number(v) => {
            if ed.doc.expression(id).is_some() {
                ed.doc.begin();
                ed.doc.set_expression(id, None);
                let tx = ed.doc.commit("Unlink");
                ed.history.push(tx);
            }
            ed.set_dimension(id, *v).map_err(|e| e.to_string())
        }
    }
}

/// Where a linear dimension's label goes when none is asked for: beside what it measures,
/// on the side away from the middle of the sketch, clear of the geometry.
fn outside(s: &Session, c: &Constraint) -> Option<DVec2> {
    let doc = &s.editor.doc;
    let anchor = crate::annotations::dimension_anchor(doc, c)?;
    let b = s.editor.bounds();
    if b.is_empty() {
        return None;
    }
    let away = anchor - b.center();
    let gap = (0.08 * (b.max - b.min).max_element()).max(10.0 * doc.draft_style().scale);
    let side = |x: f64| if x < 0.0 { -1.0 } else { 1.0 };
    let p = |id| doc.ref_point(id);
    let offset = match *c {
        Constraint::Distance { a, b, .. } => {
            let n = (p(b)? - p(a)?).normalize_or_zero().perp();
            n * gap * side(n.dot(away))
        }
        Constraint::HorizontalDistance { .. } => DVec2::new(0.0, gap * side(away.y)),
        Constraint::VerticalDistance { .. } => DVec2::new(gap * side(away.x), 0.0),
        _ => return None,
    };
    Some(crate::annotations::centred_offset(doc, c, offset))
}

pub(super) fn dimensions(s: &mut Session, a: &Value) -> Res {
    let list = args::array(a, "dimensions")?;
    let mut added = Vec::new();
    for (i, d) in list.iter().enumerate() {
        let at = |e: String| format!("dimensions[{i}]: {e}");
        let c = dimension(&s.editor.doc, d).map_err(at)?;
        let value = wanted(d.get("value")).map_err(at)?;
        let driven = args::opt_bool(d, "driven").map_err(at)?.unwrap_or(false);
        let placement = match args::opt_vec2(d, "label_offset").map_err(at)? {
            Some(p) => Some(p),
            None => outside(s, &c),
        };
        // Added at what it measures now (nothing moves), then set to the value wanted.
        let measured = drift_solver::measure(&s.editor.doc, &c)
            .ok_or_else(|| at("those entities can't be measured that way".into()))?;
        let c = c.with_value(measured);
        let id = if driven {
            s.editor
                .add_driven_dimension(c, placement)
                .map_err(|e| at(e.to_string()))?
        } else {
            match s.editor.add_dimension(c, placement) {
                Ok(id) => id,
                Err(e @ (EditError::Redundant | EditError::Conflict)) => {
                    s.editor.pending_driven = None;
                    return Err(at(format!(
                        "{e}: the sketch is already defined there (add it with \"driven\": true to show it as a reference)"
                    )));
                }
                Err(e) => return Err(at(e.to_string())),
            }
        };
        if let (Some(w), false) = (&value, driven) {
            set_value(s, id, w).map_err(at)?;
        }
        added.push(super::inspect::describe_constraint(&s.editor.doc, id));
    }
    Ok(json!({ "added": added }))
}

pub(super) fn set_dimension(s: &mut Session, a: &Value) -> Res {
    let id = args::constraint(&s.editor.doc, a.get("id").ok_or("`id` is required")?)?;
    if !s
        .editor
        .doc
        .constraint(id)
        .is_some_and(Constraint::is_dimension)
    {
        return Err(format!("{id} is a relation, not a dimension"));
    }
    if let Some(driven) = args::opt_bool(a, "driven")? {
        s.editor.set_driven(id, driven).map_err(|e| e.to_string())?;
    }
    if let Some(w) = wanted(a.get("value"))? {
        set_value(s, id, &w)?;
    }
    Ok(json!({ "dimension": super::inspect::describe_constraint(&s.editor.doc, id) }))
}

pub(super) fn remove(s: &mut Session, a: &Value) -> Res {
    let ids: Vec<ConstraintId> = args::array(a, "ids")?
        .iter()
        .map(|v| args::constraint(&s.editor.doc, v))
        .collect::<Result<_, _>>()?;
    let ed = &mut s.editor;
    ed.doc.begin();
    for id in &ids {
        ed.doc.remove_constraint(*id);
    }
    ed.commit("Remove constraints");
    Ok(json!({ "removed": ids.iter().map(|c| c.to_string()).collect::<Vec<_>>() }))
}

fn kind(v: Option<&str>) -> Result<Kind, String> {
    Ok(match v.unwrap_or("length") {
        "length" => Kind::Length,
        "angle" => Kind::Angle,
        "number" => Kind::Number,
        other => {
            return Err(format!(
                "unknown variable kind `{other}` (length, angle or number)"
            ));
        }
    })
}

pub(super) fn variables(s: &mut Session, a: &Value) -> Res {
    let empty = Vec::new();
    let set = match a.get("set") {
        Some(_) => args::array(a, "set")?,
        None => &empty,
    };
    for (i, v) in set.iter().enumerate() {
        let at = |e: String| format!("set[{i}]: {e}");
        let name = args::str_(v, "name").map_err(at)?;
        let existing = s.editor.doc.variable_named(name);
        let k = match (args::opt_str(v, "kind").map_err(at)?, existing) {
            (None, Some(id)) => s.editor.doc.variable(id).map_or(Kind::Length, |v| v.kind),
            (k, _) => kind(k).map_err(at)?,
        };
        let text = match v.get("value") {
            Some(Value::Number(n)) => s
                .editor
                .doc
                .units()
                .literal(k, n.as_f64().unwrap_or_default()),
            Some(Value::String(t)) => t.clone(),
            _ => return Err(at("`value` (a number or an expression) is required".into())),
        };
        match existing {
            Some(id) => {
                if s.editor.doc.variable(id).is_some_and(|v| v.kind != k) {
                    s.editor
                        .set_variable_kind(id, k)
                        .map_err(|e| at(e.to_string()))?;
                }
                s.editor
                    .set_variable(id, &text)
                    .map_err(|e| at(e.to_string()))?;
            }
            None => {
                s.editor
                    .add_variable(name, &text, k)
                    .map_err(|e| at(e.to_string()))?;
            }
        }
    }
    if let Some(Value::Array(names)) = a.get("remove") {
        for n in names {
            let name = n.as_str().ok_or("`remove` lists variable names")?;
            let id = s
                .editor
                .doc
                .variable_named(name)
                .ok_or(format!("no variable `{name}`"))?;
            s.editor
                .remove_variable(id)
                .map_err(|e| format!("{name}: {e}"))?;
        }
    }
    let doc = &s.editor.doc;
    let eval = s.editor.evaluation();
    let vars: Vec<Value> = doc
        .variables()
        .map(|(id, v)| {
            let value = match eval.value(doc, id) {
                Some(Ok(x)) => args::num(x),
                Some(Err(e)) => json!(format!("error: {}", e.0)),
                None => Value::Null,
            };
            json!({ "name": v.name, "expression": v.expr, "value": value })
        })
        .collect();
    Ok(json!({ "variables": vars }))
}
