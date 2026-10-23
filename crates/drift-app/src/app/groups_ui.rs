//! Groups in the app: frames around picked, hovered and open groups on the canvas, and the
//! group section of the inspector.

use drift_core::{Bounds, GroupId};

use crate::*;

impl DriftApp {
    /// A group's members' extent on screen.
    fn group_rect(&self, map: &ScreenMap, g: GroupId) -> Option<egui::Rect> {
        let group = self.editor.doc.group(g)?;
        let mut b = Bounds::EMPTY;
        for id in &group.members {
            if let Some(shape) = self.editor.index.shape(*id) {
                let s = shape.bounds();
                b.include(s.min);
                b.include(s.max);
            }
        }
        if b.is_empty() {
            return None;
        }
        Some(egui::Rect::from_two_pos(map.to_screen(b.min), map.to_screen(b.max)).expand(7.0))
    }

    fn frame(
        painter: &egui::Painter,
        rect: egui::Rect,
        color: egui::Color32,
        dashed: bool,
        label: &str,
    ) {
        let stroke = egui::Stroke::new(1.0, color);
        let corners = vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.left_top(),
        ];
        if dashed {
            crate::overlay::dashed(painter, &corners, stroke, 5.0, 3.0);
        } else {
            painter.add(egui::Shape::line(corners, stroke));
        }
        if !label.is_empty() {
            let galley = painter.layout_no_wrap(
                label.to_owned(),
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
            let tag = egui::Rect::from_min_size(
                rect.left_top() - egui::vec2(0.0, galley.size().y + 4.0),
                galley.size() + egui::vec2(10.0, 4.0),
            );
            painter.rect_filled(tag, 4.0, color);
            painter.galley(tag.min + egui::vec2(5.0, 2.0), galley, egui::Color32::WHITE);
        }
    }

    /// Frames: the open group (solid), picked groups (dashed, named) and the hovered group.
    pub(crate) fn paint_groups(&self, painter: &egui::Painter, map: &ScreenMap, dark: bool) {
        let ed = &self.editor;
        let accent = theme::accent(dark);
        if let Some(g) = ed.open_group
            && let Some(rect) = self.group_rect(map, g)
        {
            let name = ed.doc.group(g).map_or("", |x| x.name.as_str());
            Self::frame(
                painter,
                rect,
                accent,
                false,
                &format!("Editing {name} · Esc to finish"),
            );
        }
        let mut shown = std::collections::BTreeSet::new();
        for id in &ed.selection {
            if let Some(g) = ed.doc.group_of(*id)
                && Some(g) != ed.open_group
                && shown.insert(g)
                && let Some(rect) = self.group_rect(map, g)
            {
                let name = ed.doc.group(g).map_or("", |x| x.name.as_str());
                Self::frame(painter, rect, accent, true, name);
            }
        }
        if let Some(h) = ed.hovered
            && let Some(g) = ed.doc.group_of(h)
            && Some(g) != ed.open_group
            && !shown.contains(&g)
            && let Some(rect) = self.group_rect(map, g)
        {
            Self::frame(painter, rect, accent.gamma_multiply(0.55), true, "");
        }
    }

    /// The inspector's group section: shown for a picked group, or the open one.
    pub(crate) fn group_inspector(&mut self, ui: &mut egui::Ui) {
        if self.editor.block_edit.is_some() {
            return;
        }
        let Some(g) = self.editor.selected_group().or(self.editor.open_group) else {
            return;
        };
        let Some(group) = self.editor.doc.group(g).cloned() else {
            return;
        };
        let open = self.editor.open_group == Some(g);
        theme::section(ui, if open { "Editing group" } else { "Group" });
        ui.label(
            egui::RichText::new("A group is one thing; a block is a thing you reuse.")
                .small()
                .weak(),
        );
        let mut name = group.name.clone();
        theme::row(ui, "Name", |ui| {
            if ui
                .add(
                    egui::TextEdit::singleline(&mut name).desired_width(ui.available_width() - 8.0),
                )
                .changed()
            {
                // Typing merges into one undo step.
                self.editor.doc.begin();
                self.editor.doc.rename_group(g, &name);
                let tx = self.editor.doc.commit("Rename group");
                if self.editor.history.undo_label() == Some("Rename group") {
                    self.editor.history.merge_into_last(tx);
                } else {
                    self.editor.history.push(tx);
                }
            }
        });
        theme::row(ui, "Members", |ui| {
            let n = group.members.len();
            ui.label(format!("{n} {}", if n == 1 { "item" } else { "items" }));
        });
        ui.horizontal_wrapped(|ui| {
            if open {
                if ui.button("Finish editing").clicked() {
                    self.editor.open_group = None;
                    self.editor.selection = group.members.clone();
                }
            } else if ui
                .button("Edit in place")
                .on_hover_text("Pick its members one by one (or double-click the group)")
                .clicked()
            {
                self.editor.open_group = Some(g);
                self.editor.selection.clear();
            }
            if ui
                .button("Ungroup")
                .on_hover_text(app::panels::shortcut("G", true))
                .clicked()
            {
                self.editor.selection = group.members.clone();
                self.editor.ungroup();
            }
            if ui
                .button("Make block")
                .on_hover_text("Make it a block, to reuse: named after it, with its fills")
                .clicked()
            {
                let now = ui.input(|i| i.time);
                self.block_from_group(g, now);
            }
        });
    }
}
