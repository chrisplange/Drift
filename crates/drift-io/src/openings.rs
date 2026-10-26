//! Openings in `.drift` files: the `openings` table (an addition at format 42, fixture
//! `v42.2`). Older 42 builds pass over it and keep it when they save; the jambs are the wall's
//! faces and the symbols are ordinary entities, so a file opens without regenerating.
//!
//! An opening is written with its actual sizes (never a size table's row), where it sits
//! (`from` and `offset`, or centred when there is no `from`), and what its symbol made, by key
//! (`curves`: `"0": "e40"`; `points`: curve × 3 + n).

use std::collections::{BTreeMap, HashSet};

use drift_core::{
    Document, EntityId, Hand, Opening, OpeningAt, OpeningId, OpeningMade, SegEnd, Symbol, WallFace,
    WallId,
};
use serde::{Deserialize, Serialize};

use crate::{LoadError, parse_id, parse_layer_id, parse_prefixed};

fn is_left(s: &String) -> bool {
    s == "left"
}

fn left() -> String {
    "left".to_owned()
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// What an opening shows: a library family's symbol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SymbolRecord {
    family: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    type_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct OpeningRecord {
    id: String,
    wall: String,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    seg: u16,
    /// `start` or `end`: the segment's end the offset is from. Absent: centred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset_expr: Option<String>,
    width: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width_expr: Option<String>,
    /// What the symbol draws (a door's leaf); absent: the width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    leaf: Option<f64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    height: f64,
    /// Absent: a void (a plain cut).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    symbol: Option<SymbolRecord>,
    #[serde(default = "left", skip_serializing_if = "is_left")]
    hand: String,
    #[serde(default = "left", skip_serializing_if = "is_left")]
    facing: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layer: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    curves: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    points: BTreeMap<String, String>,
}

fn is_zero_u16(v: &u16) -> bool {
    *v == 0
}

#[cfg(not(feature = "demo"))]
pub(crate) fn opening_record(doc: &Document, id: OpeningId, o: &Opening) -> OpeningRecord {
    let side = |left: bool| if left { "left" } else { "right" }.to_owned();
    let (from, offset, offset_expr) = match &o.at {
        OpeningAt::Centred => (None, None, None),
        OpeningAt::From { end, offset, expr } => (
            Some(
                match end {
                    SegEnd::Start => "start",
                    SegEnd::End => "end",
                }
                .to_owned(),
            ),
            Some(*offset),
            expr.clone(),
        ),
    };
    let live = |e: &EntityId| doc.contains(*e);
    OpeningRecord {
        id: id.to_string(),
        wall: o.wall.to_string(),
        seg: o.seg,
        from,
        offset,
        offset_expr,
        width: o.width,
        width_expr: o.width_expr.clone(),
        leaf: (o.leaf != o.width).then_some(o.leaf),
        height: o.height,
        symbol: match &o.symbol {
            Symbol::Library { family, type_name } => Some(SymbolRecord {
                family: family.clone(),
                type_name: type_name.clone(),
            }),
            Symbol::Void => None,
        },
        hand: side(o.hand == Hand::Left),
        facing: side(o.facing == WallFace::Left),
        layer: o.layer.map(|l| l.to_string()),
        curves: o
            .made
            .curves
            .iter()
            .filter(|(_, e)| live(e))
            .map(|(k, e)| (k.to_string(), crate::id_string(*e)))
            .collect(),
        points: o
            .made
            .points
            .iter()
            .filter(|(_, e)| live(e))
            .map(|(k, e)| (k.to_string(), crate::id_string(*e)))
            .collect(),
    }
}

/// Read the openings into `doc` (after its walls). An opening whose wall is gone (an older
/// build deleted it) is left out, its symbol staying as plain geometry; what only a hand
/// edit could make (a symbol entity another wall or opening made, a curve key naming a
/// point) is refused. Returns notes on what was left out.
pub(crate) fn insert_all(
    doc: &mut Document,
    openings: Vec<OpeningRecord>,
) -> Result<Vec<String>, LoadError> {
    let mut notes = Vec::new();
    let owners = doc.wall_owners();
    let mut seen: HashSet<EntityId> = HashSet::new();
    for r in openings {
        let id = OpeningId(parse_prefixed(&r.id, 'o')?);
        if doc.opening(id).is_some() {
            return Err(LoadError::DuplicateId(r.id));
        }
        let wall = WallId(parse_prefixed(&r.wall, 'w')?);
        let finite = r.width.is_finite()
            && r.width > 0.0
            && r.height.is_finite()
            && r.leaf.is_none_or(f64::is_finite)
            && r.offset.is_none_or(f64::is_finite);
        if !finite {
            return Err(LoadError::NonFinite(r.id));
        }
        let side = |s: &str, field| match s {
            "left" => Ok(true),
            "right" => Ok(false),
            _ => Err(LoadError::MissingField {
                id: r.id.clone(),
                field,
            }),
        };
        let hand = if side(&r.hand, "hand")? {
            Hand::Left
        } else {
            Hand::Right
        };
        let facing = if side(&r.facing, "facing")? {
            WallFace::Left
        } else {
            WallFace::Right
        };
        let at = match r.from.as_deref() {
            None => OpeningAt::Centred,
            Some(f) => OpeningAt::From {
                end: match f {
                    "start" => SegEnd::Start,
                    "end" => SegEnd::End,
                    _ => {
                        return Err(LoadError::MissingField {
                            id: r.id,
                            field: "from",
                        });
                    }
                },
                offset: r.offset.ok_or(LoadError::MissingField {
                    id: r.id.clone(),
                    field: "offset",
                })?,
                expr: r.offset_expr.clone(),
            },
        };
        let layer = r.layer.as_deref().map(parse_layer_id).transpose()?;
        if let Some(l) = layer
            && doc.layer(l).is_none()
        {
            return Err(LoadError::Document(
                drift_core::DocumentError::MissingLayer(l),
            ));
        }
        let mut made = OpeningMade::default();
        let mut take = |e: &str, point: bool| -> Result<Option<EntityId>, LoadError> {
            let eid = parse_id(e)?;
            if !seen.insert(eid) || owners.get(eid).is_some() {
                return Err(LoadError::BadId(format!("{e} is made by two openings")));
            }
            match doc.get(eid) {
                // Deleted by an older build: made again on opening.
                None => Ok(None),
                Some(x) if x.kind.is_point() != point => Err(LoadError::BadId(format!(
                    "{e} is an opening's {}, but isn't one",
                    if point { "point" } else { "curve" }
                ))),
                Some(_) => Ok(Some(eid)),
            }
        };
        let key = |k: &str| k.parse::<u32>().map_err(|_| LoadError::BadId(k.to_owned()));
        for (k, e) in &r.curves {
            if let Some(eid) = take(e, false)? {
                made.curves.insert(key(k)?, eid);
            }
        }
        for (k, e) in &r.points {
            if let Some(eid) = take(e, true)? {
                made.points.insert(key(k)?, eid);
            }
        }
        if doc.wall(wall).is_none() {
            // Its wall was deleted (by an older build): its symbol stays as plain geometry.
            notes.push(format!("opening {id} (its wall is gone)"));
            continue;
        }
        let opening = Opening {
            wall,
            seg: r.seg,
            at,
            width: r.width,
            width_expr: r.width_expr,
            leaf: r.leaf.unwrap_or(r.width),
            height: r.height,
            symbol: match r.symbol {
                Some(s) => Symbol::Library {
                    family: s.family,
                    type_name: s.type_name,
                },
                None => Symbol::Void,
            },
            hand,
            facing,
            layer,
            made,
        };
        doc.insert_opening(id, opening)
            .map_err(LoadError::Document)?;
    }
    Ok(notes)
}
