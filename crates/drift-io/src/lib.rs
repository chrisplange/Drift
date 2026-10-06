//! The native `.drift` file format.
//!
//! A `.drift` file is UTF-8 JSON with one entity per line, so files diff and merge cleanly in
//! version control:
//!
//! ```json
//! {
//!   "format": "drift",
//!   "version": 1,
//!   "meta": {"title":"Bracket","units":"mm","generator":"Drift 0.1.0"},
//!   "entities": [
//!     {"type":"point","id":"e1","at":[0.0,0.0]},
//!     {"type":"point","id":"e2","at":[120.0,0.0]},
//!     {"type":"line","id":"e3","a":"e1","b":"e2"}
//!   ]
//! }
//! ```
//!
//! Numbers use the shortest representation that round-trips exactly, so save → load is
//! bit-exact. Older versions are upgraded by [`migrate`] before parsing.

use drift_core::{Document, DocumentError, Entity, EntityId, EntityKind, LineKind};
use serde::{Deserialize, Serialize};

/// File extension, without the dot.
pub const EXTENSION: &str = "drift";
/// The version this build writes. Files with a higher version are rejected.
pub const FORMAT_VERSION: u32 = 1;
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
    out.push_str("  \"entities\": [");
    let mut first = true;
    for (id, entity) in doc.iter() {
        out.push_str(if first { "\n    " } else { ",\n    " });
        first = false;
        let line = serde_json::to_string(&record(id, entity)).expect("entity serializes");
        out.push_str(&line);
    }
    out.push_str(if first { "]\n" } else { "\n  ]\n" });
    out.push_str("}\n");
    out
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
    let doc = Document::from_entities(entities).map_err(LoadError::Document)?;
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
    // Version 1 is current; future upgrades chain here: `if version < 2 { value = v1_to_v2(value) }`.
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
        let text = save(&doc, &Metadata::default());
        let expected = r#"{
  "format": "drift",
  "version": 1,
  "meta": {"units":"mm"},
  "entities": [
    {"type":"point","id":"e1","at":[0.0,0.0]},
    {"type":"point","id":"e2","at":[10.5,0.0]},
    {"type":"line","id":"e3","a":"e1","b":"e2","style":"construction"}
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
        assert!(text.contains("\"entities\": []"));
        let (doc, _) = load(&text).unwrap();
        assert!(doc.is_empty());
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
