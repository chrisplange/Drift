//! Joins kept through a drag or a solve (W2): the generator works joins out from the geometry
//! (`Gen::joins`), so geometry that moves while something else is dragged, or while a
//! dimension on a face is solved, could flip a join from one frame to the next (an end
//! sliding past a corner turns an L into a T, a face piece splits and changes its key). A
//! drag takes the joins as they were when it started ([`infer_joins`]) and keeps them until
//! it ends; only the ends it moves itself (the [`Joins::live`] points) are worked out again
//! each frame, so dragging an end into a wall still joins it as it goes. The commit works
//! every join out afresh.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use drift_core::{DVec2, EntityId, WallId};

use super::{Gen, TeeAt, WallIn};

/// The joins of a set of walls, by id, as found at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Joins {
    /// Each free run end (wall, 0 for its start or 1 for its end) that tees into a wall: the
    /// wall and its segment.
    pub tees: BTreeMap<(WallId, u8), (WallId, u16)>,
    /// End points joined to others by where they are (an L or n-way join with no shared
    /// point): each to the point that names its join.
    pub alias: BTreeMap<EntityId, EntityId>,
    /// Points whose joins are worked out afresh each time all the same (the ends a drag
    /// moves itself).
    pub live: BTreeSet<EntityId>,
}

/// The joins `walls` have now (see [`Joins`]).
pub fn infer_joins(walls: &[WallIn], snap: f64) -> Joins {
    let mut g = Gen::new(walls, snap);
    g.make_faces();
    let made: Vec<bool> = g.faces.iter().map(Option::is_some).collect();
    let (tees, alias, ..) = g.joins(&made);
    Joins {
        tees: tees
            .iter()
            .map(|(&(w, e), t)| ((walls[w].id, e as u8), (walls[t.host].id, t.seg as u16)))
            .collect(),
        alias: alias.into_iter().collect(),
        live: BTreeSet::new(),
    }
}

/// What `Gen::joins` returns.
pub(super) type Found = (
    BTreeMap<(usize, usize), TeeAt>,
    HashMap<EntityId, EntityId>,
    HashMap<EntityId, DVec2>,
    Vec<(usize, usize)>,
);

impl Gen<'_> {
    /// The joins to make: worked out from the geometry, except (with joins kept) for ends
    /// that aren't live, which keep the joins they had. Records the position joins made.
    pub(super) fn joins_kept(&mut self, made: &[bool]) -> Found {
        let (mut tees, mut alias, at_point, free) = self.joins(made);
        if let Some(kept) = self.frozen {
            let index: HashMap<WallId, usize> = self
                .walls
                .iter()
                .enumerate()
                .map(|(i, w)| (w.id, i))
                .collect();
            for &(w, e) in &free {
                let (id, p) = self.end_point(w, e);
                if kept.live.contains(&id) {
                    continue;
                }
                tees.remove(&(w, e));
                let Some(&(host, k)) = kept.tees.get(&(self.walls[w].id, e as u8)) else {
                    continue;
                };
                let (h, k) = match index.get(&host) {
                    Some(&h) if made[h] && usize::from(k) < self.walls[h].segs.len() => {
                        (h, usize::from(k))
                    }
                    _ => continue,
                };
                if !self.near(h, k) {
                    continue;
                }
                let s = self.walls[h].segs[k];
                let u = s.param(p).clamp(0.0, s.length());
                tees.insert((w, e), TeeAt { host: h, seg: k, u });
            }
            // Joins by position: the kept ones among points that aren't live, and those
            // found now that a live point is in.
            let mut parent: HashMap<EntityId, EntityId> = HashMap::new();
            fn find(parent: &HashMap<EntityId, EntityId>, mut p: EntityId) -> EntityId {
                while let Some(&q) = parent.get(&p) {
                    p = q;
                }
                p
            }
            let union = |parent: &mut HashMap<EntityId, EntityId>, a: EntityId, b: EntityId| {
                let (a, b) = (find(parent, a), find(parent, b));
                if a != b {
                    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                    parent.insert(hi, lo);
                }
            };
            let here = |p: &EntityId| at_point.contains_key(p) && !kept.live.contains(p);
            for (p, r) in &kept.alias {
                if here(p) && here(r) {
                    union(&mut parent, *p, *r);
                }
            }
            let mut groups: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
            for (&p, &r) in &alias {
                groups.entry(r).or_insert_with(|| vec![r]).push(p);
            }
            for (root, members) in groups {
                if members.iter().any(|m| kept.live.contains(m)) {
                    for m in members {
                        union(&mut parent, root, m);
                    }
                }
            }
            alias = parent.keys().map(|&p| (p, find(&parent, p))).collect();
        }
        self.out.aliased = alias.iter().map(|(p, r)| (*p, *r)).collect();
        (tees, alias, at_point, free)
    }
}
