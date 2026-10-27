//! Sketch text and underlays.

use super::*;

impl Document {
    pub fn texts(&self) -> impl Iterator<Item = (TextId, &SketchText)> {
        self.texts.iter().map(|(id, t)| (*id, t))
    }

    pub fn text(&self, id: TextId) -> Option<&SketchText> {
        self.texts.get(&id)
    }

    /// Add a text on the current layer.
    pub fn add_text(&mut self, mut text: SketchText) -> TextId {
        text.layer = self.current_layer;
        let id = TextId(self.next_drafting());
        self.touch_key(Key::Text(id));
        self.texts.insert(id, text);
        id
    }

    /// Insert a text with a known id (loading a file).
    pub fn insert_text(&mut self, id: TextId, text: SketchText) {
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Text(id));
        self.texts.insert(id, text);
    }

    pub fn set_text(&mut self, id: TextId, text: SketchText) {
        if self.texts.get(&id).is_some_and(|t| *t != text) {
            self.touch_key(Key::Text(id));
            self.texts.insert(id, text);
        }
    }

    pub fn remove_text(&mut self, id: TextId) {
        if self.texts.contains_key(&id) {
            self.touch_key(Key::Text(id));
            self.texts.remove(&id);
        }
    }

    pub fn underlays(&self) -> impl Iterator<Item = (UnderlayId, &Underlay)> {
        self.underlays.iter().map(|(id, u)| (*id, u))
    }

    pub fn underlay(&self, id: UnderlayId) -> Option<&Underlay> {
        self.underlays.get(&id)
    }

    pub fn add_underlay(&mut self, underlay: Underlay) -> UnderlayId {
        let id = UnderlayId(self.next_drafting());
        self.touch_key(Key::Underlay(id));
        self.underlays.insert(id, underlay);
        id
    }

    /// Insert an underlay with a known id (loading a file).
    pub fn insert_underlay(&mut self, id: UnderlayId, underlay: Underlay) {
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Underlay(id));
        self.underlays.insert(id, underlay);
    }

    pub fn set_underlay(&mut self, id: UnderlayId, underlay: Underlay) {
        if self.underlays.get(&id).is_some_and(|u| *u != underlay) {
            self.touch_key(Key::Underlay(id));
            self.underlays.insert(id, underlay);
        }
    }

    pub fn remove_underlay(&mut self, id: UnderlayId) {
        if self.underlays.contains_key(&id) {
            self.touch_key(Key::Underlay(id));
            self.underlays.remove(&id);
        }
    }
}
