//! Walls in the editor: keeping them regenerated as the drawing changes
//! ([`Editor::follow_walls`], run by every commit and drag step), who owns what, the owner
//! guard modify tools ask, picking a wall by any of its parts, and the wall commands (set
//! its thickness, justification or poché, explode it, make walls from curves).
//!
//! The geometry is `drift_geom::walls`; the record is `drift_core::Wall`. See
//! `docs/walls-design.md`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use drift_core::{
    DVec2, Document, EntityId, EntityKind, FillStyle, Justify, Kind, LineKind, Owner, Owners, Path,
    PropertyDim, PropertyRef, Shape, Wall, WallId,
};

use crate::editor::{EditError, Editor};

/// What a modify tool wants to do to entities that may belong to walls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Trim, extend or split.
    Cut,
    /// Fillet or chamfer.
    Round,
    /// Make construction (or solid).
    Construction,
    /// Make walls from them.
    MakeWalls,
}

/// Thickness presets for the tool bar and Inspector: (label, actual thickness in mm).
pub struct Preset {
    pub label: &'static str,
    pub mm: f64,
}

const INCH: f64 = 25.4;

/// US walls at their actual finished thickness, labelled with the construction they come
/// from (a "2×4" is 3½″, an "8-inch" block 7⅝″).
pub const US_PRESETS: &[Preset] = &[
    Preset {
        label: "2×4 stud, ½″ gypsum both sides",
        mm: 4.5 * INCH,
    },
    Preset {
        label: "2×4 stud, ⅝″ gypsum both sides",
        mm: 4.75 * INCH,
    },
    Preset {
        label: "2×6 stud, ½″ gypsum both sides",
        mm: 6.5 * INCH,
    },
    Preset {
        label: "2×6 stud, ⅝″ gypsum both sides",
        mm: 6.75 * INCH,
    },
    Preset {
        label: "6″ CMU",
        mm: 5.625 * INCH,
    },
    Preset {
        label: "8″ CMU",
        mm: 7.625 * INCH,
    },
    Preset {
        label: "12″ CMU",
        mm: 11.625 * INCH,
    },
];

/// The generic metric list.
pub const METRIC_PRESETS: &[Preset] = &[
    Preset {
        label: "100 mm",
        mm: 100.0,
    },
    Preset {
        label: "115 mm",
        mm: 115.0,
    },
    Preset {
        label: "140 mm",
        mm: 140.0,
    },
    Preset {
        label: "150 mm",
        mm: 150.0,
    },
    Preset {
        label: "200 mm",
        mm: 200.0,
    },
    Preset {
        label: "240 mm",
        mm: 240.0,
    },
    Preset {
        label: "300 mm",
        mm: 300.0,
    },
    Preset {
        label: "365 mm",
        mm: 365.0,
    },
];

impl Editor {
    /// Who owns which entity among the walls (cached by document revision).
    pub fn wall_owners(&self) -> Rc<Owners> {
        let revision = self.doc.revision();
        if let Some((r, o)) = &*self.wall_owner_cache.borrow()
            && *r == revision
        {
            return o.clone();
        }
        let o = Rc::new(self.doc.wall_owners());
        *self.wall_owner_cache.borrow_mut() = Some((revision, o.clone()));
        o
    }

    /// The wall an entity belongs to (a face, corner or centreline curve or point).
    pub fn wall_of(&self, id: EntityId) -> Option<WallId> {
        if !self.doc.has_walls() {
            return None;
        }
        self.wall_owners().wall(id)
    }

    /// The thickness new walls get: the tool bar's, else 4½″ (a 2×4 wall, finished) in
    /// imperial drawings and 100 mm in metric ones.
    pub fn wall_thickness_now(&self) -> f64 {
        if self.wall_thickness > 0.0 {
            self.wall_thickness
        } else if self.doc.units().length.is_metric() {
            100.0
        } else {
            4.5 * INCH
        }
    }

    /// The thickness presets for the drawing's units.
    pub fn wall_presets(&self) -> &'static [Preset] {
        if self.doc.units().length.is_metric() {
            METRIC_PRESETS
        } else {
            US_PRESETS
        }
    }

    /// The poché new walls get.
    pub fn wall_poche_now(&self) -> Option<FillStyle> {
        self.wall_poche.on.then(|| self.wall_poche.style.clone())
    }

    /// A wall's centreline curves.
    pub fn wall_curves(&self, w: WallId) -> Vec<EntityId> {
        self.doc
            .wall(w)
            .and_then(|w| self.doc.path(w.centre))
            .map(|p| p.curves.clone())
            .unwrap_or_default()
    }

    /// What picking a wall selects: its centreline curves (and its group, if it is in one
    /// that isn't open).
    pub fn wall_pick_set(&self, w: WallId) -> BTreeSet<EntityId> {
        let curves = self.wall_curves(w);
        let Some(&first) = curves.first() else {
            return BTreeSet::new();
        };
        let mut set = match self.doc.group_of(first) {
            Some(g) if self.open_group != Some(g) && self.pick_filter.blocks() => self
                .doc
                .group(g)
                .map(|g| g.members.clone())
                .unwrap_or_default(),
            _ => BTreeSet::new(),
        };
        set.extend(curves);
        set
    }

    /// The walls picked: those whose centreline curves are all in the selection.
    pub fn selected_walls(&self) -> BTreeSet<WallId> {
        if !self.doc.has_walls() {
            return BTreeSet::new();
        }
        let owners = self.wall_owners();
        let candidates: BTreeSet<WallId> = self
            .selection
            .iter()
            .filter_map(|id| match owners.get(*id) {
                Some(Owner::Centre(w)) => Some(w),
                _ => None,
            })
            .collect();
        candidates
            .into_iter()
            .filter(|w| {
                let curves = self.wall_curves(*w);
                !curves.is_empty() && curves.iter().all(|c| self.selection.contains(c))
            })
            .collect()
    }

    /// Picking a wall's face, corner or poché picks the wall: its centreline curves. Points
    /// of its centreline stay points (they are its grips).
    pub(crate) fn walls_for_picks(&self, ids: impl IntoIterator<Item = EntityId>) -> Vec<EntityId> {
        if !self.doc.has_walls() {
            return ids.into_iter().collect();
        }
        let owners = self.wall_owners();
        let mut out = Vec::new();
        for id in ids {
            match owners.get(id) {
                Some(Owner::Wall(w)) => out.extend(self.wall_curves(w)),
                _ => out.push(id),
            }
        }
        out
    }

    /// The wall whose centreline is nearest `p` among those whose poché holds it (a click
    /// inside a merged poché, at a node, picks the nearest wall).
    pub fn wall_in_fill_at(&mut self, fill: drift_core::FillId, p: DVec2) -> Option<WallId> {
        let owners = self.wall_owners();
        owners.fills.get(&fill)?;
        let outline: BTreeSet<EntityId> = self
            .doc
            .fill(fill)
            .map(|f| f.boundary.iter().copied().collect())
            .unwrap_or_default();
        // The walls of its region: owners of its outline (and of its holes).
        let mut walls: BTreeSet<WallId> = outline.iter().filter_map(|c| owners.wall(*c)).collect();
        if let Some(lead) = owners.fills.get(&fill).copied()
            && let Some(w) = self.doc.wall(lead)
        {
            for p in w.made.outlines.values() {
                if let Some(path) = self.doc.path(*p) {
                    walls.extend(path.curves.iter().filter_map(|c| owners.wall(*c)));
                }
            }
        }
        walls
            .into_iter()
            .map(|w| {
                let d = self
                    .wall_curves(w)
                    .iter()
                    .filter_map(|c| self.doc.shape(*c))
                    .map(|s| drift_geom::distance(&s, p))
                    .fold(f64::INFINITY, f64::min);
                (d, w)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, w)| w)
    }

    /// Centreline curves and points not shown: those of walls neither picked nor hovered,
    /// unless centrelines are shown (the Wall or Dimension tool, or View → Wall centrelines).
    pub fn hidden_centrelines(&self) -> BTreeSet<EntityId> {
        if !self.doc.has_walls() || self.show_centrelines {
            return BTreeSet::new();
        }
        let owners = self.wall_owners();
        let mut shown: BTreeSet<WallId> = self.selected_walls();
        let lit = self
            .hovered
            .iter()
            .chain(self.selection.iter())
            .chain(self.highlight.iter());
        for id in lit {
            if let Some(w) = owners.wall(*id) {
                shown.insert(w);
            }
        }
        owners
            .of
            .iter()
            .filter(|(_, o)| matches!(o, Owner::Centre(w) if !shown.contains(w)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Centreline curves and points of walls that aren't picked (none while centrelines are
    /// shown): their relations' glyphs stay hidden, so they don't sit in a poché taking
    /// clicks meant for the wall.
    pub fn unpicked_centrelines(&self) -> BTreeSet<EntityId> {
        if !self.doc.has_walls() || self.show_centrelines {
            return BTreeSet::new();
        }
        let picked = self.selected_walls();
        self.wall_owners()
            .of
            .iter()
            .filter(|(_, o)| matches!(o, Owner::Centre(w) if !picked.contains(w)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// The walls lit up: picked, and the one under the pointer.
    pub(crate) fn lit_walls(&self) -> (BTreeSet<WallId>, Option<WallId>) {
        if !self.doc.has_walls() {
            return (BTreeSet::new(), None);
        }
        let hovered = self.hovered.and_then(|h| self.wall_of(h));
        (self.selected_walls(), hovered)
    }

    /// The owner guard: what a modify tool may do to `ids`. Cuts and rounds of a wall's
    /// faces are refused (its centreline is what to edit), as is making walls of a wall or
    /// making its parts construction. Returns false (with a notice) when refused.
    pub fn guard_owned(&mut self, ids: &[EntityId], op: Op) -> bool {
        if !self.doc.has_walls() {
            return true;
        }
        let owners = self.wall_owners();
        let generated = ids.iter().any(|id| owners.generated(*id));
        let centre = ids
            .iter()
            .any(|id| matches!(owners.get(*id), Some(Owner::Centre(_))));
        let refused = match op {
            Op::Cut if generated => {
                Some("Wall faces follow the wall: trim its centreline, or Explode it".to_owned())
            }
            Op::Round if generated => Some(
                "Wall faces follow the wall: round its centreline's corner, or Explode it"
                    .to_owned(),
            ),
            Op::Construction if generated || centre => {
                Some("A wall's lines follow the wall: Explode it to change them".to_owned())
            }
            Op::MakeWalls if generated || centre => Some("That is already a wall".to_owned()),
            _ => None,
        };
        match refused {
            Some(n) => {
                self.notice = Some(n);
                false
            }
            None => true,
        }
    }

    /// [`guard_owned`](Self::guard_owned) as an edit's error.
    pub(crate) fn refuse_owned(&mut self, ids: &[EntityId], op: Op) -> Result<(), EditError> {
        if self.guard_owned(ids, op) {
            Ok(())
        } else {
            Err(EditError::Invalid(self.notice.take().unwrap_or_default()))
        }
    }

    // ----- Keeping walls generated --------------------------------------------------------

    /// What the open transaction did to what walls are made from: nothing; only moved
    /// centreline points or curves (and curves ends stop against), by segment; or more (a
    /// wall or its centreline changed otherwise), when every wall is made again.
    fn wall_edit(&self) -> WallEdit {
        let (walls, paths) = self.doc.walls_and_paths_touched_in_transaction();
        // A wall's record changed (not just what it made): added, removed, re-thickened…
        for w in walls {
            let strip = |w: Option<Wall>| {
                w.map(|w| Wall {
                    made: Default::default(),
                    ..w
                })
            };
            if strip(self.doc.wall_before_transaction(w)) != strip(self.doc.wall(w).cloned()) {
                return WallEdit::Full;
            }
        }
        // Each centreline curve and point, the segments it is part of; each curve an end
        // stops against (and its points), that end's segment.
        let mut segments: HashMap<EntityId, Vec<(WallId, u16)>> = HashMap::new();
        let mut centres: HashSet<drift_core::PathId> = HashSet::new();
        for (id, w) in self.doc.walls() {
            centres.insert(w.centre);
            let Some(path) = self.doc.path(w.centre) else {
                continue;
            };
            for (k, c) in path.curves.iter().enumerate() {
                let seg = (id, k as u16);
                segments.entry(*c).or_default().push(seg);
                if let Some(e) = self.doc.get(*c) {
                    for p in e.kind.points() {
                        segments.entry(p).or_default().push(seg);
                    }
                }
            }
            let last = path.curves.len().saturating_sub(1) as u16;
            for (which, end) in w.ends.iter().enumerate() {
                if let Some(c) = end.against {
                    let seg = (id, if which == 0 { 0 } else { last });
                    segments.entry(c).or_default().push(seg);
                    if let Some(e) = self.doc.get(c) {
                        for p in e.kind.points() {
                            segments.entry(p).or_default().push(seg);
                        }
                    }
                }
            }
        }
        if paths.iter().any(|p| centres.contains(p)) {
            return WallEdit::Full;
        }
        let mut moved: BTreeSet<(WallId, u16)> = BTreeSet::new();
        for id in self.doc.entities_touched_in_transaction() {
            let Some(segs) = segments.get(&id) else {
                continue;
            };
            // A centreline curve split, trimmed, made or deleted: more than a move.
            let curve = self.doc.get(id).is_some_and(|e| !e.kind.is_point());
            if curve
                && (self.doc.reshaped_in_transaction(id)
                    || self.doc.entity_before_transaction(id).is_none())
            {
                return WallEdit::Full;
            }
            moved.extend(segs.iter().copied());
        }
        if moved.is_empty() {
            WallEdit::None
        } else {
            WallEdit::Moved(moved)
        }
    }

    /// Regenerate the walls an edit touched, inside its open transaction (so undo takes the
    /// regeneration back with the edit): centrelines follow splits, trims and fillets, runs
    /// cut in two become two walls, tee ends follow their hosts, and faces, corners, outlines
    /// and pochés are updated by key. Where only centreline points moved and the joins stay
    /// what they were, only the nodes round the moved segments are worked out again.
    pub(crate) fn follow_walls(&mut self) {
        if !self.doc.has_walls() || !self.doc.in_transaction() {
            return;
        }
        let report = match self.wall_edit() {
            WallEdit::None => return,
            WallEdit::Moved(moved) => {
                // Tees and ends against curves follow what moved first.
                let walls: BTreeSet<WallId> = moved.iter().map(|(w, _)| *w).collect();
                let before = self.doc.revision();
                self.follow_tees(Some(&walls));
                let edit = if self.doc.revision() == before {
                    WallEdit::Moved(moved)
                } else {
                    self.wall_edit()
                };
                let fast = match edit {
                    WallEdit::Moved(moved) => {
                        drift_geom::walls::regenerate_moved(&mut self.doc, &moved)
                    }
                    WallEdit::None => Some(Default::default()),
                    WallEdit::Full => None,
                };
                match fast {
                    Some(r) => r,
                    None => {
                        self.normalize_walls();
                        drift_geom::walls::regenerate(&mut self.doc)
                    }
                }
            }
            WallEdit::Full => {
                self.normalize_walls();
                drift_geom::walls::regenerate(&mut self.doc)
            }
        };
        if self.notice.is_none()
            && let Some(n) = report.notices.first()
        {
            self.notice = Some(n.clone());
        }
    }

    /// Every wall regenerated, inside the open transaction (opening a file: what an older
    /// build edited is made again). The number of generated entities that had to change.
    pub(crate) fn repair_walls(&mut self) -> usize {
        if !self.doc.has_walls() {
            return 0;
        }
        self.normalize_walls();
        drift_geom::walls::regenerate(&mut self.doc).rebuilt
    }

    /// Centrelines as the edit left them: a centreline curve split, trimmed or filleted takes
    /// the new curves into its path; a run cut in two becomes two walls; centrelines stay
    /// construction; tee and against ends follow what they meet.
    fn normalize_walls(&mut self) {
        let created: Vec<EntityId> = self
            .doc
            .created_in_transaction()
            .into_iter()
            .filter(|c| {
                matches!(
                    self.doc.get(*c).map(|e| e.kind),
                    Some(EntityKind::Line { .. } | EntityKind::Arc { .. })
                ) && self.doc.path_of(*c).is_none()
            })
            .collect();
        let walls: Vec<(WallId, Wall)> = self.doc.walls().map(|(id, w)| (id, w.clone())).collect();
        for (id, wall) in walls {
            let Some(path) = self.doc.path(wall.centre).cloned() else {
                continue;
            };
            let own: Vec<EntityId> = path
                .curves
                .iter()
                .copied()
                .filter(|c| self.doc.contains(*c))
                .collect();
            let reshaped = own.iter().any(|c| self.doc.reshaped_in_transaction(*c));
            let mut pool: Vec<EntityId> = own.clone();
            if reshaped {
                // New curves meeting this centreline's points (a split's other half, a
                // fillet's arc).
                let pts: HashSet<EntityId> = self
                    .doc
                    .defining_points(own.iter().copied())
                    .into_iter()
                    .collect();
                for c in &created {
                    if self
                        .doc
                        .get(*c)
                        .and_then(|e| e.kind.ends())
                        .is_some_and(|e| e.iter().any(|p| pts.contains(p)))
                    {
                        pool.push(*c);
                    }
                }
            }
            if !reshaped && own.len() == path.curves.len() {
                self.keep_construction(&own);
                continue;
            }
            let chains = chains(&self.doc, &pool);
            let Some(first) = chains.first().cloned() else {
                continue;
            };
            let (curves, closed) = first;
            let new_path = Path { curves, closed };
            if self.doc.path(wall.centre) != Some(&new_path) {
                let _ = self.doc.set_path(wall.centre, new_path);
            }
            self.keep_construction(&pool);
            // The other pieces of a run cut in two are walls of their own, like it.
            for (curves, closed) in chains.into_iter().skip(1) {
                if let Ok(p) = self.doc.add_path(Path { curves, closed }) {
                    let mut copy = wall.clone();
                    copy.centre = p;
                    copy.made = Default::default();
                    copy.ends = Default::default();
                    let _ = self.doc.add_wall(copy);
                }
            }
            let _ = id;
        }
        self.follow_tees(None);
    }

    fn keep_construction(&mut self, curves: &[EntityId]) {
        for c in curves {
            if self
                .doc
                .get(*c)
                .is_some_and(|e| e.style != LineKind::Construction)
            {
                self.doc.set_style(*c, LineKind::Construction);
            }
        }
    }

    /// Tee ends sit on their host's centreline, and ends against a curve on that curve: an
    /// end the host moved away from goes back onto it; one pulled clear of it lets go.
    /// (Only ends of `walls`, and ends on them, when given.)
    fn follow_tees(&mut self, only: Option<&BTreeSet<WallId>>) {
        let snap = drift_geom::walls::regen::snap(&self.doc);
        let walls: Vec<(WallId, Wall)> = self
            .doc
            .walls()
            .filter(|(id, w)| {
                only.is_none_or(|o| {
                    o.contains(id)
                        || w.ends.iter().any(|e| {
                            e.tee.is_some_and(|(h, _)| o.contains(&h)) || e.against.is_some()
                        })
                })
            })
            .filter(|(_, w)| {
                w.ends
                    .iter()
                    .any(|e| e.tee.is_some() || e.against.is_some())
            })
            .map(|(id, w)| (id, w.clone()))
            .collect();
        for (id, wall) in walls {
            let mut changed = wall.clone();
            for which in 0..2 {
                let Some(point) = self.run_end_point(&wall, which) else {
                    continue;
                };
                let Ok(at) = self.doc.point(point) else {
                    continue;
                };
                // A point held by relations goes where they say (constrained tees are W2's).
                let held = self.doc.constraints_on(point).next().is_some();
                // An end that wasn't moved itself follows what it meets however far that
                // went; one pulled away lets go beyond its reach.
                let pulled = self.doc.touched_in_transaction(point);
                if let Some((host, _)) = wall.ends[which].tee {
                    let target = self.doc.wall(host).and_then(|h| {
                        let (segs, ..) = drift_geom::walls::regen::centreline(&self.doc, h).ok()?;
                        // (The host may have more or fewer segments now: its nearest.)
                        let (k, s) = segs.iter().enumerate().min_by(|a, b| {
                            let d =
                                |s: &drift_geom::walls::Seg| drift_geom::distance(&s.shape(), at);
                            d(a.1).total_cmp(&d(b.1)).then(a.0.cmp(&b.0))
                        })?;
                        let u = s.param(at).clamp(0.0, s.length());
                        Some((k, s.at(u), h.thickness / 2.0 + snap))
                    });
                    match target {
                        Some((k, on, reach)) if !pulled || on.distance(at) <= reach => {
                            changed.ends[which].tee = Some((host, k as u16));
                            if on != at && !held {
                                let _ = self.doc.set_point(point, on);
                            }
                        }
                        _ => changed.ends[which].tee = None,
                    }
                }
                if let Some(c) = wall.ends[which].against {
                    match self.doc.shape(c) {
                        Some(s) => {
                            let on = drift_geom::nearest_point(&s, at);
                            if pulled && on.distance(at) > wall.thickness + snap {
                                changed.ends[which].against = None;
                            } else if on != at && !held {
                                let _ = self.doc.set_point(point, on);
                            }
                        }
                        None => changed.ends[which].against = None,
                    }
                }
            }
            if changed != wall {
                let _ = self.doc.set_wall(id, changed);
            }
        }
    }

    /// The centreline point at a run's start (0) or end (1); None for a closed run.
    fn run_end_point(&self, wall: &Wall, which: usize) -> Option<EntityId> {
        let (_, points, closed) = drift_geom::walls::regen::centreline(&self.doc, wall).ok()?;
        if closed {
            return None;
        }
        if which == 0 {
            points.first().copied()
        } else {
            points.last().copied()
        }
    }

    // ----- Commands -------------------------------------------------------------------------

    /// Change walls' records in one undo step.
    pub fn edit_walls(
        &mut self,
        walls: &[WallId],
        label: &str,
        mut f: impl FnMut(&mut Wall),
    ) -> bool {
        self.doc.begin();
        let mut any = false;
        for w in walls {
            if let Some(mut wall) = self.doc.wall(*w).cloned() {
                f(&mut wall);
                if self.doc.wall(*w) != Some(&wall) && self.doc.set_wall(*w, wall).is_ok() {
                    any = true;
                }
            }
        }
        if !any {
            self.doc.rollback();
            self.sync();
            return false;
        }
        self.commit(label)
    }

    /// Set walls' thickness from typed text (a length, or an expression of variables, which
    /// is kept and re-evaluated when they change).
    pub fn set_wall_thickness(&mut self, walls: &[WallId], text: &str) -> Result<(), EditError> {
        let e = self.evaluate_text(text, Kind::Length)?;
        if !(e.value > 0.0 && e.value.is_finite()) {
            return Err(EditError::Invalid(
                "A wall's thickness must be more than 0".into(),
            ));
        }
        let expr = (!e.references.is_empty()).then(|| e.text.clone());
        self.edit_walls(walls, "Wall thickness", |w| {
            w.thickness = e.value;
            w.thickness_expr = expr.clone();
        });
        Ok(())
    }

    pub fn set_wall_justify(&mut self, walls: &[WallId], j: Justify) -> bool {
        self.edit_walls(walls, "Wall justification", |w| w.justify = j)
    }

    pub fn set_wall_poche(&mut self, walls: &[WallId], poche: Option<FillStyle>) -> bool {
        self.edit_walls(walls, "Wall poché", |w| w.poche = poche.clone())
    }

    /// Re-evaluate walls' thickness expressions (inside a transaction, before a solve).
    pub(crate) fn apply_wall_thickness(&mut self) -> Result<(), EditError> {
        let walls: Vec<(WallId, Wall)> = self
            .doc
            .walls()
            .filter(|(_, w)| w.thickness_expr.is_some())
            .map(|(id, w)| (id, w.clone()))
            .collect();
        if walls.is_empty() {
            return Ok(());
        }
        let eval = drift_core::params::evaluate(&self.doc);
        for (id, w) in walls {
            let expr = w.thickness_expr.clone().unwrap_or_default();
            let v = drift_core::params::evaluate_text(&self.doc, &eval, &expr, Kind::Length)
                .map_err(|e| EditError::Invalid(format!("Wall {id}'s thickness: {}", e.0)))?
                .value;
            if v > 0.0 && v.is_finite() && v != w.thickness {
                let _ = self.doc.set_wall(id, Wall { thickness: v, ..w });
            }
        }
        Ok(())
    }

    /// The open ends of a wall: (which, its point, where it is, the way it points out of
    /// the wall if its last segment is straight, that segment's length).
    fn open_ends(&self, w: WallId) -> Vec<(usize, EntityId, DVec2, Option<DVec2>, f64)> {
        let Some(wall) = self.doc.wall(w) else {
            return Vec::new();
        };
        let Ok((segs, points, closed)) = drift_geom::walls::regen::centreline(&self.doc, wall)
        else {
            return Vec::new();
        };
        if closed || segs.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for which in 0..2 {
            let (s, pt) = if which == 0 {
                (segs[0], points[0])
            } else {
                (segs[segs.len() - 1], points[points.len() - 1])
            };
            let (at, out_dir) = match s {
                drift_geom::walls::Seg::Line { a, b } => {
                    if which == 0 {
                        (a, (a - b).try_normalize())
                    } else {
                        (b, (b - a).try_normalize())
                    }
                }
                drift_geom::walls::Seg::Arc { .. } => {
                    (if which == 0 { s.start() } else { s.end() }, None)
                }
            };
            out.push((which, pt, at, out_dir, s.length()));
        }
        out
    }

    /// Join two picked walls where they come nearest: their nearest ends meet where their
    /// centrelines cross (an L, or straight on where they run in line), or one end runs on to
    /// (or back to) the other wall's centreline (a T), whichever moves the ends least. Ends
    /// set not to join join again. One undo step; false, with a notice, when they can't.
    pub fn join_walls(&mut self, walls: &[WallId]) -> bool {
        let &[a, b] = walls else {
            self.notice = Some("Pick two walls to join".into());
            return false;
        };
        enum Plan {
            /// Both ends to this point, made one.
            L(WallId, usize, EntityId, WallId, usize, EntityId, DVec2),
            /// This end onto that wall's segment, at this point.
            T(WallId, usize, EntityId, WallId, u16, DVec2),
        }
        let snap = drift_geom::walls::regen::snap(&self.doc);
        let mut best: Option<(f64, Plan)> = None;
        let mut offer = |cost: f64, plan: Plan| {
            if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                best = Some((cost, plan));
            }
        };
        let (ea, eb) = (self.open_ends(a), self.open_ends(b));
        // L: two straight ends to where their lines cross.
        for &(wa, pa, at_a, da, la) in &ea {
            for &(wb, pb, at_b, db, lb) in &eb {
                let (Some(da), Some(db)) = (da, db) else {
                    continue;
                };
                let den = da.perp_dot(db);
                let x = if den.abs() <= 1e-9 {
                    // In line and facing each other: meet half way.
                    let off = (at_b - at_a).perp_dot(da).abs();
                    if off > snap.max(1e-6) || da.dot(db) >= 0.0 || (at_b - at_a).dot(da) < 0.0 {
                        continue;
                    }
                    (at_a + at_b) / 2.0
                } else {
                    at_a + da * ((at_b - at_a).perp_dot(db) / den)
                };
                // How far each end moves out (negative: back), never past its segment.
                let (ta, tb) = ((x - at_a).dot(da), (x - at_b).dot(db));
                if ta <= -la + snap || tb <= -lb + snap {
                    continue;
                }
                offer(ta.abs() + tb.abs(), Plan::L(a, wa, pa, b, wb, pb, x));
            }
        }
        // T: one wall's straight end on to the other's centreline.
        for (me, ends, other) in [(a, &ea, b), (b, &eb, a)] {
            let Some(host) = self.doc.wall(other).cloned() else {
                continue;
            };
            let Ok((segs, points, closed)) = drift_geom::walls::regen::centreline(&self.doc, &host)
            else {
                continue;
            };
            let me_thick = self.doc.wall(me).map_or(0.0, |w| w.thickness);
            for &(which, pt, at, d, len) in ends {
                let Some(d) = d else {
                    continue;
                };
                let reach = 1e3 * (1.0 + len + at.abs().max_element());
                let ray = Shape::Line {
                    a: at - d * len,
                    b: at + d * reach,
                };
                for (k, s) in segs.iter().enumerate() {
                    for hit in drift_geom::intersect::intersect(&ray, &s.shape()) {
                        let u = s.param(hit.at);
                        // Near one of its free ends that is an L, not a T.
                        let free = |i: usize| !closed && (i == 0 || i + 1 == points.len());
                        let corner = me_thick / 2.0 + host.thickness + snap;
                        if u <= snap
                            || u >= s.length() - snap
                            || (free(k) && u <= corner)
                            || (free(k + 1) && s.length() - u <= corner)
                        {
                            continue;
                        }
                        let t = (hit.at - at).dot(d);
                        if t <= -len + snap {
                            continue;
                        }
                        offer(t.abs(), Plan::T(me, which, pt, other, k as u16, hit.at));
                    }
                }
            }
        }
        let Some((_, plan)) = best else {
            self.notice = Some(
                "Those walls' straight ends don't point at each other: drag an end onto the \
                 other wall instead"
                    .into(),
            );
            return false;
        };
        self.doc.begin();
        let join_end = |doc: &mut Document, w: WallId, which: usize, tee| {
            if let Some(mut wall) = doc.wall(w).cloned() {
                wall.ends[which].join = true;
                wall.ends[which].tee = tee;
                let _ = doc.set_wall(w, wall);
            }
        };
        match plan {
            Plan::L(wa, ia, pa, wb, ib, pb, x) => {
                let _ = self.doc.set_point(pa, x);
                let _ = self.doc.set_point(pb, x);
                if pa != pb && self.doc.merge_points(pb, pa).is_err() {
                    self.doc.rollback();
                    self.notice = Some("Those walls can't be joined there".into());
                    self.sync();
                    return false;
                }
                join_end(&mut self.doc, wa, ia, None);
                join_end(&mut self.doc, wb, ib, None);
            }
            Plan::T(w, which, pt, host, seg, x) => {
                let _ = self.doc.set_point(pt, x);
                join_end(&mut self.doc, w, which, Some((host, seg)));
            }
        }
        self.commit("Join walls")
    }

    /// Stop walls joining: each end of a picked wall that joins another wall (one of the
    /// picked, when more than one is picked) is set not to join, and is capped. One undo
    /// step; false, with a notice, when no end of theirs joins one.
    pub fn unjoin_walls(&mut self, walls: &[WallId]) -> bool {
        if walls.is_empty() {
            return false;
        }
        let (inputs, _) = drift_geom::walls::regen::wall_inputs(&self.doc);
        let g = drift_geom::walls::generate(&inputs, drift_geom::walls::regen::snap(&self.doc));
        let picked: BTreeSet<WallId> = walls.iter().copied().collect();
        let ends: Vec<(WallId, usize)> = g
            .joined
            .iter()
            .filter(|((w, _), met)| {
                picked.contains(w)
                    && (picked.len() == 1 || met.iter().any(|m| picked.contains(m) && m != w))
            })
            .map(|((w, e), _)| (*w, usize::from(*e)))
            .collect();
        if ends.is_empty() {
            self.notice = Some(if picked.len() == 1 {
                "That wall's ends don't join anything".into()
            } else {
                "Those walls' ends don't join each other".into()
            });
            return false;
        }
        self.doc.begin();
        for (w, which) in ends {
            if let Some(mut wall) = self.doc.wall(w).cloned() {
                wall.ends[which].join = false;
                wall.ends[which].tee = None;
                let _ = self.doc.set_wall(w, wall);
            }
        }
        self.commit("Unjoin walls")
    }

    /// Explode walls into plain lines, arcs and shape fills: the records go, what they made
    /// stays as unowned geometry, and their centrelines go.
    pub fn explode_walls(&mut self, walls: &[WallId]) -> bool {
        if walls.is_empty() {
            return false;
        }
        self.doc.begin();
        let mut centrelines = Vec::new();
        for w in walls {
            centrelines.extend(self.wall_curves(*w));
            if let Some(wall) = self.doc.wall(*w).cloned() {
                self.doc.remove_wall(*w);
                // (Its centreline path goes with its curves; its outlines and fills stay.)
                let _ = wall;
            }
        }
        self.doc.delete(centrelines);
        let ok = self.commit("Explode walls");
        self.selection.clear();
        ok
    }

    /// Walls along existing curves (a chain of lines and arcs, a rectangle, a pen path's
    /// lines): they become construction and get walls at the tool bar's settings, one per
    /// chain.
    pub fn walls_from_curves(&mut self, curves: &[EntityId]) -> Result<Vec<WallId>, String> {
        let curves: Vec<EntityId> = curves
            .iter()
            .copied()
            .filter(|c| {
                matches!(
                    self.doc.get(*c).map(|e| e.kind),
                    Some(EntityKind::Line { .. } | EntityKind::Arc { .. })
                )
            })
            .collect();
        if curves.is_empty() {
            return Err("Pick lines or arcs to make walls along".into());
        }
        if !self.guard_owned(&curves, Op::MakeWalls) {
            return Err(self.notice.clone().unwrap_or_default());
        }
        let chains = chains(&self.doc, &curves);
        self.doc.begin();
        let mut made = Vec::new();
        for (cs, closed) in chains {
            let Ok(p) = self.doc.add_path(Path {
                curves: cs.clone(),
                closed,
            }) else {
                continue;
            };
            self.keep_construction(&cs);
            let layer = self.doc.get(cs[0]).map(|e| e.layer).unwrap_or_default();
            let mut w = Wall::new(p, self.wall_thickness_now(), layer);
            w.thickness_expr = self.wall_thickness_expr.clone();
            w.justify = self.wall_justify;
            w.poche = self.wall_poche_now();
            if let Ok(id) = self.doc.add_wall(w) {
                made.push(id);
            }
        }
        if made.is_empty() {
            self.doc.rollback();
            self.sync();
            return Err("No walls could be made along those curves".into());
        }
        self.commit("Walls from curves");
        self.selection = made.iter().flat_map(|w| self.wall_curves(*w)).collect();
        Ok(made)
    }

    /// Split a run into two walls at the node after segment `seg`.
    pub fn split_wall(&mut self, w: WallId, seg: usize) -> bool {
        let Some(wall) = self.doc.wall(w).cloned() else {
            return false;
        };
        let curves = self.wall_curves(w);
        let closed = self.doc.path(wall.centre).is_some_and(|p| p.closed);
        if seg + 1 >= curves.len() && !closed {
            return false;
        }
        self.doc.begin();
        let (a, b): (Vec<EntityId>, Vec<EntityId>) = if closed {
            // A ring opens there.
            let mut c = curves[seg + 1..].to_vec();
            c.extend_from_slice(&curves[..=seg]);
            (c, Vec::new())
        } else {
            (curves[..=seg].to_vec(), curves[seg + 1..].to_vec())
        };
        let _ = self.doc.set_path(
            wall.centre,
            Path {
                curves: a,
                closed: false,
            },
        );
        let mut first = wall.clone();
        if !closed {
            first.ends[1] = Default::default();
        }
        let _ = self.doc.set_wall(w, first);
        if !b.is_empty()
            && let Ok(p) = self.doc.add_path(Path {
                curves: b,
                closed: false,
            })
        {
            let mut second = wall.clone();
            second.centre = p;
            second.made = Default::default();
            second.ends[0] = Default::default();
            let _ = self.doc.add_wall(second);
        }
        self.commit("Split wall")
    }

    /// Show a wall's thickness as a dimension across its first segment (one undo step).
    pub fn show_wall_thickness(&mut self, w: WallId) -> bool {
        let Some(wall) = self.doc.wall(w) else {
            return false;
        };
        if self
            .doc
            .property_dims()
            .any(|(_, d)| d.target == PropertyRef::WallThickness(w))
        {
            return false;
        }
        let out = wall.offsets().0 + wall.thickness;
        self.doc.begin();
        let _ = self.doc.add_property_dim(PropertyDim {
            target: PropertyRef::WallThickness(w),
            seg: 0,
            along: 0.5,
            out,
        });
        self.commit("Show wall thickness")
    }

    /// Copies of walls for copied curves, inside the open transaction: a wall all of whose
    /// centreline curves were copied (`curves` gives each one's copy) gets a copy along them.
    /// Tees on walls that were copied too point at the copies; others let go.
    pub(crate) fn copy_walls_onto(&mut self, curves: &dyn Fn(EntityId) -> Option<EntityId>) {
        let walls: Vec<(WallId, Wall)> = self.doc.walls().map(|(id, w)| (id, w.clone())).collect();
        let mut copies: BTreeMap<WallId, WallId> = BTreeMap::new();
        let mut made: Vec<WallId> = Vec::new();
        for (id, w) in &walls {
            let Some(path) = self.doc.path(w.centre).cloned() else {
                continue;
            };
            let Some(cs) = path
                .curves
                .iter()
                .map(|c| curves(*c))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            // (Copying shapes' paths may have made its path already.)
            let existing = self
                .doc
                .path_of(cs[0])
                .filter(|p| self.doc.path(*p).is_some_and(|x| x.curves == cs));
            let p = match existing {
                Some(p) => p,
                None => match self.doc.add_path(Path {
                    curves: cs,
                    closed: path.closed,
                }) {
                    Ok(p) => p,
                    Err(_) => continue,
                },
            };
            let mut copy = w.clone();
            copy.centre = p;
            copy.made = Default::default();
            for e in &mut copy.ends {
                e.against = e.against.and_then(curves);
            }
            if let Ok(new) = self.doc.add_wall(copy) {
                copies.insert(*id, new);
                made.push(new);
            }
        }
        for new in made {
            let Some(mut w) = self.doc.wall(new).cloned() else {
                continue;
            };
            for e in &mut w.ends {
                e.tee = e.tee.and_then(|(h, s)| Some((*copies.get(&h)?, s)));
            }
            let _ = self.doc.set_wall(new, w);
        }
    }

    /// The poché of the cluster a wall is in.
    pub fn wall_fill(&self, w: WallId) -> Option<drift_core::FillId> {
        let mine: HashSet<EntityId> = self.doc.wall(w)?.made.curves.values().copied().collect();
        let owners = self.wall_owners();
        let mut fills: Vec<drift_core::FillId> = owners.fills.keys().copied().collect();
        fills.sort_unstable();
        fills.into_iter().find(|f| {
            self.doc
                .fill(*f)
                .is_some_and(|fill| fill.boundary.iter().any(|c| mine.contains(c)))
        })
    }

    /// The region of a wall cluster's poché (its outline and holes, known exactly).
    pub(crate) fn wall_fill_region(
        &self,
        fill: drift_core::FillId,
    ) -> Option<drift_geom::regions::Region> {
        let owners = self.wall_owners();
        let lead = *owners.fills.get(&fill)?;
        let wall = self.doc.wall(lead)?;
        let key = wall
            .made
            .fills
            .iter()
            .find(|(_, f)| **f == fill)
            .map(|(k, _)| *k)?;
        let outer = *wall.made.outlines.get(&key)?;
        let holes: Vec<drift_core::PathId> = wall
            .made
            .outlines
            .iter()
            .filter(|(k, _)| k.outer == key.outer && k.hole != 0)
            .map(|(_, p)| *p)
            .collect();
        drift_geom::walls::regen::outline_region(&self.doc, outer, &holes)
    }

    /// The wall under `p` and where on its centreline the point nearest `p` is: (wall,
    /// segment, that point). Anywhere inside a wall is on it, poché or not (and up to half
    /// its thickness past a segment's end: a corner's outside, beyond a cap). Not segment
    /// `but.1` of wall `but.0` (the Wall tool's run, along the segment it is drawing from).
    pub fn wall_under(
        &mut self,
        p: DVec2,
        tol: f64,
        but: Option<(WallId, usize)>,
    ) -> Option<(WallId, u16, DVec2)> {
        if !self.doc.has_walls() {
            return None;
        }
        // The nearest segment of each wall `p` is inside of (within tolerance).
        let mut best: Option<(f64, WallId, u16, DVec2)> = None;
        for (id, wall) in self.doc.walls() {
            let Ok((segs, ..)) = drift_geom::walls::regen::centreline(&self.doc, wall) else {
                continue;
            };
            let (left, right) = wall.justify.offsets(wall.thickness);
            for (k, s) in segs.iter().enumerate() {
                if but == Some((id, k)) {
                    continue;
                }
                let u = s.param(p).clamp(0.0, s.length());
                let on = s.at(u);
                // How far to the left of the centreline `p` is, and whether that is in the wall.
                let side = s.tangent(u).perp().dot(p - on);
                let off = p.distance(on);
                let inside = side <= left + tol && side >= right - tol;
                // (Past a segment's end by up to half the wall: a corner's outside, a cap.)
                let reach = left.abs().max(right.abs()) + tol;
                let along = (p - on).dot(s.tangent(u)).abs() <= reach;
                if !(inside && along) {
                    continue;
                }
                if best.is_none_or(|b| (off, id, k as u16) < (b.0, b.1, b.2)) {
                    best = Some((off, id, k as u16, on));
                }
            }
        }
        best.map(|(_, w, k, on)| (w, k, on))
    }

    /// A curve under `p` that isn't a wall's (a column, a slab edge, a grid line): a wall
    /// can end against it.
    pub fn against_under(&self, p: DVec2, tol: f64) -> Option<EntityId> {
        let owners = self.wall_owners();
        self.index.pick(p, tol, |id| {
            owners.get(id).is_none()
                && self.doc.get(id).is_some_and(|e| {
                    matches!(
                        e.kind,
                        EntityKind::Line { .. }
                            | EntityKind::Arc { .. }
                            | EntityKind::Circle { .. }
                    )
                })
        })
    }

    /// The faces of the Wall tool's next segment as they would be made: joined to the run it
    /// continues (`from`, its last corner, if placed), to `to` (a point it ends on), and to
    /// the walls it starts or ends in, by where it is.
    pub fn wall_preview(
        &self,
        seg: drift_geom::walls::Seg,
        offsets: (f64, f64),
        from: Option<EntityId>,
        to: Option<EntityId>,
    ) -> Vec<Shape> {
        let me = WallId(u32::MAX);
        let snap = drift_geom::walls::regen::snap(&self.doc);
        let (mut inputs, _) = drift_geom::walls::regen::wall_inputs(&self.doc);
        // Its cluster: a wall's of the same poché and layer, if there is one.
        let layer = self.doc.current_layer();
        let poche = self.wall_poche_now();
        let cluster = self
            .doc
            .walls()
            .find(|(_, w)| w.layer == layer && w.poche == poche)
            .and_then(|(id, _)| inputs.iter().find(|i| i.id == id))
            .map_or(u32::MAX, |i| i.cluster);
        // Only the walls near it matter.
        let thick = (offsets.0 - offsets.1).abs();
        let reach = inputs
            .iter()
            .map(|w| (w.offsets.0 - w.offsets.1).abs())
            .fold(thick, f64::max)
            + snap;
        let b = seg.bounds();
        inputs.retain(|w| {
            w.segs.iter().any(|s| {
                let c = s.bounds();
                c.min.x <= b.max.x + reach
                    && c.max.x >= b.min.x - reach
                    && c.min.y <= b.max.y + reach
                    && c.max.y >= b.min.y - reach
            })
        });
        inputs.push(drift_geom::walls::WallIn {
            id: me,
            segs: vec![seg],
            points: vec![
                from.unwrap_or(EntityId(u32::MAX)),
                to.filter(|t| Some(*t) != from)
                    .unwrap_or(EntityId(u32::MAX - 1)),
            ],
            closed: false,
            offsets,
            cluster,
            cross: true,
            ends: Default::default(),
        });
        let g = drift_geom::walls::generate(&inputs, snap);
        g.curves
            .iter()
            .filter(|((w, _), p)| *w == me && p.stroke)
            .filter_map(|(_, p)| g.shape(p))
            .collect()
    }
}

/// A wall's thickness dimension as the dimension renderer draws it: from a corner of its
/// left face to its right face, on segment `seg`, with the label offset from its anchor;
/// None for a curved segment or a wall not made.
pub(crate) fn thickness_dimension(
    doc: &drift_core::Document,
    d: &PropertyDim,
) -> Option<(drift_core::Constraint, DVec2)> {
    let PropertyRef::WallThickness(w) = d.target;
    let wall = doc.wall(w)?;
    let (segs, ..) = drift_geom::walls::regen::centreline(doc, wall).ok()?;
    let seg = segs.get(usize::from(d.seg))?;
    if !matches!(seg, drift_geom::walls::Seg::Line { .. }) {
        return None;
    }
    let piece = |part: drift_core::Part| {
        wall.made
            .curves
            .iter()
            .filter(|(k, _)| k.seg == d.seg && k.part == part)
            .map(|(_, id)| *id)
            .next()
    };
    let (left, right) = (
        piece(drift_core::Part::Left)?,
        piece(drift_core::Part::Right)?,
    );
    let at = seg.at(seg.length() * d.along.clamp(0.0, 1.0));
    // The left face's end nearest the place along the wall.
    let point = match doc.get(left)?.kind {
        EntityKind::Line { a, b } => {
            let (pa, pb) = (doc.point(a).ok()?, doc.point(b).ok()?);
            if pa.distance(at) <= pb.distance(at) {
                a
            } else {
                b
            }
        }
        _ => return None,
    };
    let c = drift_core::Constraint::PointLineDistance {
        point,
        line: right,
        value: wall.thickness,
    };
    let label = at + seg.tangent(0.0).perp() * d.out;
    let anchor = crate::annotations::dimension_anchor(doc, &c)?;
    Some((c, label - anchor))
}

/// `curves` (lines and arcs) chained end to end through the points they share: each chain
/// in order, and whether it closes. The first chain holds the first curve.
pub(crate) fn chains(
    doc: &drift_core::Document,
    curves: &[EntityId],
) -> Vec<(Vec<EntityId>, bool)> {
    let ends = |c: EntityId| doc.get(c).and_then(|e| e.kind.ends());
    let mut left: Vec<EntityId> = curves.to_vec();
    let mut out = Vec::new();
    while let Some(first) = left.first().copied() {
        left.remove(0);
        let Some([start, mut at]) = ends(first) else {
            continue;
        };
        let mut chain = vec![first];
        let mut closed = false;
        // Forward from its end, then back from its start.
        for forward in [true, false] {
            if !forward {
                if closed {
                    break;
                }
                at = start;
            }
            loop {
                if forward && at == start && chain.len() > 1 {
                    closed = true;
                    break;
                }
                let next = left
                    .iter()
                    .position(|c| ends(*c).is_some_and(|e| e.contains(&at)));
                let Some(i) = next else {
                    break;
                };
                let c = left.remove(i);
                let [s, e] = ends(c).expect("has ends");
                if forward {
                    chain.push(c);
                } else {
                    chain.insert(0, c);
                }
                at = if s == at { e } else { s };
            }
        }
        out.push((chain, closed));
    }
    out
}

/// What an edit did to what walls are made from (see `Editor::wall_edit`).
enum WallEdit {
    None,
    /// Only these segments moved.
    Moved(BTreeSet<(WallId, u16)>),
    Full,
}

/// Whether new walls get a poché, and its style (solid in the layer's colour by default).
#[derive(Clone, Debug, PartialEq)]
pub struct PocheSetting {
    pub on: bool,
    pub style: FillStyle,
}

impl Default for PocheSetting {
    fn default() -> Self {
        Self {
            on: true,
            style: FillStyle::solid(),
        }
    }
}
