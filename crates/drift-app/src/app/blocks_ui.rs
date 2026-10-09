//! Blocks in the app: the Blocks tab (library), the inspector for picked copies, and the
//! block-editing banner.

use drift_core::{BlockId, InstanceId, LayerId, Shape};

use crate::*;

/// A block's thumbnail: its curves fitted into `size`.
fn thumbnail(ui: &mut egui::Ui, block: &drift_core::Block, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let visuals = ui.visuals();
    ui.painter()
        .rect_filled(rect, 6.0, visuals.extreme_bg_color);
    let b = block.bounds();
    if b.is_empty() {
        return response;
    }
    let extent = (b.max - b.min).max_element().max(1e-9);
    let scale = (size as f64 - 10.0) / extent;
    let c = b.center();
    let to = |p: DVec2| {
        let d = (p - c) * scale;
        rect.center() + egui::vec2(d.x as f32, -d.y as f32)
    };
    let stroke = egui::Stroke::new(1.0, visuals.text_color());
    for item in &block.items {
        match item.shape {
            Shape::Line { a, b } => {
                ui.painter().line_segment([to(a), to(b)], stroke);
            }
            Shape::Circle { center, radius } => {
                ui.painter()
                    .circle_stroke(to(center), (radius * scale) as f32, stroke);
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let n = 16;
                let pts: Vec<egui::Pos2> =
                    (0..=n)
                        .map(|k| {
                            to(center
                                + DVec2::from_angle(start + sweep * k as f64 / n as f64) * radius)
                        })
                        .collect();
                ui.painter().add(egui::Shape::line(pts, stroke));
            }
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                let pts =
                    drift_core::shape::flatten_ellipse(center, major, minor, start, sweep, 24)
                        .into_iter()
                        .map(to)
                        .collect();
                ui.painter().add(egui::Shape::line(pts, stroke));
            }
            Shape::Bezier { p } => {
                let pts = drift_core::shape::flatten_bezier(&p, 16)
                    .into_iter()
                    .map(to)
                    .collect();
                ui.painter().add(egui::Shape::line(pts, stroke));
            }
            Shape::Point(_) => {}
        }
    }
    response
}

impl DriftApp {
    /// Pick a block to place: the Place block tool, with that block.
    pub(crate) fn start_placing(&mut self, block: BlockId) {
        self.editor.place_block = Some(block);
        self.set_tool(ToolKind::Block);
    }

    /// The Blocks tab.
    pub(crate) fn blocks_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.blocks_contents(ui));
    }

    fn blocks_contents(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        let blocks: Vec<(BlockId, drift_core::Block)> = self
            .editor
            .doc
            .blocks()
            .map(|(id, b)| (id, b.clone()))
            .collect();
        let counts = self.editor.copy_counts();
        if !self.editor.selection.is_empty() {
            theme::section(ui, "New block");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_block_name)
                        .hint_text(format!("Block {}", blocks.len() + 1))
                        .desired_width(ui.available_width() - 70.0),
                );
                if ui
                    .button("Make")
                    .on_hover_text("Make a block from the selection; a copy takes its place")
                    .clicked()
                {
                    let name = std::mem::take(&mut self.new_block_name);
                    if let Err(e) = self.editor.make_block(&name) {
                        self.set_status(e, true, now);
                    }
                }
            });
        }
        theme::section(ui, &format!("Blocks · {}", blocks.len()));
        if blocks.is_empty() {
            ui.label(
                egui::RichText::new(
                    "A block is drawn once and placed many times: edit it and every copy \
                     updates. Select geometry and make one, or start from the furniture \
                     library.",
                )
                .weak(),
            );
        }
        let mut place = None;
        let mut delete = None;
        let mut rename = None;
        for (id, block) in &blocks {
            let picked =
                self.tool.kind() == ToolKind::Block && self.editor.place_block == Some(*id);
            let row = egui::Frame::new()
                .fill(if picked {
                    theme::accent(ui.visuals().dark_mode).gamma_multiply(0.18)
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(8.0)
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let thumb = thumbnail(ui, block, 44.0);
                        ui.vertical(|ui| {
                            let name = egui::RichText::new(&block.name).family(theme::bold());
                            if ui
                                .add(egui::Label::new(name).sense(egui::Sense::click()))
                                .clicked()
                            {
                                place = Some(*id);
                            }
                            let n = counts.get(id).copied().unwrap_or(0);
                            ui.label(
                                egui::RichText::new(match n {
                                    0 => "Not placed".to_owned(),
                                    1 => "1 copy".to_owned(),
                                    n => format!("{n} copies"),
                                })
                                .small()
                                .weak(),
                            );
                        });
                        if thumb.clicked() {
                            place = Some(*id);
                        }
                    });
                });
            let response = row
                .response
                .interact(egui::Sense::click())
                .on_hover_text("Click to place copies (I)");
            if response.clicked() {
                place = Some(*id);
            }
            response.context_menu(|ui| {
                if ui.button("Place copies").clicked() {
                    place = Some(*id);
                    ui.close();
                }
                if ui.button("Select all copies").clicked() {
                    self.editor.select_copies(*id);
                    ui.close();
                }
                let mut name = block.name.clone();
                ui.horizontal(|ui| {
                    ui.label("Name");
                    if ui.text_edit_singleline(&mut name).lost_focus() && name.trim() != block.name
                    {
                        rename = Some((*id, name.trim().to_owned()));
                    }
                });
                ui.separator();
                if ui
                    .button(
                        egui::RichText::new("Delete block and copies")
                            .color(egui::Color32::from_rgb(0xff, 0x5d, 0x5d)),
                    )
                    .clicked()
                {
                    delete = Some(*id);
                    ui.close();
                }
            });
        }
        if let Some(id) = place {
            self.start_placing(id);
        }
        if let Some((id, name)) = rename
            && !name.is_empty()
            && self.editor.doc.block_named(&name).is_none_or(|b| b == id)
            && let Some(mut b) = self.editor.doc.block(id).cloned()
        {
            b.name = name;
            self.editor.doc.begin();
            self.editor.doc.set_block(id, b);
            self.editor.commit("Rename block");
        }
        if let Some(id) = delete {
            self.editor.doc.begin();
            self.editor.doc.remove_block(id);
            self.editor.commit("Delete block");
            self.editor.selected_instances.clear();
            if self.editor.place_block == Some(id) {
                self.editor.place_block = None;
            }
        }
        ui.add_space(8.0);
        let missing = crate::blocks::starter_blocks()
            .iter()
            .filter(|b| self.editor.doc.block_named(&b.name).is_none())
            .count();
        if missing > 0
            && ui
                .button("Add furniture library")
                .on_hover_text("Chair, desk, bed, sofa, WC, basin and a 900 door, in millimetres")
                .clicked()
        {
            let n = self.editor.add_starter_blocks();
            self.set_status(format!("Added {n} furniture blocks"), false, now);
        }
    }

    /// The inspector for picked copies of blocks. Returns true if any are picked.
    pub(crate) fn instance_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        if !self.editor.selection.is_empty() || self.editor.selected_instances.is_empty() {
            return false;
        }
        let picked: Vec<(InstanceId, drift_core::Instance)> = self.editor.picked_instances();
        let Some(&(first_id, first)) = picked.first() else {
            return false;
        };
        let doc = &self.editor.doc;
        let name = doc
            .block(first.block)
            .map_or("?".to_owned(), |b| b.name.clone());
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(if picked.len() == 1 {
                name.clone()
            } else {
                format!("{} copies", picked.len())
            })
            .family(theme::bold())
            .size(15.0),
        );
        ui.label(egui::RichText::new("Block copy").small().weak());
        let blocks: Vec<(BlockId, String)> =
            doc.blocks().map(|(id, b)| (id, b.name.clone())).collect();
        let layers: Vec<(LayerId, String)> =
            doc.layers().map(|(id, l)| (id, l.name.clone())).collect();
        let units = doc.units();
        theme::section(ui, "Block");
        let mut replace = None;
        egui::ComboBox::from_id_salt("instance_block")
            .selected_text(&name)
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for (id, n) in &blocks {
                    if ui.selectable_label(*id == first.block, n).clicked() {
                        replace = Some(*id);
                    }
                }
            });
        if let Some(b) = replace {
            self.editor.replace_block(b);
        }
        let mut edited = first;
        if picked.len() == 1 {
            theme::section(ui, "Placement");
            let unit = units.length.mm_per_unit();
            let length_row = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
                theme::row(ui, label, |ui| {
                    let mut shown = *v / unit;
                    if ui
                        .add(
                            egui::DragValue::new(&mut shown)
                                .speed(1.0)
                                .max_decimals(4)
                                .suffix(format!(" {}", units.length.symbol())),
                        )
                        .changed()
                    {
                        *v = shown * unit;
                    }
                });
            };
            length_row(ui, "X", &mut edited.at.x);
            length_row(ui, "Y", &mut edited.at.y);
            theme::row(ui, "Angle", |ui| {
                ui.add(
                    egui::DragValue::new(&mut edited.angle)
                        .speed(1.0)
                        .max_decimals(4)
                        .suffix("°"),
                );
                for a in [0.0, 90.0, 180.0, 270.0] {
                    if ui.small_button(format!("{a}")).clicked() {
                        edited.angle = a;
                    }
                }
            });
            theme::row(ui, "Scale", |ui| {
                ui.add(
                    egui::DragValue::new(&mut edited.scale)
                        .speed(0.01)
                        .range(0.001..=1000.0)
                        .prefix("× "),
                );
                ui.checkbox(&mut edited.mirror, "Mirror");
            });
        }
        theme::row(ui, "Layer", |ui| {
            let current = layers
                .iter()
                .find(|(id, _)| *id == first.layer)
                .map_or("?".to_owned(), |(_, n)| n.clone());
            egui::ComboBox::from_id_salt("instance_layer")
                .selected_text(current)
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    for (id, n) in &layers {
                        ui.selectable_value(&mut edited.layer, *id, n);
                    }
                });
        });
        if edited != first {
            self.editor.doc.begin();
            if picked.len() == 1 {
                let _ = self.editor.doc.set_instance(first_id, edited);
            } else {
                for (id, mut i) in picked.clone() {
                    i.layer = edited.layer;
                    let _ = self.editor.doc.set_instance(id, i);
                }
            }
            self.editor.commit("Edit block copy");
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if picked.len() == 1
                && ui
                    .button("Edit block")
                    .on_hover_text(
                        "Change the block in place: every copy updates (or double-click it)",
                    )
                    .clicked()
            {
                self.editor.edit_block(first_id);
            }
            if ui
                .button("Explode")
                .on_hover_text("Turn the copy back into ordinary geometry")
                .clicked()
            {
                self.editor.explode_selected();
            }
            if ui.button("Select all copies").clicked() {
                self.editor.select_copies(first.block);
            }
        });
        ui.add_space(10.0);
        if ui
            .add_sized(
                [ui.available_width(), 26.0],
                egui::Button::new(
                    egui::RichText::new("Delete").color(egui::Color32::from_rgb(0xff, 0x5d, 0x5d)),
                ),
            )
            .clicked()
        {
            self.editor.delete_selection();
        }
        true
    }

    /// While a block is edited in place: what it is and how to finish. Returns true if so.
    pub(crate) fn block_edit_banner(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(edit) = self.editor.block_edit else {
            return false;
        };
        let name = self
            .editor
            .doc
            .block(edit.block)
            .map_or_else(String::new, |b| b.name.clone());
        theme::section(ui, "Editing block");
        ui.label(egui::RichText::new(&name).family(theme::bold()).size(15.0));
        ui.label(
            egui::RichText::new(
                "Change it with any tool; what you draw becomes part of it. Every copy updates \
                 when you finish.",
            )
            .small()
            .weak(),
        );
        if ui
            .add_sized(
                [ui.available_width(), 26.0],
                egui::Button::new("Finish editing"),
            )
            .on_hover_text("Esc")
            .clicked()
        {
            self.editor.finish_block_edit();
        }
        ui.separator();
        false
    }
}
