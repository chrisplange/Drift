//! The native `.drift` file format.
//!
//! A `.drift` file is UTF-8 JSON with one entity or constraint per line, so files diff and
//! merge cleanly in version control:
//!
//! ```json
//! {
//!   "format": "drift",
//!   "version": 43,
//!   "reader": 43,
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
//! dimensions) and `options` (named sets of variable expressions, or whole sketches) are written
//! only when used.
//! Coordinates are always millimetres (`meta.units`), whatever the display units.
//!
//! Angles are stored in degrees, as the user typed them.
//!
//! Numbers use the shortest representation that round-trips exactly, so save → load is
//! bit-exact. Older versions are upgraded by [`migrate`] before parsing.
//!
//! `reader` is the oldest version that reads the file; a file whose `reader` is above this
//! build's [`FORMAT_VERSION`] is refused. Anything else this build doesn't know (a field or a
//! table a newer Drift added) is passed over when reading and written back when saving, where
//! the record that held it still exists (`carry.rs`).

use std::collections::BTreeMap;

use drift_core::{
    Appearance, Constraint, ConstraintId, DVec2, Document, DocumentError, DrawingRules, Entity,
    EntityId, EntityKind, Kind, Layer, LayerId, LengthFormat, LineKind, LineType, LineWeight,
    MeasurementId, Rgb, Units, Variable, VariableId, Variant, VariantId,
};
use serde::{Deserialize, Serialize};

mod blocks;
#[cfg(not(feature = "demo"))]
mod carry;
pub mod clip;
mod drafting;
#[cfg(not(feature = "demo"))]
mod layout;
// DWG and DXF R2018 export (the writers are left out of the demo).
#[cfg(all(feature = "dwg", not(feature = "demo")))]
pub mod cad;
#[cfg(feature = "dwg")]
pub mod dwg_import;
pub mod dxf;
mod dxf_import;
mod fonts;
mod icc;
// The writers (PDF, SVG, DXF and `.drift`) are left out of the browser demo.
mod openings;
#[cfg(not(feature = "demo"))]
pub mod pdf;
mod pdf_glyphs;
mod pdf_image;
pub mod pdf_import;
mod pdf_text;
#[cfg(not(feature = "demo"))]
pub mod profiles;
mod sheets;
// What the CAD writers share: layer naming, line types, text escaping.
#[cfg(not(feature = "demo"))]
mod style;
#[cfg(not(feature = "demo"))]
pub mod svg;
pub mod svg_import;
mod texts;
mod underlays;
pub mod vector;
mod walls;

/// File extension, without the dot.
pub const EXTENSION: &str = "drift";
/// The version this build writes. A file with a higher version still opens when its `reader`
/// (see [`READER_VERSION`]) isn't higher; what this build doesn't know in it is kept.
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
/// `bold`, `italic`, `height`); 25 = adds a sheet's `colours` (greyscale or black & white);
/// 26 = adds `styles` (a dimension's or annotation's own settings over the drafting style);
/// 27 = views are the model's (`views`: a named crop at a scale), placed on sheets by their
/// `viewports` (a sheet's own `views` become model views when read); 28 = adds a view's
/// `hidden_layers`; 29 = adds a section or elevation mark's `view` (the view it points at);
/// 30 = adds the `project` the sheets share (name, number, client, address, company and
/// field set) and a sheet's `status`; 31 = a `shown` entry's `views` (a dimension or
/// annotation drawn in those views only); 32 = adds a view's `construction` (construction
/// geometry shown on sheets); 33 = adds an underlay's `images` (pictures from a PDF page);
/// 34 = adds a dimension's `ext` (extension line starts set by hand); 35 = a sheet's
/// `title_block` can be `blank` (blank paper; `none` stays the border without a title block);
/// 36 = adds a block's `profile` flag (written to the Blender profiles file); 37 = adds a
/// fill's `divided` (an edit left it covering more than one area); 38 = adds `paths` (curves
/// drawn as one shape); 39 = adds `rules` (how closed areas and fills behave, and the gap
/// tolerance; they were the app's preferences before, so an older file gets the app's);
/// 40 = adds `stack` (where curves stack among their layer's fills, for those not at the top
/// of their layer) and an underlay's `layer` and `order` (where its pictures stack);
/// 41 = `variants` become `options`, which hold variable values or a whole `sketch` (a Drift
/// file of its own, leaving out the sections it has the `same` as the drawing), and a block's
/// fill can have `"path": "shape"` (it filled a shape, which exploding makes again);
/// 42 = adds `reader` (the oldest version that reads the file), and reading passes over what
/// it doesn't know and keeps it for saving (from here on, additions don't bump the version);
/// 43 = adds `fix_x` and `fix_y` relations (a point's x or y alone held where it is, by Fix on
/// geometry something else already partly holds).
pub const FORMAT_VERSION: u32 = 43;
/// The oldest version that reads what this build writes: the last breaking change, written as
/// a file's `"reader"`. A reader refuses a file only when its `reader` is above the reader's
/// own [`FORMAT_VERSION`] (a file without one, from before 42, gives its `version`).
///
/// History: 42 = files carry `reader` (the strict readers before 42 refused any field they
/// didn't know); 43 = `fix_x` and `fix_y` relations (a 42 reader refuses a relation type it
/// doesn't know).
pub const READER_VERSION: u32 = 43;
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
    /// The file is from before drawings kept their own [`rules`](Document::rules) (v39):
    /// they were the app's preferences then, so the app should apply its own.
    #[serde(skip)]
    pub older_rules: bool,
    /// What the file had that this build doesn't know, written back on saving.
    #[serde(skip)]
    pub carried: Carried,
}

/// What a file had that this build doesn't know (fields and tables from a newer Drift), kept
/// so that [`save`] writes it back. Empty for a drawing that has none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Carried(#[cfg(not(feature = "demo"))] Option<std::sync::Arc<carry::Kept>>);

impl Carried {
    /// The file had things from a newer Drift that this build doesn't know (they are kept
    /// where they can be: see [`save_checked`]).
    pub fn any(&self) -> bool {
        #[cfg(not(feature = "demo"))]
        return self.0.is_some();
        #[cfg(feature = "demo")]
        false
    }
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
            older_rules: false,
            carried: Carried::default(),
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
    MissingField {
        id: String,
        field: &'static str,
    },
    UnknownConstraint(String),
    DuplicateId(String),
    NonFinite(String),
    /// A sheet places a view the file doesn't have.
    UnknownView(String),
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
            LoadError::UnknownView(id) => {
                write!(f, "a sheet places view \"{id}\", which is missing")
            }
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
#[serde(tag = "type", rename_all = "lowercase")]
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
    /// Where a linear dimension's extension lines start, set by hand (mm from each measured
    /// point toward the dimension line; `null` for an end left automatic).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ext: Option<[Option<f64>; 2]>,
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

/// How a drawing's closed areas and fills behave (v39). Every field is optional (defaults
/// apply).
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct RulesRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    areas_by_layer: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pick_filled_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stacked_fills: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    follow_divided: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gap_tolerance: Option<f64>,
    /// The door and window size set (an addition at 42: fixture v42.2). Unset: by the
    /// drawing's units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size_set: Option<String>,
}

impl RulesRecord {
    /// Only what differs from the defaults.
    #[cfg(not(feature = "demo"))]
    fn from_rules(rules: DrawingRules) -> Self {
        let d = DrawingRules::default();
        let differs = |a: bool, b: bool| (a != b).then_some(a);
        Self {
            areas_by_layer: differs(rules.regions_by_layer, d.regions_by_layer),
            pick_filled_only: differs(rules.regions_need_fill, d.regions_need_fill),
            stacked_fills: differs(rules.stacked_fills, d.stacked_fills),
            follow_divided: differs(rules.follow_divided_fills, d.follow_divided_fills),
            gap_tolerance: (rules.gap_tolerance != d.gap_tolerance).then_some(rules.gap_tolerance),
            size_set: rules.size_set.map(|s| s.to_string()),
        }
    }

    fn to_rules(&self) -> Result<DrawingRules, LoadError> {
        let d = DrawingRules::default();
        let gap_tolerance = self.gap_tolerance.unwrap_or(d.gap_tolerance);
        if !(gap_tolerance.is_finite() && gap_tolerance > 0.0) {
            return Err(LoadError::NonFinite("rules.gap_tolerance".into()));
        }
        Ok(DrawingRules {
            regions_by_layer: self.areas_by_layer.unwrap_or(d.regions_by_layer),
            regions_need_fill: self.pick_filled_only.unwrap_or(d.regions_need_fill),
            stacked_fills: self.stacked_fills.unwrap_or(d.stacked_fills),
            follow_divided_fills: self.follow_divided.unwrap_or(d.follow_divided_fills),
            gap_tolerance,
            // (An id this build can't hold is left unset: the drawing's units choose.)
            size_set: self
                .size_set
                .as_deref()
                .and_then(drift_core::SizeSetId::new),
        })
    }
}

/// How lengths and angles are displayed. Every field is optional (defaults apply).
#[derive(Debug, Default, Serialize, Deserialize)]
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
    #[cfg(not(feature = "demo"))]
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

pub(crate) fn parse_layer_id(s: &str) -> Result<LayerId, LoadError> {
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
    #[cfg(not(feature = "demo"))]
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
struct VariableRecord {
    id: String,
    name: String,
    expr: String,
    kind: String,
}

/// A design option (v41; `variants` before, which held values only).
#[derive(Debug, Serialize, Deserialize)]
struct OptionRecord {
    id: String,
    name: String,
    /// Variable id → expression, for an option of values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    values: BTreeMap<String, String>,
    /// The whole sketch, as a Drift file of its own, for a whole-sketch option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sketch: Option<serde_json::Value>,
    /// Sections the sketch has the same as the drawing, left out of `sketch` (fonts,
    /// pictures, blocks: they would double the file).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    same: Vec<String>,
}

/// The sections a whole-sketch option shares with the drawing when they are the same.
const SHAREABLE: [&str; 5] = ["patterns", "logo", "fonts", "blocks", "underlays"];

const KINDS: [(Kind, &str); 3] = [
    (Kind::Length, "length"),
    (Kind::Angle, "angle"),
    (Kind::Number, "number"),
];

#[cfg(not(feature = "demo"))]
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

#[cfg(not(feature = "demo"))]
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
        Constraint::FixX { point, x } => {
            (r.point, r.value) = (s(point), Some(x));
            "fix_x"
        }
        Constraint::FixY { point, y } => {
            (r.point, r.value) = (s(point), Some(y));
            "fix_y"
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
        "fix_x" => Constraint::FixX {
            point: entity(&r.point, "point")?,
            x: number(r.value, "value")?,
        },
        "fix_y" => Constraint::FixY {
            point: entity(&r.point, "point")?,
            y: number(r.value, "value")?,
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

/// The whole file. (`format`, `version` and `reader` are checked by `migrate`; anything this
/// build doesn't know is passed over here and carried through by [`carry`].)
#[derive(Debug, Deserialize)]
struct FileRecord {
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
    options: Vec<OptionRecord>,
    #[serde(default)]
    fills: Vec<drafting::FillRecord>,
    #[serde(default)]
    annotations: Vec<drafting::AnnotationRecord>,
    /// Dimensions and annotations not shown everywhere.
    #[serde(default)]
    shown: Vec<drafting::ShownRecord>,
    /// Dimensions and annotations with settings of their own over the drafting style.
    #[serde(default)]
    styles: Vec<drafting::StyleRecord>,
    /// Imported hatch patterns, as `.pat` text.
    #[serde(default)]
    patterns: Option<String>,
    /// The title blocks' logo, as SVG text.
    #[serde(default)]
    logo: Option<String>,
    /// How closed areas and fills behave (v39).
    #[serde(default)]
    rules: Option<RulesRecord>,
    /// The project the sheets share.
    #[serde(default)]
    project: Option<sheets::ProjectRecord>,
    /// The fonts the drawing carries, and the one its annotations are set in.
    #[serde(default)]
    fonts: Option<fonts::FontsRecord>,
    #[serde(default)]
    views: Vec<sheets::ViewRecord>,
    #[serde(default)]
    sheets: Vec<sheets::SheetRecord>,
    #[serde(default)]
    groups: Vec<GroupRecord>,
    /// Curves drawn as one shape (v38).
    #[serde(default)]
    paths: Vec<PathRecord>,
    /// Where curves stack on their layer, for those not at its top (v40).
    #[serde(default)]
    stack: Vec<StackRecord>,
    #[serde(default)]
    blocks: Vec<blocks::BlockRecord>,
    #[serde(default)]
    instances: Vec<blocks::InstanceRecord>,
    #[serde(default)]
    underlays: Vec<underlays::UnderlayRecord>,
    #[serde(default)]
    texts: Vec<texts::TextRecord>,
    /// Runs of wall and what they made (an addition at 42: fixture v42.1).
    #[serde(default)]
    walls: Vec<walls::WallRecord>,
    /// Dimensions showing a wall's thickness (an addition at 42: fixture v42.1).
    #[serde(default)]
    property_dims: Vec<walls::PropertyDimRecord>,
    /// Doors, windows and openings in walls (an addition at 42: fixture v42.2).
    #[serde(default)]
    openings: Vec<openings::OpeningRecord>,
}

/// A named group of entities.
#[derive(Debug, Serialize, Deserialize)]
struct GroupRecord {
    id: String,
    name: String,
    members: Vec<String>,
}

/// Where a curve stacks among its layer's fills and curves.
#[derive(Debug, Serialize, Deserialize)]
struct StackRecord {
    id: String,
    order: i32,
}

/// Curves drawn as one shape, in order round it.
#[derive(Debug, Serialize, Deserialize)]
struct PathRecord {
    id: String,
    curves: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    closed: bool,
}

#[cfg(not(feature = "demo"))]
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

#[cfg(not(feature = "demo"))]
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

/// Serialize a document to `.drift` JSON, with what this build didn't know in the file it was
/// read from written back (see [`save_checked`]).
#[cfg(not(feature = "demo"))]
pub fn save(doc: &Document, meta: &Metadata) -> String {
    save_checked(doc, meta).text
}

/// A saved drawing.
#[cfg(not(feature = "demo"))]
#[derive(Debug)]
pub struct Saved {
    pub text: String,
    /// Things from a newer Drift in the file it was read from couldn't be kept: the record
    /// that held them was deleted, the field cleared, or a list without ids changed length.
    /// (Records that were only changed keep them.)
    pub loses_newer_data: bool,
    /// What is carried from here on, once `text` is written: it goes in the drawing's
    /// [`Metadata::carried`], so that the next save compares with this file (and doesn't
    /// warn again about what this one left out).
    pub carried: Carried,
}

/// [`save`], also saying whether anything from a newer Drift had to be left out.
#[cfg(not(feature = "demo"))]
pub fn save_checked(doc: &Document, meta: &Metadata) -> Saved {
    let plain = write(doc, meta);
    match &meta.carried.0 {
        Some(kept) => {
            let holds = |id: &str| doc.holds_id(id);
            let (text, loses_newer_data) = kept.merge(&plain, holds);
            let carried = Carried(
                kept.after_saving(&text, plain, holds)
                    .map(std::sync::Arc::new),
            );
            Saved {
                text,
                loses_newer_data,
                carried,
            }
        }
        None => Saved {
            text: plain,
            loses_newer_data: false,
            carried: Carried::default(),
        },
    }
}

/// The whole file as this build writes it, in the order it is written. (`FileRecord` reads
/// it.) Tables a drawing doesn't use are left out; `entities` and `constraints` always come.
#[cfg(not(feature = "demo"))]
#[derive(Serialize)]
struct FileOut<'a> {
    format: &'static str,
    version: u32,
    reader: u32,
    meta: &'a Metadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    display: Option<DisplayRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    patterns: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logo: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rules: Option<RulesRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<sheets::ProjectRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fonts: Option<fonts::FontsRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    drafting: Option<drafting::DraftRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    layers: Option<Vec<LayerRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_layer: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    variables: Vec<VariableRecord>,
    entities: Vec<EntityRecord>,
    constraints: Vec<ConstraintRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    measurements: Vec<ConstraintRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    options: Vec<OptionRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fills: Vec<drafting::FillRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    annotations: Vec<drafting::AnnotationRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    shown: Vec<drafting::ShownRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    styles: Vec<drafting::StyleRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    views: Vec<sheets::ViewRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sheets: Vec<sheets::SheetRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    groups: Vec<GroupRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    paths: Vec<PathRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stack: Vec<StackRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    blocks: Vec<blocks::BlockRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    instances: Vec<blocks::InstanceRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    underlays: Vec<underlays::UnderlayRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    texts: Vec<texts::TextRecord>,
    // (After the entities, paths and fills they name.)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    walls: Vec<walls::WallRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    property_dims: Vec<walls::PropertyDimRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    openings: Vec<openings::OpeningRecord>,
}

/// The drawing as this build writes it, without anything carried through.
#[cfg(not(feature = "demo"))]
fn write(doc: &Document, meta: &Metadata) -> String {
    layout::to_file(&file_out(doc, meta))
}

/// The records of the file for `doc`.
#[cfg(not(feature = "demo"))]
fn file_out<'a>(doc: &'a Document, meta: &'a Metadata) -> FileOut<'a> {
    let default_layers = {
        let mut layers = doc.layers();
        matches!(
            (layers.next(), layers.next()),
            (Some((LayerId::DEFAULT, l)), None) if *l == Layer::new("0")
        )
    };
    let layers = (!default_layers).then(|| {
        doc.layers()
            .map(|(id, l)| LayerRecord {
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
            .collect()
    });
    let variables = doc
        .variables()
        .map(|(id, v)| VariableRecord {
            id: id.to_string(),
            name: v.name.clone(),
            expr: v.expr.clone(),
            kind: kind_name(v.kind).to_owned(),
        })
        .collect();
    let constraints = doc
        .constraints()
        .map(|(id, c)| {
            let mut r = constraint_record(id, c, doc.placement(id), doc.is_driven(id));
            r.expr = doc.expression(id).map(str::to_owned);
            let ext = doc.extension_starts(id);
            r.ext = (ext != [None, None]).then_some(ext);
            if let Some(n) = doc.dim_note(id) {
                r.prefix = (!n.prefix.is_empty()).then(|| n.prefix.clone());
                r.suffix = (!n.suffix.is_empty()).then(|| n.suffix.clone());
                r.tol = drafting::tolerance_record(&n.tolerance);
                r.below = (!n.below.is_empty()).then(|| n.below.clone());
                r.text = (!n.text.is_empty()).then(|| n.text.clone());
            }
            r
        })
        .collect();
    let measurements = doc
        .measurements()
        .map(|(id, c)| {
            let mut r = constraint_record(ConstraintId(1), c, None, false);
            r.id = id.to_string();
            // Measurements have no value of their own.
            (r.value, r.degrees) = (None, None);
            r
        })
        .collect();
    let mut sections = None;
    let options = doc
        .variants()
        .map(|(id, v)| {
            let mut record = OptionRecord {
                id: id.to_string(),
                name: v.name.clone(),
                values: v
                    .values
                    .iter()
                    .map(|(var, e)| (var.to_string(), e.clone()))
                    .collect(),
                sketch: None,
                same: Vec::new(),
            };
            if let Some(sketch) = &v.sketch {
                let sections = sections.get_or_insert_with(|| shareable_sections(doc));
                let mut value = serde_json::to_value(file_out(sketch, &Metadata::default()))
                    .expect("a sketch serializes");
                if let Some(map) = value.as_object_mut() {
                    for key in SHAREABLE {
                        if map.get(key).is_some_and(|v| sections.get(key) == Some(v)) {
                            map.remove(key);
                            record.same.push(key.to_owned());
                        }
                    }
                }
                record.sketch = Some(value);
            }
            record
        })
        .collect();
    let shown_refs: std::collections::BTreeSet<drift_core::AnnotationRef> = doc
        .shown_settings()
        .map(|(r, _)| r)
        .chain(doc.only_in_settings().map(|(r, _)| r))
        .collect();
    let rules = RulesRecord::from_rules(doc.rules());
    FileOut {
        format: FORMAT_TAG,
        version: FORMAT_VERSION,
        reader: READER_VERSION,
        meta,
        display: (doc.units() != Units::default()).then(|| DisplayRecord::from_units(doc.units())),
        patterns: Some(doc.custom_patterns()).filter(|p| !p.is_empty()),
        logo: Some(doc.logo()).filter(|l| !l.is_empty()),
        rules: (rules != RulesRecord::default()).then_some(rules),
        project: sheets::project_record(doc.project()),
        fonts: fonts::fonts_record(doc.fonts(), &doc.fonts_in_use()),
        drafting: drafting::draft_record(&doc.draft_style()),
        layers,
        current_layer: (doc.current_layer() != LayerId::DEFAULT)
            .then(|| doc.current_layer().to_string()),
        variables,
        entities: doc.iter().map(|(id, entity)| record(id, entity)).collect(),
        constraints,
        measurements,
        options,
        fills: doc
            .fills()
            .map(|(id, f)| drafting::fill_record(doc, id, f))
            .collect(),
        annotations: doc
            .annotations()
            .map(|(id, a)| drafting::annotation_record(id, a))
            .collect(),
        shown: shown_refs
            .into_iter()
            .map(|r| drafting::shown_record(r, doc.shown(r), doc.only_in(r)))
            .collect(),
        styles: doc
            .style_overrides()
            .map(|(r, s)| drafting::override_record(r, &s))
            .collect(),
        views: doc
            .views()
            .map(|(id, v)| sheets::view_record(id, v))
            .collect(),
        sheets: doc
            .sheets()
            .map(|(id, s)| sheets::sheet_record(id, s))
            .collect(),
        groups: doc
            .groups()
            .map(|(id, g)| GroupRecord {
                id: id.to_string(),
                name: g.name.clone(),
                members: g.members.iter().map(|e| id_string(*e)).collect(),
            })
            .collect(),
        paths: doc
            .paths()
            .map(|(id, p)| PathRecord {
                id: id.to_string(),
                curves: p.curves.iter().map(|e| id_string(*e)).collect(),
                closed: p.closed,
            })
            .collect(),
        stack: doc
            .iter()
            .filter_map(|(id, e)| Some((id, e.order?)))
            .map(|(id, order)| StackRecord {
                id: id_string(id),
                order,
            })
            .collect(),
        blocks: doc
            .blocks()
            .map(|(id, b)| blocks::block_record(id, b))
            .collect(),
        instances: doc
            .instances()
            .map(|(id, i)| blocks::instance_record(id, i))
            .collect(),
        underlays: doc
            .underlays()
            .map(|(id, u)| underlays::underlay_record(id, u))
            .collect(),
        texts: doc
            .texts()
            .map(|(id, t)| texts::text_record(id, t))
            .collect(),
        walls: doc
            .walls()
            .map(|(id, w)| walls::wall_record(doc, id, w))
            .collect(),
        property_dims: doc
            .property_dims()
            .map(|(id, d)| walls::property_dim_record(id, d))
            .collect(),
        openings: doc
            .openings()
            .map(|(id, o)| openings::opening_record(doc, id, o))
            .collect(),
    }
}

/// The drawing's [`SHAREABLE`] sections as [`save`] writes them.
#[cfg(not(feature = "demo"))]
fn shareable_sections(doc: &Document) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    if !doc.custom_patterns().is_empty() {
        map.insert("patterns".into(), doc.custom_patterns().into());
    }
    if !doc.logo().is_empty() {
        map.insert("logo".into(), doc.logo().into());
    }
    if let Some(record) = fonts::fonts_record(doc.fonts(), &doc.fonts_in_use()) {
        map.insert(
            "fonts".into(),
            serde_json::to_value(record).expect("fonts serialize"),
        );
    }
    let blocks: Vec<_> = doc
        .blocks()
        .map(|(id, b)| serde_json::to_value(blocks::block_record(id, b)).expect("block serializes"))
        .collect();
    if !blocks.is_empty() {
        map.insert("blocks".into(), blocks.into());
    }
    let underlays: Vec<_> = doc
        .underlays()
        .map(|(id, u)| {
            serde_json::to_value(underlays::underlay_record(id, u)).expect("underlay serializes")
        })
        .collect();
    if !underlays.is_empty() {
        map.insert("underlays".into(), underlays.into());
    }
    map
}

/// Parse `.drift` JSON, upgrading older versions.
pub fn load(text: &str) -> Result<(Document, Metadata), LoadError> {
    load_with_notes(text).map(|(doc, meta, _)| (doc, meta))
}

/// Load a file, also saying what had to be left out to open it: relations that had become
/// meaningless (a point held on a curve it is the end of), which older builds could save.
///
/// Fields and tables this build doesn't know are passed over; when the file has any, it is
/// kept in the [`Metadata`] so that [`save`] writes them back.
pub fn load_with_notes(text: &str) -> Result<(Document, Metadata, Vec<String>), LoadError> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    // Older files were written by strict builds, whose fields this one knows.
    #[cfg(not(feature = "demo"))]
    let may_carry = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|v| v >= u64::from(FORMAT_VERSION));
    #[cfg_attr(feature = "demo", allow(unused_mut))]
    let (mut doc, mut meta, notes) = load_value(value)?;
    #[cfg(not(feature = "demo"))]
    if may_carry
        && let Some(kept) = carry::Kept::find(text, write(&doc, &meta), |id| doc.holds_id(id))
    {
        // What it names keeps referring to what the newer build meant, never to something
        // made here.
        doc.reserve_ids(kept.named());
        meta.carried = Carried(Some(std::sync::Arc::new(kept)));
    }
    Ok((doc, meta, notes))
}

/// [`load_with_notes`] from parsed JSON.
fn load_value(value: serde_json::Value) -> Result<(Document, Metadata, Vec<String>), LoadError> {
    let older_rules = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|v| v < 39);
    let value = migrate(value)?;
    // What whole-sketch options share with the drawing.
    let sections: BTreeMap<&str, serde_json::Value> = SHAREABLE
        .into_iter()
        .filter_map(|key| Some((key, value.get(key)?.clone())))
        .collect();
    let mut file: FileRecord = serde_json::from_value(value)?;
    file.meta.older_rules = older_rules;
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
    let mut exts = Vec::new();
    for mut rec in file.constraints {
        let expr = rec.expr.take();
        let ext = rec.ext.take();
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
        if let Some(ext) = ext {
            if ext.iter().flatten().any(|s| !s.is_finite()) {
                return Err(LoadError::NonFinite(id.to_string()));
            }
            exts.push((id, ext));
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
    let mut notes_out: Vec<String> = dropped
        .iter()
        .map(|(id, why)| format!("{id} ({why})"))
        .collect();
    let gone = |id: &ConstraintId| dropped.iter().any(|(d, _)| d == id);
    labels.retain(|(id, _)| !gone(id));
    driven.retain(|id| !gone(id));
    exts.retain(|(id, _)| !gone(id));
    notes.retain(|(id, _)| !gone(id));
    for (id, label) in labels {
        doc.set_placement(id, label).map_err(LoadError::Document)?;
    }
    for id in driven {
        doc.set_driven(id, true).map_err(LoadError::Document)?;
    }
    for (id, ext) in exts {
        for (end, start) in ext.into_iter().enumerate() {
            doc.set_extension_start(id, end, start)
                .map_err(LoadError::Document)?;
        }
    }
    for (id, n) in notes {
        doc.set_dim_note(id, Some(n));
    }
    if let Some(display) = &file.display {
        doc.set_units(display.to_units()?);
    }
    if let Some(rules) = &file.rules {
        doc.set_rules(rules.to_rules()?);
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
    // (What is drawn in some views only waits for the views.)
    let mut only_in = Vec::new();
    for r in file.shown {
        let (of, shown, views) = drafting::parse_shown(r)?;
        doc.set_shown(of, shown);
        if !views.is_empty() {
            only_in.push((of, views));
        }
    }
    for r in file.styles {
        let (of, style) = drafting::parse_override(r)?;
        doc.set_style_override(of, style);
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
    for s in file.stack {
        doc.set_order(parse_id(&s.id)?, Some(s.order))
            .map_err(LoadError::Document)?;
    }
    for p in file.paths {
        let id = drift_core::PathId(parse_prefixed(&p.id, 'p')?);
        if doc.path(id).is_some() {
            return Err(LoadError::DuplicateId(p.id));
        }
        let curves = p
            .curves
            .iter()
            .map(|c| parse_id(c))
            .collect::<Result<_, _>>()?;
        doc.insert_path(
            id,
            drift_core::Path {
                curves,
                closed: p.closed,
            },
        )
        .map_err(LoadError::Document)?;
    }
    let wall_notes = walls::insert_all(&mut doc, file.walls, file.property_dims)?;
    notes_out.extend(wall_notes);
    notes_out.extend(openings::insert_all(&mut doc, file.openings)?);
    for r in file.views {
        let id_text = r.id.clone();
        let (id, mut view) = sheets::parse_view(r)?;
        if doc.view(id).is_some() {
            return Err(LoadError::DuplicateId(id_text));
        }
        // A layer that's gone hides nothing.
        view.hidden_layers.retain(|l| doc.layer(*l).is_some());
        doc.insert_view(id, view);
    }
    // Drawn in some views only: those that are there. With none left, it stays off sheets.
    for (of, views) in only_in {
        let views: std::collections::BTreeSet<_> = views
            .into_iter()
            .filter(|v| doc.view(*v).is_some())
            .collect();
        if views.is_empty() {
            let mut shown = doc.shown(of);
            shown.sheets = false;
            doc.set_shown(of, shown);
        }
        doc.set_only_in(of, views);
    }
    let mut old_views = Vec::new();
    for r in file.sheets {
        let id_text = r.id.clone();
        let (id, sheet, old) = sheets::parse_sheet(r)?;
        if doc.sheet(id).is_some() {
            return Err(LoadError::DuplicateId(id_text));
        }
        if let Some(p) = sheet.viewports.iter().find(|p| doc.view(p.view).is_none()) {
            return Err(LoadError::UnknownView(p.view.to_string()));
        }
        doc.insert_sheet(id, sheet);
        old_views.push((id, old));
    }
    // Before v27 a sheet held its views itself: each becomes a model view of its own (never
    // merged, so every sheet prints as it did), named after its title or its sheet.
    for (id, old) in old_views {
        let Some(mut sheet) = doc.sheet(id).filter(|_| !old.is_empty()).cloned() else {
            continue;
        };
        let several = old.len() > 1;
        for (i, v) in old.iter().enumerate() {
            let name = match v.name.trim() {
                "" if several => format!("{} view {}", sheet.name, i + 1),
                "" => sheet.name.clone(),
                n => n.to_owned(),
            };
            let (view, mut port) = v.upgrade(&sheet, name);
            port.view = doc.add_view(view);
            sheet.viewports.push(port);
        }
        doc.insert_sheet(id, sheet);
    }
    // A mark pointing at a view that isn't there points at nothing.
    let dead: Vec<_> = doc
        .annotations()
        .filter(|(_, a)| a.view().is_some_and(|v| doc.view(v).is_none()))
        .map(|(id, a)| {
            let mut a = a.clone();
            a.set_view(None);
            (id, a)
        })
        .collect();
    for (id, a) in dead {
        doc.set_annotation(id, a).map_err(LoadError::Document)?;
    }
    if let Some(d) = file.drafting {
        doc.set_draft_style(drafting::parse_draft(d)?);
    }
    match file.project {
        Some(p) => doc.set_project(sheets::parse_project(p)?),
        None => lift_project(&mut doc),
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
    for v in file.options {
        let id = VariantId(parse_prefixed(&v.id, 'k')?);
        let mut values = BTreeMap::new();
        for (var, e) in v.values {
            let var_id = VariableId(parse_prefixed(&var, 'v')?);
            if doc.variable(var_id).is_none() {
                return Err(LoadError::Document(DocumentError::MissingVariable(var_id)));
            }
            values.insert(var_id, e);
        }
        let mut option = Variant::values(v.name, values);
        if let Some(mut sketch) = v.sketch {
            if let Some(map) = sketch.as_object_mut() {
                for key in &v.same {
                    if let Some(section) = sections.get(key.as_str()) {
                        map.insert(key.clone(), section.clone());
                    }
                }
            }
            let (sketch, _, notes) = load_value(sketch)?;
            notes_out.extend(
                notes
                    .into_iter()
                    .map(|n| format!("{n}, in option {}", option.name)),
            );
            option.sketch = Some(sketch.sketch_copy().into());
        }
        doc.set_variant(id, option);
    }
    Ok((doc, file.meta, notes_out))
}

/// A file without a project (from before v30): a title or company every sheet shares
/// becomes the project's, and the sheets' own are cleared, so they print as before and a new
/// sheet gets them too.
fn lift_project(doc: &mut Document) {
    let sheets: Vec<(drift_core::SheetId, drift_core::Sheet)> =
        doc.sheets().map(|(id, s)| (id, s.clone())).collect();
    let shared = |get: fn(&drift_core::Sheet) -> &String| -> Option<String> {
        let first = get(&sheets.first()?.1).trim().to_owned();
        (!first.is_empty() && sheets.iter().all(|(_, s)| get(s).trim() == first)).then_some(first)
    };
    let title = shared(|s| &s.fields.title);
    let company = shared(|s| &s.fields.company);
    if title.is_none() && company.is_none() {
        return;
    }
    let mut project = doc.project().clone();
    for (id, mut sheet) in sheets {
        if let Some(t) = &title {
            project.name.clone_from(t);
            sheet.fields.title.clear();
        }
        if let Some(c) = &company {
            project.company.clone_from(c);
            sheet.fields.company.clear();
        }
        doc.set_sheet(id, sheet);
    }
    doc.set_project(project);
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
    // The oldest version that reads the file (v42); before, the version itself.
    let reader = match value.get("reader") {
        None => version,
        Some(r) => r
            .as_u64()
            .and_then(|r| u32::try_from(r).ok())
            .filter(|r| *r > 0)
            .ok_or(LoadError::NotADriftFile)?,
    };
    if version == 0 || reader > FORMAT_VERSION {
        return Err(LoadError::UnsupportedVersion(version.max(reader)));
    }
    let mut value = value;
    if version < 2 {
        // v1 → v2: constraints were introduced; a v1 file simply has none.
        value["constraints"] = serde_json::Value::Array(Vec::new());
    }
    if version < 41
        && let Some(variants) = value.as_object_mut().and_then(|m| m.remove("variants"))
    {
        // v40 → v41: variants (values only) are options.
        value["options"] = variants;
    }
    // v38 → v39: a drawing's rules were the app's preferences; the file is read with the
    // defaults, and `Metadata::older_rules` tells the app to apply its own.
    // v2 → v26 only added optional fields, sections and constraint types: nothing to convert
    // here (a sheet's own views, from before v27, are made model views as the file is read).
    // v42 → v43 added the `fix_x` and `fix_y` relation types: nothing to convert.
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
    fn drawing_rules_round_trip() {
        let mut doc = demo::mounting_plate();
        let text = save(&doc, &Metadata::default());
        assert!(!text.contains("\"rules\""), "the defaults aren't written");
        let rules = DrawingRules {
            regions_by_layer: true,
            regions_need_fill: false,
            stacked_fills: false,
            follow_divided_fills: true,
            gap_tolerance: 0.25,
            size_set: drift_core::SizeSetId::new("din-18101"),
        };
        doc.set_rules(rules);
        let text = save(&doc, &Metadata::default());
        assert!(text.contains("\"rules\": {"), "{text}");
        let (loaded, meta) = load(&text).unwrap();
        assert_eq!(loaded.rules(), rules);
        assert!(!meta.older_rules);
        assert_eq!(save(&loaded, &meta), text);
        // A bad tolerance doesn't open.
        let bad = text.replace("\"gap_tolerance\":0.25", "\"gap_tolerance\":-1.0");
        assert_ne!(bad, text);
        assert!(load(&bad).is_err());
        assert!(text.contains("\"size_set\":\"din-18101\""), "{text}");
    }

    #[test]
    fn a_file_from_before_drawing_rules_opens_with_the_defaults_and_says_so() {
        let doc = demo::mounting_plate();
        let text = save(&doc, &Metadata::default());
        let old = text.replace(
            &format!(r#""version": {FORMAT_VERSION}"#),
            r#""version": 38"#,
        );
        assert_ne!(old, text);
        let (loaded, meta) = load(&old).unwrap();
        assert_eq!(loaded.rules(), DrawingRules::default());
        assert!(meta.older_rules, "the app applies its own");
        // Saved again, it is a current file.
        assert!(save(&loaded, &meta).contains(&format!(r#""version": {FORMAT_VERSION}"#)));
        let (_, meta) = load(&save(&loaded, &meta)).unwrap();
        assert!(!meta.older_rules);
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
  "version": 43,
  "reader": 43,
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
    fn extension_line_starts_set_by_hand_round_trip() {
        let mut doc = Document::new();
        let a = doc.add_point(DVec2::new(0.0, 0.0));
        let b = doc.add_point(DVec2::new(40.0, 0.0));
        let c = doc.add_point(DVec2::new(0.0, 30.0));
        let d = doc.add_point(DVec2::new(40.0, 30.0));
        let h = doc
            .add_constraint(Constraint::HorizontalDistance { a, b, value: 40.0 })
            .unwrap();
        let both = doc
            .add_constraint(Constraint::Distance {
                a: c,
                b: d,
                value: 40.0,
            })
            .unwrap();
        let auto = doc
            .add_constraint(Constraint::Distance {
                a,
                b: d,
                value: 50.0,
            })
            .unwrap();
        // One end set by hand, the other left automatic; both ends set; none.
        doc.set_extension_start(h, 1, Some(12.5)).unwrap();
        doc.set_extension_start(both, 0, Some(-2.0)).unwrap();
        doc.set_extension_start(both, 1, Some(3.25)).unwrap();
        let text = save(&doc, &Metadata::default());
        assert!(text.contains(r#""ext":[null,12.5]"#), "{text}");
        assert!(text.contains(r#""ext":[-2.0,3.25]"#), "{text}");
        let (loaded, _) = load(&text).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(loaded.extension_starts(h), [None, Some(12.5)]);
        assert_eq!(loaded.extension_starts(both), [Some(-2.0), Some(3.25)]);
        assert_eq!(loaded.extension_starts(auto), [None, None]);
        // Back to automatic: nothing written.
        let mut doc = loaded;
        doc.set_extension_start(h, 1, None).unwrap();
        assert!(!save(&doc, &Metadata::default()).contains(r#""ext":[null"#));
        // A file from before extension starts opens with them all automatic.
        let old = text.replace(r#","ext":[null,12.5]"#, "");
        let old = old.replace(r#","ext":[-2.0,3.25]"#, "");
        let old = old.replace(
            &format!(r#""version": {FORMAT_VERSION}"#),
            r#""version": 33"#,
        );
        let (loaded, _) = load(&old).unwrap();
        assert_eq!(loaded.extension_starts(h), [None, None]);
        assert_eq!(loaded.extension_starts(both), [None, None]);
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
        doc.add_variant(Variant::values("Small", [(w, "5 mm".to_owned())].into()));
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

    /// A drawing with a variable, an embedded font, a block placed once, and both kinds of
    /// option.
    fn optioned_drawing() -> (Document, VariantId, VariantId) {
        let mut doc = demo::mounting_plate();
        let w = doc
            .add_variable(Variable {
                name: "w".into(),
                expr: "10 mm".into(),
                kind: Kind::Length,
            })
            .unwrap();
        doc.set_fonts(drift_core::FontSet {
            embedded: [(
                "Big".to_owned(),
                drift_core::EmbeddedFont {
                    regular: drift_core::FontFile::new(vec![1, 2, 3, 4, 5], 0),
                    bold: None,
                },
            )]
            .into(),
            drawing: Some("Big".into()),
        });
        let block = doc.add_block(drift_core::Block {
            name: "Mark".into(),
            items: vec![drift_core::BlockItem {
                shape: drift_core::Shape::Line {
                    a: DVec2::ZERO,
                    b: DVec2::new(5.0, 0.0),
                },
                style: Default::default(),
                appearance: Default::default(),
            }],
            fills: Vec::new(),
            profile: false,
        });
        doc.add_instance(drift_core::Instance::new(block, DVec2::new(1.0, 2.0)))
            .unwrap();
        let values = doc.add_variant(Variant::values("Narrow", [(w, "5 mm".to_owned())].into()));
        let whole = doc.add_variant(Variant::sketch("Before", &doc));
        // The drawing moves on from the copy.
        let first = doc.iter().find(|(_, e)| e.kind.is_point()).unwrap().0;
        doc.set_point(first, DVec2::new(-7.0, -7.0)).unwrap();
        (doc, values, whole)
    }

    #[test]
    fn options_of_both_kinds_round_trip() {
        let (doc, values, whole) = optioned_drawing();
        let text = save(&doc, &Metadata::default());
        assert!(text.contains("\"options\": ["), "{text}");
        assert!(!text.contains("\"variants\""));
        // The copy leaves out what it has the same as the drawing (the font, the block).
        assert!(
            text.contains(r#""same":["fonts","blocks"]"#),
            "{}",
            text.lines().find(|l| l.contains("Before")).unwrap_or("")
        );
        assert_eq!(
            text.matches("AQIDBAU").count(),
            1,
            "the font is written once"
        );
        let (loaded, _) = load(&text).unwrap();
        assert_eq!(loaded, doc);
        let sketch = loaded.variant(whole).unwrap().sketch.as_ref().unwrap();
        assert_eq!(sketch.fonts(), doc.fonts());
        assert_eq!(sketch.blocks().count(), 1);
        assert_eq!(sketch.variants().count(), 0);
        assert!(!loaded.variant(values).unwrap().is_sketch());
        assert_eq!(save(&loaded, &Metadata::default()), text);
    }

    #[test]
    fn a_whole_sketch_option_keeps_what_differs_from_the_drawing() {
        let (mut doc, _, whole) = optioned_drawing();
        doc.set_fonts(drift_core::FontSet::default());
        let text = save(&doc, &Metadata::default());
        assert!(text.contains(r#""same":["blocks"]"#), "{text}");
        let (loaded, _) = load(&text).unwrap();
        let sketch = loaded.variant(whole).unwrap().sketch.as_ref().unwrap();
        assert_eq!(sketch.fonts().drawing.as_deref(), Some("Big"));
        assert_eq!(loaded.fonts().drawing, None);
    }

    #[test]
    fn a_block_fill_that_filled_a_shape_says_so() {
        let mut doc = Document::new();
        let mut fill = drift_core::Fill::new(
            Vec::new(),
            DVec2::new(1.0, 1.0),
            drift_core::FillKind::Solid,
        );
        fill.path = Some(drift_core::blocks::SHAPE_FILL);
        let area = drift_core::Fill::new(
            Vec::new(),
            DVec2::new(3.0, 3.0),
            drift_core::FillKind::Solid,
        );
        doc.add_block(drift_core::Block {
            name: "Tile".into(),
            items: Vec::new(),
            fills: vec![fill, area],
            profile: false,
        });
        let text = save(&doc, &Metadata::default());
        assert_eq!(text.matches(r#""path":"shape""#).count(), 1, "{text}");
        let (loaded, _) = load(&text).unwrap();
        assert_eq!(loaded, doc);
    }

    #[test]
    fn version_40_variants_open_as_options_of_values() {
        let mut doc = demo::mounting_plate();
        let w = doc
            .add_variable(Variable {
                name: "w".into(),
                expr: "10 mm".into(),
                kind: Kind::Length,
            })
            .unwrap();
        let k = doc.add_variant(Variant::values("Narrow", [(w, "5 mm".to_owned())].into()));
        let text = save(&doc, &Metadata::default());
        let old = text
            .replace(&format!("\"version\": {FORMAT_VERSION}"), "\"version\": 40")
            .replace("\"options\"", "\"variants\"");
        assert!(old.contains(r#""variants": ["#) && old.contains("\"version\": 40"));
        let (loaded, _) = load(&old).unwrap();
        let option = loaded.variant(k).unwrap();
        assert_eq!(option.name, "Narrow");
        assert_eq!(option.values, [(w, "5 mm".to_owned())].into());
        assert!(!option.is_sketch());
        assert_eq!(save(&loaded, &Metadata::default()), text);
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
    fn a_sheets_own_views_become_model_views() {
        use drift_core::{Bounds, ViewFrame};
        // v25: a sheet with a view filling its drawing area and a framed, titled detail; a
        // second sheet whose one view has no name.
        let v25 = r#"{
  "format": "drift",
  "version": 25,
  "meta": {"units":"mm"},
  "entities": [],
  "sheets": [
    {"id":"s1","name":"A-101","paper":"A3","views":[
      {"scale":50.0,"center":[1000.0,2000.0],"notes":false},
      {"name":"Detail A","scale":5.0,"center":[10.0,20.0],"at":[300.0,200.0],"size":[80.0,60.0],"title":true,"border":true}
    ],"title_block":"iso"},
    {"id":"s2","name":"A-102","paper":"A4","portrait":true,"views":[{"scale":2.0,"center":[0.0,0.0]}],"title_block":"modern"}
  ]
}"#;
        let (doc, _) = load(v25).unwrap();
        assert_eq!(doc.views().count(), 3, "one model view per sheet view");
        let sheets: Vec<_> = doc.sheets().map(|(_, s)| s.clone()).collect();
        let plan = doc.placed_views(&sheets[0]);
        assert_eq!(plan.len(), 2);
        // The view that filled the sheet is cropped to the drawing area, centred where it was.
        let area = drift_draft::sheet::drawing_area(&sheets[0]);
        assert_eq!(plan[0].name, "A-101 view 1");
        assert_eq!(
            plan[0].frame,
            ViewFrame {
                at: area.center(),
                size: area.size()
            }
        );
        assert_eq!(
            (plan[0].scale, plan[0].center),
            (50.0, DVec2::new(1000.0, 2000.0))
        );
        assert!(plan[0].dimensions && !plan[0].notes);
        let crop = doc.view(plan[0].id).unwrap().crop();
        assert!((crop.size() - area.size() * 50.0).abs().max_element() < 1e-9);
        // The framed detail keeps its place, size and labels.
        assert_eq!(plan[1].name, "Detail A");
        assert_eq!(
            plan[1].frame,
            ViewFrame {
                at: DVec2::new(300.0, 200.0),
                size: DVec2::new(80.0, 60.0)
            }
        );
        assert!(plan[1].title && plan[1].border);
        assert_eq!(
            doc.view(plan[1].id).unwrap().crop(),
            Bounds {
                min: DVec2::new(-190.0, -130.0),
                max: DVec2::new(210.0, 170.0)
            }
        );
        // An unnamed view alone on its sheet is named after the sheet.
        let other = doc.placed_views(&sheets[1]);
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].name, "A-102");
        // Saved, it is written the new way and reads back the same.
        let text = save(&doc, &Metadata::default());
        assert!(!text.contains(r#""at":[300.0,200.0],"size""#), "{text}");
        let (again, _) = load(&text).unwrap();
        assert_eq!(again, doc);

        // Before v18 a sheet had one view, in its own fields.
        let v17 = r#"{"format":"drift","version":17,"meta":{"units":"mm"},"entities":[],
            "sheets":[{"id":"s1","name":"Old","paper":"A3","scale":10.0,"center":[5.0,5.0],
            "dimensions":false,"title_block":"none"}]}"#;
        let (old, _) = load(v17).unwrap();
        let sheet = old.sheets().next().unwrap().1.clone();
        let views = old.placed_views(&sheet);
        assert_eq!(views.len(), 1);
        assert_eq!(
            (views[0].scale, views[0].center),
            (10.0, DVec2::new(5.0, 5.0))
        );
        assert!(!views[0].dimensions && views[0].notes);
        assert_eq!(
            views[0].frame.bounds(),
            drift_draft::sheet::drawing_area(&sheet)
        );

        // A sheet placing a view the file doesn't have is refused.
        let dangling = r#"{"format":"drift","version":26,"meta":{"units":"mm"},"entities":[],
            "sheets":[{"id":"s1","name":"S","paper":"A3","viewports":[{"view":"w9","at":[0.0,0.0]}],
            "title_block":"none"}]}"#;
        assert!(matches!(load(dangling), Err(LoadError::UnknownView(_))));
        let flat = r#"{"format":"drift","version":26,"meta":{"units":"mm"},"entities":[],
            "views":[{"id":"w1","scale":1.0,"center":[0.0,0.0],"size":[0.0,10.0]}]}"#;
        assert!(load(flat).is_err(), "a crop with no width");
    }

    #[test]
    fn a_blocks_profile_flag_round_trips_and_old_blocks_are_not_profiles() {
        let mut doc = Document::new();
        for (name, profile) in [("Crown", true), ("Chair", false)] {
            doc.add_block(drift_core::Block {
                name: name.into(),
                profile,
                ..Default::default()
            });
        }
        let text = save(&doc, &Metadata::default());
        assert_eq!(text.matches(r#""profile":true"#).count(), 1, "{text}");
        assert!(!text.contains(r#""profile":false"#), "{text}");
        let (again, _) = load(&text).unwrap();
        assert_eq!(again, doc);
        // A file from before the flag: its blocks aren't profiles.
        let v35 = r#"{"format":"drift","version":35,"meta":{"units":"mm"},"entities":[],
            "blocks":[{"id":"b1","name":"Old","items":[{"type":"line","a":[0,0],"b":[1,0]}]}]}"#;
        let (old, _) = load(v35).unwrap();
        assert!(!old.blocks().next().unwrap().1.profile);
    }

    #[test]
    fn blank_paper_round_trips_and_old_none_keeps_its_border() {
        use drift_core::{Sheet, TitleBlock};
        let mut doc = Document::new();
        for (name, title_block) in [("Blank", TitleBlock::None), ("Framed", TitleBlock::Border)] {
            let mut sheet = Sheet::new(name);
            sheet.title_block = title_block;
            doc.add_sheet(sheet);
        }
        let text = save(&doc, &Metadata::default());
        assert!(text.contains(r#""title_block":"blank""#), "{text}");
        assert!(text.contains(r#""title_block":"none""#), "{text}");
        let (again, _) = load(&text).unwrap();
        assert_eq!(again, doc);
        // A file from before blank paper: its "none" is the border, as it printed then.
        let v34 = r#"{"format":"drift","version":34,"meta":{"units":"mm"},"entities":[],
            "sheets":[{"id":"s1","name":"Old","paper":"A3","title_block":"none"}]}"#;
        let (old, _) = load(v34).unwrap();
        assert_eq!(
            old.sheets().next().unwrap().1.title_block,
            TitleBlock::Border
        );
    }

    #[test]
    fn sheet_colours_round_trip_and_default_to_colour() {
        use drift_core::{Sheet, SheetColours};
        let mut doc = Document::new();
        for (name, colours) in [
            ("Colour", SheetColours::Colour),
            ("Grey", SheetColours::Greyscale),
            ("Mono", SheetColours::BlackWhite),
        ] {
            let mut sheet = Sheet::new(name);
            sheet.colours = colours;
            doc.add_sheet(sheet);
        }
        let text = save(&doc, &Metadata::default());
        assert!(text.contains(r#""colours":"greyscale""#), "{text}");
        assert!(text.contains(r#""colours":"mono""#), "{text}");
        // Colour, the default, stays out of the file.
        assert_eq!(text.matches(r#""colours""#).count(), 2, "{text}");
        let (loaded, _) = load(&text).unwrap();
        assert_eq!(loaded, doc);
        let modes: Vec<_> = loaded.sheets().map(|(_, s)| s.colours).collect();
        assert_eq!(
            modes,
            [
                SheetColours::Colour,
                SheetColours::Greyscale,
                SheetColours::BlackWhite
            ]
        );
        // A sheet from before v25 prints in colour.
        let old = r#"{
  "format": "drift",
  "version": 24,
  "meta": {"units":"mm"},
  "entities": [],
  "sheets": [
    {"id":"s1","name":"Old","paper":"A3","views":[{"scale":1.0,"center":[0.0,0.0]}],"title_block":"modern"}
  ]
}"#;
        let (old, _) = load(old).unwrap();
        assert_eq!(old.sheets().next().unwrap().1.colours, SheetColours::Colour);
        // An unknown mode is refused.
        let bad = text.replace(r#""colours":"mono""#, r#""colours":"sepia""#);
        assert!(load(&bad).is_err());
    }

    #[test]
    fn the_project_is_saved_and_older_sheets_share_theirs() {
        use drift_core::{FieldSet, Project, Sheet};
        let mut doc = Document::new();
        doc.set_project(Project {
            name: "Riverside house".into(),
            number: "2026-014".into(),
            client: "Harbour Trust".into(),
            address: "4 Quay Lane".into(),
            company: "Plange Studio".into(),
            fields: FieldSet::Architectural,
        });
        let mut sheet = Sheet::new("Ground floor");
        sheet.fields.status = "For permit".into();
        doc.add_sheet(sheet);
        let text = save(&doc, &Metadata::default());
        assert!(text.contains(r#""fields":"architectural""#), "{text}");
        assert!(text.contains(r#""status":"For permit""#), "{text}");
        let (loaded, _) = load(&text).unwrap();
        assert_eq!(loaded, doc);
        // No project: nothing written.
        let plain = save(&Document::new(), &Metadata::default());
        assert!(!plain.contains(r#""project""#));
        // Before v30, a title and company every sheet shares become the project's.
        let old = r#"{
  "format": "drift",
  "version": 29,
  "meta": {"units":"mm"},
  "entities": [],
  "sheets": [
    {"id":"s1","name":"One","paper":"A3","title_block":"modern","fields":{"title":"Plate","company":"Acme","number":"A-101"}},
    {"id":"s2","name":"Two","paper":"A3","title_block":"modern","fields":{"title":"Plate","company":"Other"}}
  ]
}"#;
        let (old, _) = load(old).unwrap();
        assert_eq!(old.project().name, "Plate");
        assert_eq!(old.project().company, "", "the companies differ");
        let sheets: Vec<_> = old.sheets().map(|(_, s)| s.fields.clone()).collect();
        assert!(sheets.iter().all(|f| f.title.is_empty()));
        assert_eq!(sheets[0].company, "Acme");
        assert_eq!(sheets[0].number, "A-101");
        // An unknown field set is refused.
        let bad = text.replace(r#""fields":"architectural""#, r#""fields":"nautical""#);
        assert!(load(&bad).is_err());
    }

    #[test]
    fn a_mark_linked_to_a_missing_view_loads_unlinked() {
        let text = r#"{
  "format": "drift",
  "version": 29,
  "meta": {"units":"mm"},
  "entities": [],
  "annotations": [
    {"type":"section","id":"n1","a":[0.0,0.0],"b":[10.0,0.0],"label":"A","view":"w9"},
    {"type":"elevation","id":"n2","at":[0.0,0.0],"angle":0.0,"label":"1","view":"w3"}
  ],
  "views": [
    {"id":"w3","name":"East","scale":50.0,"center":[0.0,0.0],"size":[100.0,100.0]}
  ]
}"#;
        let (doc, _) = load(text).unwrap();
        let views: Vec<_> = doc.annotations().map(|(_, a)| a.view()).collect();
        assert_eq!(views, [None, Some(drift_core::ViewId(3))]);
    }

    #[test]
    fn a_views_construction_switch_round_trips() {
        let mut doc = Document::new();
        let crop = drift_core::Bounds {
            min: DVec2::ZERO,
            max: DVec2::new(100.0, 50.0),
        };
        let plain = doc.add_view(drift_core::View::new("Plan", crop, 50.0));
        let mut guides = drift_core::View::new("Setting out", crop, 50.0);
        guides.construction = true;
        let guides = doc.add_view(guides);
        let text = save(&doc, &Metadata::default());
        // Written only when shown.
        assert_eq!(text.matches(r#""construction":true"#).count(), 1, "{text}");
        assert!(!text.contains(r#""construction":false"#));
        let (loaded, _) = load(&text).unwrap();
        assert_eq!(loaded, doc);
        assert!(!loaded.view(plain).unwrap().construction);
        assert!(loaded.view(guides).unwrap().construction);
    }

    #[test]
    fn a_view_from_before_v32_hides_construction() {
        let text = r#"{
  "format": "drift",
  "version": 31,
  "meta": {"units":"mm"},
  "entities": [],
  "views": [
    {"id":"w1","name":"Plan","scale":50.0,"center":[0.0,0.0],"size":[100.0,100.0]}
  ]
}"#;
        let (doc, _) = load(text).unwrap();
        assert!(!doc.view(drift_core::ViewId(1)).unwrap().construction);
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
        h.divided = true;
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
        // One dimension with settings of its own: bigger text, open arrows, no dual units
        // (though the drawing has them) and its own colour.
        doc.set_style_override(
            drift_core::AnnotationRef::Dimension(dims[2]),
            drift_core::StyleOverride {
                text_height: Some(5.0),
                arrow: Some(ArrowStyle::Open),
                leading_zero: Some(false),
                alt_units: Some(None),
                color: Some(Some(drift_core::Rgb([0xc0, 0x30, 0x30]))),
                ..Default::default()
            },
        );
        doc.set_custom_patterns("*MINE, Mine\n0, 0,0, 0,2\n".into());
        doc.set_logo(
            r##"<svg viewBox="0 0 4 2"><rect width="4" height="2" fill="#f60"/></svg>"##.into(),
        );
        let mut sheet = drift_core::Sheet::new("Plate");
        sheet.paper = drift_core::PaperSize::Custom(500.0, 350.0);
        sheet.portrait = true;
        let crop = |min: [f64; 2], max: [f64; 2]| drift_core::Bounds {
            min: DVec2::from_array(min),
            max: DVec2::from_array(max),
        };
        let plan = doc.add_view(drift_core::View::new(
            "Plan",
            crop([-200.0, -300.0], [320.0, 380.0]),
            2.5,
        ));
        let mut detail_view =
            drift_core::View::new("Detail A", crop([-18.0, -14.5], [42.0, 30.5]), 0.5);
        detail_view.crop_visible = false;
        let detail_id = doc.add_view(detail_view);
        sheet
            .viewports
            .push(drift_core::Viewport::new(plan, DVec2::new(180.0, 250.0)));
        let mut port = drift_core::Viewport::new(detail_id, DVec2::new(400.0, 250.0));
        port.title = true;
        port.border = true;
        sheet.viewports.push(port);
        sheet.title_block = drift_core::TitleBlock::Ansi;
        sheet.colours = drift_core::SheetColours::BlackWhite;
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
        detail.colours = drift_core::SheetColours::Greyscale;
        let mut bare = drift_core::View::new("Bare", crop([0.0, 0.0], [100.0, 50.0]), 1.0);
        bare.dimensions = false;
        bare.notes = false;
        let bare = doc.add_view(bare);
        detail
            .viewports
            .push(drift_core::Viewport::new(bare, DVec2::new(100.0, 100.0)));
        // A view can be on several sheets.
        detail.viewports.push(drift_core::Viewport::new(
            detail_id,
            DVec2::new(300.0, 150.0),
        ));
        doc.add_sheet(detail);
        // And on none.
        doc.add_view(drift_core::View::new(
            "Key plan",
            crop([0.0, 0.0], [10.0, 10.0]),
            200.0,
        ));
        doc.add_annotation(drift_core::Annotation::Section {
            a: DVec2::new(-50.0, 0.0),
            b: DVec2::new(50.0, 0.0),
            label: "A".into(),
            sheet: String::new(),
            flip: true,
            view: Some(detail_id),
        })
        .unwrap();
        doc.add_annotation(drift_core::Annotation::Elevation {
            at: DVec2::new(0.0, -60.0),
            angle: 90.0,
            label: "1".into(),
            sheet: "A-201".into(),
            view: None,
        })
        .unwrap();
        let chair = doc.add_block(drift_core::Block {
            name: "Chair".into(),
            profile: true,
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
        // A shape drawn as one, with its fill.
        let shape = doc
            .add_path(drift_core::Path {
                curves: curves.iter().copied().skip(2).take(2).collect(),
                closed: true,
            })
            .unwrap();
        let mut shaped = drift_core::Fill::new(
            Vec::new(),
            DVec2::new(1.0, 2.0),
            drift_core::FillKind::Solid,
        );
        shaped.path = Some(shape);
        doc.add_fill(shaped);
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
        // A survey with a picture only (two pixels, one half see-through).
        let mut survey = drift_core::Underlay::new("Survey", "", Vec::new());
        survey.images = std::sync::Arc::new(vec![drift_core::UnderlayImage {
            width: 2,
            height: 1,
            rgba: std::sync::Arc::new(vec![255, 0, 0, 255, 0, 0, 255, 128]),
            at: DVec2::new(0.0, 10.0),
            across: DVec2::new(20.0, 0.0),
            down: DVec2::new(0.0, -10.0),
        }]);
        // Placed in the stack, on a layer among its fills.
        survey.layer = Some(LayerId::DEFAULT);
        survey.order = 3;
        doc.add_underlay(survey);
        // A curve with a place of its own in the stack.
        let first_curve = doc
            .iter()
            .find(|(_, e)| !e.kind.is_point())
            .map(|(id, _)| id)
            .unwrap();
        doc.set_order(first_curve, Some(-2)).unwrap();
        let mut sign = drift_core::SketchText::new("EXIT → 2", DVec2::new(5.0, -40.0), 12.5);
        sign.angle = 15.0;
        sign.font = drift_core::TextFont::Mono;
        sign.appearance.color = Some(Rgb([200, 10, 10]));
        doc.add_text(sign);
        doc.add_text(drift_core::SketchText::new("A", DVec2::ZERO, 3.0));

        let text = save(&doc, &Metadata::default());
        assert!(text.contains(&format!("\"version\": {FORMAT_VERSION}")));
        assert!(text.contains(r#""layer":"l0","order":3"#), "{text}");
        assert!(
            text.contains(&format!(
                r#""stack": [
    {{"id":"{first_curve}","order":-2}}"#
            )),
            "{text}"
        );
        // A fill an edit divided keeps its mark.
        assert!(text.contains(r#""divided":true"#), "{text}");
        // A mark points at its view.
        assert!(text.contains(r#""flip":true,"view":"w"#), "{text}");
        assert!(text.contains(r#""styles": ["#) && text.contains(r#""alt_units":"none""#));
        // Views are the model's; sheets place them.
        assert!(text.contains(r#""views": ["#) && text.contains(r#""crop":false"#));
        assert!(text.contains(r#""viewports":[{"view":"w"#), "{text}");
        assert!(
            text.contains(r#"{"a":[80.0,20.0,30.0,0.0,90.0]}"#),
            "{text}"
        );
        assert!(text.contains(r#""below":"TO FACE OF STUD""#) && text.contains(r#""text":"EQ""#));
        assert!(text.contains(r#""shown": ["#) && text.contains(r#""canvas":false"#));
        assert!(
            text.contains(
                r#""images":[{"at":[0.0,10.0],"across":[20.0,0.0],"down":[0.0,-10.0],"png":"iVBOR"#
            ),
            "{text}"
        );
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
            "views",
            "groups",
            "paths",
            "blocks",
            "instances",
            "underlays",
            "texts",
        ] {
            assert!(!plain.contains(absent), "{absent}");
        }
    }
}
