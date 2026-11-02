//! Writing offsets into a document: the pieces of each offset's source chain, as entities
//! recorded by key in the offset's [`OffsetMade`], so regenerating updates the same ones. A
//! key whose geometry is unchanged isn't touched, so a commit journals only what an edit
//! changed.
//!
//! What each key names: a piece is the `n`-th offset curve of one source curve (by id, so a
//! re-chain keeps the pieces of the curves that stayed), or the bridge across the corner
//! after one; a point is where a piece starts (shared with the piece before when they meet),
//! where it ends when nothing starts there, or a fitted pen curve's handle.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use drift_core::{
    DVec2, Document, EntityId, EntityKind, Offset, OffsetId, OffsetMade, PieceKey, PointKey, Shape,
    tolerance,
};

use super::{OffsetPiece, chain_through, piece_end, pieces};

/// What a regeneration did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Why an offset isn't as asked (it keeps what it had).
    pub notices: Vec<String>,
    /// Generated entities that existed and had to be changed: a piece whose kind changed,
    /// a point that moved (on opening a file: what an older build edited outside the
    /// offset).
    pub rebuilt: usize,
    pub created: usize,
    pub deleted: usize,
    /// Anything at all was written.
    pub changed: bool,
}

/// Regenerate every offset of `doc` inside its open transaction (or outside one): pieces
/// and the points between them are updated by key, new keys get new entities and keys that
/// have gone delete theirs. An offset that can't be made keeps what it had.
pub fn regenerate(doc: &mut Document) -> Report {
    let ids: BTreeSet<OffsetId> = doc.offsets().map(|(id, _)| id).collect();
    regenerate_some(doc, &ids)
}

/// [`regenerate`] for the offsets in `ids` only (those whose source, record or pieces an
/// edit touched).
pub fn regenerate_some(doc: &mut Document, ids: &BTreeSet<OffsetId>) -> Report {
    let mut report = Report::default();
    for &id in ids {
        if let Some(o) = doc.offset(id).cloned() {
            regenerate_one(doc, id, o, &mut report);
        }
    }
    report
}

fn is_curve(doc: &Document, id: EntityId) -> bool {
    doc.get(id).is_some_and(|e| !e.kind.is_point())
}

/// The source chain as it is now: the curves of `source` that still exist, in travel
/// order, with whatever the chain through them now holds between the first and the last of
/// them (a split or filleted curve's new pieces), and whether it closes. Curves beyond the
/// first and last old curve are never adopted, even when they connect. None when nothing
/// is left.
pub fn rechain(doc: &Document, source: &[EntityId]) -> Option<(Vec<(EntityId, bool)>, bool)> {
    let alive: Vec<EntityId> = source
        .iter()
        .copied()
        .filter(|c| is_curve(doc, *c))
        .collect();
    let &first = alive.first()?;
    let (mut walk, closed) = chain_through(doc, first);
    if walk.len() == 1 {
        return Some((walk, closed));
    }
    let pos = |walk: &[(EntityId, bool)], c: EntityId| walk.iter().position(|(w, _)| *w == c);
    // Oriented as the old chain ran: the old curves still on the walk come in their old
    // order; a lone survivor runs as it did (as stored).
    let on_walk: Vec<usize> = alive.iter().filter_map(|c| pos(&walk, *c)).collect();
    let reversed = match on_walk.as_slice() {
        [] => return None,
        [one] => !walk[*one].1,
        many => {
            let start = many[0];
            // Steps forward from the first to each later one, round the loop when closed.
            let ahead = |p: usize| {
                if p >= start {
                    p - start
                } else if closed {
                    walk.len() + p - start
                } else {
                    usize::MAX
                }
            };
            let fwd = many.windows(2).all(|w| ahead(w[1]) > ahead(w[0]));
            !fwd
        }
    };
    if reversed {
        walk.reverse();
        for w in &mut walk {
            w.1 = !w.1;
        }
    }
    let on_walk: Vec<usize> = alive.iter().filter_map(|c| pos(&walk, *c)).collect();
    let (&first_at, &last_at) = (on_walk.first()?, on_walk.iter().max()?);
    if closed {
        // The loop, from the first old curve round; whole when every old curve is still on
        // it, else the run from the first to the last of them.
        walk.rotate_left(first_at);
        if on_walk.len() == source.len() {
            return Some((walk, true));
        }
        let on_walk: Vec<usize> = alive.iter().filter_map(|c| pos(&walk, *c)).collect();
        let last_at = *on_walk.iter().max()?;
        walk.truncate(last_at + 1);
        return Some((walk, false));
    }
    let run = walk[first_at..=last_at].to_vec();
    Some((run, false))
}

/// What one piece is written as.
struct Plan {
    key: PieceKey,
    piece: OffsetPiece,
    start: Option<PointKey>,
    end: Option<PointKey>,
}

fn regenerate_one(doc: &mut Document, id: OffsetId, mut o: Offset, report: &mut Report) {
    // 1. The source as it is now (nothing left: the prune step takes the record).
    let Some((chain, closed)) = rechain(doc, &o.source) else {
        return;
    };
    let source: Vec<EntityId> = chain.iter().map(|(c, _)| *c).collect();
    let source_changed = source != o.source;
    o.source = source;
    // 2. The pieces; an offset that can't be made keeps what it had.
    let made = match pieces(doc, &chain, closed, o.signed()) {
        Ok(p) => p,
        Err(e) => {
            report
                .notices
                .push(format!("Offset {id} can't be made at that distance: {e}"));
            if source_changed && doc.set_offset(id, o).is_ok() {
                report.changed = true;
            }
            return;
        }
    };
    // 3. Keys: each piece by its source curve and its number among that curve's pieces (or
    // the bridge after the source curve); points by the piece that starts there.
    let n = made.len();
    let mut counts: HashMap<EntityId, u16> = HashMap::new();
    let keys: Vec<PieceKey> = made
        .iter()
        .map(|p| {
            let of = chain[p.source].0;
            if p.bridge {
                PieceKey::Bridge { after: of }
            } else {
                let c = counts.entry(of).or_default();
                let k = PieceKey::Of { of, n: *c };
                *c += 1;
                k
            }
        })
        .collect();
    let same = |a: DVec2, b: DVec2| a.distance(b) <= tolerance::same(b.abs().max_element());
    let has_ends = |s: &Shape| !matches!(s, Shape::Circle { .. });
    let mut points: BTreeMap<PointKey, DVec2> = BTreeMap::new();
    let mut plans: Vec<Plan> = Vec::with_capacity(n);
    for (k, piece) in made.iter().enumerate() {
        if !has_ends(&piece.shape) {
            plans.push(Plan {
                key: keys[k],
                piece: *piece,
                start: None,
                end: None,
            });
            continue;
        }
        let s = piece_end(&piece.shape, piece.forward, false);
        let e = piece_end(&piece.shape, piece.forward, true);
        let start = PointKey::Start(keys[k]);
        points.insert(start, s);
        let next = if k + 1 < n {
            Some(k + 1)
        } else if closed && n > 1 {
            Some(0)
        } else {
            None
        };
        let shared = next.filter(|j| {
            has_ends(&made[*j].shape)
                && same(e, piece_end(&made[*j].shape, made[*j].forward, false))
        });
        let end = match shared {
            Some(j) => PointKey::Start(keys[j]),
            None => {
                let key = PointKey::End(keys[k]);
                points.insert(key, e);
                key
            }
        };
        if let Shape::Bezier { p } = piece.shape {
            points.insert(PointKey::Handle(keys[k], 0), p[1]);
            points.insert(PointKey::Handle(keys[k], 1), p[2]);
        }
        plans.push(Plan {
            key: keys[k],
            piece: *piece,
            start: Some(start),
            end: Some(end),
        });
    }
    // 4. Write: points first (moved or new), then the pieces on them.
    let old = o.made.clone();
    let mut new = OffsetMade::default();
    for (key, at) in &points {
        match old.points.get(key).copied().filter(|p| doc.contains(*p)) {
            Some(p) => {
                if doc.point(p).ok() != Some(*at) {
                    let _ = doc.set_point(p, *at);
                    report.rebuilt += 1;
                    report.changed = true;
                }
                new.points.insert(*key, p);
            }
            None => {
                new.points.insert(*key, doc.add_point(*at));
                report.created += 1;
                report.changed = true;
            }
        }
    }
    // The look a new piece gets: a sibling piece's, else its source curve's.
    let sibling = old.curves.values().copied().find(|c| doc.contains(*c));
    for plan in &plans {
        let pt = |k: Option<PointKey>| k.and_then(|k| new.points.get(&k).copied());
        let of = chain[plan.piece.source].0;
        let kind = match plan.piece.shape {
            Shape::Line { .. } => {
                let (Some(a), Some(b)) = (pt(plan.start), pt(plan.end)) else {
                    continue;
                };
                EntityKind::Line { a, b }
            }
            Shape::Circle { radius, .. } => {
                let Some(EntityKind::Circle { center, .. }) = doc.get(of).map(|e| e.kind) else {
                    continue;
                };
                EntityKind::Circle { center, radius }
            }
            Shape::Arc { .. } => {
                let Some(EntityKind::Arc { center, .. }) = doc.get(of).map(|e| e.kind) else {
                    continue;
                };
                let (Some(s), Some(e)) = (pt(plan.start), pt(plan.end)) else {
                    continue;
                };
                // Stored counter-clockwise: start and end follow the source's.
                let (start, end) = if plan.piece.forward { (s, e) } else { (e, s) };
                EntityKind::Arc { center, start, end }
            }
            Shape::Bezier { .. } => {
                let (Some(a), Some(b)) = (pt(plan.start), pt(plan.end)) else {
                    continue;
                };
                let (Some(c1), Some(c2)) = (
                    new.points.get(&PointKey::Handle(plan.key, 0)).copied(),
                    new.points.get(&PointKey::Handle(plan.key, 1)).copied(),
                ) else {
                    continue;
                };
                EntityKind::Bezier { a, c1, c2, b }
            }
            Shape::Ellipse { .. } | Shape::Point(_) => continue,
        };
        match old
            .curves
            .get(&plan.key)
            .copied()
            .filter(|c| is_curve(doc, *c))
        {
            Some(c) => {
                if doc.get(c).map(|e| e.kind) != Some(kind) {
                    let set = match kind {
                        EntityKind::Arc { .. } => doc.set_entity_kind_exact(c, kind),
                        _ => doc.set_entity_kind(c, kind),
                    };
                    if set.is_ok() {
                        report.rebuilt += 1;
                        report.changed = true;
                    }
                }
                new.curves.insert(plan.key, c);
            }
            None => {
                let added = match kind {
                    EntityKind::Line { a, b } => doc.add_line(a, b),
                    EntityKind::Circle { center, radius } => doc.add_circle(center, radius),
                    EntityKind::Arc { center, start, end } => doc.add_arc(center, start, end),
                    EntityKind::Bezier { a, c1, c2, b } => doc.add_bezier(a, c1, c2, b),
                    _ => continue,
                };
                if let Ok(c) = added {
                    doc.copy_look(sibling.unwrap_or(of), c);
                    new.curves.insert(plan.key, c);
                    report.created += 1;
                    report.changed = true;
                }
            }
        }
    }
    // 5. The record first (so the prune step sees the pieces that are there), then what has
    // gone.
    let stale_curves: Vec<EntityId> = old
        .curves
        .iter()
        .filter(|(k, _)| !new.curves.contains_key(k))
        .map(|(_, c)| *c)
        .filter(|c| doc.contains(*c))
        .collect();
    let stale_points: Vec<EntityId> = old
        .points
        .iter()
        .filter(|(k, _)| !new.points.contains_key(k))
        .map(|(_, p)| *p)
        .collect();
    o.made = new;
    if doc.set_offset(id, o).is_ok() && source_changed {
        report.changed = true;
    }
    if !stale_curves.is_empty() {
        report.deleted += stale_curves.len();
        report.changed = true;
        doc.delete(stale_curves);
    }
    let stale_points: Vec<EntityId> = stale_points
        .into_iter()
        .filter(|p| doc.contains(*p) && doc.users_of(*p).next().is_none())
        .collect();
    if !stale_points.is_empty() {
        report.deleted += stale_points.len();
        report.changed = true;
        doc.delete(stale_points);
    }
}
