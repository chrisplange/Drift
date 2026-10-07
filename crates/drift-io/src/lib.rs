//! The native `.drift` file format.
//!
//! A `.drift` file is UTF-8 JSON with one entity or constraint per line, so files diff and
//! merge cleanly in version control:
//!
//! ```json
//! {
//!   "format": "drift",
//!   "version": 5,
//!   "meta": {"title":"Bracket","units":"mm","generator":"Drift 0.1.0"},
//!   "display": {"length":"architectural","fraction":16},
//!   "variables": [
//!     {"id":"v1","name":"width","expr":"120 mm","kind":"length"}
//!   ],
//!   "entities": [
//!     {"type":"point","id":"e1","at":[0.0,0.0]},
//!     {"type":"point","id":"e2","at":[120.0,0.0]},
//!     {"type":"line","id":"e3","a":"e1","b":"e2"}
//!   ],
//!   "constraints": [
//!     {"type":"horizontal","id":"c1","line":"e3"},
//!     {"type":"distance","id":"c2","a":"e1","b":"e2","value":120.0,"expr":"width"}
//!   ]
//! }
//! ```
//!
//! `display` (how lengths are shown), `variables`, `measurements` (pinned, measured-only
//! dimensions) and `variants` (named sets of variable expressions) are written only when used.
//! Coordinates are always millimetres (`meta.units`), whatever the display units.
//!
//! Angles are stored in degrees, as the user typed them.
//!
//! Numbers use the shortest representation that round-trips exactly, so save → load is
//! bit-exact. Older versions are upgraded by [`migrate`] before parsing.

use std::collections::BTreeMap;

use drift_core::{
    Constraint, ConstraintId, DVec2, Document, DocumentError, Entity, EntityId, EntityKind, Kind,
    LengthFormat, LineKind, MeasurementId, Units, Variable, VariableId, Variant, VariantId,
};
use serde::{Deserialize, Serialize};

/// File extension, without the dot.
pub const EXTENSION: &str = "drift";
/// The version this build writes. Files with a higher version are rejected.
///
/// History: 1 = entities only (M1); 2 = adds `constraints` (M2); 3 = adds diameter,
/// horizontal/vertical and point–line distances, and dimension `label` offsets (M3);
/// 4 = adds tangent, concentric, coradial, collinear, point-on-curve and symmetric relations,
/// and `driven` dimensions (M4); 5 = adds `display` units, `variables`, dimension `expr`,
/// `measurements` and `variants` (M5).
pub const FORMAT_VERSION: u32 = 5;
const FORMAT_TAG: &str = "drift";

/// Document-level properties stored alongside the geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Length unit of all coordinates. Only `"mm"` is supported for now.
    #[serde(default = "default_units")]
    pub units: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
}

fn default_units() -> String {
    "mm".to_owned()
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            title: None,
            units: default_units(),
            generator: None,
        }
    }
}

#[derive(Debug)]
pub enum LoadError {
    Json(serde_json::Error),
    NotADriftFile,
    UnsupportedVersion(u32),
    UnsupportedUnits(String),
    BadId(String),
    MissingField { id: String, field: &'static str },
    UnknownConstraint(String),
    DuplicateId(String),
    NonFinite(String),
    Document(DocumentError),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Json(e) => write!(f, "invalid JSON: {e}"),
            LoadError::NotADriftFile => write!(f, "not a Drift file"),
            LoadError::UnsupportedVersion(v) => write!(
                f,
                "file version {v} is newer than this version of Drift supports ({FORMAT_VERSION}); \
                 please update Drift"
            ),
            LoadError::UnsupportedUnits(u) => write!(f, "unsupported units \"{u}\""),
            LoadError::BadId(id) => write!(f, "invalid entity id \"{id}\""),
            LoadError::MissingField { id, field } => write!(f, "{id} is missing \"{field}\""),
            LoadError::UnknownConstraint(kind) => write!(f, "unknown constraint type \"{kind}\""),
            LoadError::DuplicateId(id) => write!(f, "duplicate entity id \"{id}\""),
            LoadError::NonFinite(id) => write!(f, "entity {id} has a non-finite value"),
            LoadError::Document(e) => write!(f, "invalid geometry: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<serde_json::Error> for LoadError {
    fn from(e: serde_json::Error) -> Self {
        LoadError::Json(e)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Style {
    #[default]
    Solid,
    Construction,
    Center,
}

impl Style {
    fn is_solid(&self) -> bool {
        *self == Style::Solid
    }
}

impl From<LineKind> for Style {
    fn from(kind: LineKind) -> Self {
        match kind {
            LineKind::Solid => Style::Solid,
            LineKind::Construction => Style::Construction,
            LineKind::Center => Style::Center,
        }
    }
}

impl From<Style> for LineKind {
    fn from(style: Style) -> Self {
        match style {
            Style::Solid => LineKind::Solid,
            Style::Construction => LineKind::Construction,
            Style::Center => LineKind::Center,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum EntityRecord {
    Point {
        id: String,
        at: [f64; 2],
        #[serde(default, skip_serializing_if = "Style::is_solid")]
        style: Style,
    },
    Line {
        id: String,
        a: String,
        b: String,
        #[serde(default, skip_serializing_if = "Style::is_solid")]
        style: Style,
    },
    Circle {
        id: String,
        center: String,
        radius: f64,
        #[serde(default, skip_serializing_if = "Style::is_solid")]
        style: Style,
    },
    Arc {
        id: String,
        center: String,
        start: String,
        end: String,
        #[serde(default, skip_serializing_if = "Style::is_solid")]
        style: Style,
    },
}

/// One constraint, as a flat record: `type` and `id` plus whichever fields that type uses.
/// Field order is fixed (struct order), so files stay stable and diff cleanly.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstraintRecord {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    a: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    b: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    at: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    radius: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    degrees: Option<f64>,
    /// Dimension label offset from its anchor, in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<[f64; 2]>,
    /// A driven (reference) dimension: shown but not enforced.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    driven: bool,
    /// The expression the dimension's value comes from (the value is its last result).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expr: Option<String>,
}

/// How lengths and angles are displayed. Every field is optional (defaults apply).
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    length: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decimals: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fraction: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    zero_feet: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle_decimals: Option<u8>,
}

const LENGTH_FORMATS: [(LengthFormat, &str); 6] = [
    (LengthFormat::Millimetre, "mm"),
    (LengthFormat::Centimetre, "cm"),
    (LengthFormat::Metre, "m"),
    (LengthFormat::Inch, "in"),
    (LengthFormat::Foot, "ft"),
    (LengthFormat::Architectural, "architectural"),
];

impl DisplayRecord {
    /// Only the settings that differ from the defaults.
    fn from_units(u: Units) -> Self {
        fn diff<T: PartialEq>(a: T, b: T) -> Option<T> {
            (a != b).then_some(a)
        }
        let d = Units::default();
        Self {
            length: (u.length != d.length).then(|| {
                LENGTH_FORMATS
                    .iter()
                    .find(|(f, _)| *f == u.length)
                    .map_or("mm", |(_, n)| n)
                    .to_owned()
            }),
            decimals: diff(u.decimals, d.decimals),
            fraction: diff(u.fraction, d.fraction),
            zero_feet: diff(u.zero_feet, d.zero_feet),
            angle_decimals: diff(u.angle_decimals, d.angle_decimals),
        }
    }

    fn to_units(&self) -> Result<Units, LoadError> {
        let d = Units::default();
        let length = match &self.length {
            None => d.length,
            Some(name) => LENGTH_FORMATS
                .iter()
                .find(|(_, n)| n == name)
                .map(|(f, _)| *f)
                .ok_or_else(|| LoadError::UnsupportedUnits(name.clone()))?,
        };
        Ok(Units {
            length,
            decimals: self.decimals.unwrap_or(d.decimals).min(10),
            fraction: self
                .fraction
                .filter(|f| drift_core::units::FRACTIONS.contains(f))
                .unwrap_or(d.fraction),
            zero_feet: self.zero_feet.unwrap_or(d.zero_feet),
            angle_decimals: self.angle_decimals.unwrap_or(d.angle_decimals).min(10),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VariableRecord {
    id: String,
    name: String,
    expr: String,
    kind: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantRecord {
    id: String,
    name: String,
    /// Variable id → expression.
    values: BTreeMap<String, String>,
}

const KINDS: [(Kind, &str); 3] = [
    (Kind::Length, "length"),
    (Kind::Angle, "angle"),
    (Kind::Number, "number"),
];

fn kind_name(kind: Kind) -> &'static str {
    KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or("length", |(_, n)| n)
}

/// Parse a prefixed id like `v12`.
fn parse_prefixed(s: &str, prefix: char) -> Result<u32, LoadError> {
    s.strip_prefix(prefix)
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| LoadError::BadId(s.to_owned()))
}

fn constraint_record(
    id: ConstraintId,
    c: &Constraint,
    label: Option<DVec2>,
    driven: bool,
) -> ConstraintRecord {
    let s = |e: EntityId| Some(id_string(e));
    let mut r = ConstraintRecord {
        id: id.to_string(),
        label: label.map(|l| l.to_array()),
        driven,
        expr: None,
        ..Default::default()
    };
    let kind = match *c {
        Constraint::Coincident { a, b } => {
            (r.a, r.b) = (s(a), s(b));
            "coincident"
        }
        Constraint::Horizontal { line } => {
            r.line = s(line);
            "horizontal"
        }
        Constraint::Vertical { line } => {
            r.line = s(line);
            "vertical"
        }
        Constraint::HorizontalPoints { a, b } => {
            (r.a, r.b) = (s(a), s(b));
            "horizontal_points"
        }
        Constraint::VerticalPoints { a, b } => {
            (r.a, r.b) = (s(a), s(b));
            "vertical_points"
        }
        Constraint::Parallel { a, b } => {
            (r.a, r.b) = (s(a), s(b));
            "parallel"
        }
        Constraint::Perpendicular { a, b } => {
            (r.a, r.b) = (s(a), s(b));
            "perpendicular"
        }
        Constraint::Equal { a, b } => {
            (r.a, r.b) = (s(a), s(b));
            "equal"
        }
        Constraint::Tangent { a, b } => {
            (r.a, r.b) = (s(a), s(b));
            "tangent"
        }
        Constraint::Concentric { a, b } => {
            (r.a, r.b) = (s(a), s(b));
            "concentric"
        }
        Constraint::Coradial { a, b } => {
            (r.a, r.b) = (s(a), s(b));
            "coradial"
        }
        Constraint::Collinear { a, b } => {
            (r.a, r.b) = (s(a), s(b));
            "collinear"
        }
        Constraint::PointOnCurve { point, curve } => {
            (r.point, r.curve) = (s(point), s(curve));
            "point_on_curve"
        }
        Constraint::Symmetric { a, b, axis } => {
            (r.a, r.b, r.line) = (s(a), s(b), s(axis));
            "symmetric"
        }
        Constraint::Midpoint { point, line } => {
            (r.line, r.point) = (s(line), s(point));
            "midpoint"
        }
        Constraint::Fix { point, at } => {
            (r.point, r.at) = (s(point), Some(at.to_array()));
            "fix"
        }
        Constraint::FixRadius { curve, radius } => {
            (r.curve, r.radius) = (s(curve), Some(radius));
            "fix_radius"
        }
        Constraint::Distance { a, b, value } => {
            (r.a, r.b, r.value) = (s(a), s(b), Some(value));
            "distance"
        }
        Constraint::HorizontalDistance { a, b, value } => {
            (r.a, r.b, r.value) = (s(a), s(b), Some(value));
            "horizontal_distance"
        }
        Constraint::VerticalDistance { a, b, value } => {
            (r.a, r.b, r.value) = (s(a), s(b), Some(value));
            "vertical_distance"
        }
        Constraint::PointLineDistance { point, line, value } => {
            (r.point, r.line, r.value) = (s(point), s(line), Some(value));
            "point_line_distance"
        }
        Constraint::Radius { curve, value } => {
            (r.curve, r.value) = (s(curve), Some(value));
            "radius"
        }
        Constraint::Diameter { curve, value } => {
            (r.curve, r.value) = (s(curve), Some(value));
            "diameter"
        }
        Constraint::Angle { a, b, degrees } => {
            (r.a, r.b, r.degrees) = (s(a), s(b), Some(degrees));
            "angle"
        }
    };
    r.kind = kind.to_owned();
    r
}

fn parse_constraint_id(s: &str) -> Result<ConstraintId, LoadError> {
    s.strip_prefix('c')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .map(ConstraintId)
        .ok_or_else(|| LoadError::BadId(s.to_owned()))
}

/// A parsed constraint, its label offset (if any) and whether it is driven.
type ParsedConstraint = (ConstraintId, Constraint, Option<DVec2>, bool);

fn parse_constraint(r: ConstraintRecord) -> Result<ParsedConstraint, LoadError> {
    let missing = |field: &'static str| LoadError::MissingField {
        id: r.id.clone(),
        field,
    };
    let entity = |v: &Option<String>, field| -> Result<EntityId, LoadError> {
        parse_id(v.as_deref().ok_or_else(|| missing(field))?)
    };
    let number = |v: Option<f64>, field| v.ok_or_else(|| missing(field));
    let (a, b) = (|| entity(&r.a, "a"), || entity(&r.b, "b"));
    let constraint = match r.kind.as_str() {
        "coincident" => Constraint::Coincident { a: a()?, b: b()? },
        "horizontal" => Constraint::Horizontal {
            line: entity(&r.line, "line")?,
        },
        "vertical" => Constraint::Vertical {
            line: entity(&r.line, "line")?,
        },
        "horizontal_points" => Constraint::HorizontalPoints { a: a()?, b: b()? },
        "vertical_points" => Constraint::VerticalPoints { a: a()?, b: b()? },
        "parallel" => Constraint::Parallel { a: a()?, b: b()? },
        "perpendicular" => Constraint::Perpendicular { a: a()?, b: b()? },
        "equal" => Constraint::Equal { a: a()?, b: b()? },
        "tangent" => Constraint::Tangent { a: a()?, b: b()? },
        "concentric" => Constraint::Concentric { a: a()?, b: b()? },
        "coradial" => Constraint::Coradial { a: a()?, b: b()? },
        "collinear" => Constraint::Collinear { a: a()?, b: b()? },
        "point_on_curve" => Constraint::PointOnCurve {
            point: entity(&r.point, "point")?,
            curve: entity(&r.curve, "curve")?,
        },
        "symmetric" => Constraint::Symmetric {
            a: a()?,
            b: b()?,
            axis: entity(&r.line, "line")?,
        },
        "midpoint" => Constraint::Midpoint {
            point: entity(&r.point, "point")?,
            line: entity(&r.line, "line")?,
        },
        "fix" => Constraint::Fix {
            point: entity(&r.point, "point")?,
            at: DVec2::from(r.at.ok_or_else(|| missing("at"))?),
        },
        "fix_radius" => Constraint::FixRadius {
            curve: entity(&r.curve, "curve")?,
            radius: number(r.radius, "radius")?,
        },
        "distance" => Constraint::Distance {
            a: a()?,
            b: b()?,
            value: number(r.value, "value")?,
        },
        "horizontal_distance" => Constraint::HorizontalDistance {
            a: a()?,
            b: b()?,
            value: number(r.value, "value")?,
        },
        "vertical_distance" => Constraint::VerticalDistance {
            a: a()?,
            b: b()?,
            value: number(r.value, "value")?,
        },
        "point_line_distance" => Constraint::PointLineDistance {
            point: entity(&r.point, "point")?,
            line: entity(&r.line, "line")?,
            value: number(r.value, "value")?,
        },
        "radius" => Constraint::Radius {
            curve: entity(&r.curve, "curve")?,
            value: number(r.value, "value")?,
        },
        "diameter" => Constraint::Diameter {
            curve: entity(&r.curve, "curve")?,
            value: number(r.value, "value")?,
        },
        "angle" => Constraint::Angle {
            a: a()?,
            b: b()?,
            degrees: number(r.degrees, "degrees")?,
        },
        other => return Err(LoadError::UnknownConstraint(other.to_owned())),
    };
    let label = r.label.map(DVec2::from);
    if label.is_some_and(|l| !l.is_finite()) {
        return Err(LoadError::NonFinite(r.id));
    }
    Ok((parse_constraint_id(&r.id)?, constraint, label, r.driven))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRecord {
    // Validated by `migrate` before parsing; declared so `deny_unknown_fields` accepts them.
    #[allow(dead_code)]
    format: String,
    #[allow(dead_code)]
    version: u32,
    #[serde(default)]
    meta: Metadata,
    #[serde(default)]
    display: Option<DisplayRecord>,
    #[serde(default)]
    variables: Vec<VariableRecord>,
    #[serde(default)]
    entities: Vec<EntityRecord>,
    #[serde(default)]
    constraints: Vec<ConstraintRecord>,
    #[serde(default)]
    measurements: Vec<ConstraintRecord>,
    #[serde(default)]
    variants: Vec<VariantRecord>,
}

fn id_string(id: EntityId) -> String {
    id.to_string()
}

fn parse_id(s: &str) -> Result<EntityId, LoadError> {
    s.strip_prefix('e')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .map(EntityId)
        .ok_or_else(|| LoadError::BadId(s.to_owned()))
}

fn record(id: EntityId, entity: &Entity) -> EntityRecord {
    let style = entity.style.into();
    let id = id_string(id);
    match entity.kind {
        EntityKind::Point(at) => EntityRecord::Point {
            id,
            at: at.to_array(),
            style,
        },
        EntityKind::Line { a, b } => EntityRecord::Line {
            id,
            a: id_string(a),
            b: id_string(b),
            style,
        },
        EntityKind::Circle { center, radius } => EntityRecord::Circle {
            id,
            center: id_string(center),
            radius,
            style,
        },
        EntityKind::Arc { center, start, end } => EntityRecord::Arc {
            id,
            center: id_string(center),
            start: id_string(start),
            end: id_string(end),
            style,
        },
    }
}

/// Serialize a document to `.drift` JSON.
pub fn save(doc: &Document, meta: &Metadata) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"format\": \"{FORMAT_TAG}\",\n"));
    out.push_str(&format!("  \"version\": {FORMAT_VERSION},\n"));
    let meta = serde_json::to_string(meta).expect("metadata serializes");
    out.push_str(&format!("  \"meta\": {meta},\n"));
    if doc.units() != Units::default() {
        let display = serde_json::to_string(&DisplayRecord::from_units(doc.units()))
            .expect("display serializes");
        out.push_str(&format!("  \"display\": {display},\n"));
    }
    let variables: Vec<String> = doc
        .variables()
        .map(|(id, v)| {
            serde_json::to_string(&VariableRecord {
                id: id.to_string(),
                name: v.name.clone(),
                expr: v.expr.clone(),
                kind: kind_name(v.kind).to_owned(),
            })
            .expect("variable serializes")
        })
        .collect();
    if !variables.is_empty() {
        push_array(&mut out, "variables", &variables, true);
    }
    let entities: Vec<String> = doc
        .iter()
        .map(|(id, entity)| serde_json::to_string(&record(id, entity)).expect("entity serializes"))
        .collect();
    let constraints: Vec<String> = doc
        .constraints()
        .map(|(id, c)| {
            let mut r = constraint_record(id, c, doc.placement(id), doc.is_driven(id));
            r.expr = doc.expression(id).map(str::to_owned);
            serde_json::to_string(&r).expect("constraint serializes")
        })
        .collect();
    let measurements: Vec<String> = doc
        .measurements()
        .map(|(id, c)| {
            let mut r = constraint_record(ConstraintId(1), c, None, false);
            r.id = id.to_string();
            // Measurements have no value of their own.
            (r.value, r.degrees) = (None, None);
            serde_json::to_string(&r).expect("measurement serializes")
        })
        .collect();
    let variants: Vec<String> = doc
        .variants()
        .map(|(id, v)| {
            serde_json::to_string(&VariantRecord {
                id: id.to_string(),
                name: v.name.clone(),
                values: v
                    .values
                    .iter()
                    .map(|(var, e)| (var.to_string(), e.clone()))
                    .collect(),
            })
            .expect("variant serializes")
        })
        .collect();
    let tail = [("measurements", &measurements), ("variants", &variants)];
    let last = tail.iter().rposition(|(_, items)| !items.is_empty());
    push_array(&mut out, "entities", &entities, true);
    push_array(&mut out, "constraints", &constraints, last.is_some());
    for (i, (name, items)) in tail.iter().enumerate() {
        if !items.is_empty() {
            push_array(&mut out, name, items, Some(i) != last);
        }
    }
    out.push_str("}\n");
    out
}

/// Write `"name": [ ... ]` with one item per line.
fn push_array(out: &mut String, name: &str, items: &[String], more: bool) {
    out.push_str(&format!("  \"{name}\": ["));
    if items.is_empty() {
        out.push(']');
    } else {
        for (i, item) in items.iter().enumerate() {
            out.push_str(if i == 0 { "\n    " } else { ",\n    " });
            out.push_str(item);
        }
        out.push_str("\n  ]");
    }
    out.push_str(if more { ",\n" } else { "\n" });
}

/// Parse `.drift` JSON, upgrading older versions.
pub fn load(text: &str) -> Result<(Document, Metadata), LoadError> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let value = migrate(value)?;
    let file: FileRecord = serde_json::from_value(value)?;
    if file.meta.units != "mm" {
        return Err(LoadError::UnsupportedUnits(file.meta.units));
    }

    let mut entities = Vec::with_capacity(file.entities.len());
    let mut seen = std::collections::HashSet::new();
    for rec in file.entities {
        let (id_text, entity) = match rec {
            EntityRecord::Point { id, at, style } => {
                let entity = Entity {
                    kind: EntityKind::Point(at.into()),
                    style: style.into(),
                };
                (id, entity)
            }
            EntityRecord::Line { id, a, b, style } => {
                let kind = EntityKind::Line {
                    a: parse_id(&a)?,
                    b: parse_id(&b)?,
                };
                (
                    id,
                    Entity {
                        kind,
                        style: style.into(),
                    },
                )
            }
            EntityRecord::Circle {
                id,
                center,
                radius,
                style,
            } => {
                let kind = EntityKind::Circle {
                    center: parse_id(&center)?,
                    radius,
                };
                (
                    id,
                    Entity {
                        kind,
                        style: style.into(),
                    },
                )
            }
            EntityRecord::Arc {
                id,
                center,
                start,
                end,
                style,
            } => {
                let kind = EntityKind::Arc {
                    center: parse_id(&center)?,
                    start: parse_id(&start)?,
                    end: parse_id(&end)?,
                };
                (
                    id,
                    Entity {
                        kind,
                        style: style.into(),
                    },
                )
            }
        };
        let id = parse_id(&id_text)?;
        if !seen.insert(id) {
            return Err(LoadError::DuplicateId(id_text));
        }
        let finite = match entity.kind {
            EntityKind::Point(p) => p.is_finite(),
            EntityKind::Circle { radius, .. } => radius.is_finite() && radius > 0.0,
            _ => true,
        };
        if !finite {
            return Err(LoadError::NonFinite(id_text));
        }
        entities.push((id, entity));
    }
    let mut constraints = Vec::with_capacity(file.constraints.len());
    let mut seen_constraints = std::collections::HashSet::new();
    let mut labels = Vec::new();
    let mut driven = Vec::new();
    let mut exprs = Vec::new();
    for mut rec in file.constraints {
        let expr = rec.expr.take();
        let (id, c, label, is_driven) = parse_constraint(rec)?;
        if let Some(e) = expr {
            exprs.push((id, e));
        }
        if is_driven {
            driven.push(id);
        }
        if !seen_constraints.insert(id) {
            return Err(LoadError::DuplicateId(id.to_string()));
        }
        constraints.push((id, c));
        if let Some(label) = label {
            labels.push((id, label));
        }
    }
    let mut doc = Document::from_parts(entities, constraints).map_err(LoadError::Document)?;
    for (id, label) in labels {
        doc.set_placement(id, label).map_err(LoadError::Document)?;
    }
    for id in driven {
        doc.set_driven(id, true).map_err(LoadError::Document)?;
    }
    if let Some(display) = &file.display {
        doc.set_units(display.to_units()?);
    }
    for v in file.variables {
        let id = VariableId(parse_prefixed(&v.id, 'v')?);
        let kind = KINDS
            .iter()
            .find(|(_, n)| *n == v.kind)
            .map(|(k, _)| *k)
            .ok_or_else(|| LoadError::MissingField {
                id: v.id.clone(),
                field: "kind",
            })?;
        if doc.variable(id).is_some() {
            return Err(LoadError::DuplicateId(v.id));
        }
        doc.insert_variable(
            id,
            Variable {
                name: v.name,
                expr: v.expr,
                kind,
            },
        )
        .map_err(LoadError::Document)?;
    }
    for (id, e) in exprs {
        let is_dimension = doc.constraint(id).is_some_and(Constraint::is_dimension);
        if is_dimension && !doc.is_driven(id) {
            doc.set_expression(id, Some(e));
        }
    }
    for mut rec in file.measurements {
        let id_text = std::mem::replace(&mut rec.id, "c1".to_owned());
        let id = MeasurementId(parse_prefixed(&id_text, 'm')?);
        // Parse as a dimension with a placeholder value (measurements store none).
        if rec.kind == "angle" {
            rec.degrees = Some(1.0);
        } else {
            rec.value = Some(1.0);
        }
        let (_, c, ..) = parse_constraint(rec)?;
        doc.insert_measurement(id, c).map_err(LoadError::Document)?;
    }
    for v in file.variants {
        let id = VariantId(parse_prefixed(&v.id, 'k')?);
        let mut values = BTreeMap::new();
        for (var, e) in v.values {
            let var_id = VariableId(parse_prefixed(&var, 'v')?);
            if doc.variable(var_id).is_none() {
                return Err(LoadError::Document(DocumentError::MissingVariable(var_id)));
            }
            values.insert(var_id, e);
        }
        doc.set_variant(
            id,
            Variant {
                name: v.name,
                values,
            },
        );
    }
    Ok((doc, file.meta))
}

/// Upgrade a parsed file of any supported version to the current [`FORMAT_VERSION`].
pub fn migrate(value: serde_json::Value) -> Result<serde_json::Value, LoadError> {
    if value.get("format").and_then(|f| f.as_str()) != Some(FORMAT_TAG) {
        return Err(LoadError::NotADriftFile);
    }
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(LoadError::NotADriftFile)?;
    let version = u32::try_from(version).map_err(|_| LoadError::UnsupportedVersion(u32::MAX))?;
    if version == 0 || version > FORMAT_VERSION {
        return Err(LoadError::UnsupportedVersion(version));
    }
    let mut value = value;
    if version < 2 {
        // v1 → v2: constraints were introduced; a v1 file simply has none.
        value["constraints"] = serde_json::Value::Array(Vec::new());
    }
    // v2 → v5 only added optional fields, sections and constraint types: nothing to convert.
    value["version"] = FORMAT_VERSION.into();
    Ok(value)
}

#[cfg(test)]
mod tests {
    use drift_core::{DVec2, demo};

    use super::*;

    #[test]
    fn round_trip_is_exact() {
        let mut doc = demo::mounting_plate();
        // Values that are not exactly representable in decimal must still round-trip.
        let p = doc.add_point(DVec2::new(0.1 + 0.2, 1.0 / 3.0));
        let q = doc.add_point(DVec2::new(1.0e6 + 1e-9, -2.5e-12));
        doc.add_line(p, q).unwrap();
        let meta = Metadata {
            title: Some("Plate".into()),
            ..Default::default()
        };
        let text = save(&doc, &meta);
        let (loaded, loaded_meta) = load(&text).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(loaded_meta, meta);
        assert_eq!(save(&loaded, &loaded_meta), text, "saving is deterministic");
    }

    #[test]
    fn writes_one_entity_per_line() {
        let mut doc = Document::new();
        let a = doc.add_point(DVec2::new(0.0, 0.0));
        let b = doc.add_point(DVec2::new(10.5, 0.0));
        let l = doc.add_line(a, b).unwrap();
        doc.set_style(l, LineKind::Construction);
        doc.add_constraint(Constraint::Horizontal { line: l })
            .unwrap();
        doc.add_constraint(Constraint::Distance { a, b, value: 10.5 })
            .unwrap();
        let text = save(&doc, &Metadata::default());
        let expected = r#"{
  "format": "drift",
  "version": 5,
  "meta": {"units":"mm"},
  "entities": [
    {"type":"point","id":"e1","at":[0.0,0.0]},
    {"type":"point","id":"e2","at":[10.5,0.0]},
    {"type":"line","id":"e3","a":"e1","b":"e2","style":"construction"}
  ],
  "constraints": [
    {"type":"horizontal","id":"c1","line":"e3"},
    {"type":"distance","id":"c2","a":"e1","b":"e2","value":10.5}
  ]
}
"#;
        assert_eq!(text, expected);
    }

    #[test]
    fn default_document_ids_are_loadable() {
        // Regression: `Document::default()` used to hand out `e0`, which files reject.
        let mut doc = Document::default();
        doc.add_point(DVec2::new(1.0, 2.0));
        let (loaded, _) = load(&save(&doc, &Metadata::default())).unwrap();
        assert_eq!(loaded, doc);
    }

    #[test]
    fn empty_document_round_trips() {
        let text = save(&Document::new(), &Metadata::default());
        assert!(text.contains("\"entities\": [],"));
        assert!(text.contains("\"constraints\": []"));
        let (doc, _) = load(&text).unwrap();
        assert!(doc.is_empty());
    }

    #[test]
    fn every_constraint_kind_round_trips() {
        let mut doc = Document::new();
        let p = [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)]
            .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
        let l1 = doc.add_line(p[0], p[1]).unwrap();
        let l2 = doc.add_line(p[1], p[2]).unwrap();
        let l3 = doc.add_line(p[2], p[3]).unwrap();
        let c = doc.add_point(DVec2::new(20.0, 0.0));
        let circle = doc.add_circle(c, 2.5).unwrap();
        let c2 = doc.add_point(DVec2::new(30.0, 0.0));
        let circle2 = doc.add_circle(c2, 2.5).unwrap();
        for constraint in [
            Constraint::Coincident { a: p[3], b: c },
            Constraint::Horizontal { line: l1 },
            Constraint::Vertical { line: l2 },
            Constraint::HorizontalPoints { a: p[0], b: c },
            Constraint::VerticalPoints { a: p[2], b: p[1] },
            Constraint::Parallel { a: l1, b: l3 },
            Constraint::Perpendicular { a: l1, b: l2 },
            Constraint::Equal {
                a: circle,
                b: circle2,
            },
            Constraint::Fix {
                point: p[0],
                at: DVec2::new(0.0, 0.0),
            },
            Constraint::FixRadius {
                curve: circle,
                radius: 2.5,
            },
            Constraint::Distance {
                a: p[0],
                b: p[1],
                value: 10.0,
            },
            Constraint::Radius {
                curve: circle2,
                value: 2.5,
            },
            Constraint::Angle {
                a: l1,
                b: l2,
                degrees: 90.0,
            },
            Constraint::Diameter {
                curve: circle,
                value: 5.0,
            },
            Constraint::HorizontalDistance {
                a: p[0],
                b: p[2],
                value: 10.0,
            },
            Constraint::VerticalDistance {
                a: p[0],
                b: p[2],
                value: 5.0,
            },
            Constraint::PointLineDistance {
                point: p[3],
                line: l1,
                value: 5.0,
            },
            Constraint::Midpoint { point: c, line: l3 },
            Constraint::Tangent { a: l1, b: circle },
            Constraint::Concentric {
                a: circle,
                b: circle2,
            },
            Constraint::Coradial {
                a: circle,
                b: circle2,
            },
            Constraint::Collinear { a: l1, b: l3 },
            Constraint::PointOnCurve {
                point: p[3],
                curve: circle,
            },
            Constraint::Symmetric {
                a: p[0],
                b: p[3],
                axis: l2,
            },
        ] {
            let id = doc.add_constraint(constraint).unwrap();
            if constraint.is_dimension() {
                doc.set_placement(id, DVec2::new(1.5, -2.25)).unwrap();
            }
            if matches!(constraint, Constraint::Diameter { .. }) {
                doc.set_driven(id, true).unwrap();
            }
        }
        let text = save(&doc, &Metadata::default());
        let (loaded, _) = load(&text).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(loaded.constraint_count(), 24);
        assert!(text.contains(r#""label":[1.5,-2.25]"#));
        assert!(text.contains(r#""driven":true"#));
    }

    #[test]
    fn parameters_round_trip() {
        let mut doc = Document::new();
        let p = doc.add_point(DVec2::ZERO);
        let q = doc.add_point(DVec2::new(10.0, 0.0));
        let r = doc.add_point(DVec2::new(10.0, 5.0));
        let line = doc.add_line(p, q).unwrap();
        let other = doc.add_line(q, r).unwrap();
        let d = doc
            .add_constraint(Constraint::Distance {
                a: p,
                b: q,
                value: 10.0,
            })
            .unwrap();
        let w = doc
            .add_variable(Variable {
                name: "width".into(),
                expr: "10 mm".into(),
                kind: Kind::Length,
            })
            .unwrap();
        doc.add_variable(Variable {
            name: "tilt".into(),
            expr: "30 deg".into(),
            kind: Kind::Angle,
        })
        .unwrap();
        doc.set_expression(d, Some("width".into()));
        doc.add_measurement(Constraint::Angle {
            a: line,
            b: other,
            degrees: 0.0,
        })
        .unwrap();
        doc.add_measurement(Constraint::Distance {
            a: p,
            b: r,
            value: 0.0,
        })
        .unwrap();
        doc.add_variant(Variant {
            name: "Small".into(),
            values: [(w, "5 mm".to_owned())].into(),
        });
        doc.set_units(Units {
            length: LengthFormat::Architectural,
            fraction: 32,
            ..Units::default()
        });
        let text = save(&doc, &Metadata::default());
        assert!(text.contains(r#""display": {"length":"architectural","fraction":32}"#));
        assert!(text.contains(r#""expr":"width""#));
        let (loaded, _) = load(&text).unwrap();
        // Measurements store no value, so compare the rest field by field.
        assert_eq!(loaded.units(), doc.units());
        assert_eq!(
            loaded.variables().collect::<Vec<_>>(),
            doc.variables().collect::<Vec<_>>()
        );
        assert_eq!(loaded.expression(d), Some("width"));
        assert_eq!(
            loaded.variants().collect::<Vec<_>>(),
            doc.variants().collect::<Vec<_>>()
        );
        assert_eq!(loaded.measurements().count(), 2);
        assert_eq!(save(&loaded, &Metadata::default()), text);
        // New ids don't collide with loaded ones.
        let mut loaded = loaded;
        let v = loaded
            .add_variable(Variable {
                name: "h".into(),
                expr: "1 mm".into(),
                kind: Kind::Length,
            })
            .unwrap();
        assert!(loaded.variables().filter(|(id, _)| *id == v).count() == 1);
        assert!(v.0 > 5);
    }

    #[test]
    fn version_2_files_still_load() {
        let v2 = r#"{"format":"drift","version":2,"entities":[
            {"type":"point","id":"e1","at":[0,0]},{"type":"point","id":"e2","at":[3,4]},
            {"type":"line","id":"e3","a":"e1","b":"e2"}],
            "constraints":[{"type":"distance","id":"c1","a":"e1","b":"e2","value":5.0}]}"#;
        let (doc, _) = load(v2).unwrap();
        assert_eq!(doc.constraint_count(), 1);
    }

    #[test]
    fn reports_missing_fields_and_unknown_types() {
        let missing = r#"{"format":"drift","version":3,"entities":[
            {"type":"point","id":"e1","at":[0,0]}],
            "constraints":[{"type":"distance","id":"c1","a":"e1","value":5.0}]}"#;
        assert!(matches!(
            load(missing),
            Err(LoadError::MissingField { field: "b", .. })
        ));
        let unknown = r#"{"format":"drift","version":3,"entities":[],
            "constraints":[{"type":"tangent_ish","id":"c1"}]}"#;
        assert!(matches!(
            load(unknown),
            Err(LoadError::UnknownConstraint(_))
        ));
    }

    #[test]
    fn version_1_files_still_load() {
        let v1 = r#"{"format":"drift","version":1,"meta":{"units":"mm"},"entities":[
            {"type":"point","id":"e1","at":[1.5,2.0]}]}"#;
        let (doc, _) = load(v1).unwrap();
        assert_eq!(doc.len(), 1);
        assert_eq!(doc.constraint_count(), 0);
    }

    #[test]
    fn rejects_constraints_on_the_wrong_kind_of_entity() {
        let bad = r#"{"format":"drift","version":2,"entities":[
            {"type":"point","id":"e1","at":[0,0]}],
            "constraints":[{"type":"horizontal","id":"c1","line":"e1"}]}"#;
        assert!(matches!(load(bad), Err(LoadError::Document(_))));
    }

    #[test]
    fn rejects_newer_versions_and_foreign_files() {
        let newer = r#"{"format":"drift","version":99,"entities":[]}"#;
        assert!(matches!(
            load(newer),
            Err(LoadError::UnsupportedVersion(99))
        ));
        assert!(matches!(
            load(r#"{"hello":1}"#),
            Err(LoadError::NotADriftFile)
        ));
        assert!(matches!(load("not json"), Err(LoadError::Json(_))));
    }

    #[test]
    fn rejects_dangling_and_duplicate_references() {
        let dangling = r#"{"format":"drift","version":1,"entities":[
            {"type":"point","id":"e1","at":[0,0]},
            {"type":"line","id":"e2","a":"e1","b":"e9"}]}"#;
        assert!(matches!(load(dangling), Err(LoadError::Document(_))));
        let duplicate = r#"{"format":"drift","version":1,"entities":[
            {"type":"point","id":"e1","at":[0,0]},
            {"type":"point","id":"e1","at":[1,0]}]}"#;
        assert!(matches!(load(duplicate), Err(LoadError::DuplicateId(_))));
        let bad_id = r#"{"format":"drift","version":1,"entities":[
            {"type":"point","id":"p1","at":[0,0]}]}"#;
        assert!(matches!(load(bad_id), Err(LoadError::BadId(_))));
    }

    #[test]
    fn new_entities_after_load_get_fresh_ids() {
        let text = r#"{"format":"drift","version":1,"entities":[
            {"type":"point","id":"e7","at":[0,0]}]}"#;
        let (mut doc, _) = load(text).unwrap();
        let id = doc.add_point(DVec2::ONE);
        assert_eq!(id, EntityId(8));
    }
}
