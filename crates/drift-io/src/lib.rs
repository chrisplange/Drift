//! The native `.drift` file format.
//!
//! A `.drift` file is UTF-8 JSON with one entity or constraint per line, so files diff and
//! merge cleanly in version control:
//!
//! ```json
//! {
//!   "format": "drift",
//!   "version": 24,
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
    Appearance, Constraint, ConstraintId, DVec2, Document, DocumentError, Entity, EntityId,
    EntityKind, Kind, Layer, LayerId, LengthFormat, LineKind, LineType, LineWeight, MeasurementId,
    Rgb, Units, Variable, VariableId, Variant, VariantId,
};
use serde::{Deserialize, Serialize};

mod blocks;
mod drafting;
pub mod dxf;
mod dxf_import;
mod fonts;
pub mod pdf;
pub mod pdf_import;
mod sheets;
pub mod svg;
mod texts;
mod underlays;

/// File extension, without the dot.
pub const EXTENSION: &str = "drift";
/// The version this build writes. Files with a higher version are rejected.
///
/// History: 1 = entities only (M1); 2 = adds `constraints` (M2); 3 = adds diameter,
/// horizontal/vertical and point–line distances, and dimension `label` offsets (M3);
/// 4 = adds tangent, concentric, coradial, collinear, point-on-curve and symmetric relations,
/// and `driven` dimensions (M4); 5 = adds `display` units, `variables`, dimension `expr`,
/// `measurements` and `variants` (M5); 6 = adds `layers`, `current_layer`, and per-entity
/// `layer`, `color`, `weight` (mm) and `linetype` (M6); 7 = adds `drafting` (the dimension
/// style), `fills` (solid fills and hatches), `annotations` (notes, centre marks, callouts,
/// ordinate dimensions) and dimension `prefix`, `suffix` and `tol` (M7); 8 = adds `sheets`
/// (paper, scale, view, title block and revisions) (M8); 9 = adds `groups`; 10 = adds `blocks`
/// and `instances`; 11 = adds a layer's `fill` colour; 12 = adds `underlays` (M8.5); 13 = adds
/// `ellipse` entities; 14 = adds `bezier` curves and `smooth` relations; 15 = adds `texts`
/// (M9); 16 = adds the `modern` title block; 17 = adds sheet flags; 18 = adds a sheet's
/// `views`; 19 = adds `fonts` and a text's `align`, `spacing` and `wrap`; 20 = adds the
/// drafting style's `color`; 21 = adds `shown` (where dimensions and annotations show) and a
/// dimension's `below` and `text`; 22 = adds a block's `fills`; 23 = adds a layer's `snap`
/// (and layers are listed in draw order); 24 = notes have any number of `leaders` (each with a
/// `bend` or `bulge` and a shoulder) and a text style (`align`, `wrap`, `frame`, `mask`,
/// `bold`, `italic`, `height`).
pub const FORMAT_VERSION: u32 = 24;
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linetype: Option<String>,
    },
    Line {
        id: String,
        a: String,
        b: String,
        #[serde(default, skip_serializing_if = "Style::is_solid")]
        style: Style,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linetype: Option<String>,
    },
    Circle {
        id: String,
        center: String,
        radius: f64,
        #[serde(default, skip_serializing_if = "Style::is_solid")]
        style: Style,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linetype: Option<String>,
    },
    Arc {
        id: String,
        center: String,
        start: String,
        end: String,
        #[serde(default, skip_serializing_if = "Style::is_solid")]
        style: Style,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linetype: Option<String>,
    },
    /// A cubic Bézier curve (v14): end points `a` and `b`, handles `c1` and `c2`.
    Bezier {
        id: String,
        a: String,
        c1: String,
        c2: String,
        b: String,
        #[serde(default, skip_serializing_if = "Style::is_solid")]
        style: Style,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linetype: Option<String>,
    },
    /// An ellipse (v13); with `start` and `end`, the counter-clockwise part between them.
    Ellipse {
        id: String,
        center: String,
        major: String,
        minor: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end: Option<String>,
        #[serde(default, skip_serializing_if = "Style::is_solid")]
        style: Style,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linetype: Option<String>,
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
    /// Text before a dimension's value (e.g. `4×`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prefix: Option<String>,
    /// Text after a dimension's value (e.g. `THRU`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suffix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tol: Option<drafting::ToleranceRecord>,
    /// A line of text under the dimension line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    below: Option<String>,
    /// Text shown instead of the value (`EQ`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
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

const LENGTH_FORMATS: [(LengthFormat, &str); 7] = [
    (LengthFormat::Millimetre, "mm"),
    (LengthFormat::Centimetre, "cm"),
    (LengthFormat::Metre, "m"),
    (LengthFormat::Inch, "in"),
    (LengthFormat::Foot, "ft"),
    (LengthFormat::Architectural, "architectural"),
    (LengthFormat::FractionalInch, "fractional_in"),
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
struct LayerRecord {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    /// Fill and hatch colour (v11); absent uses the stroke colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fill: Option<String>,
    weight: f64,
    linetype: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    visible: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    locked: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    print: bool,
    /// Snapped to (v23); absent is yes.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    snap: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn parse_layer_id(s: &str) -> Result<LayerId, LoadError> {
    s.strip_prefix('l')
        .and_then(|n| n.parse::<u32>().ok())
        .map(LayerId)
        .ok_or_else(|| LoadError::BadId(s.to_owned()))
}

/// Per-entity layer and appearance fields, as stored.
struct Look {
    layer: Option<String>,
    color: Option<String>,
    weight: Option<f64>,
    linetype: Option<String>,
}

impl Look {
    fn of(entity: &Entity) -> Look {
        let a = entity.appearance;
        Look {
            layer: (entity.layer != LayerId::DEFAULT).then(|| entity.layer.to_string()),
            color: a.color.map(Rgb::hex),
            weight: a.weight.map(LineWeight::mm),
            linetype: a.line_type.map(|t| t.name().to_owned()),
        }
    }

    fn apply(self, id: &str, entity: &mut Entity) -> Result<(), LoadError> {
        if let Some(l) = self.layer {
            entity.layer = parse_layer_id(&l)?;
        }
        let bad = |field: &'static str| LoadError::MissingField {
            id: id.to_owned(),
            field,
        };
        entity.appearance = Appearance {
            color: match self.color {
                Some(c) => Some(Rgb::parse(&c).ok_or_else(|| bad("color"))?),
                None => None,
            },
            weight: self.weight.map(LineWeight::nearest),
            line_type: match self.linetype {
                Some(t) => Some(LineType::from_name(&t).ok_or_else(|| bad("linetype"))?),
                None => None,
            },
        };
        Ok(())
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
        Constraint::Smooth { point, a, b } => {
            (r.a, r.b, r.point) = (s(a), s(b), s(point));
            "smooth"
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
        "smooth" => Constraint::Smooth {
            point: entity(&r.point, "point")?,
            a: a()?,
            b: b()?,
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
    drafting: Option<drafting::DraftRecord>,
    #[serde(default)]
    layers: Vec<LayerRecord>,
    #[serde(default)]
    current_layer: Option<String>,
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
    #[serde(default)]
    fills: Vec<drafting::FillRecord>,
    #[serde(default)]
    annotations: Vec<drafting::AnnotationRecord>,
    /// Dimensions and annotations not shown everywhere.
    #[serde(default)]
    shown: Vec<drafting::ShownRecord>,
    /// Imported hatch patterns, as `.pat` text.
    #[serde(default)]
    patterns: Option<String>,
    /// The title blocks' logo, as SVG text.
    #[serde(default)]
    logo: Option<String>,
    /// The fonts the drawing carries, and the one its annotations are set in.
    #[serde(default)]
    fonts: Option<fonts::FontsRecord>,
    #[serde(default)]
    sheets: Vec<sheets::SheetRecord>,
    #[serde(default)]
    groups: Vec<GroupRecord>,
    #[serde(default)]
    blocks: Vec<blocks::BlockRecord>,
    #[serde(default)]
    instances: Vec<blocks::InstanceRecord>,
    #[serde(default)]
    underlays: Vec<underlays::UnderlayRecord>,
    #[serde(default)]
    texts: Vec<texts::TextRecord>,
}

/// A named group of entities.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupRecord {
    id: String,
    name: String,
    members: Vec<String>,
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
    let Look {
        layer,
        color,
        weight,
        linetype,
    } = Look::of(entity);
    match entity.kind {
        EntityKind::Point(at) => EntityRecord::Point {
            id,
            at: at.to_array(),
            style,
            layer,
            color,
            weight,
            linetype,
        },
        EntityKind::Line { a, b } => EntityRecord::Line {
            id,
            a: id_string(a),
            b: id_string(b),
            style,
            layer,
            color,
            weight,
            linetype,
        },
        EntityKind::Circle { center, radius } => EntityRecord::Circle {
            id,
            center: id_string(center),
            radius,
            style,
            layer,
            color,
            weight,
            linetype,
        },
        EntityKind::Ellipse {
            center,
            major,
            minor,
        } => EntityRecord::Ellipse {
            id,
            center: id_string(center),
            major: id_string(major),
            minor: id_string(minor),
            start: None,
            end: None,
            style,
            layer,
            color,
            weight,
            linetype,
        },
        EntityKind::EllipseArc {
            center,
            major,
            minor,
            start,
            end,
        } => EntityRecord::Ellipse {
            id,
            center: id_string(center),
            major: id_string(major),
            minor: id_string(minor),
            start: Some(id_string(start)),
            end: Some(id_string(end)),
            style,
            layer,
            color,
            weight,
            linetype,
        },
        EntityKind::Bezier { a, c1, c2, b } => EntityRecord::Bezier {
            id,
            a: id_string(a),
            c1: id_string(c1),
            c2: id_string(c2),
            b: id_string(b),
            style,
            layer,
            color,
            weight,
            linetype,
        },
        EntityKind::Arc { center, start, end } => EntityRecord::Arc {
            id,
            center: id_string(center),
            start: id_string(start),
            end: id_string(end),
            style,
            layer,
            color,
            weight,
            linetype,
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
    if !doc.custom_patterns().is_empty() {
        let p = serde_json::to_string(doc.custom_patterns()).expect("patterns serialize");
        out.push_str(&format!("  \"patterns\": {p},\n"));
    }
    if !doc.logo().is_empty() {
        let l = serde_json::to_string(doc.logo()).expect("logo serializes");
        out.push_str(&format!("  \"logo\": {l},\n"));
    }
    if let Some(record) = fonts::fonts_record(doc.fonts(), &doc.fonts_in_use()) {
        let f = serde_json::to_string(&record).expect("fonts serialize");
        out.push_str(&format!("  \"fonts\": {f},\n"));
    }
    if let Some(d) = drafting::draft_record(&doc.draft_style()) {
        let d = serde_json::to_string(&d).expect("drafting serializes");
        out.push_str(&format!("  \"drafting\": {d},\n"));
    }
    let default_layers = {
        let mut layers = doc.layers();
        matches!(
            (layers.next(), layers.next()),
            (Some((LayerId::DEFAULT, l)), None) if *l == Layer::new("0")
        )
    };
    if !default_layers {
        let layers: Vec<String> = doc
            .layers()
            .map(|(id, l)| {
                serde_json::to_string(&LayerRecord {
                    id: id.to_string(),
                    name: l.name.clone(),
                    color: l.color.map(Rgb::hex),
                    fill: l.fill_color.map(Rgb::hex),
                    weight: l.weight.mm(),
                    linetype: l.line_type.name().to_owned(),
                    visible: l.visible,
                    locked: l.locked,
                    print: l.printable,
                    snap: l.snappable,
                })
                .expect("layer serializes")
            })
            .collect();
        push_array(&mut out, "layers", &layers, true);
    }
    if doc.current_layer() != LayerId::DEFAULT {
        out.push_str(&format!(
            "  \"current_layer\": \"{}\",\n",
            doc.current_layer()
        ));
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
            if let Some(n) = doc.dim_note(id) {
                r.prefix = (!n.prefix.is_empty()).then(|| n.prefix.clone());
                r.suffix = (!n.suffix.is_empty()).then(|| n.suffix.clone());
                r.tol = drafting::tolerance_record(&n.tolerance);
                r.below = (!n.below.is_empty()).then(|| n.below.clone());
                r.text = (!n.text.is_empty()).then(|| n.text.clone());
            }
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
    let fills: Vec<String> = doc
        .fills()
        .map(|(id, f)| {
            serde_json::to_string(&drafting::fill_record(doc, id, f)).expect("fill serializes")
        })
        .collect();
    let annotations: Vec<String> = doc
        .annotations()
        .map(|(id, a)| {
            serde_json::to_string(&drafting::annotation_record(id, a))
                .expect("annotation serializes")
        })
        .collect();
    let shown: Vec<String> = doc
        .shown_settings()
        .map(|(r, s)| {
            serde_json::to_string(&drafting::shown_record(r, s)).expect("shown serializes")
        })
        .collect();
    let sheets: Vec<String> = doc
        .sheets()
        .map(|(id, s)| {
            serde_json::to_string(&sheets::sheet_record(id, s)).expect("sheet serializes")
        })
        .collect();
    let groups: Vec<String> = doc
        .groups()
        .map(|(id, g)| {
            serde_json::to_string(&GroupRecord {
                id: id.to_string(),
                name: g.name.clone(),
                members: g.members.iter().map(|e| id_string(*e)).collect(),
            })
            .expect("group serializes")
        })
        .collect();
    let block_lines: Vec<String> = doc
        .blocks()
        .map(|(id, b)| {
            serde_json::to_string(&blocks::block_record(id, b)).expect("block serializes")
        })
        .collect();
    let instances: Vec<String> = doc
        .instances()
        .map(|(id, i)| {
            serde_json::to_string(&blocks::instance_record(id, i)).expect("instance serializes")
        })
        .collect();
    let underlay_lines: Vec<String> = doc
        .underlays()
        .map(|(id, u)| {
            serde_json::to_string(&underlays::underlay_record(id, u)).expect("underlay serializes")
        })
        .collect();
    let text_lines: Vec<String> = doc
        .texts()
        .map(|(id, t)| serde_json::to_string(&texts::text_record(id, t)).expect("text serializes"))
        .collect();
    let tail = [
        ("measurements", &measurements),
        ("variants", &variants),
        ("fills", &fills),
        ("annotations", &annotations),
        ("shown", &shown),
        ("sheets", &sheets),
        ("groups", &groups),
        ("blocks", &block_lines),
        ("instances", &instances),
        ("underlays", &underlay_lines),
        ("texts", &text_lines),
    ];
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
    load_with_notes(text).map(|(doc, meta, _)| (doc, meta))
}

/// Load a file, also saying what had to be left out to open it: relations that had become
/// meaningless (a point held on a curve it is the end of), which older builds could save.
pub fn load_with_notes(text: &str) -> Result<(Document, Metadata, Vec<String>), LoadError> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let value = migrate(value)?;
    let file: FileRecord = serde_json::from_value(value)?;
    if file.meta.units != "mm" {
        return Err(LoadError::UnsupportedUnits(file.meta.units));
    }

    let mut entities = Vec::with_capacity(file.entities.len());
    let mut seen = std::collections::HashSet::new();
    for rec in file.entities {
        let (id_text, mut entity, look) = match rec {
            EntityRecord::Point {
                id,
                at,
                style,
                layer,
                color,
                weight,
                linetype,
            } => {
                let entity = Entity {
                    kind: EntityKind::Point(at.into()),
                    style: style.into(),
                    ..Entity::new(EntityKind::Point(at.into()))
                };
                (
                    id,
                    entity,
                    Look {
                        layer,
                        color,
                        weight,
                        linetype,
                    },
                )
            }
            EntityRecord::Line {
                id,
                a,
                b,
                style,
                layer,
                color,
                weight,
                linetype,
            } => {
                let kind = EntityKind::Line {
                    a: parse_id(&a)?,
                    b: parse_id(&b)?,
                };
                (
                    id,
                    Entity {
                        style: style.into(),
                        ..Entity::new(kind)
                    },
                    Look {
                        layer,
                        color,
                        weight,
                        linetype,
                    },
                )
            }
            EntityRecord::Circle {
                id,
                center,
                radius,
                style,
                layer,
                color,
                weight,
                linetype,
            } => {
                let kind = EntityKind::Circle {
                    center: parse_id(&center)?,
                    radius,
                };
                (
                    id,
                    Entity {
                        style: style.into(),
                        ..Entity::new(kind)
                    },
                    Look {
                        layer,
                        color,
                        weight,
                        linetype,
                    },
                )
            }
            EntityRecord::Bezier {
                id,
                a,
                c1,
                c2,
                b,
                style,
                layer,
                color,
                weight,
                linetype,
            } => {
                let kind = EntityKind::Bezier {
                    a: parse_id(&a)?,
                    c1: parse_id(&c1)?,
                    c2: parse_id(&c2)?,
                    b: parse_id(&b)?,
                };
                (
                    id,
                    Entity {
                        style: style.into(),
                        ..Entity::new(kind)
                    },
                    Look {
                        layer,
                        color,
                        weight,
                        linetype,
                    },
                )
            }
            EntityRecord::Ellipse {
                id,
                center,
                major,
                minor,
                start,
                end,
                style,
                layer,
                color,
                weight,
                linetype,
            } => {
                let (center, major, minor) =
                    (parse_id(&center)?, parse_id(&major)?, parse_id(&minor)?);
                let kind = match (start, end) {
                    (Some(s), Some(e)) => EntityKind::EllipseArc {
                        center,
                        major,
                        minor,
                        start: parse_id(&s)?,
                        end: parse_id(&e)?,
                    },
                    (None, None) => EntityKind::Ellipse {
                        center,
                        major,
                        minor,
                    },
                    _ => {
                        return Err(LoadError::MissingField { id, field: "end" });
                    }
                };
                (
                    id,
                    Entity {
                        style: style.into(),
                        ..Entity::new(kind)
                    },
                    Look {
                        layer,
                        color,
                        weight,
                        linetype,
                    },
                )
            }
            EntityRecord::Arc {
                id,
                center,
                start,
                end,
                style,
                layer,
                color,
                weight,
                linetype,
            } => {
                let kind = EntityKind::Arc {
                    center: parse_id(&center)?,
                    start: parse_id(&start)?,
                    end: parse_id(&end)?,
                };
                (
                    id,
                    Entity {
                        style: style.into(),
                        ..Entity::new(kind)
                    },
                    Look {
                        layer,
                        color,
                        weight,
                        linetype,
                    },
                )
            }
        };
        look.apply(&id_text, &mut entity)?;
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
    let mut notes = Vec::new();
    for mut rec in file.constraints {
        let expr = rec.expr.take();
        let note = drafting::dim_note(
            rec.prefix.take(),
            rec.suffix.take(),
            rec.tol.take(),
            rec.below.take(),
            rec.text.take(),
        );
        let (id, c, label, is_driven) = parse_constraint(rec)?;
        if let Some(e) = expr {
            exprs.push((id, e));
        }
        if let Some(n) = note {
            notes.push((id, n));
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
    let (mut doc, dropped) = Document::from_parts_dropping_degenerate(entities, constraints)
        .map_err(LoadError::Document)?;
    let notes_out: Vec<String> = dropped
        .iter()
        .map(|(id, why)| format!("{id} ({why})"))
        .collect();
    let gone = |id: &ConstraintId| dropped.iter().any(|(d, _)| d == id);
    labels.retain(|(id, _)| !gone(id));
    driven.retain(|id| !gone(id));
    notes.retain(|(id, _)| !gone(id));
    for (id, label) in labels {
        doc.set_placement(id, label).map_err(LoadError::Document)?;
    }
    for id in driven {
        doc.set_driven(id, true).map_err(LoadError::Document)?;
    }
    for (id, n) in notes {
        doc.set_dim_note(id, Some(n));
    }
    if let Some(display) = &file.display {
        doc.set_units(display.to_units()?);
    }
    for (rank, l) in file.layers.into_iter().enumerate() {
        let id = parse_layer_id(&l.id)?;
        let line_type = LineType::from_name(&l.linetype).ok_or(LoadError::MissingField {
            id: l.id.clone(),
            field: "linetype",
        })?;
        let color = match &l.color {
            Some(c) => Some(Rgb::parse(c).ok_or(LoadError::MissingField {
                id: l.id.clone(),
                field: "color",
            })?),
            None => None,
        };
        let fill_color = match &l.fill {
            Some(c) => Some(Rgb::parse(c).ok_or(LoadError::MissingField {
                id: l.id.clone(),
                field: "fill",
            })?),
            None => None,
        };
        doc.set_layer(
            id,
            Layer {
                name: l.name,
                color,
                fill_color,
                weight: LineWeight::nearest(l.weight),
                line_type,
                visible: l.visible,
                locked: l.locked,
                printable: l.print,
                snappable: l.snap,
                // The file lists layers in draw order, bottom first.
                order: i32::try_from(rank).unwrap_or(i32::MAX),
            },
        );
    }
    for (id, _) in doc.iter() {
        let layer = doc.get(id).map(|e| e.layer).unwrap_or_default();
        if doc.layer(layer).is_none() {
            return Err(LoadError::Document(DocumentError::MissingLayer(layer)));
        }
    }
    drafting::insert_all(&mut doc, file.fills, file.annotations)?;
    for r in file.shown {
        let (of, shown) = drafting::parse_shown(r)?;
        doc.set_shown(of, shown);
    }
    if let Some(p) = file.patterns {
        doc.set_custom_patterns(p);
    }
    if let Some(l) = file.logo {
        doc.set_logo(l);
    }
    if let Some(f) = file.fonts {
        doc.set_fonts(fonts::parse_fonts(f)?);
    }
    for b in file.blocks {
        let id_text = b.id.clone();
        let (id, block) = blocks::parse_block(b)?;
        if doc.block(id).is_some() {
            return Err(LoadError::DuplicateId(id_text));
        }
        doc.insert_block(id, block);
    }
    for i in file.instances {
        let id_text = i.id.clone();
        let (id, instance) = blocks::parse_instance(i)?;
        if doc.instance(id).is_some() {
            return Err(LoadError::DuplicateId(id_text));
        }
        doc.insert_instance(id, instance)
            .map_err(LoadError::Document)?;
    }
    for t in file.texts {
        let id_text = t.id.clone();
        let (id, text) = texts::parse_text(t)?;
        if doc.text(id).is_some() {
            return Err(LoadError::DuplicateId(id_text));
        }
        if doc.layer(text.layer).is_none() {
            return Err(LoadError::Document(DocumentError::MissingLayer(text.layer)));
        }
        doc.insert_text(id, text);
    }
    for u in file.underlays {
        let id_text = u.id.clone();
        let (id, underlay) = underlays::parse_underlay(u)?;
        if doc.underlay(id).is_some() {
            return Err(LoadError::DuplicateId(id_text));
        }
        doc.insert_underlay(id, underlay);
    }
    for g in file.groups {
        let id = drift_core::GroupId(parse_prefixed(&g.id, 'g')?);
        if doc.group(id).is_some() {
            return Err(LoadError::DuplicateId(g.id));
        }
        let members = g
            .members
            .iter()
            .map(|m| parse_id(m))
            .collect::<Result<_, _>>()?;
        doc.insert_group(
            id,
            drift_core::Group {
                name: g.name,
                members,
            },
        )
        .map_err(LoadError::Document)?;
    }
    for r in file.sheets {
        let id_text = r.id.clone();
        let (id, sheet) = sheets::parse_sheet(r)?;
        if doc.sheet(id).is_some() {
            return Err(LoadError::DuplicateId(id_text));
        }
        doc.insert_sheet(id, sheet);
    }
    if let Some(d) = file.drafting {
        doc.set_draft_style(drafting::parse_draft(d)?);
    }
    if let Some(current) = &file.current_layer {
        doc.set_current_layer(parse_layer_id(current)?)
            .map_err(LoadError::Document)?;
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
        match doc.insert_measurement(id, c) {
            Ok(_) | Err(DocumentError::Degenerate(_)) => {}
            Err(e) => return Err(LoadError::Document(e)),
        }
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
    Ok((doc, file.meta, notes_out))
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
    // v2 → v24 only added optional fields, sections and constraint types: nothing to convert
    // (a sheet's single view from before v18 is read as it was written).
    value["version"] = FORMAT_VERSION.into();
    Ok(value)
}

#[cfg(test)]
mod tests {
    use drift_core::{DVec2, demo};

    use super::*;

    #[test]
    fn a_point_held_on_its_own_curve_is_left_out_on_opening() {
        // An alpha 2 file could hold one (left by joining points); it refused to open.
        let mut doc = Document::new();
        let a = doc.add_point(DVec2::ZERO);
        let b = doc.add_point(DVec2::new(10.0, 0.0));
        let p = doc.add_point(DVec2::new(5.0, 0.0));
        let line = doc.add_line(a, b).unwrap();
        let held = doc
            .add_constraint(Constraint::PointOnCurve {
                point: p,
                curve: line,
            })
            .unwrap();
        doc.add_constraint(Constraint::Horizontal { line }).unwrap();
        let text = save(&doc, &Metadata::default());
        let bad = text.replace(
            &format!(r#""point":"{p}","curve":"{line}""#),
            &format!(r#""point":"{b}","curve":"{line}""#),
        );
        assert_ne!(bad, text);
        let (doc, _, notes) = load_with_notes(&bad).unwrap();
        assert_eq!(notes, [format!("{held} (point on its own curve)")]);
        assert_eq!(doc.constraints().count(), 1, "the horizontal stays");
        assert!(load(&bad).is_ok());
    }

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
  "version": 24,
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
    fn layers_round_trip() {
        let mut doc = Document::new();
        let walls = doc.add_layer(Layer {
            name: "Walls".into(),
            color: Some(Rgb([200, 30, 40])),
            fill_color: Some(Rgb([250, 230, 200])),
            weight: LineWeight(50),
            line_type: LineType::Continuous,
            visible: true,
            locked: true,
            printable: true,
            snappable: false,
            order: 0,
        });
        let hidden = doc.add_layer(Layer {
            line_type: LineType::Hidden,
            visible: false,
            printable: false,
            ..Layer::new("Hidden lines")
        });
        doc.set_current_layer(walls).unwrap();
        let a = doc.add_point(DVec2::ZERO);
        let b = doc.add_point(DVec2::new(10.0, 0.0));
        let line = doc.add_line(a, b).unwrap();
        doc.set_entity_layer(line, hidden).unwrap();
        doc.set_appearance(
            line,
            Appearance {
                color: Some(Rgb([1, 2, 3])),
                weight: Some(LineWeight(35)),
                line_type: Some(LineType::Phantom),
            },
        )
        .unwrap();
        let text = save(&doc, &Metadata::default());
        assert!(text.contains(r#""current_layer": "l1""#), "{text}");
        assert!(text.contains(r##""color":"#010203""##));
        assert!(text.contains(r##""fill":"#fae6c8""##), "{text}");
        let (loaded, _) = load(&text).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(save(&loaded, &Metadata::default()), text);
        // A plain document writes no layer section.
        assert!(!save(&Document::new(), &Metadata::default()).contains("layers"));
        // An entity on a missing layer is rejected.
        let broken = text.replace(r#""id":"l2""#, r#""id":"l9""#);
        assert!(load(&broken).is_err());
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

    #[test]
    fn a_note_with_its_one_leader_from_before_v24_loads() {
        let text = r#"{
  "format": "drift",
  "version": 23,
  "meta": {"units":"mm"},
  "entities": [
    {"type":"point","id":"e1","at":[0.0,0.0]}
  ],
  "annotations": [
    {"type":"note","id":"n1","text":"Old","at":[5.0,2.0],"leader":{"to":{"point":"e1"},"end":"dot"}}
  ]
}"#;
        let (doc, _) = load(text).unwrap();
        let (_, note) = doc.annotations().next().unwrap();
        let drift_core::Annotation::Note { leaders, at, .. } = note else {
            panic!("a note");
        };
        assert_eq!(leaders.len(), 1);
        assert_eq!(leaders[0].end, drift_core::LeaderEnd::Dot);
        assert!(leaders[0].shoulder);
        assert_eq!(*at, DVec2::new(5.0, 2.0));
        // Saved again, as a list.
        let saved = save(&doc, &Metadata::default());
        assert!(saved.contains(r#""leaders":["#) && !saved.contains(r#""leader":"#));
    }

    #[test]
    fn drafting_round_trips() {
        use drift_core::{
            Anchor, Annotation, ArrowStyle, DimNote, DraftStyle, Fill, FillKind, Hatch, Leader,
            LeaderEnd, OrdinateAxis, Tolerance,
        };
        let mut doc = demo::mounting_plate();
        let curves: Vec<EntityId> = doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
            .map(|(id, _)| id)
            .take(4)
            .collect();
        let circle = doc
            .iter()
            .find(|(_, e)| matches!(e.kind, EntityKind::Circle { .. }))
            .map(|(id, _)| id)
            .unwrap();
        let point = doc
            .iter()
            .find(|(_, e)| e.kind.is_point())
            .map(|(id, _)| id)
            .unwrap();
        let mut solid = Fill::new(curves.clone(), DVec2::new(1.5, 2.5), FillKind::Solid);
        solid.opacity = 0.5;
        solid.color = Some(Rgb([10, 20, 30]));
        doc.add_fill(solid);
        let mut hatch = Hatch::new("ANSI31");
        hatch.scale = 2.5;
        hatch.angle = 15.0;
        hatch.origin = DVec2::new(1.0, -2.0);
        hatch.model = true;
        hatch.background = Some(Rgb([255, 255, 200]));
        let mut h = Fill::new(curves.clone(), DVec2::new(3.0, 4.0), FillKind::Hatch(hatch));
        h.weight = Some(LineWeight(35));
        doc.add_fill(h);
        doc.add_annotation(Annotation::Note {
            text: "Deburr all edges\nBreak sharp corners".into(),
            at: DVec2::new(10.0, 20.0),
            leaders: Vec::new(),
            style: Default::default(),
        })
        .unwrap();
        doc.add_annotation(Annotation::Note {
            text: "Edge".into(),
            at: DVec2::new(-5.0, 3.0),
            leaders: vec![Leader {
                to: Anchor::Curve {
                    curve: curves[0],
                    t: 0.25,
                },
                end: LeaderEnd::Dot,
                shape: drift_core::LeaderShape::Straight,
                shoulder: true,
                shoulder_len: drift_core::Leader::SHOULDER,
            }],
            style: Default::default(),
        })
        .unwrap();
        doc.add_annotation(Annotation::Note {
            text: "Here".into(),
            at: DVec2::new(1.0, 1.0),
            leaders: vec![
                Leader {
                    to: Anchor::Point(point),
                    end: LeaderEnd::Arrow,
                    shape: drift_core::LeaderShape::Straight,
                    shoulder: true,
                    shoulder_len: drift_core::Leader::SHOULDER,
                },
                Leader {
                    to: Anchor::Free(DVec2::new(40.0, -2.0)),
                    end: LeaderEnd::OpenArrow,
                    shape: drift_core::LeaderShape::Bent(DVec2::new(3.0, 4.5)),
                    shoulder: false,
                    shoulder_len: drift_core::Leader::SHOULDER,
                },
                Leader {
                    to: Anchor::Curve {
                        curve: curves[1],
                        t: 0.75,
                    },
                    end: LeaderEnd::None,
                    shape: drift_core::LeaderShape::Curved(-0.3),
                    shoulder: true,
                    shoulder_len: 6.25,
                },
            ],
            style: drift_core::NoteStyle {
                align: drift_core::TextAlign::Center,
                wrap: Some(42.5),
                frame: drift_core::NoteFrame::Rounded,
                mask: true,
                bold: true,
                italic: true,
                height: Some(3.5),
            },
        })
        .unwrap();
        doc.add_annotation(Annotation::CentreMark { curve: circle })
            .unwrap();
        doc.add_annotation(Annotation::Centreline {
            a: curves[0],
            b: curves[2],
        })
        .unwrap();
        doc.add_annotation(Annotation::HoleCallout {
            curve: circle,
            at: DVec2::new(8.0, 8.0),
            suffix: "THRU".into(),
        })
        .unwrap();
        doc.add_annotation(Annotation::ChamferCallout {
            line: curves[1],
            at: DVec2::new(4.0, -4.0),
        })
        .unwrap();
        doc.add_annotation(Annotation::Ordinate {
            origin: point,
            point,
            axis: OrdinateAxis::Y,
            at: -12.5,
        })
        .unwrap();
        let dims: Vec<ConstraintId> = doc
            .constraints()
            .filter(|(_, c)| c.is_dimension())
            .map(|(id, _)| id)
            .take(4)
            .collect();
        let tolerances = [
            Tolerance::Symmetric(0.05),
            Tolerance::Deviation {
                upper: 0.1,
                lower: 0.02,
            },
            Tolerance::Limits {
                upper: 0.2,
                lower: 0.0,
            },
            Tolerance::Fit("H7".into()),
        ];
        for (k, (id, t)) in dims.iter().zip(tolerances).enumerate() {
            doc.set_dim_note(
                *id,
                Some(DimNote {
                    prefix: "4×".into(),
                    suffix: "THRU".into(),
                    tolerance: t,
                    // A line under one, text instead of the value on another.
                    below: if k == 0 {
                        "TO FACE OF STUD".into()
                    } else {
                        String::new()
                    },
                    text: if k == 1 { "EQ".into() } else { String::new() },
                }),
            );
        }
        // One hidden on the canvas, one left off sheets.
        doc.set_shown(
            drift_core::AnnotationRef::Dimension(dims[0]),
            drift_core::Shown {
                canvas: false,
                sheets: true,
            },
        );
        doc.set_shown(
            drift_core::AnnotationRef::Dimension(dims[1]),
            drift_core::Shown {
                canvas: true,
                sheets: false,
            },
        );
        let mut style = DraftStyle::asme();
        style.arrow = ArrowStyle::Tick;
        style.alt_units = Some(LengthFormat::Millimetre);
        style.scale = 50.0;
        style.color = Some(drift_core::Rgb([0x20, 0xa0, 0xff]));
        doc.set_draft_style(style);
        doc.set_custom_patterns("*MINE, Mine\n0, 0,0, 0,2\n".into());
        doc.set_logo(
            r##"<svg viewBox="0 0 4 2"><rect width="4" height="2" fill="#f60"/></svg>"##.into(),
        );
        let mut sheet = drift_core::Sheet::new("Plate");
        sheet.paper = drift_core::PaperSize::Custom(500.0, 350.0);
        sheet.portrait = true;
        sheet.views[0].scale = 2.5;
        sheet.views[0].center = DVec2::new(60.0, 40.0);
        let mut detail_view = drift_core::SheetView::new(0.5, DVec2::new(12.0, 8.0));
        detail_view.name = "Detail A".into();
        detail_view.frame = Some(drift_core::ViewFrame {
            at: DVec2::new(400.0, 250.0),
            size: DVec2::new(120.0, 90.0),
        });
        detail_view.title = true;
        detail_view.border = true;
        sheet.views.push(detail_view);
        sheet.title_block = drift_core::TitleBlock::Ansi;
        sheet.fields.title = "Mounting plate".into();
        sheet.fields.number = "DR-001".into();
        sheet.revisions.push(drift_core::Revision {
            rev: "A".into(),
            date: "2026-10-08".into(),
            description: "First issue".into(),
            by: "CP".into(),
        });
        doc.add_sheet(sheet);
        let mut detail = drift_core::Sheet::new("Detail");
        detail.views[0].dimensions = false;
        detail.views[0].notes = false;
        doc.add_sheet(detail);
        let chair = doc.add_block(drift_core::Block {
            name: "Chair".into(),
            fills: vec![drift_core::Fill::new(
                Vec::new(),
                DVec2::new(0.0, 50.0),
                drift_core::FillKind::Hatch(drift_core::Hatch {
                    angle: 30.0,
                    origin: DVec2::new(5.0, 0.0),
                    ..drift_core::Hatch::new("AR-BRSTD")
                }),
            )],
            items: vec![
                drift_core::BlockItem {
                    shape: drift_core::Shape::Line {
                        a: DVec2::new(-200.0, 0.0),
                        b: DVec2::new(200.0, 0.0),
                    },
                    style: LineKind::Solid,
                    appearance: Appearance::default(),
                },
                drift_core::BlockItem {
                    shape: drift_core::Shape::Arc {
                        center: DVec2::ZERO,
                        radius: 200.0,
                        start: 0.0,
                        sweep: std::f64::consts::PI,
                    },
                    style: LineKind::Center,
                    appearance: Appearance {
                        color: Some(Rgb([1, 2, 3])),
                        weight: Some(LineWeight(35)),
                        line_type: Some(LineType::Dashed),
                    },
                },
            ],
        });
        let mut placed = drift_core::Instance::new(chair, DVec2::new(500.0, 250.0));
        placed.angle = 37.5;
        placed.scale = 1.25;
        placed.mirror = true;
        doc.add_instance(placed).unwrap();
        doc.add_instance(drift_core::Instance::new(chair, DVec2::ZERO))
            .unwrap();
        doc.add_group(drift_core::Group {
            name: "Edges".into(),
            members: curves.iter().copied().take(2).collect(),
        })
        .unwrap();
        // An ellipse and part of it, with every point exactly where adding them puts them.
        let [ec, em, en, es, ee] = [
            DVec2::new(300.0, 0.0),
            DVec2::new(340.0, 0.0),
            DVec2::new(300.0, 10.0),
            DVec2::new(300.0, -10.0),
            DVec2::new(260.0, 0.0),
        ]
        .map(|p| doc.add_point(p));
        doc.add_ellipse(ec, em, en).unwrap();
        doc.add_ellipse_arc(ec, em, en, es, ee).unwrap();
        // Two curves meeting at a smooth point.
        let [b0, b1, b2, b3, b4, b5, b6] = [
            (400.0, 0.0),
            (410.0, 10.0),
            (420.0, 10.0),
            (430.0, 0.0),
            (440.0, -10.0),
            (450.0, -10.0),
            (460.0, 0.0),
        ]
        .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
        doc.add_bezier(b0, b1, b2, b3).unwrap();
        doc.add_bezier(b3, b4, b5, b6).unwrap();
        doc.add_constraint(Constraint::Smooth {
            point: b3,
            a: b2,
            b: b4,
        })
        .unwrap();
        let mut plan = drift_core::Underlay::new(
            "Plan",
            "plan.pdf, page 2",
            vec![
                drift_core::RefPrim::Line(DVec2::new(0.1, 0.2), DVec2::new(300.5, 0.2)),
                drift_core::RefPrim::Polyline(vec![
                    DVec2::new(0.0, 0.0),
                    DVec2::new(10.0, 0.0),
                    DVec2::new(10.0, 7.25),
                ]),
                drift_core::RefPrim::Circle {
                    center: DVec2::new(50.0, 50.0),
                    radius: 12.5,
                },
                drift_core::RefPrim::Arc {
                    center: DVec2::new(80.0, 20.0),
                    radius: 30.0,
                    start: 0.0,
                    sweep: std::f64::consts::FRAC_PI_2,
                },
            ],
        );
        plan.at = DVec2::new(-1000.0, 250.0);
        plan.scale = 17.6389;
        plan.angle = 12.0;
        plan.color = Some(Rgb([90, 120, 200]));
        plan.opacity = 0.45;
        plan.snap = false;
        plan.locked = true;
        doc.add_underlay(plan);
        doc.add_underlay(drift_core::Underlay::new("Survey", "", Vec::new()));
        let mut sign = drift_core::SketchText::new("EXIT → 2", DVec2::new(5.0, -40.0), 12.5);
        sign.angle = 15.0;
        sign.font = drift_core::TextFont::Mono;
        sign.appearance.color = Some(Rgb([200, 10, 10]));
        doc.add_text(sign);
        doc.add_text(drift_core::SketchText::new("A", DVec2::ZERO, 3.0));

        let text = save(&doc, &Metadata::default());
        assert!(text.contains("\"version\": 24"));
        assert!(
            text.contains(r#"{"a":[80.0,20.0,30.0,0.0,90.0]}"#),
            "{text}"
        );
        assert!(text.contains(r#""below":"TO FACE OF STUD""#) && text.contains(r#""text":"EQ""#));
        assert!(text.contains(r#""shown": ["#) && text.contains(r#""canvas":false"#));
        let (loaded, _) = load(&text).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(save(&loaded, &Metadata::default()), text);
        // Defaults stay out of the file.
        let plain = save(&demo::mounting_plate(), &Metadata::default());
        for absent in [
            "drafting",
            "fills",
            "annotations",
            "\"tol\"",
            "sheets",
            "groups",
            "blocks",
            "instances",
            "underlays",
            "texts",
        ] {
            assert!(!plain.contains(absent), "{absent}");
        }
    }
}
