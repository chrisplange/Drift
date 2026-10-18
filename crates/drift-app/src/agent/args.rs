//! Reading tool arguments, and writing ids and numbers into results.

use drift_core::{
    ConstraintId, DVec2, Document, EntityId, EntityKind, FillId, LayerId, LineKind, TextId,
};
use serde_json::{Value, json};

pub(super) fn opt_f64(a: &Value, key: &str) -> Result<Option<f64>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .map(Some)
            .ok_or(format!("`{key}` must be a number")),
    }
}

pub(super) fn f64_(a: &Value, key: &str) -> Result<f64, String> {
    opt_f64(a, key)?.ok_or(format!("`{key}` is required"))
}

pub(super) fn opt_str<'a>(a: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_str()
            .map(Some)
            .ok_or(format!("`{key}` must be a string")),
    }
}

pub(super) fn str_<'a>(a: &'a Value, key: &str) -> Result<&'a str, String> {
    opt_str(a, key)?.ok_or(format!("`{key}` is required"))
}

pub(super) fn opt_bool(a: &Value, key: &str) -> Result<Option<bool>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_bool()
            .map(Some)
            .ok_or(format!("`{key}` must be true or false")),
    }
}

pub(super) fn array<'a>(a: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    a.get(key)
        .and_then(Value::as_array)
        .ok_or(format!("`{key}` must be an array"))
}

/// A coordinate: `[x, y]` or `{"x": .., "y": ..}`.
pub(super) fn vec2(v: &Value) -> Result<DVec2, String> {
    let pair = match v {
        Value::Array(xs) if xs.len() == 2 => (xs[0].as_f64(), xs[1].as_f64()),
        Value::Object(o) => (
            o.get("x").and_then(Value::as_f64),
            o.get("y").and_then(Value::as_f64),
        ),
        _ => (None, None),
    };
    match pair {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok(DVec2::new(x, y)),
        _ => Err(format!("expected a coordinate [x, y], got {v}")),
    }
}

pub(super) fn vec2_at(a: &Value, key: &str) -> Result<DVec2, String> {
    vec2(a.get(key).ok_or(format!("`{key}` is required"))?).map_err(|e| format!("`{key}`: {e}"))
}

pub(super) fn opt_vec2(a: &Value, key: &str) -> Result<Option<DVec2>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => vec2(v).map(Some).map_err(|e| format!("`{key}`: {e}")),
    }
}

/// The number in an id such as `e12` (or a bare number).
fn number_of(v: &Value, prefix: char) -> Option<u32> {
    if let Some(n) = v.as_u64() {
        return u32::try_from(n).ok();
    }
    let s = v.as_str()?.trim();
    s.strip_prefix(prefix)
        .unwrap_or(s)
        .parse()
        .ok()
        .filter(|_| s.starts_with(prefix) || s.chars().all(|c| c.is_ascii_digit()))
}

/// An entity id (`"e12"`) that exists.
pub(super) fn entity(doc: &Document, v: &Value) -> Result<EntityId, String> {
    let id =
        EntityId(number_of(v, 'e').ok_or(format!("expected an entity id like \"e12\", got {v}"))?);
    if doc.contains(id) {
        Ok(id)
    } else {
        Err(format!("{id} does not exist"))
    }
}

pub(super) fn entity_at(doc: &Document, a: &Value, key: &str) -> Result<EntityId, String> {
    entity(doc, a.get(key).ok_or(format!("`{key}` is required"))?)
        .map_err(|e| format!("`{key}`: {e}"))
}

/// A point entity id.
pub(super) fn point_at(doc: &Document, a: &Value, key: &str) -> Result<EntityId, String> {
    let id = entity_at(doc, a, key)?;
    if doc.get(id).is_some_and(|e| e.kind.is_point()) {
        Ok(id)
    } else {
        Err(format!("`{key}`: {id} is not a point"))
    }
}

pub(super) fn entities_at(doc: &Document, a: &Value, key: &str) -> Result<Vec<EntityId>, String> {
    array(a, key)?
        .iter()
        .map(|v| entity(doc, v).map_err(|e| format!("`{key}`: {e}")))
        .collect()
}

pub(super) fn constraint(doc: &Document, v: &Value) -> Result<ConstraintId, String> {
    let id = ConstraintId(
        number_of(v, 'c').ok_or(format!("expected a constraint id like \"c4\", got {v}"))?,
    );
    doc.constraint(id)
        .map(|_| id)
        .ok_or(format!("{id} does not exist"))
}

/// A layer by id (`"l2"`) or by name.
pub(super) fn layer(doc: &Document, v: &Value) -> Result<LayerId, String> {
    if let Some(name) = v.as_str()
        && let Some((id, _)) = doc
            .layers()
            .find(|(_, l)| l.name.eq_ignore_ascii_case(name))
    {
        return Ok(id);
    }
    let id = LayerId(number_of(v, 'l').ok_or(format!("no layer {v}"))?);
    doc.layer(id).map(|_| id).ok_or(format!("no layer {v}"))
}

pub(super) fn opt_layer(doc: &Document, a: &Value, key: &str) -> Result<Option<LayerId>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => layer(doc, v).map(Some),
    }
}

/// What an id names, by its prefix.
pub(super) enum AnyId {
    Entity(EntityId),
    Constraint(ConstraintId),
    Fill(FillId),
    Text(TextId),
}

pub(super) fn any_id(doc: &Document, v: &Value) -> Result<AnyId, String> {
    let s = v.as_str().unwrap_or_default();
    match s.chars().next() {
        Some('c') => constraint(doc, v).map(AnyId::Constraint),
        Some('f') => {
            let id = FillId(number_of(v, 'f').ok_or(format!("bad id {v}"))?);
            doc.fill(id)
                .map(|_| AnyId::Fill(id))
                .ok_or(format!("{id} does not exist"))
        }
        Some('t') => {
            let id = TextId(number_of(v, 't').ok_or(format!("bad id {v}"))?);
            doc.text(id)
                .map(|_| AnyId::Text(id))
                .ok_or(format!("{id} does not exist"))
        }
        _ => entity(doc, v).map(AnyId::Entity),
    }
}

/// A number rounded for reading (to a millionth), without `-0`.
pub(super) fn num(x: f64) -> Value {
    let r = (x * 1e6).round() / 1e6;
    json!(if r == 0.0 { 0.0 } else { r })
}

pub(super) fn pt(p: DVec2) -> Value {
    json!([num(p.x), num(p.y)])
}

pub(super) fn id(e: EntityId) -> Value {
    json!(e.to_string())
}

/// A lowercase name for an entity kind.
pub(super) fn kind_name(kind: &EntityKind) -> &'static str {
    match kind {
        EntityKind::Point(_) => "point",
        EntityKind::Line { .. } => "line",
        EntityKind::Circle { .. } => "circle",
        EntityKind::Arc { .. } => "arc",
        EntityKind::Ellipse { .. } => "ellipse",
        EntityKind::EllipseArc { .. } => "ellipse_arc",
        EntityKind::Bezier { .. } => "bezier",
    }
}

pub(super) fn style_name(style: LineKind) -> &'static str {
    match style {
        LineKind::Solid => "solid",
        LineKind::Construction => "construction",
        LineKind::Center => "centre_line",
    }
}

/// A curve as connected points, fine enough to measure and draw.
pub(super) fn polyline(shape: &drift_core::Shape) -> Vec<DVec2> {
    use drift_core::Shape;
    let arc = |c: DVec2, r: f64, start: f64, sweep: f64| -> Vec<DVec2> {
        let n = ((sweep.abs() / std::f64::consts::TAU) * 128.0)
            .ceil()
            .max(8.0) as usize;
        (0..=n)
            .map(|i| c + DVec2::from_angle(start + sweep * i as f64 / n as f64) * r)
            .collect()
    };
    match *shape {
        Shape::Point(p) => vec![p],
        Shape::Line { a, b } => vec![a, b],
        Shape::Circle { center, radius } => arc(center, radius, 0.0, std::f64::consts::TAU),
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => arc(center, radius, start, sweep),
        Shape::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => drift_core::shape::flatten_ellipse(center, major, minor, start, sweep, 16),
        Shape::Bezier { p } => drift_core::shape::flatten_bezier(&p, 16),
    }
}
