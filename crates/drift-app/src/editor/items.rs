//! The sketch as drawable items, styled and tagged for the canvas.

use super::*;

impl Editor {
    /// Render items: each entity with its layer's (or its own) weight and line type, and either
    /// constraint-status colours or layer colours (adjusted for contrast with the theme).
    pub fn items_styled(&self, status_colors: bool, dark: bool) -> Vec<SketchItem> {
        self.items_tagged(status_colors, dark)
            .into_iter()
            .map(|t| t.item)
            .collect()
    }

    /// [`items_styled`](Self::items_styled), each item with the entity and layer it draws:
    /// what stacking fills and curves by object needs. Lit-up (hovered, picked) items come
    /// last.
    pub fn items_tagged(&self, status_colors: bool, dark: bool) -> Vec<TaggedItem> {
        let mut items = Vec::with_capacity(self.doc.len());
        let mut emphasized = Vec::new();
        let mut hidden = self.hidden_handles();
        hidden.extend(self.preview_hidden.iter().copied());
        let shown_ends = (!self.always_endpoints).then(|| self.touched_points());
        let (picked_walls, hovered_wall) = self.lit_walls();
        let wall_owners =
            (!picked_walls.is_empty() || hovered_wall.is_some()).then(|| self.wall_owners());
        // Layer by layer, in their draw order: one pass into each layer's list (sorting every
        // entity each frame showed on big drawings with many layers).
        let ranks = self.doc.layer_ranks();
        let mut by_rank: Vec<Vec<(EntityId, &drift_core::Entity)>> =
            vec![Vec::new(); ranks.len().max(1)];
        let mut last: Option<(drift_core::LayerId, usize)> = None;
        for (id, e) in self.doc.iter() {
            let rank = match last {
                Some((l, r)) if l == e.layer => r,
                _ => {
                    let r = ranks.get(&e.layer).copied().unwrap_or(0);
                    last = Some((e.layer, r));
                    r
                }
            };
            by_rank[rank].push((id, e));
        }
        let mut layer_of: Option<(drift_core::LayerId, &drift_core::Layer)> = None;
        for (id, entity) in by_rank.into_iter().flatten() {
            if hidden.contains(&id) {
                continue;
            }
            let role = self.index.role(id);
            // End and centre points of untouched curves stay out of the way.
            if let Some(shown) = &shown_ends
                && entity.kind.is_point()
                && matches!(role, Some(PointRole::Endpoint | PointRole::Center))
                && !shown.contains(&id)
            {
                continue;
            }
            let Some(shape) = self.index.shape(id) else {
                continue;
            };
            let dof = match self.analysis.definitions.get(&id) {
                Some(Definition::Under) => DofState::Under,
                Some(Definition::Full) => DofState::Full,
                Some(Definition::Over) => DofState::Over,
                None => DofState::Unknown,
            };
            let layer = match layer_of {
                Some((l, layer)) if l == entity.layer => layer,
                _ => {
                    let layer = self.doc.layer_of(id);
                    layer_of = Some((entity.layer, layer));
                    layer
                }
            };
            let look = entity.appearance;
            let color = if status_colors {
                None
            } else {
                look.color.or(layer.color).map(|c| contrast(c.0, dark))
            };
            let dof = if status_colors || dof == DofState::Over {
                dof
            } else {
                DofState::Unknown
            };
            let mut item = SketchItem::new(*shape)
                .with_kind(entity.style)
                .with_dof(dof)
                .with_look(
                    color.flatten(),
                    look.weight.unwrap_or(layer.weight).0,
                    look.line_type.unwrap_or(layer.line_type),
                );
            // End and centre points are small markers; handles and lone points full size.
            if role.is_some_and(|r| r != PointRole::Standalone && r != PointRole::Handle) {
                item = item.as_vertex();
            }
            let wall = wall_owners
                .as_ref()
                .and_then(|o| {
                    o.get(id)
                        .filter(|o| matches!(o, drift_core::Owner::Wall(_)))
                })
                .map(drift_core::Owner::wall);
            let emphasis = if self.selection.contains(&id)
                || wall.is_some_and(|w| picked_walls.contains(&w))
            {
                Emphasis::Selected
            } else if (wall.is_some() && wall == hovered_wall)
                || self.hovered == Some(id)
                || self.highlight.contains(&id)
                || self.snapped_point == Some(id)
                || self
                    .chain_preview
                    .as_ref()
                    .is_some_and(|(_, set)| set.contains(&id))
            {
                Emphasis::Hovered
            } else {
                Emphasis::Normal
            };
            let tagged = TaggedItem {
                entity: Some(id),
                layer: entity.layer,
                item,
            };
            if emphasis == Emphasis::Normal {
                items.push(tagged);
            } else {
                // Draw emphasized items last so they sit on top.
                emphasized.push(TaggedItem {
                    item: item.with_emphasis(emphasis),
                    ..tagged
                });
            }
        }
        // Copies of blocks (not the one being edited in place).
        let editing = self.block_edit.map(|e| e.instance);
        for (id, i) in self.doc.instances() {
            let Some(layer) = self.doc.layer(i.layer) else {
                continue;
            };
            if !layer.visible || Some(id) == editing {
                continue;
            }
            let emphasis = if self.selected_instances.contains(&id) {
                Emphasis::Selected
            } else if self.hovered_instance == Some(id) {
                Emphasis::Hovered
            } else {
                Emphasis::Normal
            };
            for item in self.doc.instance_items(id) {
                if matches!(item.shape, Shape::Point(_)) {
                    continue;
                }
                let look = item.appearance;
                let color = if status_colors {
                    None
                } else {
                    look.color.or(layer.color).map(|c| contrast(c.0, dark))
                };
                let sketch = SketchItem::new(item.shape)
                    .with_kind(item.style)
                    .with_look(
                        color.flatten(),
                        look.weight.unwrap_or(layer.weight).0,
                        look.line_type.unwrap_or(layer.line_type),
                    )
                    .with_emphasis(emphasis);
                let tagged = TaggedItem {
                    entity: None,
                    layer: i.layer,
                    item: sketch,
                };
                if emphasis == Emphasis::Normal {
                    items.push(tagged);
                } else {
                    emphasized.push(tagged);
                }
            }
        }
        items.extend(emphasized);
        items
    }
}

/// A render item with what it draws: the entity (none for a copy of a block) and its layer.
#[derive(Clone, Copy, Debug)]
pub struct TaggedItem {
    pub entity: Option<EntityId>,
    pub layer: drift_core::LayerId,
    pub item: SketchItem,
}

/// A layer colour that would be hard to see on the theme's background (WCAG contrast ratio
/// under 2:1, e.g. black on the dark theme) is shown as the theme's foreground instead
/// (`None`), like AutoCAD's colour 7.
pub(crate) fn contrast(c: [u8; 3], dark: bool) -> Option<[u8; 3]> {
    let linear = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let lum = 0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2]);
    let background: f64 = if dark { 0.012 } else { 0.95 };
    let ratio = (lum.max(background) + 0.05) / (lum.min(background) + 0.05);
    (ratio >= 2.0).then_some(c)
}
