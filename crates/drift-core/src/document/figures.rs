//! Figures: which shape, as it was drawn, each curve belongs to (#439). Shapes drawn
//! separately stay separate even where snapping made them share corner points, so Tab
//! chains only within one.
//!
//! Curves in no figure (every curve of a file saved before figures) are a figure of their
//! own together: they chain with each other as Tab always did, and not with curves in a
//! figure. A new shape that continues one of their chains from its open end takes that
//! whole chain into its figure.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::*;

impl Document {
    /// The figure curve `id` belongs to (`None` for points, circles, ellipses and curves from
    /// files saved before figures).
    pub fn figure_of(&self, id: EntityId) -> Option<FigureId> {
        self.entities.get(&id).and_then(|e| e.figure)
    }

    /// Put curve `id` in figure `figure` (or in none).
    pub fn set_figure(&mut self, id: EntityId, figure: Option<FigureId>) {
        if self.entities.get(&id).is_some_and(|e| e.figure != figure) {
            self.touch(id);
            if let Some(e) = self.entities.get_mut(&id) {
                e.figure = figure;
            }
        }
    }

    /// A figure no curve is in yet (numbered from 1, as ids in files are).
    pub fn fresh_figure(&self) -> FigureId {
        FigureId(
            self.entities
                .values()
                .filter_map(|e| e.figure)
                .map(|f| f.0.saturating_add(1))
                .max()
                .unwrap_or(1)
                .max(1),
        )
    }

    /// Put `to`, a piece made from curve `from` (split off it, the far side of a trim), in
    /// `from`'s figure; a piece of a curve in no figure stays in none.
    pub fn copy_figure(&mut self, from: EntityId, to: EntityId) {
        match self.figure_of(from) {
            Some(f) => self.set_figure(to, Some(f)),
            None => self.unfigured_pieces.push(to),
        }
    }

    /// Make the curves `curves` and every curve in a figure one of them is in one figure (the
    /// remains of shapes trimmed against each other, ends joined). A curve in no figure
    /// brings the chain of such curves it is in. When none of them is in a figure they are
    /// left as they are (they chain together already). Points, circles and ellipses among
    /// them are left out. Returns the figure.
    pub fn join_figures(&mut self, curves: &[EntityId]) -> Option<FigureId> {
        let curves: Vec<EntityId> = curves
            .iter()
            .copied()
            .filter(|c| {
                self.entities
                    .get(c)
                    .is_some_and(|e| e.kind.ends().is_some())
            })
            .collect();
        let figures: BTreeSet<FigureId> =
            curves.iter().filter_map(|c| self.figure_of(*c)).collect();
        let Some(&target) = figures.first() else {
            self.unfigured_pieces.extend(curves);
            return None;
        };
        self.merge_figures(target, &figures);
        for c in curves {
            for k in self.unfigured_chain(c) {
                self.set_figure(k, Some(target));
            }
        }
        Some(target)
    }

    /// Before points `from` and `into` are merged into one: where both are open ends (of a
    /// figure, or of a chain of curves in no figure), the shapes they end join, as dragging
    /// an end onto another end joins two paths. A closed corner keeps its shape separate,
    /// as drawing onto one does.
    pub(super) fn join_figures_at_merge(&mut self, from: EntityId, into: EntityId) {
        let mut at: [BTreeMap<Option<FigureId>, Vec<EntityId>>; 2] = Default::default();
        for (id, e) in self.entities.iter() {
            for (i, p) in [from, into].into_iter().enumerate() {
                if let Some(ends) = e.kind.ends() {
                    for _ in ends.iter().filter(|q| **q == p) {
                        at[i].entry(e.figure).or_default().push(*id);
                    }
                }
            }
        }
        let open = |m: &BTreeMap<Option<FigureId>, Vec<EntityId>>| -> Vec<EntityId> {
            m.values()
                .filter(|curves| curves.len() == 1)
                .flatten()
                .copied()
                .collect()
        };
        let (a, b) = (open(&at[0]), open(&at[1]));
        let both: Vec<EntityId> = a.iter().chain(&b).copied().collect();
        // (Curves in no figure on both sides chain already; new ones get theirs when the
        // edit is committed, joined by the merged point.)
        if !a.is_empty() && !b.is_empty() && both.iter().any(|c| self.figure_of(*c).is_some()) {
            self.join_figures(&both);
        }
    }

    /// The curves in no figure joined end to end with `start` on its layer, itself included
    /// (none if `start` is in a figure): the chain Tab picked in a drawing from before
    /// figures.
    fn unfigured_chain(&self, start: EntityId) -> Vec<EntityId> {
        let Some(layer) = self.entities.get(&start).map(|e| e.layer) else {
            return Vec::new();
        };
        let unfigured =
            |e: &Entity| e.figure.is_none() && e.kind.ends().is_some() && e.layer == layer;
        if !self.entities.get(&start).is_some_and(unfigured) {
            return Vec::new();
        }
        let mut by_point: HashMap<EntityId, Vec<EntityId>> = HashMap::new();
        for (id, e) in self.entities.iter() {
            if unfigured(e) {
                for p in e.kind.ends().into_iter().flatten() {
                    by_point.entry(p).or_default().push(*id);
                }
            }
        }
        let mut seen: BTreeSet<EntityId> = [start].into();
        let mut queue = vec![start];
        while let Some(c) = queue.pop() {
            for p in self.entities[&c].kind.ends().into_iter().flatten() {
                for &u in by_point.get(&p).into_iter().flatten() {
                    if seen.insert(u) {
                        queue.push(u);
                    }
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Move every curve of the figures `from` into `target`.
    fn merge_figures(&mut self, target: FigureId, from: &BTreeSet<FigureId>) {
        if from.iter().all(|f| *f == target) {
            return;
        }
        let moved: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| e.figure.is_some_and(|f| f != target && from.contains(&f)))
            .map(|(id, _)| *id)
            .collect();
        for id in moved {
            self.set_figure(id, Some(target));
        }
    }

    /// Give the curves made in the open transaction that are in no figure yet their figures,
    /// as a drawing program keeps each shape drawn its own object: curves made together and
    /// joined end to end form one figure, which joins an existing shape only where it meets
    /// one of that shape's open ends (a path continued from its end, as in Illustrator).
    /// Meeting any other point (a closed shape's corner, a point in the middle of a path)
    /// keeps it a figure of its own. Pieces of curves in no figure (see
    /// [`copy_figure`](Self::copy_figure)) stay in none. Called when the transaction is
    /// committed.
    pub fn figure_new_curves(&mut self) {
        let pieces: HashSet<EntityId> = std::mem::take(&mut self.unfigured_pieces)
            .into_iter()
            .collect();
        let new: Vec<EntityId> = self
            .created_in_transaction()
            .into_iter()
            .filter(|id| !pieces.contains(id))
            .collect();
        self.figure_curves(&new);
    }

    /// Give the curves `curves` that are in no figure yet their figures, by the rule of
    /// [`figure_new_curves`](Self::figure_new_curves): those joined end to end form one
    /// figure, which joins an existing shape only at one of its open ends. For an edit that
    /// names its shapes one by one (an AI-link item or script statement, #476), so shapes made
    /// in one step that share corners stay apart. Points, circles and ellipses are left out.
    pub fn figure_curves(&mut self, curves: &[EntityId]) {
        let new: Vec<EntityId> = curves
            .iter()
            .copied()
            .filter(|id| {
                self.entities
                    .get(id)
                    .is_some_and(|e| e.figure.is_none() && e.kind.ends().is_some())
            })
            .collect();
        if new.is_empty() {
            return;
        }
        let ends = |doc: &Self, id: EntityId| -> [EntityId; 2] {
            doc.entities
                .get(&id)
                .and_then(|e| e.kind.ends())
                .unwrap_or([id, id])
        };
        // The new curves joined end to end with each other, as components.
        let mut parent: Vec<usize> = (0..new.len()).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let mut at_point: HashMap<EntityId, usize> = HashMap::new();
        for (i, &c) in new.iter().enumerate() {
            for p in ends(self, c) {
                match at_point.get(&p) {
                    Some(&j) => {
                        let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                        parent[a] = b;
                    }
                    None => {
                        at_point.insert(p, i);
                    }
                }
            }
        }
        // The ends of the existing shapes (by figure; `None`: curves in no figure) at each of
        // those points: a shape with one end there has an open end there.
        let new_set: HashSet<EntityId> = new.iter().copied().collect();
        let mut existing: HashMap<EntityId, BTreeMap<Option<FigureId>, Vec<EntityId>>> =
            HashMap::new();
        for (id, e) in self.entities.iter() {
            let Some(curve_ends) = e.kind.ends() else {
                continue;
            };
            if new_set.contains(id) {
                continue;
            }
            for p in curve_ends {
                if at_point.contains_key(&p) {
                    existing
                        .entry(p)
                        .or_default()
                        .entry(e.figure)
                        .or_default()
                        .push(*id);
                }
            }
        }
        let mut components: BTreeMap<usize, Vec<EntityId>> = BTreeMap::new();
        for (i, &c) in new.iter().enumerate() {
            components.entry(root(&mut parent, i)).or_default().push(c);
        }
        let mut fresh = self.fresh_figure();
        for curves in components.into_values() {
            let points: BTreeSet<EntityId> = curves.iter().flat_map(|c| ends(self, *c)).collect();
            let mut open: BTreeSet<FigureId> = BTreeSet::new();
            let mut unfigured: Vec<EntityId> = Vec::new();
            for counts in points.iter().filter_map(|p| existing.get(p)) {
                for (f, at) in counts {
                    match (f, at.as_slice()) {
                        (Some(f), [_]) => {
                            open.insert(*f);
                        }
                        (None, [c]) => unfigured.push(*c),
                        _ => {}
                    }
                }
            }
            let target = match open.first() {
                Some(&f) => f,
                None => {
                    let f = fresh;
                    fresh.0 = fresh.0.saturating_add(1);
                    f
                }
            };
            self.merge_figures(target, &open);
            for c in unfigured {
                for k in self.unfigured_chain(c) {
                    self.set_figure(k, Some(target));
                }
            }
            for c in curves {
                self.set_figure(c, Some(target));
            }
            // (What this component joined counts as its figure for the next.)
            for counts in existing.values_mut() {
                let mut moved: Vec<EntityId> = Vec::new();
                counts.retain(|f, at| {
                    let joined = f.is_some_and(|f| open.contains(&f) && f != target)
                        || (f.is_none()
                            && at
                                .iter()
                                .any(|c| self.entities.get(c).is_some_and(|e| e.figure.is_some())));
                    if joined {
                        moved.append(at);
                    }
                    !joined
                });
                if !moved.is_empty() {
                    counts.entry(Some(target)).or_default().extend(moved);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DVec2, History};

    /// #439 review: a step committed again (reopened, or replayed after being set aside, as
    /// fills that follow an edit late do) leaves the curves its first commit left in no
    /// figure in none, and keeps the figures it gave.
    #[test]
    fn a_step_committed_again_keeps_its_figures() {
        let mut doc = Document::new();
        let p = [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (20.0, 10.0)]
            .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
        let old = doc.add_line(p[0], p[1]).unwrap();
        // A piece of the old line (in no figure), and a new line drawn apart.
        doc.begin();
        let piece = doc.add_line(p[1], p[2]).unwrap();
        doc.copy_figure(old, piece);
        let tx = doc.commit("Split");
        assert_eq!(doc.figure_of(piece), None);
        doc.begin();
        let q = doc.add_point(DVec2::new(30.0, 10.0));
        let drawn = doc.add_line(p[3], q).unwrap();
        let drawn_tx = doc.commit("Line");
        let figure = doc.figure_of(drawn);
        assert!(figure.is_some());
        // Reopened and committed again.
        let mut back = History::default();
        back.put_back(tx.clone());
        back.put_back(drawn_tx.clone());
        assert!(back.undo(&mut doc));
        doc.reopen(tx.clone());
        let tx = doc.commit("Split");
        assert_eq!(doc.figure_of(piece), None, "reopened");
        // Set aside and replayed.
        let mut back = History::default();
        back.put_back(tx.clone());
        assert!(back.undo(&mut doc));
        doc.begin();
        doc.replay(&tx);
        doc.commit("Split");
        assert_eq!(doc.figure_of(piece), None, "replayed");
        doc.begin();
        doc.replay(&drawn_tx);
        doc.commit("Line");
        assert_eq!(doc.figure_of(drawn), figure, "its figure kept");
        assert_eq!(doc.figure_of(old), None);
    }

    /// #439 review: curves made in one edit whose ends are merged in it (wall centrelines
    /// joined as they are drawn) are one figure, not left in none.
    #[test]
    fn new_curves_merged_in_their_edit_are_one_figure() {
        let mut doc = Document::new();
        doc.begin();
        let p = [(0.0, 0.0), (10.0, 0.0), (10.0, 1.0), (10.0, 10.0)]
            .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
        let a = doc.add_line(p[0], p[1]).unwrap();
        let b = doc.add_line(p[2], p[3]).unwrap();
        doc.merge_points(p[2], p[1]).unwrap();
        doc.commit("Walls");
        assert!(doc.figure_of(a).is_some());
        assert_eq!(doc.figure_of(a), doc.figure_of(b));
    }
}
