//! Fills, annotations, dimension notes, and what is shown where.

use super::*;

impl Document {
    // ----- Drafting: fills, annotations, dimension notes, drafting style ---------------------

    pub(super) fn next_drafting(&mut self) -> u32 {
        let id = self.next_drafting_id;
        self.next_drafting_id += 1;
        id
    }

    pub fn fills(&self) -> impl Iterator<Item = (FillId, &Fill)> {
        self.fills.iter().map(|(id, f)| (*id, f))
    }

    pub fn fill(&self, id: FillId) -> Option<&Fill> {
        self.fills.get(&id)
    }

    /// Add a fill on the current layer, on top of the others there.
    pub fn add_fill(&mut self, mut fill: Fill) -> FillId {
        fill.layer = self.current_layer;
        fill.order = self
            .fills
            .values()
            .filter(|f| f.layer == fill.layer)
            .map(|f| f.order + 1)
            .max()
            .unwrap_or(0);
        let id = FillId(self.next_drafting());
        self.touch_key(Key::Fill(id));
        self.fills.insert(id, fill);
        id
    }

    /// Insert a fill with a known id (loading a file). Its layer must exist.
    pub fn insert_fill(&mut self, id: FillId, fill: Fill) -> Result<(), DocumentError> {
        if !self.layers.contains_key(&fill.layer) {
            return Err(DocumentError::MissingLayer(fill.layer));
        }
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Fill(id));
        self.fills.insert(id, fill);
        Ok(())
    }

    pub fn set_fill(&mut self, id: FillId, fill: Fill) {
        if self.fills.get(&id).is_some_and(|f| *f != fill) {
            self.touch_key(Key::Fill(id));
            self.fills.insert(id, fill);
        }
    }

    pub fn remove_fill(&mut self, id: FillId) {
        if self.fills.contains_key(&id) {
            self.touch_key(Key::Fill(id));
            self.fills.remove(&id);
        }
    }

    pub fn annotations(&self) -> impl Iterator<Item = (AnnotationId, &Annotation)> {
        self.annotations.iter().map(|(id, a)| (*id, a))
    }

    pub fn annotation(&self, id: AnnotationId) -> Option<&Annotation> {
        self.annotations.get(&id)
    }

    pub(super) fn validate_annotation(&self, a: &Annotation) -> Result<(), DocumentError> {
        for e in a.entities() {
            if !self.entities.contains_key(&e) {
                return Err(DocumentError::Missing(e));
            }
        }
        Ok(())
    }

    pub fn add_annotation(&mut self, a: Annotation) -> Result<AnnotationId, DocumentError> {
        self.validate_annotation(&a)?;
        let id = AnnotationId(self.next_drafting());
        self.touch_key(Key::Annotation(id));
        self.annotations.insert(id, a);
        Ok(id)
    }

    /// Insert an annotation with a known id (loading a file).
    pub fn insert_annotation(
        &mut self,
        id: AnnotationId,
        a: Annotation,
    ) -> Result<(), DocumentError> {
        self.validate_annotation(&a)?;
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Annotation(id));
        self.annotations.insert(id, a);
        Ok(())
    }

    pub fn set_annotation(&mut self, id: AnnotationId, a: Annotation) -> Result<(), DocumentError> {
        self.validate_annotation(&a)?;
        if self.annotations.get(&id).is_some_and(|x| *x != a) {
            self.touch_key(Key::Annotation(id));
            self.annotations.insert(id, a);
        }
        Ok(())
    }

    pub fn remove_annotation(&mut self, id: AnnotationId) {
        if self.annotations.contains_key(&id) {
            self.touch_key(Key::Annotation(id));
            self.annotations.remove(&id);
        }
    }

    /// Where an anchor is now, if its geometry exists.
    pub fn anchor_pos(&self, anchor: &Anchor) -> Option<DVec2> {
        match *anchor {
            Anchor::Free(p) => Some(p),
            Anchor::Point(id) => self.ref_point(id),
            Anchor::Curve { curve, t } => match self.shape(curve)? {
                Shape::Line { a, b } => Some(a + (b - a) * t),
                Shape::Circle { center, radius } | Shape::Arc { center, radius, .. } => {
                    Some(center + DVec2::from_angle(t) * radius)
                }
                Shape::Ellipse {
                    center,
                    major,
                    minor,
                    ..
                } => Some(crate::shape::ellipse_point(center, major, minor, t)),
                Shape::Bezier { p } => Some(crate::shape::bezier_point(&p, t)),
                Shape::Point(p) => Some(p),
            },
        }
    }

    /// Remember where notes' leaders point, so a note whose target is deleted can stay put.
    /// Called before deleting geometry.
    pub fn remember_leader_targets(&mut self) {
        let targets: Vec<(AnnotationId, DVec2)> = self
            .annotations
            .iter()
            .filter_map(|(id, a)| match a {
                Annotation::Note { leaders, .. } => leaders
                    .first()
                    .and_then(|l| self.anchor_pos(&l.to))
                    .map(|p| (*id, p)),
                _ => None,
            })
            .collect();
        self.last_anchor_pos = targets.into_iter().collect();
    }

    pub fn dim_note(&self, id: ConstraintId) -> Option<&DimNote> {
        self.dim_notes.get(&id)
    }

    pub fn dim_notes(&self) -> impl Iterator<Item = (ConstraintId, &DimNote)> {
        self.dim_notes.iter().map(|(id, n)| (*id, n))
    }

    /// Where a dimension or annotation is shown (everywhere, unless set otherwise).
    pub fn shown(&self, r: crate::AnnotationRef) -> crate::Shown {
        self.shown.get(&r).copied().unwrap_or_default()
    }

    /// The dimensions and annotations not shown everywhere, that still exist.
    pub fn shown_settings(
        &self,
    ) -> impl Iterator<Item = (crate::AnnotationRef, crate::Shown)> + '_ {
        self.shown
            .iter()
            .filter(|(r, _)| self.annotation_ref_exists(**r))
            .map(|(r, s)| (*r, *s))
    }

    /// Whether the dimension or annotation is there.
    pub fn annotation_ref_exists(&self, r: crate::AnnotationRef) -> bool {
        match r {
            crate::AnnotationRef::Dimension(id) => {
                self.constraints.get(&id).is_some_and(|c| c.is_dimension())
            }
            crate::AnnotationRef::Annotation(id) => self.annotations.contains_key(&id),
        }
    }

    /// Set where a dimension or annotation is shown (the default, everywhere, is not kept).
    pub fn set_shown(&mut self, r: crate::AnnotationRef, shown: crate::Shown) {
        if self.shown(r) == shown || !self.annotation_ref_exists(r) {
            return;
        }
        self.touch_key(Key::Shown(r));
        if shown.is_default() {
            self.shown.remove(&r);
        } else {
            self.shown.insert(r, shown);
        }
    }

    /// The views a dimension or annotation is drawn in, if only some (`None`: every view).
    pub fn only_in(&self, r: crate::AnnotationRef) -> Option<&BTreeSet<ViewId>> {
        self.only_in.get(&r)
    }

    /// The dimensions and annotations drawn in some views only, that still exist.
    pub fn only_in_settings(
        &self,
    ) -> impl Iterator<Item = (crate::AnnotationRef, &BTreeSet<ViewId>)> + '_ {
        self.only_in
            .iter()
            .filter(|(r, _)| self.annotation_ref_exists(**r))
            .map(|(r, v)| (*r, v))
    }

    /// Whether a dimension or annotation is drawn in a view.
    pub fn in_view(&self, r: crate::AnnotationRef, view: ViewId) -> bool {
        self.only_in.get(&r).is_none_or(|v| v.contains(&view))
    }

    /// Draw a dimension or annotation in these views only (empty: in every view).
    pub fn set_only_in(&mut self, r: crate::AnnotationRef, views: BTreeSet<ViewId>) {
        let now = self.only_in.get(&r).cloned().unwrap_or_default();
        if now == views || !self.annotation_ref_exists(r) {
            return;
        }
        self.touch_key(Key::OnlyIn(r));
        if views.is_empty() {
            self.only_in.remove(&r);
        } else {
            self.only_in.insert(r, views);
        }
    }

    /// What a dimension or annotation sets for itself over the drawing's style.
    pub fn style_override(&self, r: crate::AnnotationRef) -> crate::StyleOverride {
        self.styles.get(&r).copied().unwrap_or_default()
    }

    /// The dimensions and annotations with settings of their own, that still exist.
    pub fn style_overrides(
        &self,
    ) -> impl Iterator<Item = (crate::AnnotationRef, crate::StyleOverride)> + '_ {
        self.styles
            .iter()
            .filter(|(r, _)| self.annotation_ref_exists(**r))
            .map(|(r, s)| (*r, *s))
    }

    /// Set what a dimension or annotation overrides (nothing overridden is not kept).
    pub fn set_style_override(&mut self, r: crate::AnnotationRef, style: crate::StyleOverride) {
        if self.style_override(r) == style || !self.annotation_ref_exists(r) {
            return;
        }
        self.touch_key(Key::Style(r));
        if style.is_empty() {
            self.styles.remove(&r);
        } else {
            self.styles.insert(r, style);
        }
    }

    /// The style a dimension or annotation is drawn with: the drawing's, with its own
    /// settings in place.
    pub fn draft_style_for(&self, r: crate::AnnotationRef) -> DraftStyle {
        match self.styles.get(&r) {
            Some(o) => o.apply(&self.draft),
            None => self.draft,
        }
    }

    /// Set (or clear, with `None` or an empty note) a dimension's prefix, suffix and tolerance.
    pub fn set_dim_note(&mut self, id: ConstraintId, note: Option<DimNote>) {
        let note = note.filter(|n| !n.is_empty());
        if self.dim_notes.get(&id) != note.as_ref() {
            if note.is_some() && !self.constraints.contains_key(&id) {
                return;
            }
            self.touch_key(Key::DimNote(id));
            match note {
                Some(n) => {
                    self.dim_notes.insert(id, n);
                }
                None => {
                    self.dim_notes.remove(&id);
                }
            }
        }
    }
}
