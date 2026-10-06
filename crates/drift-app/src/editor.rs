//! Editing state independent of the UI toolkit: document, history, selection and picking.

use std::collections::BTreeSet;

use drift_core::{
    Bounds, DVec2, Document, Emphasis, EntityId, History, LineKind, Shape, SketchItem,
};
use drift_geom::{PointRole, SceneIndex, Snap};

/// Where the pointer is, in world coordinates, with its snap target (if any).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// Unsnapped position.
    pub raw: DVec2,
    pub snap: Option<Snap>,
    /// Picking tolerance in world units.
    pub tolerance: f64,
}

impl Pointer {
    pub fn new(raw: DVec2, tolerance: f64) -> Self {
        Self {
            raw,
            snap: None,
            tolerance,
        }
    }

    /// The snapped position, or the raw position if nothing snapped.
    pub fn pos(&self) -> DVec2 {
        self.snap.map_or(self.raw, |s| s.at)
    }

    /// The existing point entity under the pointer, if it snapped to one.
    pub fn snapped_point(&self) -> Option<EntityId> {
        self.snap
            .filter(|s| s.kind.is_point())
            .and_then(|s| s.entity)
    }

    pub fn anchor(&self) -> Anchor {
        Anchor {
            pos: self.pos(),
            existing: self.snapped_point(),
        }
    }
}

/// A position a tool has captured: either an existing point to reuse or a new location.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub pos: DVec2,
    pub existing: Option<EntityId>,
}

impl Anchor {
    /// The point entity for this anchor, creating it if needed. Call inside a transaction.
    pub fn resolve(&self, doc: &mut Document) -> EntityId {
        match self.existing {
            Some(id) if doc.contains(id) => id,
            _ => doc.add_point(self.pos),
        }
    }
}

#[derive(Default)]
pub struct Editor {
    pub doc: Document,
    pub history: History,
    pub selection: BTreeSet<EntityId>,
    pub hovered: Option<EntityId>,
    pub index: SceneIndex,
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        let index = SceneIndex::build(&doc);
        Self {
            doc,
            index,
            ..Default::default()
        }
    }

    /// Refresh caches after document changes. Cheap when nothing changed.
    pub fn sync(&mut self) {
        self.index.update(&self.doc);
        let doc = &self.doc;
        self.selection.retain(|id| doc.contains(*id));
        if self.hovered.is_some_and(|id| !doc.contains(id)) {
            self.hovered = None;
        }
    }

    /// Finish the open transaction and record it for undo.
    pub fn commit(&mut self, label: &str) {
        let tx = self.doc.commit(label);
        self.history.push(tx);
        self.sync();
    }

    pub fn undo(&mut self) -> bool {
        let done = self.history.undo(&mut self.doc);
        self.sync();
        done
    }

    pub fn redo(&mut self) -> bool {
        let done = self.history.redo(&mut self.doc);
        self.sync();
        done
    }

    pub fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.doc.begin();
        self.doc.delete(self.selection.iter().copied());
        self.commit("Delete");
    }

    /// Toggle construction style on the selection: if any selected curve is not construction,
    /// all become construction; otherwise all become solid.
    pub fn toggle_construction(&mut self) {
        let curves: Vec<_> = self
            .selection
            .iter()
            .copied()
            .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
            .collect();
        if curves.is_empty() {
            return;
        }
        let make_construction = curves.iter().any(|id| {
            self.doc
                .get(*id)
                .is_some_and(|e| e.style != LineKind::Construction)
        });
        let style = if make_construction {
            LineKind::Construction
        } else {
            LineKind::Solid
        };
        self.doc.begin();
        for id in curves {
            self.doc.set_style(id, style);
        }
        self.commit("Toggle construction");
    }

    /// Select every curve and standalone point.
    pub fn select_all(&mut self) {
        self.selection = self
            .doc
            .iter()
            .filter(|(id, e)| {
                !e.kind.is_point() || self.index.role(*id) == Some(PointRole::Standalone)
            })
            .map(|(id, _)| id)
            .collect();
    }

    pub fn bounds(&self) -> Bounds {
        self.doc
            .iter()
            .filter_map(|(id, _)| self.index.shape(id))
            .map(Shape::bounds)
            .collect()
    }

    /// Render items for the document, with selection and hover emphasis.
    pub fn items(&self) -> Vec<SketchItem> {
        let mut items = Vec::with_capacity(self.doc.len());
        let mut emphasized = Vec::new();
        for (id, entity) in self.doc.iter() {
            let Some(shape) = self.index.shape(id) else {
                continue;
            };
            let mut item = SketchItem::new(*shape).with_kind(entity.style);
            let role = self.index.role(id);
            if role.is_some_and(|r| r != PointRole::Standalone) {
                item = item.as_vertex();
            }
            let emphasis = if self.selection.contains(&id) {
                Emphasis::Selected
            } else if self.hovered == Some(id) {
                Emphasis::Hovered
            } else {
                Emphasis::Normal
            };
            if emphasis == Emphasis::Normal {
                items.push(item);
            } else {
                // Draw emphasized items last so they sit on top.
                emphasized.push(item.with_emphasis(emphasis));
            }
        }
        items.extend(emphasized);
        items
    }
}
