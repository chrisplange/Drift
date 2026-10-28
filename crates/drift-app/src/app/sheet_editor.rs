//! Working on sheets: the tabs over the canvas (the model and each sheet), the sheet tools
//! in the rail, adding a view from a sheet (drawn with the View tool in the model, then put
//! on the sheet), and the Inspector for what is picked on a sheet (a view, or the sheet
//! itself).

use drift_core::{Bounds, PlacedView, Sheet, SheetId, ViewId};

use crate::icons::{self, Glyph};
use crate::overlay::ScreenMap;
use crate::*;

/// What is picked on the sheet shown: one of its views, or the sheet itself (its paper,
/// title block and revisions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SheetPick {
    #[default]
    Paper,
    View,
}

/// A sheet as it is listed and named on its tab: "A-101  Ground floor", or just the name
/// while it has no number.
pub(crate) fn sheet_label(sheet: &Sheet) -> String {
    let number = sheet.fields.number.trim();
    if number.is_empty() {
        sheet.name.clone()
    } else {
        format!("{number}  {}", sheet.name)
    }
}

/// The number after `last` ("A-101" → "A-102", "S-09" → "S-10"); "A-101" for the first.
pub(crate) fn next_sheet_number(last: Option<&str>) -> String {
    let Some(last) = last.map(str::trim).filter(|s| !s.is_empty()) else {
        return "A-101".to_owned();
    };
    let digits = last.len() - last.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return format!("{last}-2");
    }
    let (stem, n) = last.split_at(last.len() - digits);
    let next = n.parse::<u64>().map_or(1, |n| n + 1);
    format!("{stem}{next:0digits$}")
}

/// Where a placed view's drawing is on the paper: `model` (the sketch's extent) seen through
/// the view, cut at its frame.
pub(crate) fn drawing_extent(view: &PlacedView, model: Bounds) -> Bounds {
    let f = view.frame.bounds();
    if model.is_empty() {
        return f;
    }
    let to = |w: DVec2| view.frame.at + (w - view.center) / view.scale;
    Bounds {
        min: to(model.min).max(f.min),
        max: to(model.max).min(f.max),
    }
}

/// What a sheet's right-click menu asked for.
pub(crate) enum SheetMenu {
    /// Duplicate it, with copies of its views or empty.
    Duplicate(bool),
    /// Move it to this position.
    Move(usize),
    Delete,
}

/// The right-click menu of a sheet (a canvas tab or a row in the Sheets list), at position
/// `at` of `count` sheets.
pub(crate) fn sheet_menu(ui: &mut egui::Ui, at: usize, count: usize) -> Option<SheetMenu> {
    let mut chose = None;
    if ui
        .button("Duplicate with views")
        .on_hover_text("A copy after it, with copies of its views")
        .clicked()
    {
        chose = Some(SheetMenu::Duplicate(true));
    }
    if ui
        .button("Duplicate empty")
        .on_hover_text("A copy after it with its paper and title block, but no views")
        .clicked()
    {
        chose = Some(SheetMenu::Duplicate(false));
    }
    ui.separator();
    if ui
        .add_enabled(at > 0, egui::Button::new("Move earlier"))
        .clicked()
    {
        chose = Some(SheetMenu::Move(at - 1));
    }
    if ui
        .add_enabled(at + 1 < count, egui::Button::new("Move later"))
        .clicked()
    {
        chose = Some(SheetMenu::Move(at + 1));
    }
    ui.separator();
    if ui.button("Delete sheet").clicked() {
        chose = Some(SheetMenu::Delete);
    }
    if chose.is_some() {
        ui.close();
    }
    chose
}

/// The position a dragged sheet would take, dropped at `pointer` along a row of sheets
/// (`spots`: each one's start and end along it): after every other sheet whose middle it is
/// past.
pub(crate) fn drop_index(spots: &[(SheetId, f32, f32)], dragged: SheetId, pointer: f32) -> usize {
    spots
        .iter()
        .filter(|(s, a, b)| *s != dragged && (a + b) / 2.0 < pointer)
        .count()
}

/// Where along the row to mark position `to` for a dragged sheet: just after the other sheet
/// before it, or just before the first.
pub(crate) fn gap_at(spots: &[(SheetId, f32, f32)], dragged: SheetId, to: usize) -> f32 {
    let others: Vec<(f32, f32)> = spots
        .iter()
        .filter(|(s, _, _)| *s != dragged)
        .map(|(_, a, b)| (*a, *b))
        .collect();
    match to.checked_sub(1).and_then(|k| others.get(k)) {
        Some((_, end)) => end + 1.0,
        None => others.first().map_or(0.0, |(start, _)| start - 1.0),
    }
}

/// Whether a paper point is on a placed view's drawing, rather than on the empty paper round
/// it.
pub(crate) fn on_drawing(view: &PlacedView, model: Bounds, at: DVec2) -> bool {
    let b = drawing_extent(view, model);
    at.x >= b.min.x && at.x <= b.max.x && at.y >= b.min.y && at.y <= b.max.y
}

/// Where the views on `sheet` take up paper, titles included: their frames, or, for one
/// filling the drawing area, just its drawing.
fn occupied(sheet: &Sheet, placed: &[PlacedView], model: Bounds) -> Vec<Bounds> {
    placed
        .iter()
        .map(|v| {
            let mut b = if drift_draft::sheet::fills_drawing_area(sheet, v.frame.bounds()) {
                drawing_extent(v, model)
            } else {
                v.frame.bounds()
            };
            if v.title {
                b.min.y -= drift_draft::sheet::VIEW_TITLE_DEPTH;
            }
            b
        })
        .collect()
}

/// The middle of a free spot for a frame of `size` (with its title under it) in `area`,
/// clear of `taken` by `gap`: the first found scanning from the top left.
fn free_spot(area: Bounds, size: DVec2, taken: &[Bounds], gap: f64) -> Option<DVec2> {
    let title = drift_draft::sheet::VIEW_TITLE_DEPTH;
    let step = 4.0;
    let mut y = area.max.y - gap - size.y * 0.5;
    while y - size.y * 0.5 - title - gap >= area.min.y {
        let mut x = area.min.x + gap + size.x * 0.5;
        while x + size.x * 0.5 + gap <= area.max.x {
            let at = DVec2::new(x, y);
            let r = Bounds {
                min: at - size * 0.5 - DVec2::new(gap, gap + title),
                max: at + size * 0.5 + DVec2::splat(gap),
            };
            if !taken.iter().any(|t| t.intersects(&r)) {
                return Some(at);
            }
            x += step;
        }
        y -= step;
    }
    None
}

/// The middle of a free spot on `sheet` for a frame of `size` (paper mm), clear of the views
/// already there (`placed`, whose drawings show `model`), if there is one.
pub(crate) fn free_spot_on_sheet(
    sheet: &Sheet,
    placed: &[PlacedView],
    model: Bounds,
    size: DVec2,
) -> Option<DVec2> {
    let area = drift_draft::sheet::view_area(sheet);
    free_spot(area, size, &occupied(sheet, placed, model), 6.0)
}

/// The scale and the place on `sheet` for a view whose crop is `crop` (world mm): the
/// largest standard scale at which its frame finds free paper, and the spot.
fn fit_on_sheet(
    crop: DVec2,
    sheet: &Sheet,
    placed: &[PlacedView],
    model: Bounds,
    imperial: bool,
) -> Option<(f64, DVec2)> {
    let area = drift_draft::sheet::view_area(sheet);
    let taken = occupied(sheet, placed, model);
    drift_core::sheets::standard_scales(imperial)
        .into_iter()
        .find_map(|scale| {
            let size = crop / scale;
            (size.x <= area.size().x * 0.9 && size.y <= area.size().y * 0.9)
                .then(|| free_spot(area, size, &taken, 6.0))
                .flatten()
                .map(|at| (scale, at))
        })
}

impl DriftApp {
    /// Show a sheet (or the model, with `None`) on the canvas.
    pub(crate) fn show_sheet(&mut self, id: Option<SheetId>) {
        self.add_view_on = None;
        match id {
            Some(id) => {
                if self.active_sheet != Some(id) || !self.sheet_preview {
                    self.preview.zoom = 0.0;
                    self.sheet_pick = SheetPick::Paper;
                }
                self.active_sheet = Some(id);
                self.sheet_preview = true;
            }
            None => self.sheet_preview = false,
        }
    }

    /// A new sheet, numbered after the last, shown with its settings in the Inspector.
    pub(crate) fn new_sheet(&mut self) {
        let id = self.add_sheet();
        self.show_sheet(Some(id));
        self.sheet_pick = SheetPick::Paper;
        self.show_panel(crate::dock::Panel::Inspector);
    }

    /// The tabs over the canvas: the model, each sheet, and + for a new one.
    pub(crate) fn canvas_tabs(&mut self, ui: &mut egui::Ui) {
        let dark = ui.visuals().dark_mode;
        let n = theme::neutrals(dark);
        let height = 30.0;
        let (bar, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(bar, 0.0, n.panel);
        ui.painter().hline(
            bar.x_range(),
            bar.bottom() - 0.5,
            egui::Stroke::new(1.0, n.border),
        );
        let sheets: Vec<(SheetId, String)> = self
            .editor
            .doc
            .sheets()
            .map(|(id, s)| (id, sheet_label(s)))
            .collect();
        let on_sheet = self.sheet_preview;
        let current = self.current_sheet();
        let mut tabs: Vec<(Option<SheetId>, String)> = vec![(None, "Model".to_owned())];
        tabs.extend(sheets.into_iter().map(|(id, l)| (Some(id), l)));
        let font = egui::FontId::proportional(theme::size::BODY);
        let mut x = bar.left() + 8.0;
        let mut choose = None;
        let mut delete = None;
        let mut duplicate = None;
        let mut moved = None;
        // Sheet tabs (their middles across), and a tab being dragged to a new place.
        let mut spots: Vec<(SheetId, f32, f32)> = Vec::new();
        let mut dragging = None;
        let mut dropped = None;
        let count = tabs.len() - 1;
        for (k, (id, label)) in tabs.iter().enumerate() {
            let active = match id {
                None => !on_sheet,
                Some(id) => on_sheet && current == Some(*id),
            };
            let family = if active {
                theme::bold()
            } else {
                egui::FontFamily::Proportional
            };
            let galley = ui.painter().layout_no_wrap(
                label.clone(),
                egui::FontId::new(font.size, family.clone()),
                n.text,
            );
            let w = (galley.size().x + 28.0).min(220.0);
            if x + w > bar.right() - 40.0 {
                break;
            }
            let r = egui::Rect::from_min_size(
                egui::pos2(x, bar.top() + 4.0),
                egui::vec2(w, height - 4.0),
            );
            x += w + 2.0;
            let sense = if id.is_some() {
                egui::Sense::click_and_drag()
            } else {
                egui::Sense::click()
            };
            let response = ui.interact(r, ui.id().with(("canvas_tab", k)), sense);
            if let Some(sheet) = *id {
                spots.push((sheet, r.left(), r.right()));
                if response.dragged() {
                    dragging = Some(sheet);
                }
                if response.drag_stopped() {
                    dropped = Some(sheet);
                }
            }
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label)
            });
            // The active tab joins the canvas below it.
            // (The sheet's pasteboard, or the model's canvas.)
            let canvas = match (on_sheet, dark) {
                (true, true) => egui::Color32::from_rgb(0x2a, 0x2c, 0x31),
                (true, false) => egui::Color32::from_rgb(0xd5, 0xd8, 0xde),
                (false, true) => egui::Color32::from_rgb(0x1b, 0x1d, 0x21),
                (false, false) => egui::Color32::from_rgb(0xfb, 0xfb, 0xfa),
            };
            if active {
                let shape = egui::epaint::RectShape::filled(
                    r.expand2(egui::vec2(0.0, 0.5)),
                    egui::CornerRadius {
                        nw: theme::radius::MENU,
                        ne: theme::radius::MENU,
                        sw: 0,
                        se: 0,
                    },
                    canvas,
                );
                ui.painter().add(shape);
                ui.painter().line_segment(
                    [
                        r.left_top() + egui::vec2(6.0, 0.0),
                        r.right_top() - egui::vec2(6.0, 0.0),
                    ],
                    egui::Stroke::new(2.0, theme::accent(dark)),
                );
            } else if response.hovered() {
                ui.painter().rect_filled(
                    r.shrink2(egui::vec2(0.0, 2.0)),
                    theme::radius::CONTROL,
                    n.hover,
                );
            }
            let color = if active { n.text } else { n.secondary };
            let mut job = egui::text::LayoutJob::simple_singleline(
                label.clone(),
                egui::FontId::new(font.size, family),
                color,
            );
            job.wrap.max_width = w - 24.0;
            job.wrap.max_rows = 1;
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(
                egui::pos2(r.left() + 14.0, r.center().y - galley.size().y / 2.0),
                galley,
                color,
            );
            if response.clicked() {
                choose = Some(*id);
            }
            if let Some(sheet) = *id {
                let at = k - 1;
                response
                    .on_hover_text("Drag to reorder; right-click for more")
                    .context_menu(|ui| {
                        if let Some(d) = sheet_menu(ui, at, count) {
                            match d {
                                SheetMenu::Duplicate(views) => duplicate = Some((sheet, views)),
                                SheetMenu::Move(to) => moved = Some((sheet, to)),
                                SheetMenu::Delete => delete = Some(sheet),
                            }
                        }
                    });
            } else {
                let _ = response.on_hover_text("The drawing at full size");
            }
        }
        // + : a new sheet.
        let r =
            egui::Rect::from_min_size(egui::pos2(x + 2.0, bar.top() + 5.0), egui::vec2(24.0, 22.0));
        let plus = ui.interact(r, ui.id().with("canvas_tab_new"), egui::Sense::click());
        plus.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add a sheet")
        });
        if plus.hovered() {
            ui.painter().rect_filled(r, theme::radius::CONTROL, n.hover);
        }
        let c = r.center();
        let stroke = egui::Stroke::new(1.4, if plus.hovered() { n.text } else { n.secondary });
        ui.painter()
            .line_segment([c - egui::vec2(5.0, 0.0), c + egui::vec2(5.0, 0.0)], stroke);
        ui.painter()
            .line_segment([c - egui::vec2(0.0, 5.0), c + egui::vec2(0.0, 5.0)], stroke);
        if plus.on_hover_text("New sheet").clicked() {
            self.new_sheet();
        }
        // Where a dragged tab would go: a bar in the gap.
        let pointer = ui.ctx().pointer_interact_pos();
        if let (Some(sheet), Some(p)) = (dragging.or(dropped), pointer) {
            let to = drop_index(&spots, sheet, p.x);
            if dragging.is_some() {
                let x = gap_at(&spots, sheet, to);
                ui.painter().vline(
                    x,
                    bar.top() + 6.0..=bar.bottom() - 2.0,
                    egui::Stroke::new(2.0, theme::accent(dark)),
                );
            }
            if dropped.is_some() {
                moved = Some((sheet, to));
            }
        }
        if let Some(id) = choose {
            self.show_sheet(id);
        }
        if let Some(id) = delete {
            self.delete_sheet(id);
        }
        if let Some((id, views)) = duplicate {
            self.duplicate_sheet(id, views);
        }
        if let Some((id, to)) = moved {
            self.move_sheet(id, to);
        }
    }

    /// Move a sheet to position `to` among the sheets, as one undo step.
    pub(crate) fn move_sheet(&mut self, id: SheetId, to: usize) {
        let from = self.editor.doc.sheets().position(|(s, _)| s == id);
        if from.is_none_or(|from| from == to) {
            return;
        }
        self.editor.doc.begin();
        let went = self.editor.doc.move_sheet(id, to);
        self.editor.commit("Move sheet");
        self.follow_sheets(&went);
    }

    /// Keep what refers to sheets on the same sheets after they moved between ids.
    fn follow_sheets(&mut self, went: &std::collections::BTreeMap<SheetId, SheetId>) {
        let follow = |s: SheetId| went.get(&s).copied().unwrap_or(s);
        self.active_sheet = self.active_sheet.map(follow);
        self.add_view_on = self.add_view_on.map(follow);
        self.pdf_left_out = self.pdf_left_out.iter().map(|s| follow(*s)).collect();
    }

    /// A copy of a sheet, right after it and numbered after it: its paper, title block and
    /// fields, without its revisions. With `views`, it gets copies of the views (each its own
    /// view, to change apart from the first); without, it starts empty.
    pub(crate) fn duplicate_sheet(&mut self, id: SheetId, views: bool) {
        let doc = &self.editor.doc;
        let Some(mut sheet) = doc.sheet(id).cloned() else {
            return;
        };
        let at = doc.sheets().position(|(s, _)| s == id).unwrap_or(0);
        let taken: Vec<String> = doc.sheets().map(|(_, s)| s.fields.number.clone()).collect();
        let mut number = next_sheet_number(Some(&sheet.fields.number));
        for _ in 0..1000 {
            if !taken.contains(&number) {
                break;
            }
            number = next_sheet_number(Some(&number));
        }
        sheet.fields.number = number;
        sheet.name = format!("{} copy", sheet.name.trim());
        sheet.revisions.clear();
        self.editor.doc.begin();
        if views {
            for port in &mut sheet.viewports {
                if let Some(mut v) = self.editor.doc.view(port.view).cloned() {
                    v.name = format!("{} copy", v.title());
                    port.view = self.editor.doc.add_view(v);
                }
            }
        } else {
            sheet.viewports.clear();
        }
        let new = self.editor.doc.add_sheet(sheet);
        let went = self.editor.doc.move_sheet(new, at + 1);
        self.editor.commit("Duplicate sheet");
        self.follow_sheets(&went);
        self.show_sheet(Some(went.get(&new).copied().unwrap_or(new)));
        self.sheet_view = 0;
        self.sheet_pick = SheetPick::Paper;
    }

    /// Delete a sheet; the canvas goes back to the model if none are left.
    pub(crate) fn delete_sheet(&mut self, id: SheetId) {
        self.editor.doc.begin();
        self.editor.doc.remove_sheet(id);
        self.editor.commit("Delete sheet");
        if self.active_sheet == Some(id) {
            self.active_sheet = None;
        }
        if self.editor.doc.sheets().next().is_none() {
            self.sheet_preview = false;
        }
    }

    /// The rail while a sheet is shown: what can be done on paper.
    pub(crate) fn sheet_rail(&mut self, ui: &mut egui::Ui, wide: bool) {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(4.0);
        if wide {
            theme::caps_header(ui, "Sheet");
            ui.add_space(2.0);
        }
        let row =
            |ui: &mut egui::Ui, selected: bool, label: &str, key: &str, glyph: Option<Glyph>| {
                let draw = |p: &egui::Painter, r: egui::Rect, c: egui::Color32| match glyph {
                    Some(g) => icons::glyph(p, r.shrink(1.0), g, c),
                    None => icons::tool(p, r, ToolKind::Select, c),
                };
                if wide {
                    icons::tool_row(
                        ui,
                        selected,
                        label,
                        Some(key).filter(|k| !k.is_empty()),
                        draw,
                    )
                } else {
                    icons::button(ui, 38.0, selected, label, draw)
                }
            };
        row(ui, true, "Select", "S", None)
            .on_hover_text("Pick a view, or the paper; drag a view to move it");
        if row(ui, false, "Add view", "V", Some(Glyph::AddView))
            .on_hover_text(
                "Drag a rectangle round what the view shows, in the model; it goes on this \
                 sheet",
            )
            .clicked()
            && let Some(id) = self.current_sheet()
        {
            self.start_add_view(id);
        }
        ui.add_space(8.0);
        if wide {
            theme::caps_header(ui, "Model");
            ui.add_space(2.0);
        }
        if row(ui, false, "Back to model", "Esc", Some(Glyph::ChevronsLeft))
            .on_hover_text("Back to the model, to draw and edit")
            .clicked()
        {
            self.show_sheet(None);
        }
    }

    /// Add view, from a sheet: the canvas shows the model with the View tool, to drag the
    /// view's crop; the view then goes on the sheet (see [`Self::view_drawn`]).
    pub(crate) fn start_add_view(&mut self, id: SheetId) {
        self.sheet_preview = false;
        self.set_tool(ToolKind::View);
        self.add_view_on = Some(id);
    }

    /// A view was just drawn with the View tool. Asked for from a sheet, it goes on that
    /// sheet where there is room, picked, and the sheet is shown again. Returns true if so.
    pub(crate) fn view_drawn(&mut self, view: ViewId) -> bool {
        let Some(sheet) = self.add_view_on.take() else {
            return false;
        };
        self.set_tool(ToolKind::Select);
        // At the largest standard scale that finds free paper there, and in that spot (the
        // crop stays as drawn); else where there is room at the scale it has.
        let at = match (
            self.editor.doc.view(view).cloned(),
            self.editor.doc.sheet(sheet),
        ) {
            (Some(v), Some(s)) => {
                let placed = self.editor.doc.placed_views(s);
                let fit = fit_on_sheet(v.size, s, &placed, self.fit_bounds(), self.imperial());
                fit.map(|(scale, at)| {
                    if scale != v.scale {
                        self.editor.doc.begin();
                        self.editor
                            .doc
                            .set_view(view, drift_core::View { scale, ..v });
                        self.editor.commit("View scale");
                    }
                    at
                })
            }
            _ => None,
        };
        if self.place_view(sheet, view, at).is_none() {
            return false;
        }
        let index = self.sheet_view;
        self.show_sheet(Some(sheet));
        self.sheet_view = index;
        self.picked_view = Some(view);
        self.sheet_pick = SheetPick::View;
        self.show_panel(crate::dock::Panel::Inspector);
        true
    }

    /// While a view for a sheet is being drawn: what to do, at the top of the canvas.
    pub(crate) fn paint_add_view_prompt(&self, painter: &egui::Painter, map: &ScreenMap) {
        let Some(id) = self.add_view_on else {
            return;
        };
        let dark = painter.ctx().theme() == egui::Theme::Dark;
        let accent = theme::accent(dark);
        let sheet = self.editor.doc.sheet(id).map_or(String::new(), sheet_label);
        let text =
            format!("New view on {sheet}: drag a rectangle round what it shows · Esc cancels");
        let n = theme::neutrals(dark);
        let galley =
            painter.layout_no_wrap(text, egui::FontId::proportional(theme::size::BODY), n.text);
        let rect = map.rect;
        let at = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 12.0);
        let back = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(12.0, 6.0));
        painter.rect_filled(back, theme::radius::MENU, n.raised);
        painter.rect_stroke(
            back,
            theme::radius::MENU,
            egui::Stroke::new(1.0, accent),
            egui::StrokeKind::Inside,
        );
        painter.galley(at, galley, n.text);
    }

    /// The Inspector while a sheet is shown: the picked view, or the sheet itself.
    pub(crate) fn sheet_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.current_sheet() else {
            return;
        };
        let Some(original) = self.editor.doc.sheet(id).cloned() else {
            return;
        };
        let mut sheet = original.clone();
        let mut typing = false;
        let mut view = None;
        let label = sheet_label(&sheet);
        let mut remove_view = false;
        let mut delete = false;
        let mut delete_views = false;
        let mut whole = false;
        let placed = self.editor.doc.placed_views(&sheet);
        let pick = if placed.is_empty() {
            SheetPick::Paper
        } else {
            self.sheet_pick
        };
        let several = self.picked_views(&sheet);
        match pick {
            // Several views: what they are, and what can be done with them all.
            SheetPick::View if several.len() > 1 => {
                let names: Vec<&str> = several.iter().map(|i| placed[*i].title()).collect();
                crate::app::panels::title(
                    ui,
                    None,
                    &format!("{} views", several.len()),
                    &format!("On {label}"),
                );
                ui.label(egui::RichText::new(names.join(", ")).weak());
                ui.label(
                    egui::RichText::new(
                        "Drag one to move them all; the arrow keys nudge them. Shift-click \
                         one to leave it out.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(16.0);
                remove_view = theme::danger_button(ui, "Remove from sheet")
                    .on_hover_text("Take the picked views off this sheet (they stay in the model)")
                    .clicked();
                if self.confirm_delete_set {
                    ui.label(
                        egui::RichText::new(
                            "Deleting them takes them out of the model and off every sheet.",
                        )
                        .color(egui::Color32::from_rgb(0xff, 0x5d, 0x5d)),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Delete them").clicked() {
                            delete_views = true;
                        }
                        if ui.button("Keep them").clicked() {
                            self.confirm_delete_set = false;
                        }
                    });
                } else if theme::danger_button(ui, "Delete views")
                    .on_hover_text("Delete the picked views from the model and every sheet")
                    .clicked()
                {
                    self.confirm_delete_set = true;
                }
            }
            SheetPick::View => {
                let v = &placed[self.current_view(&sheet).min(placed.len() - 1)];
                crate::app::panels::title(
                    ui,
                    None,
                    v.title(),
                    &format!("On {label} · {}", drift_core::sheets::scale_label(v.scale)),
                );
                typing |= self.views_section(ui, &sheet, &mut view);
                ui.add_space(16.0);
                remove_view = theme::danger_button(ui, "Remove from sheet")
                    .on_hover_text("Take the picked view off this sheet (it stays in the model)")
                    .clicked();
            }
            SheetPick::Paper => {
                let orientation = if sheet.portrait {
                    "portrait"
                } else {
                    "landscape"
                };
                let views = placed.len();
                crate::app::panels::title(
                    ui,
                    None,
                    &label,
                    &format!(
                        "{} {orientation} · {views} view{}",
                        sheet.paper.label(),
                        if views == 1 { "" } else { "s" }
                    ),
                );
                // An empty sheet: how views get onto it, and one click for the whole drawing.
                if views == 0 {
                    ui.label(
                        egui::RichText::new(
                            "No views yet: drag one from the Views list in the Sheets panel, \
                             or press V to draw one round part of the model.",
                        )
                        .weak(),
                    );
                    if ui
                        .button("Add a view of the whole drawing")
                        .on_hover_text("A new view of everything drawn, filling the sheet")
                        .clicked()
                    {
                        whole = true;
                    }
                    ui.add_space(8.0);
                }
                typing |= self.paper_settings(ui, &mut sheet);
                ui.add_space(16.0);
                delete = theme::danger_button(ui, "Delete sheet").clicked();
            }
        }
        self.update_sheet(id, sheet, view, typing);
        if remove_view {
            self.remove_picked_views(id);
            self.sheet_pick = SheetPick::Paper;
        }
        if delete_views {
            self.delete_picked_views();
        }
        if whole {
            self.add_whole_view(id);
        }
        if delete {
            self.delete_sheet(id);
        }
        ui.add_space(10.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_numbers_count_on() {
        assert_eq!(next_sheet_number(None), "A-101");
        assert_eq!(next_sheet_number(Some("A-101")), "A-102");
        assert_eq!(next_sheet_number(Some("S-09")), "S-10");
        assert_eq!(next_sheet_number(Some("DR-001")), "DR-002");
        assert_eq!(next_sheet_number(Some("Cover")), "Cover-2");
    }
}
