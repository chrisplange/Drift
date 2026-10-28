//! Adding, reshaping, moving, merging and deleting entities.

use super::*;

impl Document {
    pub(super) fn insert(&mut self, mut entity: Entity) -> EntityId {
        entity.layer = self.current_layer;
        let id = EntityId(self.next_id);
        self.next_id += 1;
        self.touch(id);
        self.entities.insert(id, entity);
        id
    }

    pub fn add_point(&mut self, at: DVec2) -> EntityId {
        self.insert(Entity::new(EntityKind::Point(at)))
    }

    pub fn add_line(&mut self, a: EntityId, b: EntityId) -> Result<EntityId, DocumentError> {
        if a == b || self.point(a)? == self.point(b)? {
            return Err(DocumentError::Degenerate("line"));
        }
        Ok(self.insert(Entity::new(EntityKind::Line { a, b })))
    }

    pub fn add_circle(&mut self, center: EntityId, radius: f64) -> Result<EntityId, DocumentError> {
        self.point(center)?;
        if !(radius > 0.0 && radius.is_finite()) {
            return Err(DocumentError::Degenerate("circle"));
        }
        Ok(self.insert(Entity::new(EntityKind::Circle { center, radius })))
    }

    /// Add a cubic Bézier from `a` to `b` with handles `c1` and `c2`.
    pub fn add_bezier(
        &mut self,
        a: EntityId,
        c1: EntityId,
        c2: EntityId,
        b: EntityId,
    ) -> Result<EntityId, DocumentError> {
        self.check_bezier(a, c1, c2, b)?;
        Ok(self.insert(Entity::new(EntityKind::Bezier { a, c1, c2, b })))
    }

    pub(super) fn check_bezier(
        &self,
        a: EntityId,
        c1: EntityId,
        c2: EntityId,
        b: EntityId,
    ) -> Result<(), DocumentError> {
        let ids = [a, c1, c2, b];
        for (i, x) in ids.iter().enumerate() {
            self.point(*x)?;
            if ids[i + 1..].contains(x) {
                return Err(DocumentError::Degenerate("curve"));
            }
        }
        if self.point(a)? == self.point(b)? && self.point(c1)? == self.point(c2)? {
            return Err(DocumentError::Degenerate("curve"));
        }
        Ok(())
    }

    /// Add an ellipse from its centre and the ends of its two semi-axes (`minor` is moved onto
    /// the perpendicular through the centre, keeping its distance).
    pub fn add_ellipse(
        &mut self,
        center: EntityId,
        major: EntityId,
        minor: EntityId,
    ) -> Result<EntityId, DocumentError> {
        self.check_ellipse(center, major, minor)?;
        let id = self.insert(Entity::new(EntityKind::Ellipse {
            center,
            major,
            minor,
        }));
        self.normalize_arc(id);
        Ok(id)
    }

    /// Add a counter-clockwise part of an ellipse; `start` and `end` are moved onto it.
    pub fn add_ellipse_arc(
        &mut self,
        center: EntityId,
        major: EntityId,
        minor: EntityId,
        start: EntityId,
        end: EntityId,
    ) -> Result<EntityId, DocumentError> {
        self.check_ellipse(center, major, minor)?;
        let (s, e) = (self.point(start)?, self.point(end)?);
        if start == end || s == e {
            return Err(DocumentError::Degenerate("elliptical arc"));
        }
        let id = self.insert(Entity::new(EntityKind::EllipseArc {
            center,
            major,
            minor,
            start,
            end,
        }));
        self.normalize_arc(id);
        Ok(id)
    }

    pub(super) fn check_ellipse(
        &self,
        center: EntityId,
        major: EntityId,
        minor: EntityId,
    ) -> Result<(), DocumentError> {
        let (c, m, n) = (self.point(center)?, self.point(major)?, self.point(minor)?);
        if center == major || center == minor || major == minor || c == m || c == n {
            return Err(DocumentError::Degenerate("ellipse"));
        }
        Ok(())
    }

    /// Add a counter-clockwise arc. `end` is projected onto the circle through `start`.
    pub fn add_arc(
        &mut self,
        center: EntityId,
        start: EntityId,
        end: EntityId,
    ) -> Result<EntityId, DocumentError> {
        let (c, s, e) = (self.point(center)?, self.point(start)?, self.point(end)?);
        if start == end || center == start || center == end || s == c || e == c || s == e {
            return Err(DocumentError::Degenerate("arc"));
        }
        let id = self.insert(Entity::new(EntityKind::Arc { center, start, end }));
        self.normalize_arc(id);
        Ok(id)
    }

    pub fn set_style(&mut self, id: EntityId, style: LineKind) {
        if self.entities.get(&id).is_some_and(|e| e.style != style) {
            self.touch(id);
            if let Some(e) = self.entities.get_mut(&id) {
                e.style = style;
            }
        }
    }

    pub fn set_point(&mut self, id: EntityId, at: DVec2) -> Result<(), DocumentError> {
        self.point(id)?;
        // A NaN saved is written as `null`, and the file then won't open.
        if !at.is_finite() {
            return Err(DocumentError::InvalidValue);
        }
        self.touch_moved(id);
        let entity = Entity {
            kind: EntityKind::Point(at),
            ..self.entities[&id]
        };
        self.entities.insert(id, entity);
        Ok(())
    }

    /// Set a circle's radius.
    pub fn set_radius(&mut self, id: EntityId, radius: f64) -> Result<(), DocumentError> {
        match self.entities.get(&id).map(|e| e.kind) {
            Some(EntityKind::Circle { center, .. }) => {
                if !(radius > 0.0 && radius.is_finite()) {
                    return Err(DocumentError::Degenerate("circle"));
                }
                self.touch_moved(id);
                if let Some(e) = self.entities.get_mut(&id) {
                    e.kind = EntityKind::Circle { center, radius };
                }
                Ok(())
            }
            Some(_) => Err(DocumentError::WrongKind {
                id,
                expected: "a circle",
            }),
            None => Err(DocumentError::Missing(id)),
        }
    }

    /// Change what an entity is (e.g. move a line's end to another point, or turn a circle
    /// into an arc), keeping its id so relations on it stay. Validated like adding it.
    pub fn set_entity_kind(&mut self, id: EntityId, kind: EntityKind) -> Result<(), DocumentError> {
        let current = self.entities.get(&id).ok_or(DocumentError::Missing(id))?;
        if current.kind.is_point() != kind.is_point() {
            return Err(DocumentError::WrongKind {
                id,
                expected: if kind.is_point() {
                    "a point"
                } else {
                    "a curve"
                },
            });
        }
        match kind {
            EntityKind::Point(p) if !p.is_finite() => return Err(DocumentError::InvalidValue),
            EntityKind::Line { a, b } => {
                if a == b || self.point(a)? == self.point(b)? {
                    return Err(DocumentError::Degenerate("line"));
                }
            }
            EntityKind::Circle { center, radius } => {
                self.point(center)?;
                if !(radius > 0.0 && radius.is_finite()) {
                    return Err(DocumentError::Degenerate("circle"));
                }
            }
            EntityKind::Arc { center, start, end } => {
                let (c, s, e) = (self.point(center)?, self.point(start)?, self.point(end)?);
                if start == end || center == start || center == end || s == c || e == c || s == e {
                    return Err(DocumentError::Degenerate("arc"));
                }
            }
            EntityKind::Ellipse {
                center,
                major,
                minor,
            } => self.check_ellipse(center, major, minor)?,
            EntityKind::Bezier { a, c1, c2, b } => self.check_bezier(a, c1, c2, b)?,
            EntityKind::EllipseArc {
                center,
                major,
                minor,
                start,
                end,
            } => {
                self.check_ellipse(center, major, minor)?;
                if start == end || self.point(start)? == self.point(end)? {
                    return Err(DocumentError::Degenerate("elliptical arc"));
                }
            }
            EntityKind::Point(_) => {}
        }
        if self.entities[&id].kind != kind {
            if same_points(&self.entities[&id].kind, &kind) {
                self.touch_moved(id);
            } else {
                self.touch(id);
            }
            if let Some(e) = self.entities.get_mut(&id) {
                e.kind = kind;
            }
            if matches!(kind, EntityKind::Arc { .. }) {
                self.normalize_arc(id);
            }
            // Relations the change made meaningless go with it: a point held on a curve
            // that now ends at it, a curve of a kind a relation can't take.
            let stale: Vec<ConstraintId> = self
                .constraints
                .iter()
                .filter(|(_, c)| c.entities().any(|e| e == id) && self.validate(c).is_err())
                .map(|(cid, _)| *cid)
                .collect();
            for cid in stale {
                self.remove_constraint(cid);
            }
        }
        Ok(())
    }

    /// [`set_entity_kind`](Self::set_entity_kind) without moving an arc's end onto the
    /// circle through its start: for geometry generated exactly (a wall's faces), whose
    /// corners other curves share.
    pub fn set_entity_kind_exact(
        &mut self,
        id: EntityId,
        kind: EntityKind,
    ) -> Result<(), DocumentError> {
        if !matches!(kind, EntityKind::Arc { .. }) {
            return self.set_entity_kind(id, kind);
        }
        let EntityKind::Arc { center, start, end } = kind else {
            unreachable!()
        };
        let current = self.entities.get(&id).ok_or(DocumentError::Missing(id))?;
        if current.kind.is_point() {
            return Err(DocumentError::WrongKind {
                id,
                expected: "a curve",
            });
        }
        let (c, s, e) = (self.point(center)?, self.point(start)?, self.point(end)?);
        if start == end || center == start || center == end || s == c || e == c || s == e {
            return Err(DocumentError::Degenerate("arc"));
        }
        if self.entities[&id].kind != kind {
            if same_points(&self.entities[&id].kind, &kind) {
                self.touch_moved(id);
            } else {
                self.touch(id);
            }
            if let Some(e) = self.entities.get_mut(&id) {
                e.kind = kind;
            }
            let stale: Vec<ConstraintId> = self
                .constraints
                .iter()
                .filter(|(_, c)| c.entities().any(|e| e == id) && self.validate(c).is_err())
                .map(|(cid, _)| *cid)
                .collect();
            for cid in stale {
                self.remove_constraint(cid);
            }
        }
        Ok(())
    }

    /// [`add_arc`](Self::add_arc) without moving its end onto the circle through its start
    /// (see [`set_entity_kind_exact`](Self::set_entity_kind_exact)).
    pub fn add_arc_exact(
        &mut self,
        center: EntityId,
        start: EntityId,
        end: EntityId,
    ) -> Result<EntityId, DocumentError> {
        let (c, s, e) = (self.point(center)?, self.point(start)?, self.point(end)?);
        if start == end || center == start || center == end || s == c || e == c || s == e {
            return Err(DocumentError::Degenerate("arc"));
        }
        Ok(self.insert(Entity::new(EntityKind::Arc { center, start, end })))
    }

    /// Remove points no curve uses and no constraint refers to beyond themselves (cleanup
    /// after trimming moves a curve's end to a new point).
    pub fn remove_if_orphaned(&mut self, point: EntityId) {
        if self.point(point).is_ok() && self.users_of(point).next().is_none() {
            self.delete([point]);
        }
    }

    /// Every point that defines the given entities (points themselves included).
    pub fn defining_points(&self, ids: impl IntoIterator<Item = EntityId>) -> BTreeSet<EntityId> {
        let mut points = BTreeSet::new();
        for id in ids {
            if let Some(e) = self.entities.get(&id) {
                if e.kind.is_point() {
                    points.insert(id);
                }
                points.extend(e.kind.points());
            }
        }
        points
    }

    /// Translate the given entities. Shared points move once; connected curves stretch, and
    /// arcs whose points moved are kept circular.
    pub fn translate(&mut self, ids: impl IntoIterator<Item = EntityId>, delta: DVec2) {
        if delta == DVec2::ZERO {
            return;
        }
        let points = self.defining_points(ids);
        for &id in &points {
            if let Ok(p) = self.point(id) {
                let _ = self.set_point(id, p + delta);
            }
        }
        self.normalize_arcs_using(&points);
    }

    /// Re-project the end point of every arc that uses one of `points` onto its circle.
    pub fn normalize_arcs_using(&mut self, points: &BTreeSet<EntityId>) {
        let arcs: Vec<_> = self
            .entities
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e.kind,
                    EntityKind::Arc { .. }
                        | EntityKind::Ellipse { .. }
                        | EntityKind::EllipseArc { .. }
                ) && e.kind.points().any(|p| points.contains(&p))
            })
            .map(|(id, _)| *id)
            .collect();
        for arc in arcs {
            self.normalize_arc(arc);
        }
    }

    pub(super) fn normalize_arc(&mut self, arc: EntityId) {
        let kind = self.entities.get(&arc).map(|e| e.kind);
        if let Some(
            EntityKind::Ellipse {
                center,
                major,
                minor,
            }
            | EntityKind::EllipseArc {
                center,
                major,
                minor,
                ..
            },
        ) = kind
        {
            self.normalize_ellipse(center, major, minor);
            if let Some(EntityKind::EllipseArc { start, end, .. }) = kind {
                for p in [start, end] {
                    self.project_onto_ellipse(center, major, minor, p);
                }
            }
            return;
        }
        let Some(EntityKind::Arc { center, start, end }) = kind else {
            return;
        };
        let (Ok(c), Ok(s), Ok(e)) = (self.point(center), self.point(start), self.point(end)) else {
            return;
        };
        let radius = (s - c).length();
        let dir = e - c;
        if radius > 0.0 && dir.length() > 0.0 {
            let projected = c + dir.normalize() * radius;
            if projected != e {
                let _ = self.set_point(end, projected);
            }
        }
    }

    /// Keep an ellipse's minor point on the perpendicular through its centre (at the same
    /// distance, on the side it's nearer).
    pub(super) fn normalize_ellipse(&mut self, center: EntityId, major: EntityId, minor: EntityId) {
        let (Ok(c), Ok(m), Ok(n)) = (self.point(center), self.point(major), self.point(minor))
        else {
            return;
        };
        let axis = (m - c).normalize_or_zero().perp();
        let b = n.distance(c);
        if axis == DVec2::ZERO || b == 0.0 {
            return;
        }
        let side = if (n - c).dot(axis) < 0.0 { -1.0 } else { 1.0 };
        let target = c + axis * b * side;
        if target != n {
            let _ = self.set_point(minor, target);
        }
    }

    /// Move a point onto an ellipse, along the line from its centre.
    pub(super) fn project_onto_ellipse(
        &mut self,
        center: EntityId,
        major: EntityId,
        minor: EntityId,
        point: EntityId,
    ) {
        let (Ok(c), Ok(m), Ok(n), Ok(p)) = (
            self.point(center),
            self.point(major),
            self.point(minor),
            self.point(point),
        ) else {
            return;
        };
        let (a, b) = ((m - c).length(), (n - c).length());
        let u = (m - c).normalize_or_zero();
        let d = p - c;
        let q = ((d.dot(u) / a).powi(2) + (d.dot(u.perp()) / b).powi(2)).sqrt();
        if q > 0.0 && q.is_finite() {
            let target = c + d / q;
            if target != p {
                let _ = self.set_point(point, target);
            }
        }
    }

    /// Delete entities. Deleting a point also deletes the curves that use it; points left
    /// unused by deleting a curve are removed too (unless they were explicitly standalone).
    pub fn delete(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        self.remember_leader_targets();
        let mut doomed: BTreeSet<EntityId> =
            ids.into_iter().filter(|id| self.contains(*id)).collect();
        // Curves that use a deleted point go too.
        let dependents: Vec<_> = self
            .entities
            .iter()
            .filter(|(_, e)| e.kind.points().any(|p| doomed.contains(&p)))
            .map(|(id, _)| *id)
            .collect();
        doomed.extend(dependents);

        let orphan_candidates = self.defining_points(doomed.iter().copied());
        for id in &doomed {
            self.touch(*id);
            self.entities.remove(id);
        }
        // (Every point still used, found once: asking each candidate's users is a scan of
        // the whole drawing per point.)
        let used: std::collections::HashSet<EntityId> = if orphan_candidates.is_empty() {
            Default::default()
        } else {
            self.entities
                .values()
                .flat_map(|e| e.kind.points())
                .collect()
        };
        for point in orphan_candidates {
            if self.contains(point) && !used.contains(&point) {
                self.touch(point);
                self.entities.remove(&point);
            }
        }
        // Constraints on anything that no longer exists go too.
        let dangling: Vec<_> = self
            .constraints
            .iter()
            .filter(|(_, c)| c.entities().any(|e| !self.entities.contains_key(&e)))
            .map(|(id, _)| *id)
            .collect();
        for id in dangling {
            self.remove_constraint(id);
        }
        let dangling: Vec<_> = self
            .measurements
            .iter()
            .filter(|(_, c)| c.entities().any(|e| !self.entities.contains_key(&e)))
            .map(|(id, _)| *id)
            .collect();
        for id in dangling {
            self.remove_measurement(id);
        }
        // A fill whose whole boundary goes has nothing left to fill: it goes too (and comes
        // back with its curves on undo). One that keeps some of its curves keeps the deleted
        // ones' ids, so the editor can find the area around its seed now (or drop it when
        // there is none).
        let emptied: Vec<FillId> = self
            .fills
            .iter()
            .filter(|(_, f)| {
                f.boundary
                    .iter()
                    .all(|e| doomed.contains(e) || !self.entities.contains_key(e))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in emptied {
            self.remove_fill(id);
        }
        self.drop_dangling_annotations();
        self.prune_groups();
        self.prune_paths();
        self.prune_walls();
    }

    /// Walls whose centreline is gone go, with what they made (and come back with it on
    /// undo); tees on them let go, and ends against deleted curves cap.
    pub(super) fn prune_walls(&mut self) {
        if self.walls.is_empty() && self.openings.is_empty() {
            return;
        }
        let gone: Vec<WallId> = self
            .walls
            .iter()
            .filter(|(_, w)| !self.paths.contains_key(&w.centre))
            .map(|(id, _)| *id)
            .collect();
        let mut made: Vec<EntityId> = Vec::new();
        // Openings in walls that go, go with them (their symbols too).
        let orphans: Vec<OpeningId> = self
            .openings
            .iter()
            .filter(|(_, o)| gone.contains(&o.wall) || !self.walls.contains_key(&o.wall))
            .map(|(id, _)| *id)
            .collect();
        for o in orphans {
            if let Some(op) = self.openings.get(&o) {
                made.extend(op.made.entities());
            }
            self.remove_opening(o);
        }
        for id in &gone {
            if let Some(w) = self.walls.get(id).cloned() {
                made.extend(w.made.entities());
                // The cluster's outlines and pochés it held go (they span the other walls
                // too, whose faces stay): regenerating makes them again under the next lead.
                for f in w.made.fills.values() {
                    self.remove_fill(*f);
                }
                for p in w.made.outlines.values() {
                    self.remove_path(*p);
                }
            }
            self.remove_wall(*id);
        }
        let stale: Vec<(WallId, Wall)> = self
            .walls
            .iter()
            .filter_map(|(id, w)| {
                let mut w2 = w.clone();
                for end in &mut w2.ends {
                    if end
                        .tee
                        .is_some_and(|(host, _)| !self.walls.contains_key(&host))
                    {
                        end.tee = None;
                    }
                    if end.against.is_some_and(|c| !self.entities.contains_key(&c)) {
                        end.against = None;
                    }
                }
                (w2 != *w).then_some((*id, w2))
            })
            .collect();
        for (id, w) in stale {
            self.touch_key(Key::Wall(id));
            self.walls.insert(id, w);
        }
        let made: Vec<EntityId> = made
            .into_iter()
            .filter(|e| self.entities.contains_key(e))
            .collect();
        if !made.is_empty() {
            self.delete(made);
        }
    }

    /// Drop deleted curves from paths (a path missing one is open), remove paths left with
    /// none, and unbind the fills of those.
    pub(super) fn prune_paths(&mut self) {
        let stale: Vec<PathId> = self
            .paths
            .iter()
            .filter(|(_, p)| p.curves.iter().any(|e| !self.entities.contains_key(e)))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.touch_key(Key::Path(id));
            let entities = &self.entities;
            let empty = match self.paths.get_mut(&id) {
                Some(p) => {
                    p.curves.retain(|e| entities.contains_key(e));
                    p.closed = false;
                    p.curves.is_empty()
                }
                None => false,
            };
            if empty {
                self.paths.remove(&id);
                self.unbind_fills(id);
            }
        }
    }

    /// Fills of a path that is gone fill an area among the sketch's curves again.
    pub(super) fn unbind_fills(&mut self, path: PathId) {
        let bound: Vec<FillId> = self
            .fills
            .iter()
            .filter(|(_, f)| f.path == Some(path))
            .map(|(id, _)| *id)
            .collect();
        for id in bound {
            self.touch_key(Key::Fill(id));
            if let Some(f) = self.fills.get_mut(&id) {
                f.path = None;
            }
        }
    }

    /// Drop deleted entities from groups, and groups left with fewer than one member.
    pub(super) fn prune_groups(&mut self) {
        let stale: Vec<GroupId> = self
            .groups
            .iter()
            .filter(|(_, g)| g.members.iter().any(|e| !self.entities.contains_key(e)))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.touch_key(Key::Group(id));
            let entities = &self.entities;
            if let Some(g) = self.groups.get_mut(&id) {
                g.members.retain(|e| entities.contains_key(e));
                if g.members.is_empty() {
                    self.groups.remove(&id);
                }
            }
        }
    }

    /// A note's text position in the world (`at` is from its first leader's tip).
    pub fn note_text_pos(&self, at: DVec2, leaders: &[Leader]) -> Option<DVec2> {
        match leaders.first() {
            Some(l) => Some(self.anchor_pos(&l.to)? + at),
            None => Some(at),
        }
    }

    /// The `at` that puts a note's text at `pos` (world) with these leaders.
    pub fn note_at(&self, pos: DVec2, leaders: &[Leader]) -> Option<DVec2> {
        match leaders.first() {
            Some(l) => Some(pos - self.anchor_pos(&l.to)?),
            None => Some(pos),
        }
    }

    /// Annotations on deleted geometry go; a note keeps its text where it was, losing only the
    /// leaders whose targets went.
    pub(super) fn drop_dangling_annotations(&mut self) {
        let alive = |doc: &Self, e: EntityId| doc.entities.contains_key(&e);
        let dangling: Vec<(AnnotationId, Annotation)> = self
            .annotations
            .iter()
            .filter(|(_, a)| a.entities().iter().any(|e| !alive(self, *e)))
            .map(|(id, a)| (*id, a.clone()))
            .collect();
        for (id, a) in dangling {
            self.touch_key(Key::Annotation(id));
            match a {
                Annotation::Note {
                    text,
                    at,
                    mut leaders,
                    style,
                } => {
                    // Where the text was, before its first leader's target went.
                    let first = leaders.first().and_then(|l| {
                        self.last_anchor_pos.get(&id).copied().or(match l.to {
                            Anchor::Free(p) => Some(p),
                            _ => None,
                        })
                    });
                    let pos = first.unwrap_or(DVec2::ZERO) + at;
                    leaders.retain(|l| l.to.entity().is_none_or(|e| alive(self, e)));
                    let at = self.note_at(pos, &leaders).unwrap_or(pos);
                    self.annotations.insert(
                        id,
                        Annotation::Note {
                            text,
                            at,
                            leaders,
                            style,
                        },
                    );
                }
                _ => {
                    self.annotations.remove(&id);
                }
            }
        }
    }

    /// Merge point `from` into point `into`: every reference to `from` is redirected and `from`
    /// is removed. Fails (changing nothing) if that would collapse a curve.
    pub fn merge_points(&mut self, from: EntityId, into: EntityId) -> Result<(), DocumentError> {
        self.point(from)?;
        self.point(into)?;
        if from == into {
            return Ok(());
        }
        let users: Vec<_> = self.users_of(from).collect();
        for &user in &users {
            let mut kind = self.entities[&user].kind;
            kind.replace_point(from, into);
            let collapses = match kind {
                EntityKind::Line { a, b } => a == b,
                EntityKind::Arc { center, start, end } => {
                    center == start || center == end || start == end
                }
                _ => false,
            };
            if collapses {
                return Err(DocumentError::Degenerate("merge"));
            }
        }
        for user in users {
            self.touch(user);
            if let Some(e) = self.entities.get_mut(&user) {
                e.kind.replace_point(from, into);
            }
        }
        let constrained: Vec<_> = self
            .constraints
            .iter()
            .filter(|(_, c)| c.entities().any(|e| e == from))
            .map(|(id, c)| (*id, *c))
            .collect();
        for (id, mut c) in constrained {
            self.touch_constraint(id);
            c.replace_point(from, into);
            if c.is_degenerate() || self.self_referencing(&c).is_some() {
                // e.g. a coincidence between the two merged points (now implied), or a point
                // held on a curve whose end it has become.
                self.constraints.remove(&id);
            } else {
                self.constraints.insert(id, c);
            }
        }
        let measured: Vec<_> = self
            .measurements
            .iter()
            .filter(|(_, c)| c.entities().any(|e| e == from))
            .map(|(id, c)| (*id, *c))
            .collect();
        for (id, mut c) in measured {
            self.touch_key(Key::Measurement(id));
            c.replace_point(from, into);
            if c.is_degenerate() || self.self_referencing(&c).is_some() {
                self.measurements.remove(&id);
            } else {
                self.measurements.insert(id, c);
            }
        }
        let grouped: Vec<GroupId> = self
            .groups
            .iter()
            .filter(|(_, g)| g.members.contains(&from))
            .map(|(id, _)| *id)
            .collect();
        for id in grouped {
            self.touch_key(Key::Group(id));
            if let Some(g) = self.groups.get_mut(&id) {
                g.members.remove(&from);
                g.members.insert(into);
            }
        }
        let annotated: Vec<_> = self
            .annotations
            .iter()
            .filter(|(_, a)| a.entities().contains(&from))
            .map(|(id, _)| *id)
            .collect();
        for id in annotated {
            self.touch_key(Key::Annotation(id));
            if let Some(a) = self.annotations.get_mut(&id) {
                a.replace_entity(from, into);
            }
        }
        self.touch(from);
        self.entities.remove(&from);
        self.normalize_arcs_using(&BTreeSet::from([into]));
        Ok(())
    }

    // ----- Constraints -----------------------------------------------------------------------
}
