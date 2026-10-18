//! Looking at the drawing: what's in it, its constraints, and measurements.

use drift_core::{Constraint, ConstraintId, DVec2, Document, EntityId, EntityKind, Kind};
use serde_json::{Map, Value, json};

use super::args::{self, num, pt};
use super::{Res, Session};

/// Everything about one entity: its kind, layer, defining points and sizes.
pub(super) fn describe_entity(doc: &Document, id: EntityId) -> Value {
    let Some(e) = doc.get(id) else {
        return json!({ "id": id.to_string(), "missing": true });
    };
    let p = |id: EntityId| doc.point(id).unwrap_or_default();
    let mut o = Map::new();
    o.insert("id".into(), args::id(id));
    o.insert("kind".into(), json!(args::kind_name(&e.kind)));
    let mut put = |k: &str, v: Value| {
        o.insert(k.into(), v);
    };
    match e.kind {
        EntityKind::Point(at) => {
            put("at", pt(at));
            let users: Vec<Value> = doc.users_of(id).map(args::id).collect();
            put("used_by", json!(users));
        }
        EntityKind::Line { a, b } => {
            let (pa, pb) = (p(a), p(b));
            put("a", args::id(a));
            put("b", args::id(b));
            put("from", pt(pa));
            put("to", pt(pb));
            put("length", num(pa.distance(pb)));
            put("angle", num((pb - pa).to_angle().to_degrees()));
        }
        EntityKind::Circle { center, radius } => {
            put("center", args::id(center));
            put("center_at", pt(p(center)));
            put("radius", num(radius));
        }
        EntityKind::Arc { center, start, end } => {
            let (c, s, t) = (p(center), p(start), p(end));
            let a0 = (s - c).to_angle();
            let sweep = ((t - c).to_angle() - a0).rem_euclid(std::f64::consts::TAU);
            put("center", args::id(center));
            put("start", args::id(start));
            put("end", args::id(end));
            put("center_at", pt(c));
            put("start_at", pt(s));
            put("end_at", pt(t));
            put("radius", num(c.distance(s)));
            put("start_angle", num(a0.to_degrees()));
            put("sweep", num(sweep.to_degrees()));
        }
        EntityKind::Ellipse {
            center,
            major,
            minor,
        } => {
            put("center", args::id(center));
            put("major", args::id(major));
            put("minor", args::id(minor));
            put("center_at", pt(p(center)));
            put("major_radius", num(p(center).distance(p(major))));
            put("minor_radius", num(p(center).distance(p(minor))));
        }
        EntityKind::EllipseArc {
            center,
            major,
            minor,
            start,
            end,
        } => {
            put("center", args::id(center));
            put("major", args::id(major));
            put("minor", args::id(minor));
            put("start", args::id(start));
            put("end", args::id(end));
            put("center_at", pt(p(center)));
            put("start_at", pt(p(start)));
            put("end_at", pt(p(end)));
        }
        EntityKind::Bezier { a, c1, c2, b } => {
            put("a", args::id(a));
            put("c1", args::id(c1));
            put("c2", args::id(c2));
            put("b", args::id(b));
            put("points", json!([pt(p(a)), pt(p(c1)), pt(p(c2)), pt(p(b))]));
        }
    }
    if !e.kind.is_point() {
        put("style", json!(args::style_name(e.style)));
    }
    let layer = doc.layer_of(id);
    put("layer", json!(format!("{} ({})", e.layer, layer.name)));
    Value::Object(o)
}

/// A constraint's type, the entities it refers to by role, and its value.
pub(super) fn describe_constraint(doc: &Document, id: ConstraintId) -> Value {
    let Some(c) = doc.constraint(id) else {
        return json!({ "id": id.to_string(), "missing": true });
    };
    let mut o = Map::new();
    o.insert("id".into(), json!(id.to_string()));
    o.insert("type".into(), json!(type_name(c)));
    let e = |id: EntityId| args::id(id);
    let refs: Vec<(&str, Value)> = match *c {
        Constraint::Coincident { a, b }
        | Constraint::HorizontalPoints { a, b }
        | Constraint::VerticalPoints { a, b }
        | Constraint::Parallel { a, b }
        | Constraint::Perpendicular { a, b }
        | Constraint::Equal { a, b }
        | Constraint::Tangent { a, b }
        | Constraint::Concentric { a, b }
        | Constraint::Coradial { a, b }
        | Constraint::Collinear { a, b }
        | Constraint::Distance { a, b, .. }
        | Constraint::HorizontalDistance { a, b, .. }
        | Constraint::VerticalDistance { a, b, .. }
        | Constraint::Angle { a, b, .. } => vec![("a", e(a)), ("b", e(b))],
        Constraint::Horizontal { line } | Constraint::Vertical { line } => {
            vec![("line", e(line))]
        }
        Constraint::PointOnCurve { point, curve } => {
            vec![("point", e(point)), ("curve", e(curve))]
        }
        Constraint::Symmetric { a, b, axis } => vec![("a", e(a)), ("b", e(b)), ("axis", e(axis))],
        Constraint::Smooth { point, a, b } => vec![("point", e(point)), ("a", e(a)), ("b", e(b))],
        Constraint::Midpoint { point, line } => vec![("point", e(point)), ("line", e(line))],
        Constraint::Fix { point, at } => vec![("point", e(point)), ("at", pt(at))],
        Constraint::FixRadius { curve, radius } => {
            vec![("curve", e(curve)), ("radius", num(radius))]
        }
        Constraint::Radius { curve, .. } | Constraint::Diameter { curve, .. } => {
            vec![("curve", e(curve))]
        }
        Constraint::PointLineDistance { point, line, .. } => {
            vec![("point", e(point)), ("line", e(line))]
        }
    };
    for (k, v) in refs {
        o.insert(k.into(), v);
    }
    if let Some(v) = c.value() {
        o.insert("value".into(), num(v));
        o.insert(
            "shown".into(),
            json!(match drift_core::dimension_kind(c) {
                Kind::Length => doc.units().format_length(v),
                kind => doc.units().format(kind, v),
            }),
        );
        if let Some(expr) = doc.expression(id) {
            o.insert("expression".into(), json!(expr));
        }
        o.insert("driven".into(), json!(doc.is_driven(id)));
    }
    Value::Object(o)
}

/// The snake-case name the tools use for a constraint type.
pub(super) fn type_name(c: &Constraint) -> &'static str {
    match c {
        Constraint::Coincident { .. } => "coincident",
        Constraint::Horizontal { .. } | Constraint::HorizontalPoints { .. } => "horizontal",
        Constraint::Vertical { .. } | Constraint::VerticalPoints { .. } => "vertical",
        Constraint::Parallel { .. } => "parallel",
        Constraint::Perpendicular { .. } => "perpendicular",
        Constraint::Equal { .. } => "equal",
        Constraint::Tangent { .. } => "tangent",
        Constraint::Concentric { .. } => "concentric",
        Constraint::Coradial { .. } => "coradial",
        Constraint::Collinear { .. } => "collinear",
        Constraint::PointOnCurve { .. } => "on_curve",
        Constraint::Symmetric { .. } => "symmetric",
        Constraint::Smooth { .. } => "smooth",
        Constraint::Midpoint { .. } => "midpoint",
        Constraint::Fix { .. } | Constraint::FixRadius { .. } => "fix",
        Constraint::Distance { .. } => "distance",
        Constraint::PointLineDistance { .. } => "point_line",
        Constraint::Radius { .. } => "radius",
        Constraint::Diameter { .. } => "diameter",
        Constraint::HorizontalDistance { .. } => "horizontal_distance",
        Constraint::VerticalDistance { .. } => "vertical_distance",
        Constraint::Angle { .. } => "angle",
    }
}

pub(super) fn info(s: &mut Session) -> Res {
    let doc = &s.editor.doc;
    let units = doc.units();
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for (_, e) in doc.iter() {
        *counts.entry(args::kind_name(&e.kind)).or_default() += 1;
    }
    let (dims, rels) = doc.constraints().fold((0, 0), |(d, r), (_, c)| {
        if c.is_dimension() {
            (d + 1, r)
        } else {
            (d, r + 1)
        }
    });
    let layers: Vec<Value> = doc
        .layers()
        .map(|(id, l)| {
            json!({
                "id": id.to_string(), "name": l.name, "color": l.color.map(|c| c.hex()),
                "weight_mm": l.weight.mm(), "line_type": l.line_type.name(),
                "visible": l.visible, "locked": l.locked,
                "current": id == doc.current_layer(),
            })
        })
        .collect();
    let eval = s.editor.evaluation();
    let variables: Vec<Value> = doc
        .variables()
        .map(|(id, v)| {
            let value = match eval.value(doc, id) {
                Some(Ok(x)) => num(x),
                Some(Err(e)) => json!(format!("error: {}", e.0)),
                None => Value::Null,
            };
            json!({ "name": v.name, "expression": v.expr, "kind": v.kind.label(), "value": value })
        })
        .collect();
    let b = s.editor.bounds();
    let bounds =
        (!b.is_empty()).then(|| json!([num(b.min.x), num(b.min.y), num(b.max.x), num(b.max.y)]));
    Ok(json!({
        "file": s.path.as_deref().map(super::files::shown),
        "coordinates": "millimetres (y up); angles in degrees, counter-clockwise from +x",
        "display_units": units.length.label(),
        "drawing_scale": format!("1:{}", doc.draft_style().scale),
        "bounds": bounds,
        "entities": counts,
        "relations": rels,
        "dimensions": dims,
        "fills": doc.fills().count(),
        "texts": doc.texts().count(),
        "sheets": doc.sheets().map(|(id, sh)| json!({"id": id.to_string(), "name": sh.name})).collect::<Vec<_>>(),
        "underlays": doc.underlays().map(|(id, u)| json!({"id": id.to_string(), "name": u.name})).collect::<Vec<_>>(),
        "layers": layers,
        "variables": variables,
        "undo": s.editor.history.undo_label(),
        "sketch": s.status(),
    }))
}

pub(super) fn list_entities(s: &mut Session, a: &Value) -> Res {
    let doc = &s.editor.doc;
    let kind = args::opt_str(a, "kind")?;
    let layer = args::opt_layer(doc, a, "layer")?;
    let within = match a.get("within") {
        Some(Value::Array(v)) if v.len() == 4 => {
            let n: Vec<f64> = v.iter().filter_map(Value::as_f64).collect();
            if n.len() != 4 {
                return Err("`within` is [min_x, min_y, max_x, max_y]".into());
            }
            Some((
                DVec2::new(n[0].min(n[2]), n[1].min(n[3])),
                DVec2::new(n[0].max(n[2]), n[1].max(n[3])),
            ))
        }
        None | Some(Value::Null) => None,
        _ => return Err("`within` is [min_x, min_y, max_x, max_y]".into()),
    };
    let points = args::opt_bool(a, "include_points")?.unwrap_or(false) || kind == Some("point");
    let limit = args::opt_f64(a, "limit")?.unwrap_or(200.0).max(1.0) as usize;
    let ids: Option<Vec<EntityId>> = match a.get("ids") {
        Some(Value::Array(_)) => Some(args::entities_at(doc, a, "ids")?),
        _ => None,
    };
    let mut out = Vec::new();
    let mut total = 0;
    let candidates: Vec<EntityId> = match ids {
        Some(ids) => ids,
        None => doc.iter().map(|(id, _)| id).collect(),
    };
    for id in candidates {
        let Some(e) = doc.get(id) else { continue };
        if (e.kind.is_point() && !points && a.get("ids").is_none())
            || kind.is_some_and(|k| k != args::kind_name(&e.kind))
            || layer.is_some_and(|l| l != e.layer)
        {
            continue;
        }
        if let Some((lo, hi)) = within {
            let Some(shape) = doc.shape(id) else { continue };
            let b = shape.bounds();
            if b.max.x < lo.x || b.min.x > hi.x || b.max.y < lo.y || b.min.y > hi.y {
                continue;
            }
        }
        total += 1;
        if out.len() < limit {
            out.push(describe_entity(doc, id));
        }
    }
    Ok(json!({ "count": total, "entities": out, "truncated": total > out.len() }))
}

pub(super) fn list_constraints(s: &mut Session, a: &Value) -> Res {
    let doc = &s.editor.doc;
    let ids: Vec<ConstraintId> = match a.get("entities") {
        Some(Value::Array(_)) => {
            let ents = args::entities_at(doc, a, "entities")?;
            let mut ids: Vec<ConstraintId> =
                ents.iter().flat_map(|e| doc.constraints_on(*e)).collect();
            ids.sort();
            ids.dedup();
            ids
        }
        _ => doc.constraints().map(|(id, _)| id).collect(),
    };
    let only = args::opt_str(a, "type")?;
    let analysis = s.editor.analysis();
    let list: Vec<Value> = ids
        .into_iter()
        .filter(|id| only.is_none_or(|t| doc.constraint(*id).is_some_and(|c| type_name(c) == t)))
        .map(|id| {
            let mut v = describe_constraint(doc, id);
            if analysis.conflicts.contains(&id) {
                v["conflict"] = json!(true);
            }
            if analysis.redundant.contains(&id) {
                v["redundant"] = json!(true);
            }
            v
        })
        .collect();
    Ok(json!({ "count": list.len(), "constraints": list }))
}

/// Where a measurement argument is: a point entity or coordinate, or a curve.
enum Target {
    At(DVec2),
    Curve(EntityId),
}

fn target(doc: &Document, v: &Value) -> Result<Target, String> {
    if v.is_array() || v.is_object() {
        return args::vec2(v).map(Target::At);
    }
    let id = args::entity(doc, v)?;
    match doc.get(id).map(|e| e.kind) {
        Some(EntityKind::Point(p)) => Ok(Target::At(p)),
        _ => Ok(Target::Curve(id)),
    }
}

pub(super) fn measure(s: &mut Session, a: &Value) -> Res {
    let doc = &s.editor.doc;
    let units = doc.units();
    let length = |mm: f64| json!({ "mm": num(mm), "shown": units.format_length(mm) });
    let line_of = |id: EntityId| match doc.get(id).map(|e| e.kind) {
        Some(EntityKind::Line { a, b }) => Some((doc.point(a).ok()?, doc.point(b).ok()?)),
        _ => None,
    };
    // The area of the closed region around a point.
    if let Some(p) = args::opt_vec2(a, "inside")? {
        let regions = s.editor.regions();
        let r = drift_geom::regions::region_at(&regions, p)
            .ok_or("no closed area there")?
            .clone();
        let units = s.editor.doc.units();
        return Ok(json!({
            "area_mm2": num(r.area), "area_m2": num(r.area / 1e6),
            "area": units.format_area(r.area),
            "perimeter": num(r.perimeter), "centroid": pt(r.centroid),
        }));
    }
    let first = target(doc, a.get("a").ok_or("`a` is required")?)?;
    let second = match a.get("b") {
        None | Some(Value::Null) => None,
        Some(v) => Some(target(doc, v)?),
    };
    Ok(match (first, second) {
        (Target::At(p), None) => json!({ "at": pt(p) }),
        (Target::Curve(id), None) => {
            let shape = doc.shape(id).ok_or("can't measure that")?;
            let mut v = json!({ "length": length(shape_length(&shape)) });
            if let Some(r) = drift_solver::radius_of(doc, id) {
                v["radius"] = length(r);
            }
            if let Some((p, q)) = line_of(id) {
                v["angle"] = num((q - p).to_angle().to_degrees());
            }
            v
        }
        (Target::At(p), Some(Target::At(q))) => json!({
            "distance": length(p.distance(q)),
            "dx": length(q.x - p.x), "dy": length(q.y - p.y),
        }),
        (Target::At(p), Some(Target::Curve(c))) | (Target::Curve(c), Some(Target::At(p))) => {
            match line_of(c) {
                Some((u, w)) => {
                    let d = (w - u).normalize_or_zero();
                    json!({ "distance_to_line": length((p - u).perp_dot(d).abs()) })
                }
                None => {
                    let r = drift_solver::radius_of(doc, c)
                        .ok_or("measure to a line, circle or arc")?;
                    let center = match doc.get(c).map(|e| e.kind) {
                        Some(
                            EntityKind::Circle { center, .. } | EntityKind::Arc { center, .. },
                        ) => doc.point(center).unwrap_or_default(),
                        _ => return Err("measure to a line, circle or arc".into()),
                    };
                    json!({ "distance_to_circle": length((p.distance(center) - r).abs()), "distance_to_center": length(p.distance(center)) })
                }
            }
        }
        (Target::Curve(x), Some(Target::Curve(y))) => match (line_of(x), line_of(y)) {
            (Some((p, q)), Some((u, w))) => {
                let (d1, d2) = (q - p, w - u);
                let angle = d1.angle_to(d2).to_degrees().rem_euclid(180.0);
                let parallel = d1.normalize().perp_dot(d2.normalize()).abs() < 1e-9;
                let mut v = json!({ "angle": num(angle) });
                if parallel {
                    v["distance"] = length((u - p).perp_dot(d1.normalize()).abs());
                }
                v
            }
            _ => {
                return Err("between two curves, only two lines can be measured (an angle)".into());
            }
        },
    })
}

fn shape_length(shape: &drift_core::Shape) -> f64 {
    let pts = args::polyline(shape);
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}
