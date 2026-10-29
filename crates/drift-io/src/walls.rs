//! Walls and property dimensions in `.drift` files: the `walls` and `property_dims` tables
//! (additions at format 42, fixture `v42.1`). Older 42 builds pass over them and keep them
//! when they save; the faces walls made are ordinary entities, so a file opens without
//! regenerating anything.
//!
//! What a wall made is written by key (`docs/walls-design.md` §2.3), as strings:
//!
//! - a station: `start`, `end`, `tee:w3`, `x:w3.2.0` (that wall's segment 2, crossing 0) or
//!   `jamb:o2.1` (opening o2's far jamb; an addition with openings, fixture `v42.2`);
//! - a face key: its part, segment and station, `L0@start` or `R1@tee:w4`; end edges are
//!   `C` (cap), `B` (butt), `S` (step), `V` (bevel), `M` (seam) and `J` (an opening's jamb,
//!   with no direction: `J0@jamb:o2.0`), with `>` for the end at a
//!   segment's start or a station's far side and `<` for the other, and a seam's side, `+` or
//!   `-`: `C>0@start`, `M<+2@end`;
//! - an end: `w1.0@start>`;
//! - a corner: `pair:END|END`, `pairb:END|END`, `own:END+` (or `-`), `node:END`, or
//!   `centre:w1/FACE` for an arc's centre;
//! - a loop: `0.0` (outer loop 0), `0.2` (its second hole).

use std::collections::{BTreeMap, HashSet};

use drift_core::{
    CornerKey, DVec2, Dir, Document, EndKey, EntityId, FaceKey, FillId, FillKind, FillStyle, Hatch,
    Justify, LineWeight, LoopKey, Made, Part, PathId, PropertyDim, PropertyDimId, PropertyRef, Rgb,
    Station, Turn, Wall, WallEnd, WallId,
};
use serde::{Deserialize, Serialize};

use crate::{LoadError, parse_id, parse_layer_id, parse_prefixed};

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_centre(j: &String) -> bool {
    j == "centre"
}

fn centre() -> String {
    "centre".to_owned()
}

/// A wall's poché.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PocheRecord {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    model: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weight: Option<f64>,
}

/// One end of an open wall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct EndRecord {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    join: bool,
    /// `w3.0`: on that wall's segment 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    against: Option<String>,
}

impl Default for EndRecord {
    fn default() -> Self {
        Self {
            join: true,
            tee: None,
            against: None,
        }
    }
}

fn is_default_end(e: &EndRecord) -> bool {
    *e == EndRecord::default()
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WallRecord {
    id: String,
    centre: String,
    thickness: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expr: Option<String>,
    #[serde(default = "centre", skip_serializing_if = "is_centre")]
    justify: String,
    /// None: faces only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    poche: Option<PocheRecord>,
    layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    order: Option<i32>,
    #[serde(default, skip_serializing_if = "is_default_end")]
    start: EndRecord,
    #[serde(default, skip_serializing_if = "is_default_end")]
    end: EndRecord,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    cross: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    faces: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    corners: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    outlines: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fills: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PropertyDimRecord {
    id: String,
    /// What it shows: `thickness`.
    of: String,
    wall: String,
    seg: u16,
    along: f64,
    out: f64,
}

// ----- Keys as strings ------------------------------------------------------------------------

#[cfg_attr(feature = "demo", allow(dead_code))]
fn station(s: Station) -> String {
    match s {
        Station::SegStart => "start".into(),
        Station::SegEnd => "end".into(),
        Station::Tee(w, 0) => format!("tee:{w}"),
        Station::Tee(w, e) => format!("tee:{w}.{e}"),
        Station::Cross(w, seg, n) => format!("x:{w}.{seg}.{n}"),
        Station::Jamb(o, k) => format!("jamb:{o}.{k}"),
    }
}

#[cfg_attr(feature = "demo", allow(dead_code))]
fn dir(d: Dir) -> char {
    match d {
        Dir::Fwd => '>',
        Dir::Back => '<',
    }
}

#[cfg_attr(feature = "demo", allow(dead_code))]
fn part(p: Part) -> String {
    match p {
        Part::Left => "L".into(),
        Part::Right => "R".into(),
        Part::Cap(d) => format!("C{}", dir(d)),
        Part::Butt(d) => format!("B{}", dir(d)),
        Part::Step(d) => format!("S{}", dir(d)),
        Part::Bevel(d) => format!("V{}", dir(d)),
        Part::Seam(d, t) => format!(
            "M{}{}",
            dir(d),
            match t {
                Turn::Ccw => '+',
                Turn::Cw => '-',
            }
        ),
        Part::Jamb => "J".into(),
    }
}

#[cfg_attr(feature = "demo", allow(dead_code))]
pub(crate) fn face_key(k: FaceKey) -> String {
    format!("{}{}@{}", part(k.part), k.seg, station(k.from))
}

#[cfg_attr(feature = "demo", allow(dead_code))]
fn end_key(k: EndKey) -> String {
    format!("{}.{}@{}{}", k.wall, k.seg, station(k.at), dir(k.dir))
}

#[cfg_attr(feature = "demo", allow(dead_code))]
pub(crate) fn corner_key(k: CornerKey) -> String {
    match k {
        CornerKey::Pair(a, b) => format!("pair:{}|{}", end_key(a), end_key(b)),
        CornerKey::PairB(a, b) => format!("pairb:{}|{}", end_key(a), end_key(b)),
        CornerKey::Own(e, ccw) => format!("own:{}{}", end_key(e), if ccw { '+' } else { '-' }),
        CornerKey::Node(e) => format!("node:{}", end_key(e)),
        CornerKey::Centre(w, f) => format!("centre:{w}/{}", face_key(f)),
    }
}

#[cfg_attr(feature = "demo", allow(dead_code))]
fn loop_key(k: LoopKey) -> String {
    format!("{}.{}", k.outer, k.hole)
}

fn bad(s: &str) -> LoadError {
    LoadError::BadId(s.to_owned())
}

fn parse_wall_id(s: &str) -> Result<WallId, LoadError> {
    Ok(WallId(parse_prefixed(s, 'w')?))
}

fn parse_station(s: &str) -> Result<Station, LoadError> {
    Ok(match s {
        "start" => Station::SegStart,
        "end" => Station::SegEnd,
        _ => {
            if let Some(t) = s.strip_prefix("tee:") {
                let (w, e) = t.split_once('.').unwrap_or((t, "0"));
                Station::Tee(parse_wall_id(w)?, e.parse().map_err(|_| bad(s))?)
            } else if let Some(j) = s.strip_prefix("jamb:") {
                let (o, k) = j.split_once('.').ok_or_else(|| bad(s))?;
                Station::Jamb(
                    drift_core::OpeningId(parse_prefixed(o, 'o')?),
                    k.parse().map_err(|_| bad(s))?,
                )
            } else if let Some(x) = s.strip_prefix("x:") {
                let mut it = x.split('.');
                let (w, seg, n) = (
                    it.next().ok_or_else(|| bad(s))?,
                    it.next().ok_or_else(|| bad(s))?,
                    it.next().ok_or_else(|| bad(s))?,
                );
                if it.next().is_some() {
                    return Err(bad(s));
                }
                Station::Cross(
                    parse_wall_id(w)?,
                    seg.parse().map_err(|_| bad(s))?,
                    n.parse().map_err(|_| bad(s))?,
                )
            } else {
                return Err(bad(s));
            }
        }
    })
}

fn parse_dir(c: char, s: &str) -> Result<Dir, LoadError> {
    match c {
        '>' => Ok(Dir::Fwd),
        '<' => Ok(Dir::Back),
        _ => Err(bad(s)),
    }
}

pub(crate) fn parse_face_key(s: &str) -> Result<FaceKey, LoadError> {
    let (head, at) = s.split_once('@').ok_or_else(|| bad(s))?;
    let mut chars = head.chars();
    let letter = chars.next().ok_or_else(|| bad(s))?;
    let rest: String = chars.collect();
    let (part, seg) = match letter {
        'L' => (Part::Left, rest.as_str()),
        'J' => (Part::Jamb, rest.as_str()),
        'R' => (Part::Right, rest.as_str()),
        'C' | 'B' | 'S' | 'V' | 'M' => {
            let mut r = rest.chars();
            let d = parse_dir(r.next().ok_or_else(|| bad(s))?, s)?;
            let tail = &rest[1..];
            match letter {
                'C' => (Part::Cap(d), tail),
                'B' => (Part::Butt(d), tail),
                'S' => (Part::Step(d), tail),
                'V' => (Part::Bevel(d), tail),
                _ => {
                    let t = match tail.chars().next() {
                        Some('+') => Turn::Ccw,
                        Some('-') => Turn::Cw,
                        _ => return Err(bad(s)),
                    };
                    (Part::Seam(d, t), &tail[1..])
                }
            }
        }
        _ => return Err(bad(s)),
    };
    Ok(FaceKey {
        seg: seg.parse().map_err(|_| bad(s))?,
        part,
        from: parse_station(at)?,
    })
}

fn parse_end_key(s: &str) -> Result<EndKey, LoadError> {
    let (head, tail) = s.split_once('@').ok_or_else(|| bad(s))?;
    let (w, seg) = head.split_once('.').ok_or_else(|| bad(s))?;
    let d = tail.chars().last().ok_or_else(|| bad(s))?;
    Ok(EndKey {
        wall: parse_wall_id(w)?,
        seg: seg.parse().map_err(|_| bad(s))?,
        at: parse_station(&tail[..tail.len() - 1])?,
        dir: parse_dir(d, s)?,
    })
}

pub(crate) fn parse_corner_key(s: &str) -> Result<CornerKey, LoadError> {
    let (kind, rest) = s.split_once(':').ok_or_else(|| bad(s))?;
    let pair = |rest: &str| -> Result<(EndKey, EndKey), LoadError> {
        let (a, b) = rest.split_once('|').ok_or_else(|| bad(s))?;
        Ok((parse_end_key(a)?, parse_end_key(b)?))
    };
    Ok(match kind {
        "pair" => {
            let (a, b) = pair(rest)?;
            CornerKey::Pair(a, b)
        }
        "pairb" => {
            let (a, b) = pair(rest)?;
            CornerKey::PairB(a, b)
        }
        "own" => {
            let ccw = match rest.chars().last() {
                Some('+') => true,
                Some('-') => false,
                _ => return Err(bad(s)),
            };
            CornerKey::Own(parse_end_key(&rest[..rest.len() - 1])?, ccw)
        }
        "node" => CornerKey::Node(parse_end_key(rest)?),
        "centre" => {
            let (w, f) = rest.split_once('/').ok_or_else(|| bad(s))?;
            CornerKey::Centre(parse_wall_id(w)?, parse_face_key(f)?)
        }
        _ => return Err(bad(s)),
    })
}

fn parse_loop_key(s: &str) -> Result<LoopKey, LoadError> {
    let (o, h) = s.split_once('.').ok_or_else(|| bad(s))?;
    Ok(LoopKey {
        outer: o.parse().map_err(|_| bad(s))?,
        hole: h.parse().map_err(|_| bad(s))?,
    })
}

// ----- Writing --------------------------------------------------------------------------------

#[cfg(not(feature = "demo"))]
fn poche_record(s: &FillStyle) -> PocheRecord {
    let mut r = PocheRecord {
        kind: "solid".into(),
        pattern: None,
        scale: None,
        angle: None,
        origin: None,
        model: false,
        background: None,
        color: s.color.map(Rgb::hex),
        opacity: (s.opacity != 1.0).then_some(s.opacity),
        weight: s.weight.map(LineWeight::mm),
    };
    if let FillKind::Hatch(h) = &s.kind {
        r.kind = "hatch".into();
        r.pattern = Some(h.pattern.clone());
        r.scale = (h.scale != 1.0).then_some(h.scale);
        r.angle = (h.angle != 0.0).then_some(h.angle);
        r.origin = (h.origin != DVec2::ZERO).then_some(h.origin.to_array());
        r.model = h.model;
        r.background = h.background.map(Rgb::hex);
    }
    r
}

#[cfg(not(feature = "demo"))]
pub(crate) fn wall_record(doc: &Document, id: WallId, w: &Wall) -> WallRecord {
    let end = |e: &WallEnd| EndRecord {
        join: e.join,
        tee: e.tee.map(|(h, seg)| format!("{h}.{seg}")),
        against: e.against.filter(|c| doc.contains(*c)).map(crate::id_string),
    };
    let live = |e: &EntityId| doc.contains(*e);
    WallRecord {
        id: id.to_string(),
        centre: w.centre.to_string(),
        thickness: w.thickness,
        expr: w.thickness_expr.clone(),
        justify: match w.justify {
            Justify::Centre => "centre",
            Justify::Left => "left",
            Justify::Right => "right",
        }
        .into(),
        poche: w.poche.as_ref().map(poche_record),
        layer: w.layer.to_string(),
        order: w.order,
        start: end(&w.ends[0]),
        end: end(&w.ends[1]),
        cross: w.cross,
        faces: w
            .made
            .curves
            .iter()
            .filter(|(_, e)| live(e))
            .map(|(k, e)| (face_key(*k), crate::id_string(*e)))
            .collect(),
        corners: w
            .made
            .points
            .iter()
            .filter(|(_, e)| live(e))
            .map(|(k, e)| (corner_key(*k), crate::id_string(*e)))
            .collect(),
        outlines: w
            .made
            .outlines
            .iter()
            .filter(|(_, p)| doc.path(**p).is_some())
            .map(|(k, p)| (loop_key(*k), p.to_string()))
            .collect(),
        fills: w
            .made
            .fills
            .iter()
            .filter(|(_, f)| doc.fill(**f).is_some())
            .map(|(k, f)| (loop_key(*k), f.to_string()))
            .collect(),
    }
}

#[cfg(not(feature = "demo"))]
pub(crate) fn property_dim_record(id: PropertyDimId, d: &PropertyDim) -> PropertyDimRecord {
    let (of, wall) = match d.target {
        PropertyRef::WallThickness(w) => ("thickness", w),
    };
    PropertyDimRecord {
        id: id.to_string(),
        of: of.into(),
        wall: wall.to_string(),
        seg: d.seg,
        along: d.along,
        out: d.out,
    }
}

// ----- Reading --------------------------------------------------------------------------------

fn parse_poche(id: &str, r: PocheRecord) -> Result<FillStyle, LoadError> {
    let color = |c: &Option<String>, field| match c {
        Some(c) => Rgb::parse(c).map(Some).ok_or(LoadError::MissingField {
            id: id.to_owned(),
            field,
        }),
        None => Ok(None),
    };
    let kind = match r.kind.as_str() {
        "solid" => FillKind::Solid,
        "hatch" => FillKind::Hatch(Hatch {
            pattern: r.pattern.clone().ok_or(LoadError::MissingField {
                id: id.to_owned(),
                field: "pattern",
            })?,
            scale: r.scale.unwrap_or(1.0),
            angle: r.angle.unwrap_or(0.0),
            origin: r.origin.map_or(DVec2::ZERO, DVec2::from_array),
            model: r.model,
            background: color(&r.background, "background")?,
        }),
        other => return Err(LoadError::UnknownConstraint(format!("poché {other}"))),
    };
    Ok(FillStyle {
        kind,
        color: color(&r.color, "color")?,
        opacity: r.opacity.unwrap_or(1.0).clamp(0.0, 1.0),
        weight: r.weight.map(LineWeight::nearest_cad),
    })
}

/// Read the walls and property dimensions into `doc` (after its entities, paths and fills).
/// What an older build could have left (a centreline deleted, a face deleted or redrawn, a
/// tee on a wall that is gone) is left for the repair on opening; what only a hand edit
/// could make (a face key naming a point, an entity made by two walls) is refused. Returns
/// notes on what was left out.
pub(crate) fn insert_all(
    doc: &mut Document,
    walls: Vec<WallRecord>,
    dims: Vec<PropertyDimRecord>,
) -> Result<Vec<String>, LoadError> {
    let mut notes = Vec::new();
    let mut seen: HashSet<EntityId> = HashSet::new();
    let mut parsed: Vec<(WallId, Wall)> = Vec::new();
    for r in walls {
        let id = parse_wall_id(&r.id)?;
        if parsed.iter().any(|(w, _)| *w == id) {
            return Err(LoadError::DuplicateId(r.id));
        }
        if !(r.thickness > 0.0 && r.thickness.is_finite()) {
            return Err(LoadError::NonFinite(r.id));
        }
        let centre = PathId(parse_prefixed(&r.centre, 'p')?);
        let layer = parse_layer_id(&r.layer)?;
        if doc.layer(layer).is_none() {
            return Err(LoadError::Document(
                drift_core::DocumentError::MissingLayer(layer),
            ));
        }
        let justify = match r.justify.as_str() {
            "centre" => Justify::Centre,
            "left" => Justify::Left,
            "right" => Justify::Right,
            _ => {
                return Err(LoadError::MissingField {
                    id: r.id,
                    field: "justify",
                });
            }
        };
        let poche = r.poche.map(|p| parse_poche(&r.id, p)).transpose()?;
        let end = |e: EndRecord| -> Result<WallEnd, LoadError> {
            let tee = match e.tee {
                Some(t) => {
                    let (w, seg) = t.split_once('.').ok_or_else(|| bad(&t))?;
                    Some((parse_wall_id(w)?, seg.parse().map_err(|_| bad(&t))?))
                }
                None => None,
            };
            let against = e.against.map(|a| parse_id(&a)).transpose()?;
            Ok(WallEnd {
                join: e.join,
                tee,
                against: against.filter(|a| doc.contains(*a)),
            })
        };
        let mut made = Made::default();
        let mut take = |e: &str, point: bool| -> Result<Option<EntityId>, LoadError> {
            let id = parse_id(e)?;
            if !seen.insert(id) {
                return Err(LoadError::BadId(format!("{e} is made by two walls")));
            }
            match doc.get(id) {
                // Deleted by an older build: made again on opening.
                None => Ok(None),
                Some(x) if x.kind.is_point() != point => Err(LoadError::BadId(format!(
                    "{e} is a wall's {}, but isn't one",
                    if point { "corner" } else { "face" }
                ))),
                Some(_) => Ok(Some(id)),
            }
        };
        for (k, e) in &r.faces {
            if let Some(id) = take(e, false)? {
                made.curves.insert(parse_face_key(k)?, id);
            }
        }
        for (k, e) in &r.corners {
            if let Some(id) = take(e, true)? {
                made.points.insert(parse_corner_key(k)?, id);
            }
        }
        for (k, p) in &r.outlines {
            let p = PathId(parse_prefixed(p, 'p')?);
            if doc.path(p).is_some() {
                made.outlines.insert(parse_loop_key(k)?, p);
            }
        }
        for (k, f) in &r.fills {
            let f = FillId(parse_prefixed(f, 'f')?);
            if doc.fill(f).is_some() {
                made.fills.insert(parse_loop_key(k)?, f);
            }
        }
        let wall = Wall {
            centre,
            thickness: r.thickness,
            thickness_expr: r.expr,
            justify,
            poche,
            layer,
            order: r.order,
            ends: [end(r.start)?, end(r.end)?],
            cross: r.cross,
            made,
        };
        if doc.path(centre).is_none() {
            // Its centreline was deleted (by an older build): what it made stays as plain
            // geometry.
            notes.push(format!("wall {id} (its centreline is gone)"));
            continue;
        }
        parsed.push((id, wall));
    }
    let ids: HashSet<WallId> = parsed.iter().map(|(w, _)| *w).collect();
    for (id, mut wall) in parsed {
        for e in &mut wall.ends {
            if e.tee.is_some_and(|(h, _)| !ids.contains(&h)) {
                e.tee = None;
            }
        }
        doc.insert_wall(id, wall).map_err(LoadError::Document)?;
    }
    for r in dims {
        let id = PropertyDimId(parse_prefixed(&r.id, 'q')?);
        if doc.property_dim(id).is_some() {
            return Err(LoadError::DuplicateId(r.id));
        }
        let wall = parse_wall_id(&r.wall)?;
        let target = match r.of.as_str() {
            "thickness" => PropertyRef::WallThickness(wall),
            other => return Err(LoadError::UnknownConstraint(format!("property {other}"))),
        };
        if !(r.along.is_finite() && r.out.is_finite()) {
            return Err(LoadError::NonFinite(r.id));
        }
        if doc.wall(wall).is_none() {
            continue;
        }
        doc.insert_property_dim(
            id,
            PropertyDim {
                target,
                seg: r.seg,
                along: r.along,
                out: r.out,
            },
        )
        .map_err(LoadError::Document)?;
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_back_as_written() {
        let e = |w, seg, at, dir| EndKey {
            wall: WallId(w),
            seg,
            at,
            dir,
        };
        let faces = [
            FaceKey {
                seg: 0,
                part: Part::Left,
                from: Station::SegStart,
            },
            FaceKey {
                seg: 12,
                part: Part::Seam(Dir::Back, Turn::Cw),
                from: Station::Cross(WallId(3), 2, 1),
            },
            FaceKey {
                seg: 1,
                part: Part::Cap(Dir::Fwd),
                from: Station::Tee(WallId(9), 1),
            },
            FaceKey {
                seg: 2,
                part: Part::Jamb,
                from: Station::Jamb(drift_core::OpeningId(7), 1),
            },
            FaceKey {
                seg: 2,
                part: Part::Right,
                from: Station::Jamb(drift_core::OpeningId(7), 1),
            },
        ];
        for f in faces {
            assert_eq!(parse_face_key(&face_key(f)).unwrap(), f, "{}", face_key(f));
        }
        let a = e(1, 0, Station::SegEnd, Dir::Back);
        let b = e(22, 3, Station::Cross(WallId(1), 0, 0), Dir::Fwd);
        let corners = [
            CornerKey::Pair(a, b),
            CornerKey::PairB(b, a),
            CornerKey::Own(a, true),
            CornerKey::Own(b, false),
            CornerKey::Node(b),
            CornerKey::Centre(WallId(4), faces[1]),
        ];
        for c in corners {
            assert_eq!(
                parse_corner_key(&corner_key(c)).unwrap(),
                c,
                "{}",
                corner_key(c)
            );
        }
        assert_eq!(face_key(faces[0]), "L0@start");
        assert_eq!(face_key(faces[3]), "J2@jamb:o7.1");
        assert!(parse_face_key("J2@jamb:o7").is_err());
        assert!(parse_face_key("Q0@start").is_err());
        assert!(parse_corner_key("pair:w1.0@start>").is_err());
    }
}
