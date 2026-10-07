//! The native `.drift` file format.
//!
//! A `.drift` file is UTF-8 JSON with one entity or constraint per line, so files diff and
//! merge cleanly in version control:
//!
//! ```json
//! {
//!   "format": "drift",
//!   "version": 2,
//!   "meta": {"title":"Bracket","units":"mm","generator":"Drift 0.1.0"},
//!   "entities": [
//!     {"type":"point","id":"e1","at":[0.0,0.0]},
//!     {"type":"point","id":"e2","at":[120.0,0.0]},
//!     {"type":"line","id":"e3","a":"e1","b":"e2"}
//!   ],
//!   "constraints": [
//!     {"type":"horizontal","id":"c1","line":"e3"},
//!     {"type":"distance","id":"c2","a":"e1","b":"e2","value":120.0}
//!   ]
//! }
//! ```
//!
//! Angles are stored in degrees, as the user typed them.
//!
//! Numbers use the shortest representation that round-trips exactly, so save → load is
//! bit-exact. Older versions are upgraded by [`migrate`] before parsing.

use drift_core::{
    Constraint, ConstraintId, DVec2, Document, DocumentError, Entity, EntityId, EntityKind,
    LineKind,
};
use serde::{Deserialize, Serialize};

/// File extension, without the dot.
pub const EXTENSION: &str = "drift";
/// The version this build writes. Files with a higher version are rejected.
///
/// History: 1 = entities only (M1); 2 = adds `constraints` (M2).
pub const FORMAT_VERSION: u32 = 2;
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ConstraintRecord {
    Coincident {
        id: String,
        a: String,
        b: String,
    },
    Horizontal {
        id: String,
        line: String,
    },
    Vertical {
        id: String,
        line: String,
    },
    HorizontalPoints {
        id: String,
        a: String,
        b: String,
    },
    VerticalPoints {
        id: String,
        a: String,
        b: String,
    },
    Parallel {
        id: String,
        a: String,
        b: String,
    },
    Perpendicular {
        id: String,
        a: String,
        b: String,
    },
    Equal {
        id: String,
        a: String,
        b: String,
    },
    Fix {
        id: String,
        point: String,
        at: [f64; 2],
    },
    FixRadius {
        id: String,
        curve: String,
        radius: f64,
    },
    Distance {
        id: String,
        a: String,
        b: String,
        value: f64,
    },
    Radius {
        id: String,
        curve: String,
        value: f64,
    },
    Angle {
        id: String,
        a: String,
        b: String,
        degrees: f64,
    },
}

fn constraint_record(id: ConstraintId, c: &Constraint) -> ConstraintRecord {
    let id = id.to_string();
    let s = id_string;
    match *c {
        Constraint::Coincident { a, b } => ConstraintRecord::Coincident {
            id,
            a: s(a),
            b: s(b),
        },
        Constraint::Horizontal { line } => ConstraintRecord::Horizontal { id, line: s(line) },
        Constraint::Vertical { line } => ConstraintRecord::Vertical { id, line: s(line) },
        Constraint::HorizontalPoints { a, b } => ConstraintRecord::HorizontalPoints {
            id,
            a: s(a),
            b: s(b),
        },
        Constraint::VerticalPoints { a, b } => ConstraintRecord::VerticalPoints {
            id,
            a: s(a),
            b: s(b),
        },
        Constraint::Parallel { a, b } => ConstraintRecord::Parallel {
            id,
            a: s(a),
            b: s(b),
        },
        Constraint::Perpendicular { a, b } => ConstraintRecord::Perpendicular {
            id,
            a: s(a),
            b: s(b),
        },
        Constraint::Equal { a, b } => ConstraintRecord::Equal {
            id,
            a: s(a),
            b: s(b),
        },
        Constraint::Fix { point, at } => ConstraintRecord::Fix {
            id,
            point: s(point),
            at: at.to_array(),
        },
        Constraint::FixRadius { curve, radius } => ConstraintRecord::FixRadius {
            id,
            curve: s(curve),
            radius,
        },
        Constraint::Distance { a, b, value } => ConstraintRecord::Distance {
            id,
            a: s(a),
            b: s(b),
            value,
        },
        Constraint::Radius { curve, value } => ConstraintRecord::Radius {
            id,
            curve: s(curve),
            value,
        },
        Constraint::Angle { a, b, degrees } => ConstraintRecord::Angle {
            id,
            a: s(a),
            b: s(b),
            degrees,
        },
    }
}

fn parse_constraint_id(s: &str) -> Result<ConstraintId, LoadError> {
    s.strip_prefix('c')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .map(ConstraintId)
        .ok_or_else(|| LoadError::BadId(s.to_owned()))
}

fn parse_constraint(rec: ConstraintRecord) -> Result<(ConstraintId, Constraint), LoadError> {
    let e = |s: &str| parse_id(s);
    let (id, c) = match rec {
        ConstraintRecord::Coincident { id, a, b } => (
            id,
            Constraint::Coincident {
                a: e(&a)?,
                b: e(&b)?,
            },
        ),
        ConstraintRecord::Horizontal { id, line } => {
            (id, Constraint::Horizontal { line: e(&line)? })
        }
        ConstraintRecord::Vertical { id, line } => (id, Constraint::Vertical { line: e(&line)? }),
        ConstraintRecord::HorizontalPoints { id, a, b } => (
            id,
            Constraint::HorizontalPoints {
                a: e(&a)?,
                b: e(&b)?,
            },
        ),
        ConstraintRecord::VerticalPoints { id, a, b } => (
            id,
            Constraint::VerticalPoints {
                a: e(&a)?,
                b: e(&b)?,
            },
        ),
        ConstraintRecord::Parallel { id, a, b } => (
            id,
            Constraint::Parallel {
                a: e(&a)?,
                b: e(&b)?,
            },
        ),
        ConstraintRecord::Perpendicular { id, a, b } => (
            id,
            Constraint::Perpendicular {
                a: e(&a)?,
                b: e(&b)?,
            },
        ),
        ConstraintRecord::Equal { id, a, b } => (
            id,
            Constraint::Equal {
                a: e(&a)?,
                b: e(&b)?,
            },
        ),
        ConstraintRecord::Fix { id, point, at } => (
            id,
            Constraint::Fix {
                point: e(&point)?,
                at: DVec2::from(at),
            },
        ),
        ConstraintRecord::FixRadius { id, curve, radius } => (
            id,
            Constraint::FixRadius {
                curve: e(&curve)?,
                radius,
            },
        ),
        ConstraintRecord::Distance { id, a, b, value } => (
            id,
            Constraint::Distance {
                a: e(&a)?,
                b: e(&b)?,
                value,
            },
        ),
        ConstraintRecord::Radius { id, curve, value } => (
            id,
            Constraint::Radius {
                curve: e(&curve)?,
                value,
            },
        ),
        ConstraintRecord::Angle { id, a, b, degrees } => (
            id,
            Constraint::Angle {
                a: e(&a)?,
                b: e(&b)?,
                degrees,
            },
        ),
    };
    Ok((parse_constraint_id(&id)?, c))
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
    entities: Vec<EntityRecord>,
    #[serde(default)]
    constraints: Vec<ConstraintRecord>,
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
    let entities: Vec<String> = doc
        .iter()
        .map(|(id, entity)| serde_json::to_string(&record(id, entity)).expect("entity serializes"))
        .collect();
    let constraints: Vec<String> = doc
        .constraints()
        .map(|(id, c)| {
            serde_json::to_string(&constraint_record(id, c)).expect("constraint serializes")
        })
        .collect();
    push_array(&mut out, "entities", &entities, true);
    push_array(&mut out, "constraints", &constraints, false);
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
    for rec in file.constraints {
        let (id, c) = parse_constraint(rec)?;
        if !seen_constraints.insert(id) {
            return Err(LoadError::DuplicateId(id.to_string()));
        }
        constraints.push((id, c));
    }
    let doc = Document::from_parts(entities, constraints).map_err(LoadError::Document)?;
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
  "version": 2,
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
        ] {
            doc.add_constraint(constraint).unwrap();
        }
        let text = save(&doc, &Metadata::default());
        let (loaded, _) = load(&text).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(loaded.constraint_count(), 13);
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
