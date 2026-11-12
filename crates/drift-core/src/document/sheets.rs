//! Sheets and the views placed on them.

use super::*;

impl Document {
    pub fn sheets(&self) -> impl Iterator<Item = (SheetId, &Sheet)> {
        self.sheets.iter().map(|(id, s)| (*id, s))
    }

    pub fn sheet(&self, id: SheetId) -> Option<&Sheet> {
        self.sheets.get(&id)
    }

    pub fn add_sheet(&mut self, sheet: Sheet) -> SheetId {
        let id = SheetId(self.next_drafting());
        self.touch_key(Key::Sheet(id));
        self.sheets.insert(id, sheet);
        id
    }

    /// Insert a sheet with a known id (loading a file).
    pub fn insert_sheet(&mut self, id: SheetId, sheet: Sheet) {
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Sheet(id));
        self.sheets.insert(id, sheet);
    }

    pub fn set_sheet(&mut self, id: SheetId, sheet: Sheet) {
        if self.sheets.get(&id).is_some_and(|s| *s != sheet) {
            self.touch_key(Key::Sheet(id));
            self.sheets.insert(id, sheet);
        }
    }

    pub fn remove_sheet(&mut self, id: SheetId) {
        if self.sheets.contains_key(&id) {
            self.touch_key(Key::Sheet(id));
            self.sheets.remove(&id);
        }
    }

    /// Move a sheet to position `to` among the sheets, the others closing up. Sheets are in
    /// the order of their ids, so the sheets move between ids rather than the ids moving:
    /// the result says where each sheet went (its old id → its new one).
    pub fn move_sheet(&mut self, id: SheetId, to: usize) -> BTreeMap<SheetId, SheetId> {
        let ids: Vec<SheetId> = self.sheets.keys().copied().collect();
        let Some(from) = ids.iter().position(|s| *s == id) else {
            return BTreeMap::new();
        };
        let mut order = ids.clone();
        let moved = order.remove(from);
        order.insert(to.min(order.len()), moved);
        let before = self.sheets.clone();
        let mut went = BTreeMap::new();
        for (old, new) in order.into_iter().zip(ids) {
            went.insert(old, new);
            self.set_sheet(new, before[&old].clone());
        }
        went
    }

    pub fn views(&self) -> impl Iterator<Item = (ViewId, &View)> {
        self.views.iter().map(|(id, v)| (*id, v))
    }

    pub fn view(&self, id: ViewId) -> Option<&View> {
        self.views.get(&id)
    }

    pub fn add_view(&mut self, view: View) -> ViewId {
        let id = ViewId(self.next_drafting());
        self.touch_key(Key::View(id));
        self.views.insert(id, view);
        id
    }

    /// Insert a view with a known id (loading a file).
    pub fn insert_view(&mut self, id: ViewId, view: View) {
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::View(id));
        self.views.insert(id, view);
    }

    pub fn set_view(&mut self, id: ViewId, view: View) {
        if self.views.get(&id).is_some_and(|v| *v != view) {
            self.touch_key(Key::View(id));
            self.views.insert(id, view);
        }
    }

    /// Delete a view, take it off every sheet it is on, and unlink the marks that point at it.
    pub fn remove_view(&mut self, id: ViewId) {
        if !self.views.contains_key(&id) {
            return;
        }
        self.touch_key(Key::View(id));
        self.views.remove(&id);
        let showing: Vec<SheetId> = self.sheets_showing(id);
        for s in showing {
            let mut sheet = self.sheets[&s].clone();
            sheet.viewports.retain(|p| p.view != id);
            self.set_sheet(s, sheet);
        }
        for mark in self.marks_of_view(id) {
            self.touch_key(Key::Annotation(mark));
            if let Some(a) = self.annotations.get_mut(&mark) {
                a.set_view(None);
            }
        }
        // What was drawn in it alone stays off sheets, rather than showing in every view.
        let only: Vec<crate::AnnotationRef> = self
            .only_in
            .iter()
            .filter(|(_, v)| v.contains(&id))
            .map(|(r, _)| *r)
            .collect();
        for r in only {
            let mut views = self.only_in[&r].clone();
            views.remove(&id);
            if views.is_empty() {
                let mut shown = self.shown(r);
                shown.sheets = false;
                self.set_shown(r, shown);
            }
            self.touch_key(Key::OnlyIn(r));
            if views.is_empty() {
                self.only_in.remove(&r);
            } else {
                self.only_in.insert(r, views);
            }
        }
    }

    /// The section and elevation marks that point at a view.
    pub fn marks_of_view(&self, id: ViewId) -> Vec<AnnotationId> {
        self.annotations
            .iter()
            .filter(|(_, a)| a.view() == Some(id))
            .map(|(aid, _)| *aid)
            .collect()
    }

    /// How a mark linked to a view refers to it: the view's number (its place among the
    /// viewports of the first sheet showing it, from 1) and that sheet's number. `None` while
    /// the view is on no sheet.
    pub fn view_reference(&self, id: ViewId) -> Option<(String, String)> {
        let sheet = &self.sheets[self.sheets_showing(id).first()?];
        let n = sheet.viewports.iter().position(|p| p.view == id)? + 1;
        let number = match sheet.fields.number.trim() {
            "" => "–".to_owned(),
            s => s.to_owned(),
        };
        Some((n.to_string(), number))
    }

    /// The sheets a view is placed on, in order.
    pub fn sheets_showing(&self, id: ViewId) -> Vec<SheetId> {
        self.sheets
            .iter()
            .filter(|(_, s)| s.viewports.iter().any(|p| p.view == id))
            .map(|(sid, _)| *sid)
            .collect()
    }

    /// A sheet's viewports with their views, in drawing order (one whose view is missing is
    /// left out).
    pub fn placed_views(&self, sheet: &Sheet) -> Vec<PlacedView> {
        sheet
            .viewports
            .iter()
            .filter_map(|p| Some(PlacedView::new(p.view, self.views.get(&p.view)?, p)))
            .collect()
    }

    /// Whether view `view` prints dimension `id` where it is placed on a sheet, by the rules
    /// sheets print by (`drift-app`'s output uses this too): the view shows dimensions, the
    /// dimension is shown on sheets and in that view, and not every curve it is on is on a
    /// layer the view hides.
    pub fn view_prints_dimension(&self, view: ViewId, id: crate::ConstraintId) -> bool {
        let (Some(v), Some(c)) = (self.views.get(&view), self.constraint(id)) else {
            return false;
        };
        let r = crate::AnnotationRef::Dimension(id);
        let entities: Vec<crate::EntityId> = c.entities().collect();
        let hidden = !v.hidden_layers.is_empty()
            && !entities.is_empty()
            && entities.iter().all(|e| {
                self.get(*e)
                    .is_some_and(|x| v.hidden_layers.contains(&x.layer))
            });
        v.dimensions && self.shown(r).sheets && self.in_view(r, view) && !hidden
    }

    /// [`Self::view_prints_dimension`], and one of the points it measures is inside the view's
    /// crop, so it is seen there rather than clipped away.
    pub fn view_draws_dimension(&self, view: ViewId, id: crate::ConstraintId) -> bool {
        let (Some(v), Some(c)) = (self.views.get(&view), self.constraint(id)) else {
            return false;
        };
        let crop = v.crop();
        self.view_prints_dimension(view, id)
            && c.entities()
                .filter_map(|e| self.ref_point(e))
                .any(|p| p.cmpge(crop.min).all() && p.cmple(crop.max).all())
    }

    /// The scale (1 : n) dimension `id` is printed at: that of the first view placed on a
    /// sheet (in sheet order, then the sheet's order) that draws it, else the drawing scale.
    /// Labels placed before their offsets were kept on paper are read back by it, so a sheet
    /// looks as it did.
    pub fn printed_scale(&self, id: crate::ConstraintId) -> f64 {
        self.sheets
            .values()
            .flat_map(|s| s.viewports.iter())
            .find(|p| self.view_draws_dimension(p.view, id))
            .and_then(|p| self.views.get(&p.view))
            .map_or_else(|| self.draft_style().scale, |v| v.scale)
    }

    /// A sheet's title block scale: its views' scale if they share one, else `As shown`.
    pub fn sheet_scale_text(&self, sheet: &Sheet) -> String {
        crate::sheets::scale_text(
            sheet
                .viewports
                .iter()
                .filter_map(|p| self.views.get(&p.view).map(|v| v.scale)),
            !self.units().length.is_metric(),
        )
    }

    /// The scale (1 : n) every view placed on the drawing's sheets shares, if there is at
    /// least one and they all agree; `None` when there are none or they differ.
    pub fn sheets_shared_scale(&self) -> Option<f64> {
        let mut scales = self
            .sheets
            .values()
            .flat_map(|s| s.viewports.iter())
            .filter_map(|p| self.views.get(&p.view).map(|v| v.scale));
        let first = scales.next()?;
        let same = |n: f64| (n - first).abs() <= 1e-6 * n.abs().max(first.abs());
        scales.all(same).then_some(first)
    }

    /// Write back viewport `index` of sheet `id` (and its view, which every sheet showing
    /// it follows).
    pub fn set_placed_view(&mut self, id: SheetId, index: usize, placed: &PlacedView) {
        let Some(mut sheet) = self.sheets.get(&id).cloned() else {
            return;
        };
        let Some(port) = sheet.viewports.get_mut(index) else {
            return;
        };
        let Some(mut view) = self.views.get(&port.view).cloned() else {
            return;
        };
        let view_id = port.view;
        placed.apply(&mut view, port);
        self.set_view(view_id, view);
        self.set_sheet(id, sheet);
    }
}
