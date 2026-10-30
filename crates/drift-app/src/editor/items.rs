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
        let look = self.item_look(status_colors, dark, false);
        let mut items = Vec::with_capacity(self.doc.len());
        let mut emphasized = Vec::new();
        let mut layer_of = None;
        for (id, entity) in self.in_draw_order() {
            if let Some(tagged) = self.entity_item(&look, id, entity, &mut layer_of) {
                if tagged.item.emphasis == Emphasis::Normal {
                    items.push(tagged);
                } else {
                    // Draw emphasized items last so they sit on top.
                    emphasized.push(tagged);
                }
            }
        }
        // Copies of blocks (not the one being edited in place).
        for (id, _) in self.doc.instances() {
            for tagged in self.instance_tagged(&look, id) {
                if tagged.item.emphasis == Emphasis::Normal {
                    items.push(tagged);
                } else {
                    emphasized.push(tagged);
                }
            }
        }
        items.extend(emphasized);
        items
    }

    /// What decides how items look this frame. `rest`: as if nothing were hovered (nothing
    /// lit up but what is picked), for lists kept from frame to frame (see
    /// [`ItemsCache`](crate::frame_items::ItemsCache)).
    pub(crate) fn item_look(&self, status_colors: bool, dark: bool, rest: bool) -> ItemLook {
        let mut hidden = self.hidden_handles();
        hidden.extend(self.preview_hidden.iter().copied());
        // (Looked up once per entity of the drawing: hashed, not searched.)
        let shown_ends = (!self.always_endpoints).then(|| {
            if rest {
                self.points_of(self.selection.iter().copied())
            } else {
                self.touched_points()
            }
            .into_iter()
            .collect()
        });
        let (picked_walls, hovered_wall) = self.lit_walls();
        let hovered_wall = hovered_wall.filter(|_| !rest);
        let wall_owners =
            (!picked_walls.is_empty() || hovered_wall.is_some()).then(|| self.wall_owners());
        ItemLook {
            status_colors,
            dark,
            rest,
            hidden,
            shown_ends,
            selected: self.selection.iter().copied().collect(),
            picked_walls,
            hovered_wall,
            wall_owners,
        }
    }

    /// The points of `ids`: points themselves, curves' defining points.
    pub(crate) fn points_of(&self, ids: impl IntoIterator<Item = EntityId>) -> BTreeSet<EntityId> {
        let mut out = BTreeSet::new();
        for id in ids {
            if let Some(e) = self.doc.get(id) {
                if e.kind.is_point() {
                    out.insert(id);
                } else {
                    out.extend(e.kind.points());
                }
            }
        }
        out
    }

    /// The sketch's entities layer by layer, in their draw order: one pass into each layer's
    /// list (sorting every entity each frame showed on big drawings with many layers).
    pub(crate) fn in_draw_order(&self) -> Vec<(EntityId, &drift_core::Entity)> {
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
        by_rank.into_iter().flatten().collect()
    }

    /// One entity's item, if it shows. `layer_of` remembers the last layer looked up.
    pub(crate) fn entity_item<'a>(
        &'a self,
        look: &ItemLook,
        id: EntityId,
        entity: &drift_core::Entity,
        layer_of: &mut Option<(drift_core::LayerId, &'a drift_core::Layer)>,
    ) -> Option<TaggedItem> {
        self.entity_item_with(look, id, entity, layer_of, true)
    }

    /// [`entity_item`](Self::entity_item) as if every end and centre point showed.
    pub(crate) fn entity_item_any_ends<'a>(
        &'a self,
        look: &ItemLook,
        id: EntityId,
        entity: &drift_core::Entity,
        layer_of: &mut Option<(drift_core::LayerId, &'a drift_core::Layer)>,
    ) -> Option<TaggedItem> {
        self.entity_item_with(look, id, entity, layer_of, false)
    }

    /// Whether `look` keeps an end or centre point out of the way (see
    /// [`entity_item`](Self::entity_item)).
    pub(crate) fn end_hidden(
        &self,
        look: &ItemLook,
        id: EntityId,
        entity: &drift_core::Entity,
    ) -> bool {
        look.shown_ends.as_ref().is_some_and(|shown| {
            entity.kind.is_point()
                && matches!(
                    self.index.role(id),
                    Some(PointRole::Endpoint | PointRole::Center)
                )
                && !shown.contains(&id)
        })
    }

    fn entity_item_with<'a>(
        &'a self,
        look: &ItemLook,
        id: EntityId,
        entity: &drift_core::Entity,
        layer_of: &mut Option<(drift_core::LayerId, &'a drift_core::Layer)>,
        ends: bool,
    ) -> Option<TaggedItem> {
        if look.hidden.contains(&id) {
            return None;
        }
        let role = self.index.role(id);
        // End and centre points of untouched curves stay out of the way.
        if ends
            && let Some(shown) = &look.shown_ends
            && entity.kind.is_point()
            && matches!(role, Some(PointRole::Endpoint | PointRole::Center))
            && !shown.contains(&id)
        {
            return None;
        }
        let shape = self.index.shape(id)?;
        let dof = match self.analysis().definitions.get(&id) {
            Some(Definition::Under) => DofState::Under,
            Some(Definition::Full) => DofState::Full,
            Some(Definition::Over) => DofState::Over,
            None => DofState::Unknown,
        };
        let layer = match *layer_of {
            Some((l, layer)) if l == entity.layer => layer,
            _ => {
                let layer = self.doc.layer_of(id);
                *layer_of = Some((entity.layer, layer));
                layer
            }
        };
        let look_of = entity.appearance;
        let color = if look.status_colors {
            None
        } else {
            look_of
                .color
                .or(layer.color)
                .map(|c| contrast(c.0, look.dark))
        };
        let dof = if look.status_colors || dof == DofState::Over {
            dof
        } else {
            DofState::Unknown
        };
        let mut item = SketchItem::new(*shape)
            .with_kind(entity.style)
            .with_dof(dof)
            .with_look(
                color.flatten(),
                look_of.weight.unwrap_or(layer.weight).0,
                look_of.line_type.unwrap_or(layer.line_type),
            );
        // End and centre points are small markers; handles and lone points full size.
        if role.is_some_and(|r| r != PointRole::Standalone && r != PointRole::Handle) {
            item = item.as_vertex();
        }
        let wall = look
            .wall_owners
            .as_ref()
            .and_then(|o| {
                o.get(id)
                    .filter(|o| matches!(o, drift_core::Owner::Wall(_)))
            })
            .map(drift_core::Owner::wall);
        let emphasis = if look.selected.contains(&id)
            || wall.is_some_and(|w| look.picked_walls.contains(&w))
        {
            Emphasis::Selected
        } else if !look.rest
            && ((wall.is_some() && wall == look.hovered_wall)
                || self.hovered == Some(id)
                || self.highlight.contains(&id)
                || self.snapped_point == Some(id)
                || self
                    .chain_preview
                    .as_ref()
                    .is_some_and(|(_, set)| set.contains(&id)))
        {
            Emphasis::Hovered
        } else {
            Emphasis::Normal
        };
        if emphasis != Emphasis::Normal {
            item = item.with_emphasis(emphasis);
        }
        Some(TaggedItem {
            entity: Some(id),
            layer: entity.layer,
            item,
        })
    }

    /// A copy of a block's items (none while it is edited in place or its layer is hidden).
    pub(crate) fn instance_tagged(
        &self,
        look: &ItemLook,
        id: drift_core::InstanceId,
    ) -> Vec<TaggedItem> {
        let editing = self.block_edit.map(|e| e.instance);
        let Some(i) = self.doc.instance(id) else {
            return Vec::new();
        };
        let Some(layer) = self.doc.layer(i.layer) else {
            return Vec::new();
        };
        if !layer.visible || Some(id) == editing {
            return Vec::new();
        }
        let emphasis = if self.selected_instances.contains(&id) {
            Emphasis::Selected
        } else if !look.rest && self.hovered_instance == Some(id) {
            Emphasis::Hovered
        } else {
            Emphasis::Normal
        };
        let mut out = Vec::new();
        for item in self.doc.instance_items(id) {
            if matches!(item.shape, Shape::Point(_)) {
                continue;
            }
            let look_of = item.appearance;
            let color = if look.status_colors {
                None
            } else {
                look_of
                    .color
                    .or(layer.color)
                    .map(|c| contrast(c.0, look.dark))
            };
            let sketch = SketchItem::new(item.shape)
                .with_kind(item.style)
                .with_look(
                    color.flatten(),
                    look_of.weight.unwrap_or(layer.weight).0,
                    look_of.line_type.unwrap_or(layer.line_type),
                )
                .with_emphasis(emphasis);
            out.push(TaggedItem {
                entity: None,
                layer: i.layer,
                item: sketch,
            });
        }
        out
    }
}

/// What decides how the sketch's items look (see [`Editor::item_look`]).
pub(crate) struct ItemLook {
    status_colors: bool,
    dark: bool,
    rest: bool,
    hidden: BTreeSet<EntityId>,
    shown_ends: Option<HashSet<EntityId>>,
    /// The editor's selection, for the same lookup.
    selected: HashSet<EntityId>,
    picked_walls: BTreeSet<drift_core::WallId>,
    hovered_wall: Option<drift_core::WallId>,
    wall_owners: Option<std::rc::Rc<drift_core::Owners>>,
}

impl ItemLook {
    /// Entities hidden (handles not showing, a tool's preview standing in for them).
    pub(crate) fn hidden(&self) -> &BTreeSet<EntityId> {
        &self.hidden
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
