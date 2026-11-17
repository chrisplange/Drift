//! Figures: which shape, as it was drawn, each curve belongs to (#439). Shapes drawn
//! separately stay separate even where snapping made them share corner points, so Tab
//! chains only within one.

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

    /// Make the curves `curves` and every curve in a figure one of them is in one figure (the
    /// remains of shapes trimmed against each other). Points, circles and ellipses among them
    /// are left out. Returns the figure, if any curve was given.
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
        if curves.is_empty() {
            return None;
        }
        let figures: BTreeSet<FigureId> =
            curves.iter().filter_map(|c| self.figure_of(*c)).collect();
        let target = figures
            .first()
            .copied()
            .unwrap_or_else(|| self.fresh_figure());
        self.merge_figures(target, &figures);
        for c in curves {
            self.set_figure(c, Some(target));
        }
        Some(target)
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
    /// joined end to end form one figure, which joins an existing figure only where it meets
    /// one of that figure's open ends (a path continued from its end, as in Illustrator).
    /// Meeting any other point (a closed shape's corner, a point in the middle of a path)
    /// keeps it a figure of its own. Called when the transaction is committed.
    pub fn figure_new_curves(&mut self) {
        let new: Vec<EntityId> = self
            .created_in_transaction()
            .into_iter()
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
            doc.entities[&id].kind.ends().expect("a curve with ends")
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
        // How many ends of each existing figure meet each of those points: one is an open
        // end.
        let new_set: HashSet<EntityId> = new.iter().copied().collect();
        let mut existing: HashMap<EntityId, BTreeMap<FigureId, u32>> = HashMap::new();
        for (id, e) in &self.entities {
            let (Some(f), Some(ends)) = (e.figure, e.kind.ends()) else {
                continue;
            };
            if new_set.contains(id) {
                continue;
            }
            for p in ends {
                if at_point.contains_key(&p) {
                    *existing.entry(p).or_default().entry(f).or_default() += 1;
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
            let open: BTreeSet<FigureId> = points
                .iter()
                .filter_map(|p| existing.get(p))
                .flat_map(|counts| counts.iter().filter(|(_, n)| **n == 1).map(|(f, _)| *f))
                .collect();
            let target = match open.first() {
                Some(&f) => f,
                None => {
                    let f = fresh;
                    fresh.0 += 1;
                    f
                }
            };
            if open.len() > 1 {
                self.merge_figures(target, &open);
                for counts in existing.values_mut() {
                    let merged: u32 = open.iter().filter_map(|f| counts.remove(f)).sum();
                    if merged > 0 {
                        counts.insert(target, merged);
                    }
                }
            }
            for c in curves {
                self.set_figure(c, Some(target));
            }
        }
    }
}
