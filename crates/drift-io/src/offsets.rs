//! Offsets in `.drift` files: the `offsets` table (an addition at format 44, fixture
//! `v44.1`). Older 44 builds pass over it and keep it when they save; the pieces an offset
//! made are ordinary entities, so a file opens without regenerating anything. An offset's
//! distance dimension rides the `property_dims` table (`of: "distance"`, naming the
//! `offset`).
//!
//! What an offset made is written by key, as strings:
//!
//! - a piece: `e12.0` (the first offset curve of source curve e12; a fitted ellipse or pen
//!   curve gives `e12.0`, `e12.1`, …) or `bridge:e12` (the bridge across the corner after
//!   source curve e12);
//! - a point: `start:PIECE`, `end:PIECE`, `h0:PIECE` or `h1:PIECE` (a fitted curve's
//!   handles), as `start:e12.0` or `start:bridge:e12`.

use std::collections::{BTreeMap, HashSet};

use drift_core::{Document, EntityId, Offset, OffsetId, OffsetMade, PieceKey, PointKey, Side};
use serde::{Deserialize, Serialize};

use crate::{LoadError, parse_id, parse_prefixed};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct OffsetRecord {
    id: String,
    /// The source chain's curves in travel order.
    source: Vec<String>,
    /// `left` or `right` of travel (a whole circle's or ellipse's `left` is outward).
    side: String,
    distance: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expr: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    curves: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    points: BTreeMap<String, String>,
}

// ----- Keys as strings ------------------------------------------------------------------------

#[cfg_attr(feature = "demo", allow(dead_code))]
pub(crate) fn piece_key(k: PieceKey) -> String {
    match k {
        PieceKey::Of { of, n } => format!("{of}.{n}"),
        PieceKey::Bridge { after } => format!("bridge:{after}"),
    }
}

#[cfg_attr(feature = "demo", allow(dead_code))]
pub(crate) fn point_key(k: PointKey) -> String {
    match k {
        PointKey::Start(p) => format!("start:{}", piece_key(p)),
        PointKey::End(p) => format!("end:{}", piece_key(p)),
        PointKey::Handle(p, h) => format!("h{h}:{}", piece_key(p)),
    }
}

fn bad(s: &str) -> LoadError {
    LoadError::BadId(s.to_owned())
}

pub(crate) fn parse_offset_id(s: &str) -> Result<OffsetId, LoadError> {
    Ok(OffsetId(parse_prefixed(s, 'x')?))
}

pub(crate) fn parse_piece_key(s: &str) -> Result<PieceKey, LoadError> {
    if let Some(after) = s.strip_prefix("bridge:") {
        return Ok(PieceKey::Bridge {
            after: parse_id(after)?,
        });
    }
    let (of, n) = s.split_once('.').ok_or_else(|| bad(s))?;
    Ok(PieceKey::Of {
        of: parse_id(of)?,
        n: n.parse().map_err(|_| bad(s))?,
    })
}

pub(crate) fn parse_point_key(s: &str) -> Result<PointKey, LoadError> {
    let (kind, piece) = s.split_once(':').ok_or_else(|| bad(s))?;
    let piece = parse_piece_key(piece)?;
    Ok(match kind {
        "start" => PointKey::Start(piece),
        "end" => PointKey::End(piece),
        "h0" => PointKey::Handle(piece, 0),
        "h1" => PointKey::Handle(piece, 1),
        _ => return Err(bad(s)),
    })
}

// ----- Writing --------------------------------------------------------------------------------

#[cfg(not(feature = "demo"))]
pub(crate) fn offset_record(doc: &Document, id: OffsetId, o: &Offset) -> OffsetRecord {
    let live = |e: &EntityId| doc.contains(*e);
    OffsetRecord {
        id: id.to_string(),
        source: o
            .source
            .iter()
            .filter(|c| live(c))
            .map(|c| crate::id_string(*c))
            .collect(),
        side: match o.side {
            Side::Left => "left",
            Side::Right => "right",
        }
        .into(),
        distance: o.distance,
        expr: o.distance_expr.clone(),
        curves: o
            .made
            .curves
            .iter()
            .filter(|(_, e)| live(e))
            .map(|(k, e)| (piece_key(*k), crate::id_string(*e)))
            .collect(),
        points: o
            .made
            .points
            .iter()
            .filter(|(_, e)| live(e))
            .map(|(k, e)| (point_key(*k), crate::id_string(*e)))
            .collect(),
    }
}

// ----- Reading --------------------------------------------------------------------------------

/// Read the offsets into `doc` (after its entities and walls, before the property
/// dimensions, which name them). What an older build could have left (a source curve or a
/// piece deleted) is left for the repair on opening; what only a hand edit could make (a
/// piece named by two offsets, a curve key naming a point) is refused. Returns notes on
/// what was left out.
pub(crate) fn insert_all(
    doc: &mut Document,
    offsets: Vec<OffsetRecord>,
) -> Result<Vec<String>, LoadError> {
    let mut notes = Vec::new();
    let mut seen: HashSet<EntityId> = HashSet::new();
    for r in offsets {
        let id = parse_offset_id(&r.id)?;
        if doc.offset(id).is_some() {
            return Err(LoadError::DuplicateId(r.id));
        }
        if !(r.distance > 0.0 && r.distance.is_finite()) {
            return Err(LoadError::NonFinite(r.id));
        }
        let side = match r.side.as_str() {
            "left" => Side::Left,
            "right" => Side::Right,
            _ => {
                return Err(LoadError::MissingField {
                    id: r.id,
                    field: "side",
                });
            }
        };
        // Source curves an older build deleted are left out; none left, and the offset is
        // left out too (its pieces stay as plain geometry).
        let mut source = Vec::with_capacity(r.source.len());
        for c in &r.source {
            let c = parse_id(c)?;
            if doc.get(c).is_some_and(|e| !e.kind.is_point()) {
                source.push(c);
            }
        }
        if source.is_empty() {
            notes.push(format!("offset {id} (its source is gone)"));
            continue;
        }
        let mut made = OffsetMade::default();
        let mut take = |e: &str, point: bool| -> Result<Option<EntityId>, LoadError> {
            let id = parse_id(e)?;
            if !seen.insert(id) {
                return Err(LoadError::BadId(format!("{e} is made by two offsets")));
            }
            match doc.get(id) {
                // Deleted by an older build: made again on opening.
                None => Ok(None),
                Some(x) if x.kind.is_point() != point => Err(LoadError::BadId(format!(
                    "{e} is an offset's {}, but isn't one",
                    if point { "point" } else { "curve" }
                ))),
                Some(_) => Ok(Some(id)),
            }
        };
        for (k, e) in &r.curves {
            if let Some(e) = take(e, false)? {
                made.curves.insert(parse_piece_key(k)?, e);
            }
        }
        for (k, e) in &r.points {
            if let Some(e) = take(e, true)? {
                made.points.insert(parse_point_key(k)?, e);
            }
        }
        let mut o = Offset::new(source, side, r.distance);
        o.distance_expr = r.expr;
        o.made = made;
        doc.insert_offset(id, o).map_err(LoadError::Document)?;
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_back_as_written() {
        let of = PieceKey::Of {
            of: EntityId(12),
            n: 3,
        };
        let bridge = PieceKey::Bridge { after: EntityId(7) };
        for k in [of, bridge] {
            assert_eq!(
                parse_piece_key(&piece_key(k)).unwrap(),
                k,
                "{}",
                piece_key(k)
            );
        }
        assert_eq!(piece_key(of), "e12.3");
        assert_eq!(piece_key(bridge), "bridge:e7");
        let points = [
            PointKey::Start(of),
            PointKey::End(bridge),
            PointKey::Handle(of, 0),
            PointKey::Handle(of, 1),
        ];
        for p in points {
            assert_eq!(
                parse_point_key(&point_key(p)).unwrap(),
                p,
                "{}",
                point_key(p)
            );
        }
        assert_eq!(point_key(points[1]), "end:bridge:e7");
        assert_eq!(point_key(points[3]), "h1:e12.3");
        assert!(parse_point_key("h2:e12.3").is_err());
        assert!(parse_piece_key("e12").is_err());
        assert!(parse_piece_key("x3.0").is_err());
    }
}
