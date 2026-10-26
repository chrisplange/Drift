//! Relations on walls' faces, lowered onto their centrelines (`docs/walls-design.md` §4.2).
//!
//! A wall's faces are generated from its centreline and are never solver unknowns: a
//! relation stays stored on the face (by its id: it is drawn there), but the solver sees it
//! on the face's centreline segment, with the face's signed offset from it. The offset comes
//! from the wall's record (thickness and justification), not from where the face was last
//! generated, so a thickness edit solved in the same commit keeps a dimensioned face where
//! it is and moves the centreline instead. The side a distance is on is taken from the
//! geometry when lowering, as orientation-dependent relations keep their branch.
//!
//! | Relation on a face | Lowered to |
//! |---|---|
//! | Parallel, perpendicular, horizontal, vertical, angle | the same on the centreline segment |
//! | Collinear with a line or face | parallel + a signed offset |
//! | Distance from a point (or a line's midpoint) to a face | signed offset from the segment |
//! | Distance between two faces, or a face and a line | signed offset between the segments |
//! | Point on a face | signed offset (an arc face: on the concentric circle) |
//! | Radius or diameter of an arc face | the centreline arc's, less the offset |
//! | Concentric with an arc face | concentric with the centreline arc |
//! | Anything on a corner or end edge, or else on a face | refused ([`lower_constraint`]) |
//!
//! Teed ends and ends against curves of walls that relations hold are lowered too, as the
//! end's point on the host's centreline segment (or the curve): see [`Lowering::extra`].

use std::collections::{BTreeMap, HashMap, HashSet};

use drift_core::{Constraint, ConstraintId, DVec2, Document, EntityId, EntityKind, Part, WallId};
use drift_solver::{Lowered, Lowering};

use super::regen::centreline;

/// Why a relation on a wall's corner (or end edge) can't drive it.
pub const CORNER_REFUSED: &str =
    "A wall's corner follows its joins: dimension the face or the centreline";

/// Why another relation on a wall's face can't drive it.
pub const FACE_REFUSED: &str = "That relation can't hold a wall's face: dimension it square to the face, or relate the \
     centreline";

/// Why a dimension across a wall's own two faces doesn't drive it.
pub const ACROSS: &str = "Across a wall is its thickness: type into its thickness dimension, or      set it in the Inspector";

/// Why a radius on a curved wall's face can't be met.
pub const TOO_TIGHT: &str = "That radius is too tight for the wall's thickness";

/// A face of a wall's segment, as the solver sees it: its centreline curve, and how far the
/// face lies from it (a line's: to the left of it from its `a` to its `b`; an arc's: its
/// radius less the centreline's).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Face {
    wall: WallId,
    seg: u16,
    centre: EntityId,
    offset: f64,
    line: bool,
}

/// What an entity a relation names is, for lowering.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Is {
    Plain,
    Face(Face),
    /// A corner point, an end edge, an arc piece's centre: not something to hold.
    Corner,
}

/// What lowering needs to know of a document's walls.
struct Walls<'a> {
    doc: &'a Document,
    /// Each generated curve: its wall and key.
    curves: HashMap<EntityId, (WallId, drift_core::FaceKey)>,
    points: HashSet<EntityId>,
    /// Each wall's centreline: its curves, and whether each runs the way the wall does.
    centres: HashMap<WallId, Option<Vec<(EntityId, bool)>>>,
}

impl<'a> Walls<'a> {
    fn new(doc: &'a Document) -> Self {
        let mut curves = HashMap::new();
        let mut points = HashSet::new();
        for (id, w) in doc.walls() {
            for (k, e) in &w.made.curves {
                curves.insert(*e, (id, *k));
            }
            points.extend(w.made.points.values().copied());
        }
        Walls {
            doc,
            curves,
            points,
            centres: HashMap::new(),
        }
    }

    /// A wall's centreline curves, each with whether it runs the way the wall does.
    fn centre(&mut self, w: WallId) -> Option<&Vec<(EntityId, bool)>> {
        let doc = self.doc;
        self.centres
            .entry(w)
            .or_insert_with(|| {
                let wall = doc.wall(w)?;
                let (segs, ..) = centreline(doc, wall).ok()?;
                let path = doc.path(wall.centre)?;
                path.curves
                    .iter()
                    .zip(&segs)
                    .map(|(&c, s)| {
                        let fwd = match (doc.get(c)?.kind, s) {
                            (EntityKind::Line { a, .. }, super::Seg::Line { a: sa, .. }) => {
                                doc.point(a).ok()? == *sa
                            }
                            (EntityKind::Arc { .. }, super::Seg::Arc { ccw, .. }) => *ccw,
                            _ => return None,
                        };
                        Some((c, fwd))
                    })
                    .collect()
            })
            .as_ref()
    }

    fn is(&mut self, id: EntityId) -> Is {
        if self.points.contains(&id) {
            return Is::Corner;
        }
        let Some(&(w, key)) = self.curves.get(&id) else {
            return Is::Plain;
        };
        let left = match key.part {
            Part::Left => true,
            Part::Right => false,
            _ => return Is::Corner,
        };
        let Some(wall) = self.doc.wall(w) else {
            return Is::Corner;
        };
        let (ol, or) = wall.offsets();
        let o = if left { ol } else { or };
        let Some(&(centre, fwd)) = self.centre(w).and_then(|c| c.get(usize::from(key.seg))) else {
            return Is::Corner;
        };
        let face = match self.doc.get(centre).map(|e| e.kind) {
            Some(EntityKind::Line { .. }) => Face {
                wall: w,
                seg: key.seg,
                centre,
                offset: if fwd { o } else { -o },
                line: true,
            },
            // (Turning left, counter-clockwise, the left face is towards the centre.)
            Some(EntityKind::Arc { .. }) => Face {
                wall: w,
                seg: key.seg,
                centre,
                offset: if fwd { -o } else { o },
                line: false,
            },
            _ => return Is::Corner,
        };
        Is::Face(face)
    }

    /// A line's ends' positions.
    fn line(&self, id: EntityId) -> Option<(DVec2, DVec2)> {
        match self.doc.get(id)?.kind {
            EntityKind::Line { a, b } => Some((self.doc.point(a).ok()?, self.doc.point(b).ok()?)),
            _ => None,
        }
    }

    /// Where a point is, or a line's midpoint.
    fn at(&self, id: EntityId) -> Option<DVec2> {
        self.doc.ref_point(id)
    }

    /// The signed distance of `p` to the left of a line, and its unit left normal.
    fn side(&self, p: DVec2, line: EntityId) -> Option<(f64, DVec2)> {
        let (a, b) = self.line(line)?;
        let d = (b - a).try_normalize()?;
        Some((d.perp_dot(p - a), d.perp()))
    }
}

/// +1 or −1, by which side a signed distance is on (on it counts as the left).
fn sign(x: f64) -> f64 {
    if x >= 0.0 { 1.0 } else { -1.0 }
}

/// What a stored relation that names walls' generated geometry is lowered to: None when it
/// names none; Err (why) when it can't hold them.
fn lower_one(w: &mut Walls<'_>, c: &Constraint) -> Option<Result<Vec<Lowered>, &'static str>> {
    let ents: Vec<EntityId> = c.entities().collect();
    let kinds: Vec<Is> = ents.iter().map(|e| w.is(*e)).collect();
    if kinds.iter().all(|k| *k == Is::Plain) {
        return None;
    }
    if kinds.contains(&Is::Corner) {
        return Some(Err(CORNER_REFUSED));
    }
    let face = |id: EntityId, w: &mut Walls<'_>| match w.is(id) {
        Is::Face(f) => Some(f),
        _ => None,
    };
    // A face's line as the centreline's (a relation of direction).
    let centre_line = |id: EntityId, w: &mut Walls<'_>| match w.is(id) {
        Is::Face(f) if f.line => Some(f.centre),
        Is::Face(_) => None,
        _ => Some(id),
    };
    let rel = |c: Constraint| Some(Ok(vec![Lowered::Relation(c)]));
    let refused = Some(Err(FACE_REFUSED));
    match *c {
        Constraint::Horizontal { line } => match centre_line(line, w) {
            Some(line) => rel(Constraint::Horizontal { line }),
            None => refused,
        },
        Constraint::Vertical { line } => match centre_line(line, w) {
            Some(line) => rel(Constraint::Vertical { line }),
            None => refused,
        },
        Constraint::Parallel { a, b }
        | Constraint::Perpendicular { a, b }
        | Constraint::Angle { a, b, .. } => {
            let (Some(ca), Some(cb)) = (centre_line(a, w), centre_line(b, w)) else {
                return refused;
            };
            if ca == cb {
                return refused;
            }
            rel(c.map_entities(|e| if e == a { ca } else { cb }))
        }
        Constraint::Collinear { a, b } => {
            // One face (or both) on the other's line: parallel, and offset.
            let (fa, fb) = (face(a, w), face(b, w));
            let (f, other) = match (fa, fb) {
                (Some(f), None) => (f, b),
                (None, Some(f)) => (f, a),
                (Some(f), Some(g)) => {
                    if !(f.line && g.line) || f.centre == g.centre {
                        return refused;
                    }
                    let mid = w.at(f.centre)?;
                    let (_, nf) = w.side(mid, f.centre)?;
                    let (_, ng) = w.side(mid, g.centre)?;
                    return Some(Ok(vec![
                        Lowered::Relation(Constraint::Parallel {
                            a: f.centre,
                            b: g.centre,
                        }),
                        Lowered::OffsetFrom {
                            point: f.centre,
                            curve: g.centre,
                            offset: g.offset - f.offset * sign(nf.dot(ng)),
                        },
                    ]));
                }
                (None, None) => return refused,
            };
            if !f.line {
                return refused;
            }
            let Some(EntityKind::Line { a: start, .. }) = w.doc.get(other).map(|e| e.kind) else {
                return refused;
            };
            Some(Ok(vec![
                Lowered::Relation(Constraint::Parallel {
                    a: f.centre,
                    b: other,
                }),
                Lowered::OffsetFrom {
                    point: start,
                    curve: f.centre,
                    offset: f.offset,
                },
            ]))
        }
        Constraint::PointLineDistance { point, line, value } => {
            match (face(point, w), face(line, w)) {
                // A point (or a line's midpoint) from a face.
                (None, Some(fl)) if fl.line => {
                    let p = w.at(point)?;
                    let (sd, _) = w.side(p, fl.centre)?;
                    Some(Ok(vec![Lowered::OffsetFrom {
                        point,
                        curve: fl.centre,
                        offset: fl.offset + sign(sd - fl.offset) * value,
                    }]))
                }
                // A face from a line, or from another face: its centreline's midpoint, offset.
                (Some(fp), Some(fl)) if fp.centre == fl.centre => Some(Err(ACROSS)),
                (Some(fp), other) if fp.line && other.is_none_or(|f| f.line) => {
                    let mid = w.at(fp.centre)?;
                    let (_, np) = w.side(mid, fp.centre)?;
                    let on_face = mid + np * fp.offset;
                    let (curve, off) = match other {
                        Some(fl) => (fl.centre, fl.offset),
                        None => (line, 0.0),
                    };
                    let (sd, nl) = w.side(on_face, curve)?;
                    Some(Ok(vec![Lowered::OffsetFrom {
                        point: fp.centre,
                        curve,
                        // (Parallel as the dimension's faces are, or nearly: which way they
                        // face each other is what counts, and it stays put through a drag.)
                        offset: off + sign(sd - off) * value - fp.offset * sign(np.dot(nl)),
                    }]))
                }
                _ => refused,
            }
        }
        Constraint::PointOnCurve { point, curve } => match (face(point, w), face(curve, w)) {
            (None, Some(f)) => Some(Ok(vec![Lowered::OffsetFrom {
                point,
                curve: f.centre,
                offset: f.offset,
            }])),
            _ => refused,
        },
        Constraint::Radius { curve, value } | Constraint::Diameter { curve, value } => {
            let Some(f) = face(curve, w).filter(|f| !f.line) else {
                return refused;
            };
            let diameter = matches!(c, Constraint::Diameter { .. });
            let r = if diameter { value / 2.0 } else { value } - f.offset;
            if r <= 0.0 {
                return Some(Err(TOO_TIGHT));
            }
            rel(Constraint::Radius {
                curve: f.centre,
                value: r,
            })
        }
        Constraint::Concentric { a, b } => {
            let arc = |id: EntityId, w: &mut Walls<'_>| match w.is(id) {
                Is::Face(f) if !f.line => Some(f.centre),
                Is::Face(_) => None,
                _ => Some(id),
            };
            match (arc(a, w), arc(b, w)) {
                (Some(ca), Some(cb)) if ca != cb => rel(Constraint::Concentric { a: ca, b: cb }),
                _ => refused,
            }
        }
        _ => refused,
    }
}

/// What a relation naming walls' generated geometry is lowered to: None when it names none;
/// Err (why, for a notice) when it can't hold them.
pub fn lower_constraint(doc: &Document, c: &Constraint) -> Option<Result<Vec<Lowered>, String>> {
    if !doc.has_walls() {
        return None;
    }
    let mut w = Walls::new(doc);
    lower_one(&mut w, c).map(|r| r.map_err(str::to_owned))
}

/// The walls' face of a generated curve: (wall, segment, which side, true for the left), if
/// it is a face piece.
pub fn face_of(doc: &Document, id: EntityId) -> Option<(WallId, u16, bool)> {
    doc.walls().find_map(|(w, wall)| {
        wall.made.curves.iter().find_map(|(k, e)| {
            (*e == id && matches!(k.part, Part::Left | Part::Right)).then_some((
                w,
                k.seg,
                k.part == Part::Left,
            ))
        })
    })
}

/// Every driving relation on walls' generated geometry, lowered (those that can't be: to
/// nothing), and the teed and against ends of walls that relations hold.
pub fn lower(doc: &Document) -> Lowering {
    let mut out = Lowering::default();
    if !doc.has_walls() {
        return out;
    }
    let mut w = Walls::new(doc);
    let mut held: HashSet<EntityId> = HashSet::new();
    let driving: Vec<(ConstraintId, Constraint)> = doc
        .constraints()
        .filter(|(id, _)| !doc.is_driven(*id))
        .map(|(id, c)| (id, *c))
        .collect();
    for (id, c) in &driving {
        match lower_one(&mut w, c) {
            None => held.extend(c.entities()),
            Some(Ok(list)) => {
                held.extend(list.iter().flat_map(Lowered::entities));
                out.of.insert(*id, list);
            }
            Some(Err(_)) => {
                out.of.insert(*id, Vec::new());
            }
        }
    }
    if held.is_empty() {
        return out;
    }
    // A wall is held when a relation names its centreline (a curve, or one of its points).
    let mut held_walls: BTreeMap<WallId, bool> = BTreeMap::new();
    let mut is_held = |wall: WallId| {
        *held_walls.entry(wall).or_insert_with(|| {
            doc.wall(wall)
                .and_then(|x| doc.path(x.centre))
                .is_some_and(|p| {
                    p.curves.iter().any(|c| {
                        held.contains(c)
                            || doc
                                .get(*c)
                                .is_some_and(|e| e.kind.points().any(|q| held.contains(&q)))
                    })
                })
        })
    };
    let walls: Vec<(WallId, drift_core::Wall)> =
        doc.walls().map(|(id, x)| (id, x.clone())).collect();
    for (id, wall) in &walls {
        let Ok((_, points, closed)) = centreline(doc, wall) else {
            continue;
        };
        if closed || points.is_empty() {
            continue;
        }
        for (which, end) in wall.ends.iter().enumerate() {
            if !end.join {
                continue;
            }
            let point = if which == 0 {
                points[0]
            } else {
                points[points.len() - 1]
            };
            if let Some((host, k)) = end.tee
                && (is_held(*id) || is_held(host))
                && let Some(&(curve, _)) = w.centre(host).and_then(|c| c.get(usize::from(k)))
            {
                out.extra
                    .push(Lowered::Relation(Constraint::PointOnCurve { point, curve }));
            }
            if let Some(curve) = end.against
                && is_held(*id)
                && matches!(
                    doc.get(curve).map(|e| e.kind),
                    Some(
                        EntityKind::Line { .. }
                            | EntityKind::Arc { .. }
                            | EntityKind::Circle { .. }
                    )
                )
            {
                out.extra
                    .push(Lowered::Relation(Constraint::PointOnCurve { point, curve }));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
