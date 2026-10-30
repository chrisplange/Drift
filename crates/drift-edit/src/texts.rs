//! Sketch text in the editor: cached outlines, picking and moving, and turning text into
//! curves.

use std::collections::HashMap;
use std::rc::Rc;

use drift_core::{
    Bounds, DVec2, Emphasis, EntityId, LineType, Shape, SketchItem, SketchText, TextId,
};

use crate::editor::Editor;

/// The longest sketch text that can be typed or pasted.
pub const MAX_TEXT_CHARS: usize = 2000;

/// A text's outlines and their bounds, for the text as it was when they were made.
#[derive(Clone)]
pub struct TextOutline {
    pub source: SketchText,
    pub contours: Rc<Vec<Vec<Shape>>>,
    pub bounds: Bounds,
}

/// Outlines by text, rebuilt only when a text changes.
#[derive(Default)]
pub struct TextCache {
    outlines: HashMap<TextId, TextOutline>,
}

impl TextCache {
    /// Forget every outline (a font arrived or changed).
    pub fn clear(&mut self) {
        self.outlines.clear();
    }
}

impl Editor {
    /// A text's outlines (made once per change of the text).
    pub fn text_outline(&self, id: TextId) -> Option<TextOutline> {
        let text = self.doc.text(id)?;
        if let Some(o) = self.text_cache.borrow().outlines.get(&id)
            && o.source == *text
        {
            return Some(o.clone());
        }
        self.ensure_font(&text.font);
        let contours = drift_draft::outline::Paragraph {
            text: &text.text,
            font: &text.font,
            height: text.height,
            at: text.at,
            angle: text.angle,
            align: text.align,
            spacing: text.spacing,
            wrap: text.wrap,
        }
        .contours();
        let mut bounds: Bounds = contours.iter().flatten().map(Shape::bounds).collect();
        if bounds.is_empty() {
            // Empty or blank text: a cap-height box so it can still be picked.
            bounds = Bounds::from_point(text.at);
            bounds
                .include(text.at + DVec2::from_angle(text.angle.to_radians()).perp() * text.height);
        }
        let o = TextOutline {
            source: text.clone(),
            contours: Rc::new(contours),
            bounds,
        };
        let mut cache = self.text_cache.borrow_mut();
        cache.outlines.insert(id, o.clone());
        // Forget outlines of texts that no longer exist.
        if cache.outlines.len() > self.doc.texts().count() + 32 {
            cache.outlines.retain(|id, _| self.doc.text(*id).is_some());
        }
        Some(o)
    }

    /// The text under `p` (inside its bounds, with `tol` to spare), smallest first.
    pub fn text_at(&self, p: DVec2, tol: f64) -> Option<TextId> {
        self.doc
            .texts()
            .filter(|(_, t)| {
                self.doc
                    .layer(t.layer)
                    .is_some_and(|l| l.visible && !l.locked)
            })
            .filter_map(|(id, _)| {
                let b = self.text_outline(id)?.bounds;
                let inside = p.x >= b.min.x - tol
                    && p.x <= b.max.x + tol
                    && p.y >= b.min.y - tol
                    && p.y <= b.max.y + tol;
                inside.then(|| (id, (b.max - b.min).length()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Outlines of the visible texts, coloured by their own or their layer's colour.
    #[cfg(any(test, feature = "test-support"))]
    pub fn text_items(&self, status_colors: bool, dark: bool) -> Vec<SketchItem> {
        self.text_items_tagged(status_colors, dark)
            .into_iter()
            .map(|t| t.item)
            .collect()
    }

    /// [`text_items`](Self::text_items), each with its text's layer (to draw at the top of
    /// it, in the drawing's stack).
    pub fn text_items_tagged(
        &self,
        status_colors: bool,
        dark: bool,
    ) -> Vec<crate::editor::TaggedItem> {
        let mut items = Vec::new();
        for (id, t) in self.doc.texts() {
            let Some(layer) = self.doc.layer(t.layer) else {
                continue;
            };
            if !layer.visible {
                continue;
            }
            let Some(o) = self.text_outline(id) else {
                continue;
            };
            let color = if status_colors {
                None
            } else {
                t.appearance
                    .color
                    .or(layer.color)
                    .and_then(|c| crate::editor::contrast(c.0, dark))
            };
            let emphasis = if self.selected_texts.contains(&id) {
                Emphasis::Selected
            } else if self.hovered_text == Some(id) {
                Emphasis::Hovered
            } else {
                Emphasis::Normal
            };
            items.extend(o.contours.iter().flatten().map(|s| {
                crate::editor::TaggedItem {
                    entity: None,
                    layer: t.layer,
                    item: SketchItem::new(*s)
                        .with_look(color, 0, LineType::Continuous)
                        .with_emphasis(emphasis),
                }
            }));
        }
        items
    }

    /// Add a text (one undo step) and pick it.
    pub fn add_text(&mut self, text: SketchText) -> TextId {
        self.doc.begin();
        let id = self.doc.add_text(text);
        self.commit("Text");
        self.selected_texts = [id].into();
        id
    }

    /// Change a text as one undo step.
    pub fn update_text(&mut self, id: TextId, f: impl FnOnce(&mut SketchText)) {
        let Some(mut t) = self.doc.text(id).cloned() else {
            return;
        };
        f(&mut t);
        if self.doc.text(id) == Some(&t) {
            return;
        }
        self.doc.begin();
        self.doc.set_text(id, t);
        self.commit("Edit text");
    }

    /// Type into a text; `continuing` folds the change into the previous undo step.
    pub fn type_text(&mut self, id: TextId, words: &str, continuing: bool) {
        let Some(mut t) = self.doc.text(id).cloned() else {
            return;
        };
        let words: String = words.chars().take(MAX_TEXT_CHARS).collect();
        if t.text == words {
            return;
        }
        t.text = words;
        self.doc.begin();
        self.doc.set_text(id, t);
        let tx = self.doc.commit("Type text");
        if continuing {
            self.history.merge_into_last(tx);
        } else {
            self.history.push(tx);
        }
        self.sync();
    }

    /// Remove texts left empty (placed but never typed into), except `keep`. The removal
    /// joins the last undo step (placing them), so undo doesn't bring blank text back.
    pub fn drop_empty_texts(&mut self, keep: Option<TextId>) {
        let empty: Vec<TextId> = self
            .doc
            .texts()
            .filter(|(id, t)| t.text.trim().is_empty() && Some(*id) != keep)
            .map(|(id, _)| id)
            .collect();
        if empty.is_empty() {
            return;
        }
        self.doc.begin();
        for id in empty {
            self.doc.remove_text(id);
        }
        let tx = self.doc.commit("Remove empty text");
        self.history.merge_into_last(tx);
        self.sync();
    }

    pub fn remove_text(&mut self, id: TextId) {
        self.doc.begin();
        self.doc.remove_text(id);
        self.commit("Delete text");
        self.selected_texts.remove(&id);
    }

    /// Replace a text with its outlines as curves and lines (each letter's contours
    /// connected), grouped, and pick them.
    pub fn convert_text(&mut self, id: TextId) -> usize {
        let (Some(o), Some(t)) = (self.text_outline(id), self.doc.text(id).cloned()) else {
            return 0;
        };
        self.doc.begin();
        let mut created: Vec<EntityId> = Vec::new();
        for contour in o.contours.iter() {
            // Contour ends meet exactly: share their points.
            let mut ends: Vec<(DVec2, EntityId)> = Vec::new();
            let mut point = |doc: &mut drift_core::Document, p: DVec2| -> EntityId {
                if let Some((_, id)) = ends.iter().find(|(q, _)| q.distance(p) < 1e-9) {
                    return *id;
                }
                let id = doc.add_point(p);
                ends.push((p, id));
                id
            };
            for s in contour {
                let made = match *s {
                    Shape::Line { a, b } => {
                        let (pa, pb) = (point(&mut self.doc, a), point(&mut self.doc, b));
                        self.doc.add_line(pa, pb).ok()
                    }
                    Shape::Bezier { p } => {
                        let a = point(&mut self.doc, p[0]);
                        let b = point(&mut self.doc, p[3]);
                        let c1 = self.doc.add_point(p[1]);
                        let c2 = self.doc.add_point(p[2]);
                        self.doc.add_bezier(a, c1, c2, b).ok()
                    }
                    _ => None,
                };
                if let Some(e) = made {
                    let _ = self.doc.set_entity_layer(e, t.layer);
                    let _ = self.doc.set_appearance(e, t.appearance);
                    created.push(e);
                }
            }
        }
        self.doc.remove_text(id);
        let _ = self.doc.add_group(drift_core::Group {
            name: format!("Text: {}", t.text),
            members: created.iter().copied().collect(),
        });
        if !self.commit("Convert text") {
            return 0;
        }
        self.selected_texts.remove(&id);
        self.selection = created.iter().copied().collect();
        created.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_picks_moves_and_converts_to_curves() {
        let mut ed = Editor::default();
        let id = ed.add_text(SketchText::new("Hi", DVec2::new(10.0, 10.0), 10.0));
        assert_eq!(ed.text_at(DVec2::new(12.0, 15.0), 0.5), Some(id));
        assert_eq!(ed.text_at(DVec2::new(100.0, 15.0), 0.5), None);
        assert!(!ed.text_items(true, true).is_empty());
        ed.update_text(id, |t| t.at += DVec2::new(50.0, 0.0));
        assert_eq!(ed.text_at(DVec2::new(62.0, 15.0), 0.5), Some(id));
        let n = ed.convert_text(id);
        assert!(n > 10, "{n}");
        assert!(ed.doc.text(id).is_none());
        assert_eq!(ed.doc.groups().count(), 1);
        assert_eq!(ed.selection.len(), n);
        ed.undo();
        assert!(ed.doc.text(id).is_some() && ed.doc.is_empty());
    }
}
