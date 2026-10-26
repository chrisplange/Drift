//! Writing walls into a document: what [`generate`](super::generate) makes, as entities,
//! paths and fills recorded by key in each wall's [`Made`], so regenerating updates the same
//! ones. A key whose geometry is unchanged isn't touched, so a commit journals only what an
//! edit changed.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use drift_core::{
    Appearance, CornerKey, DVec2, Document, EntityId, EntityKind, Fill, FillId, FillKind, LayerId,
    LineType, LoopKey, Made, Part, PathId, Shape, Wall, WallId,
};

use super::{EndIn, Generated, Joins, Seg, WallIn, generate_with};

/// What a regeneration did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Why something isn't as asked (see [`Generated::notices`]).
    pub notices: Vec<String>,
    /// Generated entities that existed and had to be changed (on opening a file: what an
    /// older build edited outside the wall).
    pub rebuilt: usize,
    pub created: usize,
    pub deleted: usize,
    /// Anything at all was written.
    pub changed: bool,
    /// End points joined by position (see [`Generated::aliased`]).
    pub aliased: BTreeMap<EntityId, EntityId>,
}

/// The tolerance walls join within: the drawing's gap tolerance.
pub fn snap(doc: &Document) -> f64 {
    doc.rules().gap_tolerance.max(1e-6)
}

/// A wall's centreline as segments in the order and direction it runs, and its points;
/// None (with why) if it isn't a chain of lines and arcs.
pub fn centreline(doc: &Document, wall: &Wall) -> Result<(Vec<Seg>, Vec<EntityId>, bool), String> {
    let path = doc.path(wall.centre).ok_or("its centreline is gone")?;
    let ends = |c: EntityId| -> Result<[EntityId; 2], String> {
        match doc.get(c).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) => Ok([a, b]),
            Some(EntityKind::Arc { start, end, .. }) => Ok([start, end]),
            _ => Err("its centreline has a curve that isn't a line or an arc".into()),
        }
    };
    let n = path.curves.len();
    if n == 0 {
        return Err("its centreline is empty".into());
    }
    // Which way each curve runs: on from the one before.
    let mut forward = Vec::with_capacity(n);
    let first = ends(path.curves[0])?;
    let mut at = if n > 1 {
        let next = ends(path.curves[1])?;
        if next.contains(&first[1]) {
            forward.push(true);
            first[1]
        } else if next.contains(&first[0]) {
            forward.push(false);
            first[0]
        } else {
            return Err("its centreline has a gap".into());
        }
    } else {
        forward.push(true);
        first[1]
    };
    let start = if forward[0] { first[0] } else { first[1] };
    let mut points = vec![start, at];
    for &c in &path.curves[1..] {
        let e = ends(c)?;
        if e[0] == at {
            forward.push(true);
            at = e[1];
        } else if e[1] == at {
            forward.push(false);
            at = e[0];
        } else {
            return Err("its centreline has a gap".into());
        }
        points.push(at);
    }
    let closed = n > 1 && path.closed && at == start;
    if closed {
        points.pop();
    }
    let p = |id: EntityId| doc.point(id).map_err(|e| e.to_string());
    let mut segs = Vec::with_capacity(n);
    for (&c, &fwd) in path.curves.iter().zip(&forward) {
        segs.push(match doc.get(c).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) => {
                let (a, b) = (p(a)?, p(b)?);
                if fwd {
                    Seg::Line { a, b }
                } else {
                    Seg::Line { a: b, b: a }
                }
            }
            Some(EntityKind::Arc { center, start, end }) => {
                let (c, s, e) = (p(center)?, p(start)?, p(end)?);
                let radius = s.distance(c);
                if fwd {
                    Seg::Arc {
                        center: c,
                        radius,
                        a: s,
                        b: e,
                        ccw: true,
                    }
                } else {
                    Seg::Arc {
                        center: c,
                        radius,
                        a: e,
                        b: s,
                        ccw: false,
                    }
                }
            }
            _ => unreachable!("checked above"),
        });
    }
    Ok((segs, points, closed))
}

/// The walls of a document as the generator sees them, with notes on any that can't be
/// read (left as they are).
pub fn wall_inputs(doc: &Document) -> (Vec<WallIn>, Vec<String>) {
    let mut clusters: Vec<(Option<drift_core::FillStyle>, LayerId)> = Vec::new();
    let mut out = Vec::new();
    let mut notes = Vec::new();
    for (id, w) in doc.walls() {
        let (segs, points, closed) = match centreline(doc, w) {
            Ok(c) => c,
            Err(why) => {
                notes.push(format!("Wall {id} couldn't be made: {why}"));
                continue;
            }
        };
        let key = (w.poche.clone(), w.layer);
        let cluster = match clusters.iter().position(|c| *c == key) {
            Some(i) => i,
            None => {
                clusters.push(key);
                clusters.len() - 1
            }
        };
        let end = |e: &drift_core::WallEnd| EndIn {
            join: e.join,
            tee: e.tee,
            against: e.against.and_then(|c| doc.shape(c)),
        };
        out.push(WallIn {
            id,
            segs,
            points,
            closed,
            offsets: w.offsets(),
            cluster: u32::try_from(cluster).unwrap_or(u32::MAX),
            cross: w.cross,
            ends: [end(&w.ends[0]), end(&w.ends[1])],
        });
    }
    (out, notes)
}

/// The kind a generated curve should be, from its corners' entities.
fn kind_of(
    piece: &super::Piece,
    id: &impl Fn(CornerKey) -> Option<EntityId>,
) -> Option<EntityKind> {
    let (a, b) = (id(piece.from)?, id(piece.to)?);
    Some(match piece.arc {
        None => EntityKind::Line { a, b },
        Some((c, ccw)) => {
            let center = id(c)?;
            if ccw {
                EntityKind::Arc {
                    center,
                    start: a,
                    end: b,
                }
            } else {
                EntityKind::Arc {
                    center,
                    start: b,
                    end: a,
                }
            }
        }
    })
}

/// Regenerate every wall of `doc` inside its open transaction (or outside one): faces, end
/// edges and corners are updated by key, new keys get new entities and keys that have gone
/// delete theirs; each cluster's outline paths and poché fills follow. Walls that can't be
/// made keep what they had.
pub fn regenerate(doc: &mut Document) -> Report {
    regenerate_with(doc, None)
}

/// [`regenerate`], keeping `joins` (a drag's) for the ends that aren't live.
pub fn regenerate_with(doc: &mut Document, joins: Option<&Joins>) -> Report {
    let mut report = Report::default();
    if !doc.has_walls() {
        return report;
    }
    let (inputs, notes) = wall_inputs(doc);
    report.notices = notes;
    let g = generate_with(&inputs, snap(doc), joins);
    report.notices.extend(g.notices.iter().cloned());
    report.aliased = g.aliased.clone();
    write(doc, &g, &mut report);
    report
}

/// A relation on a face piece: which face it is on (wall, segment, side) and where along it,
/// so that it follows onto whichever piece is there after a regeneration (a T arriving
/// part-way along splits a piece, and the part after it is a new piece).
struct FaceRef {
    id: drift_core::ConstraintId,
    entity: EntityId,
    wall: WallId,
    seg: u16,
    part: Part,
    /// Where on the face it is: the foot of what it relates the face to, or the piece's
    /// middle.
    place: DVec2,
}

fn face_refs(doc: &Document, walls: &BTreeMap<WallId, Wall>) -> Vec<FaceRef> {
    let mut faces: HashMap<EntityId, (WallId, drift_core::FaceKey)> = HashMap::new();
    for (w, wall) in walls {
        for (k, e) in &wall.made.curves {
            if matches!(k.part, Part::Left | Part::Right) {
                faces.insert(*e, (*w, *k));
            }
        }
    }
    let mut out = Vec::new();
    if faces.is_empty() {
        return out;
    }
    for (id, c) in doc.constraints() {
        for entity in c.entities() {
            let Some(&(wall, key)) = faces.get(&entity) else {
                continue;
            };
            let Some(shape) = doc.shape(entity) else {
                continue;
            };
            let other = c
                .entities()
                .find(|e| *e != entity)
                .and_then(|e| doc.ref_point(e));
            let place = match other {
                Some(p) => crate::nearest_point(&shape, p),
                None => {
                    let b = shape.bounds();
                    crate::nearest_point(&shape, (b.min + b.max) / 2.0)
                }
            };
            out.push(FaceRef {
                id,
                entity,
                wall,
                seg: key.seg,
                part: key.part,
                place,
            });
        }
    }
    out
}

/// Move relations on face pieces onto the piece of their face nearest where they are, once
/// the walls' new pieces are known (before the pieces that have gone are deleted). Relations
/// whose face has gone are left to go with it.
fn follow_face_refs(
    doc: &mut Document,
    refs: &[FaceRef],
    g: &Generated,
    new: &BTreeMap<WallId, Made>,
    report: &mut Report,
) {
    for r in refs {
        let Some(m) = new.get(&r.wall) else {
            continue;
        };
        let best = m
            .curves
            .iter()
            .filter(|(k, _)| k.seg == r.seg && k.part == r.part)
            .filter_map(|(k, e)| {
                let shape = g.shape(g.curves.get(&(r.wall, *k))?)?;
                Some((crate::distance(&shape, r.place), *e))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, e)| e);
        let Some(to) = best.filter(|e| *e != r.entity) else {
            if best.is_none() {
                report.notices.push(format!(
                    "A relation on wall {}'s face went with the face",
                    r.wall
                ));
            }
            continue;
        };
        if let Some(c) = doc.constraint(r.id).copied() {
            let moved = c.map_entities(|e| if e == r.entity { to } else { e });
            if doc.set_constraint(r.id, moved).is_ok() {
                report.changed = true;
            }
        }
    }
}

fn write(doc: &mut Document, g: &Generated, report: &mut Report) {
    let walls: BTreeMap<WallId, Wall> = doc.walls().map(|(id, w)| (id, w.clone())).collect();
    let refs = face_refs(doc, &walls);
    // Walls that made something this time; the others keep what they had.
    let made: BTreeSet<WallId> = g.curves.keys().map(|(w, _)| *w).collect();
    let old_points: HashMap<CornerKey, EntityId> = walls
        .values()
        .flat_map(|w| w.made.points.iter().map(|(k, id)| (*k, *id)))
        .collect();
    let old_generated: HashSet<EntityId> = walls.values().flat_map(|w| w.made.entities()).collect();
    let mut new: BTreeMap<WallId, Made> = walls
        .iter()
        .map(|(id, w)| {
            let mut m = Made::default();
            if !made.contains(id) {
                m.curves = w.made.curves.clone();
                m.points = w.made.points.clone();
            }
            (*id, m)
        })
        .collect();
    let layer = |w: WallId| walls.get(&w).map_or(LayerId::DEFAULT, |w| w.layer);
    // Corners.
    let mut point_of: HashMap<CornerKey, EntityId> = HashMap::new();
    for (key, &(owner, at)) in &g.points {
        let Some(m) = new.get_mut(&owner) else {
            continue;
        };
        let reuse = old_points
            .get(key)
            .copied()
            .filter(|id| doc.get(*id).is_some_and(|e| e.kind.is_point()));
        let id = match reuse {
            Some(id) => {
                if doc.point(id).ok() != Some(at) {
                    let _ = doc.set_point(id, at);
                    report.rebuilt += 1;
                    report.changed = true;
                }
                id
            }
            None => {
                report.created += 1;
                report.changed = true;
                doc.add_point(at)
            }
        };
        if doc.get(id).is_some_and(|e| e.layer != layer(owner)) {
            let _ = doc.set_entity_layer(id, layer(owner));
            report.changed = true;
        }
        point_of.insert(*key, id);
        m.points.insert(*key, id);
    }
    // Curves.
    let corner = |k: CornerKey| point_of.get(&k).copied();
    for (&(w, key), piece) in &g.curves {
        let Some(kind) = kind_of(piece, &corner) else {
            continue;
        };
        let old = walls
            .get(&w)
            .and_then(|wall| wall.made.curves.get(&key).copied())
            .filter(|id| {
                doc.get(*id).is_some_and(|e| {
                    std::mem::discriminant(&e.kind) == std::mem::discriminant(&kind)
                })
            });
        let id = match old {
            Some(id) => {
                if doc.get(id).map(|e| e.kind) != Some(kind)
                    && doc.set_entity_kind_exact(id, kind).is_ok()
                {
                    report.rebuilt += 1;
                    report.changed = true;
                }
                id
            }
            None => {
                let made = match kind {
                    EntityKind::Line { a, b } => doc.add_line(a, b),
                    EntityKind::Arc { center, start, end } => doc.add_arc_exact(center, start, end),
                    _ => continue,
                };
                let Ok(id) = made else {
                    report
                        .notices
                        .push(format!("Wall {w}: a piece of no size was left out"));
                    continue;
                };
                report.created += 1;
                report.changed = true;
                id
            }
        };
        if let Some(e) = doc.get(id).copied() {
            if e.layer != layer(w) {
                let _ = doc.set_entity_layer(id, layer(w));
                report.changed = true;
            }
            if e.style != drift_core::LineKind::Solid && old.is_none() {
                doc.set_style(id, drift_core::LineKind::Solid);
            }
            // Edges another line draws are never stroked; a seam that is to be drawn again
            // is.
            let none = e.appearance.line_type == Some(LineType::None);
            let wanted = if !piece.stroke {
                Some(Appearance {
                    line_type: Some(LineType::None),
                    ..e.appearance
                })
            } else if none && matches!(key.part, Part::Seam(..) | Part::Butt(_)) {
                Some(Appearance {
                    line_type: None,
                    ..e.appearance
                })
            } else {
                None
            };
            if let Some(a) = wanted.filter(|a| *a != e.appearance) {
                let _ = doc.set_appearance(id, a);
                report.changed = true;
            }
        }
        if let Some(m) = new.get_mut(&w) {
            m.curves.insert(key, id);
        }
    }
    // Outline paths and poché fills, in their lead walls'.
    let old_outlines: Vec<(WallId, LoopKey, PathId)> = walls
        .iter()
        .flat_map(|(w, wall)| wall.made.outlines.iter().map(move |(k, p)| (*w, *k, *p)))
        .collect();
    let old_fills: Vec<(WallId, LoopKey, FillId)> = walls
        .iter()
        .flat_map(|(w, wall)| wall.made.fills.iter().map(move |(k, f)| (*w, *k, *f)))
        .collect();
    let mut used_paths: HashSet<PathId> = HashSet::new();
    let mut used_fills: HashSet<FillId> = HashSet::new();
    for lo in &g.loops {
        let curves: Option<Vec<EntityId>> = lo
            .edges
            .iter()
            .map(|(w, k)| new.get(w).and_then(|m| m.curves.get(k).copied()))
            .collect();
        let Some(curves) = curves else {
            continue;
        };
        let in_loop: BTreeSet<WallId> = lo.edges.iter().map(|(w, _)| *w).collect();
        // Its path: the lead's by that key, else (the lead changed) one of its walls'.
        let path = drift_core::Path {
            curves: curves.clone(),
            closed: true,
        };
        let pid = match pick(&old_outlines, lo, &in_loop)
            .filter(|p| doc.path(*p).is_some() && !used_paths.contains(p))
        {
            Some(p) => {
                if doc.path(p) != Some(&path) {
                    let _ = doc.set_path(p, path);
                    report.changed = true;
                }
                p
            }
            None => match doc.add_path(path) {
                Ok(p) => {
                    report.changed = true;
                    p
                }
                Err(_) => continue,
            },
        };
        used_paths.insert(pid);
        let Some(m) = new.get_mut(&lo.lead) else {
            continue;
        };
        m.outlines.insert(lo.key, pid);
        if lo.key.hole != 0 {
            continue;
        }
        let Some(lead) = walls.get(&lo.lead) else {
            continue;
        };
        let Some(style) = &lead.poche else {
            continue;
        };
        // Arrange order: the highest of its walls' (who set none, wherever it is).
        let order = in_loop.iter().filter_map(|w| walls.get(w)?.order).max();
        let mut boundary = curves.clone();
        boundary.sort_unstable();
        boundary.dedup();
        let existing = pick(&old_fills, lo, &in_loop)
            .filter(|f| doc.fill(*f).is_some() && !used_fills.contains(f));
        let fid = match existing {
            Some(f) => {
                let mut fill = doc.fill(f).cloned().expect("checked");
                let kept = match (&fill.kind, &style.kind) {
                    // A hatch aligned by hand on the fill keeps its layout.
                    (FillKind::Hatch(h), FillKind::Hatch(s)) if h.pattern == s.pattern => {
                        Some((h.scale, h.angle, h.origin))
                    }
                    _ => None,
                };
                style.apply(&mut fill);
                hatch_opacity(&mut fill);
                if let (Some((scale, angle, origin)), FillKind::Hatch(h)) = (kept, &mut fill.kind) {
                    h.scale = scale;
                    h.angle = angle;
                    h.origin = origin;
                }
                fill.boundary = boundary;
                fill.path = Some(pid);
                fill.layer = lead.layer;
                fill.divided = false;
                // (Its seed stays: a poché's area comes from its outline paths, not its seed.)
                if let Some(o) = order {
                    fill.order = o;
                }
                if doc.fill(f) != Some(&fill) {
                    doc.set_fill(f, fill);
                    report.changed = true;
                }
                f
            }
            None => {
                let mut fill = Fill::new(boundary, lo.seed, FillKind::Solid);
                style.apply(&mut fill);
                hatch_opacity(&mut fill);
                fill.path = Some(pid);
                let f = doc.add_fill(fill.clone());
                let mut placed = doc.fill(f).cloned().expect("just added");
                placed.layer = lead.layer;
                placed.order = order.unwrap_or_else(|| {
                    doc.fills()
                        .filter(|(id, x)| *id != f && x.layer == lead.layer)
                        .map(|(_, x)| x.order + 1)
                        .max()
                        .unwrap_or(0)
                });
                doc.set_fill(f, placed);
                report.changed = true;
                f
            }
        };
        used_fills.insert(fid);
        m.fills.insert(lo.key, fid);
    }
    // What has gone: fills and paths first (so deleting curves prunes nothing of ours), then
    // curves and the corners only they used.
    for (_, _, f) in &old_fills {
        if !used_fills.contains(f) && doc.fill(*f).is_some() {
            doc.remove_fill(*f);
            report.changed = true;
        }
    }
    for (_, _, p) in &old_outlines {
        if !used_paths.contains(p) && doc.path(*p).is_some() {
            doc.remove_path(*p);
            report.changed = true;
        }
    }
    follow_face_refs(doc, &refs, g, &new, report);
    let kept: HashSet<EntityId> = new.values().flat_map(|m| m.entities()).collect();
    let doomed_curves: HashSet<EntityId> = old_generated
        .iter()
        .copied()
        .filter(|id| !kept.contains(id) && doc.get(*id).is_some_and(|e| !e.kind.is_point()))
        .collect();
    let mut doomed: Vec<EntityId> = doomed_curves.iter().copied().collect();
    for id in old_generated.iter().copied() {
        if kept.contains(&id) || !doc.get(id).is_some_and(|e| e.kind.is_point()) {
            continue;
        }
        // (A point someone else's curve was drawn from stays, as a plain point.)
        if doc.users_of(id).all(|u| doomed_curves.contains(&u)) {
            doomed.push(id);
        }
    }
    doomed.sort_unstable();
    if !doomed.is_empty() {
        report.deleted = doomed.len();
        report.changed = true;
        doc.delete(doomed);
    }
    for (id, m) in new {
        if let Some(w) = doc.wall(id).cloned()
            && w.made != m
        {
            let _ = doc.set_wall(id, Wall { made: m, ..w });
            report.changed = true;
        }
    }
    // The tees the free ends were found in, so they follow their hosts from now on.
    for (&(id, e), &tee) in &g.tees {
        if let Some(mut w) = doc.wall(id).cloned()
            && let Some(end) = w.ends.get_mut(usize::from(e))
            && end.tee != tee
        {
            end.tee = tee;
            let _ = doc.set_wall(id, w);
            report.changed = true;
        }
    }
}

/// A hatch has no opacity of its own (files don't keep one): it stays at a fill's default,
/// so what is regenerated matches what is read back.
fn hatch_opacity(fill: &mut Fill) {
    if matches!(fill.kind, FillKind::Hatch(_)) {
        fill.opacity = Fill::new(Vec::new(), DVec2::ZERO, FillKind::Solid).opacity;
    }
}

/// What a loop had before, by its key: its lead's, else (the lead changed) one of its
/// walls'.
fn pick<T: Copy>(
    list: &[(WallId, LoopKey, T)],
    lo: &super::LoopOut,
    in_loop: &BTreeSet<WallId>,
) -> Option<T> {
    list.iter()
        .filter(|(w, k, _)| *k == lo.key && (*w == lo.lead || in_loop.contains(w)))
        .min_by_key(|(w, ..)| (*w != lo.lead, *w))
        .map(|(.., x)| *x)
}

/// The shapes of a document's walls' faces as they would be generated now (for a drag's
/// preview, or a check), without writing anything.
pub fn shapes(doc: &Document) -> Vec<Shape> {
    let (inputs, _) = wall_inputs(doc);
    generate_with(&inputs, snap(doc), None).shapes()
}

/// A closed path of lines and arcs as a loop of edges, in its order (each curve run the way
/// that meets the next); None if it isn't one.
pub fn path_loop(doc: &Document, curves: &[EntityId]) -> Option<crate::regions::Loop> {
    use crate::regions::Edge;
    let ends = |c: EntityId| doc.get(c).and_then(|e| e.kind.ends());
    let n = curves.len();
    let mut edges = Vec::with_capacity(n);
    for (i, &c) in curves.iter().enumerate() {
        let [_, e] = ends(c)?;
        let next = ends(curves[(i + 1) % n])?;
        // Run towards the end it shares with the next curve.
        let forward = next.contains(&e) || (n == 1);
        let edge = match doc.get(c)?.kind {
            EntityKind::Line { a, b } => {
                let (a, b) = (doc.point(a).ok()?, doc.point(b).ok()?);
                if forward {
                    Edge::Line { a, b }
                } else {
                    Edge::Line { a: b, b: a }
                }
            }
            EntityKind::Arc { center, start, end } => {
                let (c0, p0, p1) = (
                    doc.point(center).ok()?,
                    doc.point(start).ok()?,
                    doc.point(end).ok()?,
                );
                let a0 = (p0 - c0).y.atan2((p0 - c0).x);
                let mut sweep =
                    ((p1 - c0).y.atan2((p1 - c0).x) - a0).rem_euclid(std::f64::consts::TAU);
                if sweep <= 0.0 {
                    sweep = std::f64::consts::TAU;
                }
                let radius = p0.distance(c0);
                if forward {
                    Edge::Arc {
                        center: c0,
                        radius,
                        start: a0,
                        sweep,
                    }
                } else {
                    Edge::Arc {
                        center: c0,
                        radius,
                        start: a0 + sweep,
                        sweep: -sweep,
                    }
                }
            }
            _ => return None,
        };
        edges.push(edge);
    }
    Some(crate::regions::Loop {
        sources: (0..n).collect(),
        edges,
    })
}

/// The region of a wall cluster's poché: its outer outline path with the paths of its holes,
/// found from them alone (nothing else in the drawing is looked at).
pub fn outline_region(
    doc: &Document,
    outer: PathId,
    holes: &[PathId],
) -> Option<crate::regions::Region> {
    let lp = |p: PathId| {
        let l = path_loop(doc, &doc.path(p)?.curves)?;
        let area = l.signed_area();
        Some((l.edges, area, l.sources))
    };
    let orient = |(edges, area, sources): (Vec<crate::regions::Edge>, f64, Vec<usize>),
                  ccw: bool| {
        let edges = if (area > 0.0) == ccw {
            edges
        } else {
            edges
                .iter()
                .rev()
                .map(|e| match *e {
                    crate::regions::Edge::Line { a, b } => {
                        crate::regions::Edge::Line { a: b, b: a }
                    }
                    crate::regions::Edge::Arc {
                        center,
                        radius,
                        start,
                        sweep,
                    } => crate::regions::Edge::Arc {
                        center,
                        radius,
                        start: start + sweep,
                        sweep: -sweep,
                    },
                })
                .collect()
        };
        crate::regions::Loop { edges, sources }
    };
    let outer = orient(lp(outer)?, true);
    let holes = holes
        .iter()
        .filter_map(|h| lp(*h))
        .map(|l| orient(l, false))
        .collect();
    Some(crate::regions::Region::from_loops(outer, holes))
}

/// Regenerate walls after some of their centrelines' segments moved (`moved`: wall and
/// segment), inside the open transaction, when the joins stay what they were: only the nodes
/// round the moved segments are worked out again, and the corners they move are moved. The
/// faces keep their entities, and nothing else is touched (outlines and pochés follow their
/// curves by themselves).
///
/// None when the move changed what the walls make (a join appeared or went, a piece came or
/// went, a corner changed hands): then regenerate everything with [`regenerate`].
pub fn regenerate_moved(doc: &mut Document, moved: &BTreeSet<(WallId, u16)>) -> Option<Report> {
    regenerate_moved_with(doc, moved, None)
}

/// [`regenerate_moved`], keeping `joins` (a drag's) for the ends that aren't live.
pub fn regenerate_moved_with(
    doc: &mut Document,
    moved: &BTreeSet<(WallId, u16)>,
    joins: Option<&Joins>,
) -> Option<Report> {
    let mut report = Report::default();
    if moved.is_empty() {
        return Some(report);
    }
    let (inputs, notes) = wall_inputs(doc);
    if !notes.is_empty() {
        return None;
    }
    let mut g = super::Gen::new(&inputs, snap(doc));
    g.frozen = joins;
    g.make_faces();
    if g.faces.iter().any(Option::is_none) {
        return None;
    }
    let index: HashMap<WallId, usize> = inputs.iter().enumerate().map(|(i, w)| (w.id, i)).collect();
    let mut s0: HashSet<(usize, usize)> = HashSet::new();
    for &(w, k) in moved {
        let i = *index.get(&w)?;
        if usize::from(k) >= inputs[i].segs.len() {
            return None;
        }
        s0.insert((i, usize::from(k)));
    }
    // Only the nodes on what moved, then every node on the segments those meet.
    g.focus_on(s0.iter().copied());
    let nodes = g.nodes();
    if !g.out.notices.is_empty() {
        return None;
    }
    // The segments with an end (or a station) at a node a moved segment meets, and every
    // node those segments meet.
    let touches = |n: &super::Node, s: &HashSet<(usize, usize)>| {
        n.ends.iter().any(|e| s.contains(&(e.w, e.seg)))
    };
    let mut s1 = s0.clone();
    for n in nodes.iter().filter(|n| touches(n, &s0)) {
        s1.extend(n.ends.iter().map(|e| (e.w, e.seg)));
    }
    g.focus_on(s1.iter().copied());
    let nodes = g.nodes();
    if !g.out.notices.is_empty() {
        return None;
    }
    report.aliased = g.out.aliased.clone();
    let around: Vec<&super::Node> = nodes.iter().filter(|n| touches(n, &s1)).collect();
    for n in &around {
        g.solve(n);
    }
    if !g.out.notices.is_empty() {
        return None;
    }
    g.events.retain(|f, _| s1.contains(&(f.0, f.1)));
    g.edges.retain(|e| {
        index
            .get(&e.owner)
            .is_some_and(|&i| s1.contains(&(i, usize::from(e.key.seg))))
    });
    g.pieces();
    if !g.alias.is_empty() || !g.out.notices.is_empty() {
        return None;
    }
    g.drop_empty_edges();
    if !g.alias.is_empty() {
        return None;
    }
    // The same pieces as before, between the same corners.
    let involved: BTreeSet<WallId> = around
        .iter()
        .flat_map(|n| n.ends.iter().map(|e| e.key.wall))
        .collect();
    let mut old_points: HashMap<CornerKey, EntityId> = HashMap::new();
    for w in &involved {
        old_points.extend(doc.wall(*w)?.made.points.iter().map(|(k, id)| (*k, *id)));
    }
    let mut segs_of: BTreeMap<WallId, BTreeSet<u16>> = BTreeMap::new();
    for &(i, k) in &s1 {
        segs_of.entry(inputs[i].id).or_default().insert(k as u16);
    }
    for (w, segs) in &segs_of {
        let made = &doc.wall(*w)?.made;
        let old: BTreeSet<FaceKeyOf> = made
            .curves
            .keys()
            .filter(|k| segs.contains(&k.seg))
            .copied()
            .collect();
        let new: BTreeSet<FaceKeyOf> = g
            .edges
            .iter()
            .filter(|e| e.owner == *w)
            .map(|e| e.key)
            .collect();
        if old != new {
            return None;
        }
    }
    let corner = |k: CornerKey| old_points.get(&k).copied();
    for e in &g.edges {
        let id = *doc.wall(e.owner)?.made.curves.get(&e.key)?;
        let kind = kind_of(&e.piece, &corner)?;
        if doc.get(id).map(|x| x.kind) != Some(kind) {
            return None;
        }
    }
    for (key, (owner, _)) in &g.out.points {
        let id = corner(*key)?;
        if doc.wall(*owner)?.made.points.get(key) != Some(&id) {
            return None;
        }
    }
    // Move what moved.
    for (key, &(_, at)) in &g.out.points {
        let id = old_points[key];
        if doc.point(id).ok() != Some(at) {
            let _ = doc.set_point(id, at);
            report.changed = true;
        }
    }
    Some(report)
}

type FaceKeyOf = drift_core::FaceKey;
