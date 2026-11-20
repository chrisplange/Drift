//! Working on sheets: the tabs in the top bar (the model and each sheet), the sheet tools
//! in the rail, adding a view from a sheet (drawn with the View tool in the model, then put
//! on the sheet), and the Inspector for what is picked on a sheet (a view, or the sheet
//! itself).

use drift_core::{Bounds, PlacedView, Sheet, SheetId, ViewId};

use crate::icons::{self, Glyph};
use crate::overlay::ScreenMap;
use crate::*;

/// What is picked on the sheet shown: one of its views, one of its own annotations (#432),
/// or the sheet itself (its paper, title block and revisions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SheetPick {
    #[default]
    Paper,
    View,
    /// The annotation in `sheet_annotation`.
    Annotation,
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
        self.drawing.add_view_on = None;
        // Leaving a sheet (for the model or another sheet) lets go of its picked annotation.
        if self.drawing.view.sheet_preview
            && id != self.drawing.view.active_sheet
            && self.drawing.view.sheet_pick == SheetPick::Annotation
        {
            self.pick_sheet_annotation(None);
        }
        match id {
            Some(id) => {
                if self.drawing.view.active_sheet != Some(id) || !self.drawing.view.sheet_preview {
                    self.drawing.view.preview.zoom = 0.0;
                    self.drawing.view.sheet_pick = SheetPick::Paper;
                }
                self.drawing.view.active_sheet = Some(id);
                self.drawing.view.sheet_preview = true;
            }
            None => {
                self.drawing.view.sheet_preview = false;
                // (The model may change meanwhile: a preview drawn from a block edit's finished
                // drawing is made again, with its page.)
                if self.drawing.finished_preview.take().is_some() {
                    self.drawing.preview_cache = None;
                }
            }
        }
    }

    /// A new sheet, numbered after the last, shown with its settings in the Inspector.
    pub(crate) fn new_sheet(&mut self) {
        let id = self.add_sheet();
        self.show_sheet(Some(id));
        self.drawing.view.sheet_pick = SheetPick::Paper;
        self.show_panel(crate::dock::Panel::Inspector);
    }

    /// The model and each sheet as tabs in the top bar, and + for a new sheet: a track of
    /// tabs centred on `middle` when the room (`ui`'s, between the bar's two ends) allows,
    /// with the sheets that don't fit under a "More sheets" button.
    pub(crate) fn sheet_tabs(&mut self, ui: &mut egui::Ui, middle: f32) {
        /// A tab's height, the track's padding round the tabs, a button's width, and the
        /// widest a tab gets.
        const TAB: f32 = 24.0;
        const PAD: f32 = 2.0;
        const BUTTON: f32 = 24.0;
        const WIDEST: f32 = 180.0;
        let zone = ui.max_rect();
        let dark = ui.visuals().dark_mode;
        let n = theme::neutrals(dark);
        // (With each label, how much of its start is the sheet's number: set quieter.)
        let sheets: Vec<(SheetId, String, usize)> = self
            .drawing
            .editor
            .doc
            .sheets()
            .map(|(id, s)| {
                let label = sheet_label(s);
                let number = label.len() - s.name.len();
                (id, label, number)
            })
            .collect();
        let on_sheet = self.drawing.view.sheet_preview;
        let current = self.current_sheet();
        let mut numbers = vec![0];
        let mut tabs: Vec<(Option<SheetId>, String)> = vec![(None, "Model".to_owned())];
        for (id, label, number) in sheets {
            tabs.push((Some(id), label));
            numbers.push(number);
        }
        let is_active = |id: &Option<SheetId>| match id {
            None => !on_sheet,
            Some(id) => on_sheet && current == Some(*id),
        };
        let font = |active: bool| {
            egui::FontId::new(
                theme::size::BODY,
                if active {
                    theme::bold()
                } else {
                    egui::FontFamily::Proportional
                },
            )
        };
        let mut widths: Vec<f32> = tabs
            .iter()
            .map(|(id, label)| {
                let text = ui
                    .painter()
                    .layout_no_wrap(label.clone(), font(is_active(id)), n.text);
                (text.size().x + 24.0).min(WIDEST)
            })
            .collect();
        // Which tabs show: all of them when they fit; else the model and the sheets from the
        // first for as long as they fit beside the "More sheets" button, and always the one
        // shown on the canvas.
        let ends = 2.0 * PAD + BUTTON;
        let mut shown: Vec<usize> = (0..tabs.len()).collect();
        if widths.iter().sum::<f32>() + ends > zone.width() {
            shown.clear();
            let mut used = ends + BUTTON;
            for (k, w) in widths.iter().enumerate() {
                if k > 0 && used + w > zone.width() {
                    break;
                }
                shown.push(k);
                used += w;
            }
            if let Some(active) = tabs.iter().position(|(id, _)| is_active(id))
                && !shown.contains(&active)
            {
                while shown.len() > 1 && used + widths[active] > zone.width() {
                    if let Some(k) = shown.pop() {
                        used -= widths[k];
                    }
                }
                shown.push(active);
            }
        }
        let hidden: Vec<usize> = (0..tabs.len()).filter(|k| !shown.contains(k)).collect();
        let more = if hidden.is_empty() { 0.0 } else { BUTTON };
        let total = |widths: &[f32]| shown.iter().map(|k| widths[*k]).sum::<f32>() + ends + more;
        // Still too wide (a long name in a narrow window): the last tab's name is cut short.
        let over = total(&widths) - zone.width();
        if let Some(last) = shown.last().copied().filter(|k| *k > 0 && over > 0.0) {
            widths[last] = (widths[last] - over).max(56.0);
        }
        let width = total(&widths);
        let left = (middle - width / 2.0)
            .min(zone.right() - width)
            .max(zone.left())
            .round();
        let track = egui::Rect::from_min_size(
            egui::pos2(left, (zone.center().y - TAB / 2.0 - PAD).round()),
            egui::vec2(width, TAB + 2.0 * PAD),
        );
        // A well, as a segmented control's: the tab shown stands in it as a control.
        ui.painter().rect(
            track,
            theme::radius::CONTROL + 2,
            n.field,
            egui::Stroke::new(1.0, n.border),
            egui::StrokeKind::Inside,
        );
        let mut x = track.left() + PAD;
        let mut choose = None;
        let mut delete = None;
        let mut duplicate = None;
        let mut moved = None;
        // Sheet tabs (where each starts and ends), and a tab being dragged to a new place.
        let mut spots: Vec<(SheetId, f32, f32)> = Vec::new();
        let mut dragging = None;
        let mut dropped = None;
        let count = tabs.len() - 1;
        // A hairline between two tabs at rest, so a row of them isn't one run of words.
        let mut rest_before: Option<f32> = None;
        for k in shown {
            let (id, label) = &tabs[k];
            let active = is_active(id);
            let w = widths[k];
            let r = egui::Rect::from_min_size(egui::pos2(x, track.top() + PAD), egui::vec2(w, TAB));
            x += w;
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
            if active {
                ui.painter().rect(
                    r,
                    theme::radius::CONTROL,
                    n.control,
                    egui::Stroke::new(1.0, n.control_border),
                    egui::StrokeKind::Inside,
                );
            } else if response.hovered() {
                ui.painter().rect_filled(r, theme::radius::CONTROL, n.faint);
            }
            let at_rest = !active && !response.hovered();
            if let Some(x) = rest_before.filter(|_| at_rest) {
                ui.painter().vline(
                    x.round() + 0.5,
                    r.center().y - 7.0..=r.center().y + 7.0,
                    egui::Stroke::new(1.0, n.control_border),
                );
            }
            rest_before = at_rest.then_some(r.right());
            // The one shown in bold, in the text's colour; of the others the name in the
            // secondary tone and the sheet's number quieter still.
            let (number_color, color) = if active || response.hovered() {
                (n.text, n.text)
            } else {
                (n.tertiary, n.secondary)
            };
            let format = |color| egui::TextFormat {
                font_id: font(active),
                color,
                ..Default::default()
            };
            let (number, name) = label.split_at(numbers[k].min(label.len()));
            let mut job = egui::text::LayoutJob::default();
            job.append(number, 0.0, format(number_color));
            job.append(name, 0.0, format(color));
            job.break_on_newline = false;
            job.wrap.max_width = w - 16.0;
            job.wrap.max_rows = 1;
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(
                egui::pos2(
                    r.center().x - galley.size().x / 2.0,
                    r.center().y - galley.size().y / 2.0,
                ),
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
        // A button at the track's end: its glyph brightens under the pointer.
        let mut button = |ui: &mut egui::Ui, id: &str, name: &str, glyph: Glyph| {
            let r = egui::Rect::from_min_size(
                egui::pos2(x, track.top() + PAD),
                egui::vec2(BUTTON, TAB),
            );
            x += BUTTON;
            let response = ui.interact(r, ui.id().with(id), egui::Sense::click());
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
            if response.hovered() {
                ui.painter().rect_filled(r, theme::radius::CONTROL, n.faint);
            }
            let color = if response.hovered() {
                n.text
            } else {
                n.secondary
            };
            icons::glyph(
                ui.painter(),
                egui::Rect::from_center_size(r.center(), egui::Vec2::splat(13.0)),
                glyph,
                color,
            );
            response
        };
        // The sheets that found no room, in a menu.
        if !hidden.is_empty() {
            let response = button(ui, "canvas_tab_more", "More sheets", Glyph::ChevronDown)
                .on_hover_text("The sheets there is no room for here");
            egui::Popup::menu(&response).show(|ui| {
                for k in &hidden {
                    let (id, label) = &tabs[*k];
                    if ui.button(label).clicked() {
                        choose = Some(*id);
                        ui.close();
                    }
                }
            });
        }
        // + : a new sheet.
        if button(ui, "canvas_tab_new", "Add a sheet", Glyph::Plus)
            .on_hover_text("New sheet")
            .clicked()
        {
            self.new_sheet();
        }
        // Where a dragged tab would go: a bar in the gap.
        let pointer = ui.ctx().pointer_interact_pos();
        if let (Some(sheet), Some(p)) = (dragging.or(dropped), pointer) {
            let to = drop_index(&spots, sheet, p.x);
            // Its own spot among the tabs showing: let go there and nothing moves. (The
            // sheet shown may have its tab after the others, out of its place among the
            // sheets: a press that wobbles isn't a move to where its tab happens to be.)
            let own = spots.iter().position(|(s, _, _)| *s == sheet);
            let stays = own == Some(to);
            if dragging.is_some() && !stays {
                let x = gap_at(&spots, sheet, to);
                ui.painter().vline(
                    x,
                    track.top() + 3.0..=track.bottom() - 3.0,
                    egui::Stroke::new(2.0, theme::accent(dark)),
                );
            }
            if dropped.is_some() && !stays {
                // (Its place among every sheet: some may have no tab.)
                let every: Vec<SheetId> = tabs
                    .iter()
                    .filter_map(|(id, _)| *id)
                    .filter(|s| *s != sheet)
                    .collect();
                let showing: Vec<SheetId> = spots
                    .iter()
                    .map(|(s, _, _)| *s)
                    .filter(|s| *s != sheet)
                    .collect();
                let to = match to.checked_sub(1).and_then(|k| showing.get(k)) {
                    Some(before) => every.iter().position(|s| s == before).map_or(to, |p| p + 1),
                    None => 0,
                };
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
        let from = self.drawing.editor.doc.sheets().position(|(s, _)| s == id);
        if from.is_none_or(|from| from == to) {
            return;
        }
        self.drawing.editor.doc.begin();
        let went = self.drawing.editor.doc.move_sheet(id, to);
        self.drawing.editor.commit("Move sheet");
        self.follow_sheets(&went);
    }

    /// Keep what refers to sheets on the same sheets after they moved between ids.
    fn follow_sheets(&mut self, went: &std::collections::BTreeMap<SheetId, SheetId>) {
        let follow = |s: SheetId| went.get(&s).copied().unwrap_or(s);
        self.drawing.view.active_sheet = self.drawing.view.active_sheet.map(follow);
        self.drawing.add_view_on = self.drawing.add_view_on.map(follow);
        self.drawing.pdf_left_out = self
            .drawing
            .pdf_left_out
            .iter()
            .map(|s| follow(*s))
            .collect();
    }

    /// A copy of a sheet, right after it and numbered after it: its paper, title block and
    /// fields, without its revisions. With `views`, it gets copies of the views (each its own
    /// view, to change apart from the first); without, it starts empty.
    pub(crate) fn duplicate_sheet(&mut self, id: SheetId, views: bool) {
        let doc = &self.drawing.editor.doc;
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
        self.drawing.editor.doc.begin();
        if views {
            for port in &mut sheet.viewports {
                if let Some(mut v) = self.drawing.editor.doc.view(port.view).cloned() {
                    v.name = format!("{} copy", v.title());
                    port.view = self.drawing.editor.doc.add_view(v);
                }
            }
        } else {
            sheet.viewports.clear();
        }
        let new = self.drawing.editor.doc.add_sheet(sheet);
        let went = self.drawing.editor.doc.move_sheet(new, at + 1);
        self.drawing.editor.commit("Duplicate sheet");
        self.follow_sheets(&went);
        self.show_sheet(Some(went.get(&new).copied().unwrap_or(new)));
        self.drawing.view.sheet_view = 0;
        self.drawing.view.sheet_pick = SheetPick::Paper;
    }

    /// Delete a sheet; the canvas goes back to the model if none are left.
    pub(crate) fn delete_sheet(&mut self, id: SheetId) {
        self.drawing.editor.doc.begin();
        self.drawing.editor.doc.remove_sheet(id);
        self.drawing.editor.commit("Delete sheet");
        if self.drawing.view.active_sheet == Some(id) {
            self.drawing.view.active_sheet = None;
        }
        if self.drawing.editor.doc.sheets().next().is_none() {
            self.drawing.view.sheet_preview = false;
        }
    }

    /// The rail while a sheet is shown: what can be done on paper, as icon tiles or (with
    /// `names`) as rows with names and keys, like the model's rail. `edge` is the rail's right
    /// edge, where the tooltips start.
    pub(crate) fn sheet_rail(&mut self, ui: &mut egui::Ui, names: bool, edge: f32) {
        use crate::app::rail;
        ui.spacing_mut().item_spacing.y = 0.0;
        if names {
            ui.add_space(2.0);
            let color = theme::neutrals(ui.visuals().dark_mode).secondary;
            theme::caps_header_in(ui, "Sheet", color);
            ui.add_space(2.0);
        }
        let place = |ui: &mut egui::Ui,
                     selected: bool,
                     label: &str,
                     key: &str,
                     hint: &str,
                     icon: Result<Glyph, ToolKind>| {
            let draw = |p: &egui::Painter, r: egui::Rect, c: egui::Color32| match icon {
                Ok(g) => icons::glyph(p, r.shrink(1.0), g, c),
                Err(tool) => icons::tool(p, r, tool, c),
            };
            let response = if names {
                icons::tool_row(
                    ui,
                    selected,
                    label,
                    Some(key).filter(|k| !k.is_empty()),
                    draw,
                )
            } else {
                let size = egui::vec2(rail::TILE_WIDTH, rail::TILE);
                icons::tool_tile(ui, size, selected, label, draw)
            };
            rail::tip_beside(&response, edge, |ui| {
                theme::command_tip(ui, label, key, hint);
            });
            response.clicked()
        };
        let kind = self.tool.kind();
        let on_tool = self.sheet_tool();
        if place(
            ui,
            !on_tool,
            "Select",
            "S",
            "Pick a view, a note, a dimension or the paper; drag a view, a note's text or a \
             dimension's label to move it",
            Err(ToolKind::Select),
        ) {
            self.set_tool(ToolKind::Select);
        }
        // The annotation tools that work on paper: what they make is this sheet's own.
        for (tool, name, key, hint) in [
            (
                ToolKind::Note,
                "Note",
                "N",
                "A note on this sheet: click the paper for its text, or geometry inside a \
                 view first for a leader tied to it. It shows only on this sheet",
            ),
            (
                ToolKind::Dimension,
                "Dimension",
                "D",
                "A dimension on this sheet: click a line, circle or arc inside a view, or \
                 two points, then where it goes. It reads the model's true size at any \
                 view scale and never holds the geometry",
            ),
        ] {
            if place(ui, kind == tool, name, key, hint, Err(tool)) {
                self.set_tool(tool);
            }
        }
        if place(
            ui,
            false,
            "Add view",
            "V",
            "Drag a rectangle round what the view shows, in the model; it goes on this sheet",
            Ok(Glyph::AddView),
        ) && let Some(id) = self.current_sheet()
        {
            self.start_add_view(id);
        }
        if names {
            ui.add_space(7.0);
            let color = theme::neutrals(ui.visuals().dark_mode).secondary;
            theme::caps_header_in(ui, "Model", color);
            ui.add_space(2.0);
        } else {
            rail::group_line(ui);
        }
        if place(
            ui,
            false,
            "Back to model",
            "",
            "Back to the model, to draw and edit",
            Ok(Glyph::Back),
        ) {
            self.show_sheet(None);
        }
    }

    /// Add view, from a sheet: the canvas shows the model with the View tool, to drag the
    /// view's crop; the view then goes on the sheet (see [`Self::view_drawn`]).
    pub(crate) fn start_add_view(&mut self, id: SheetId) {
        self.drawing.view.sheet_preview = false;
        self.set_tool(ToolKind::View);
        self.drawing.add_view_on = Some(id);
    }

    /// A view was just drawn with the View tool. Asked for from a sheet, it goes on that
    /// sheet where there is room, picked, and the sheet is shown again. Returns true if so.
    pub(crate) fn view_drawn(&mut self, view: ViewId) -> bool {
        let Some(sheet) = self.drawing.add_view_on.take() else {
            return false;
        };
        self.set_tool(ToolKind::Select);
        // At the largest standard scale that finds free paper there, and in that spot (the
        // crop stays as drawn); else where there is room at the scale it has.
        let at = match (
            self.drawing.editor.doc.view(view).cloned(),
            self.drawing.editor.doc.sheet(sheet),
        ) {
            (Some(v), Some(s)) => {
                let placed = self.drawing.editor.doc.placed_views(s);
                let fit = fit_on_sheet(v.size, s, &placed, self.fit_bounds(), self.imperial());
                fit.map(|(scale, at)| {
                    if scale != v.scale {
                        self.drawing.editor.doc.begin();
                        self.drawing
                            .editor
                            .doc
                            .set_view(view, drift_core::View { scale, ..v });
                        self.drawing.editor.commit("View scale");
                    }
                    at
                })
            }
            _ => None,
        };
        if self.place_view(sheet, view, at).is_none() {
            return false;
        }
        let index = self.drawing.view.sheet_view;
        self.show_sheet(Some(sheet));
        self.drawing.view.sheet_view = index;
        self.drawing.picked_view = Some(view);
        self.drawing.view.sheet_pick = SheetPick::View;
        self.show_panel(crate::dock::Panel::Inspector);
        true
    }

    /// While a view for a sheet is being drawn: what to do, at the top of the canvas.
    pub(crate) fn paint_add_view_prompt(&self, painter: &egui::Painter, map: &ScreenMap) {
        let Some(id) = self.drawing.add_view_on else {
            return;
        };
        let dark = painter.ctx().theme() == egui::Theme::Dark;
        let accent = theme::accent(dark);
        let sheet = self
            .drawing
            .editor
            .doc
            .sheet(id)
            .map_or(String::new(), sheet_label);
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
        let Some(original) = self.drawing.editor.doc.sheet(id).cloned() else {
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
        let mut bring = None;
        let placed = self.drawing.editor.doc.placed_views(&sheet);
        let pick = if placed.is_empty() {
            SheetPick::Paper
        } else {
            self.drawing.view.sheet_pick
        };
        let several = self.picked_views(&sheet);
        // One of the sheet's own annotations: its own Inspector (it writes the sheet itself).
        if self.drawing.view.sheet_pick == SheetPick::Annotation {
            if self.picked_sheet_annotation().is_some() {
                self.sheet_annotation_inspector(ui, &label);
                return;
            }
            // (It went: an undo, or its geometry was deleted.)
            self.drawing.view.sheet_pick = SheetPick::Paper;
            self.drawing.view.sheet_annotation = None;
        }
        let pick = if pick == SheetPick::Annotation {
            SheetPick::Paper
        } else {
            pick
        };
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
                if self.drawing.confirm_delete_set {
                    ui.label(
                        egui::RichText::new(
                            "Deleting them takes them out of the model and off every sheet.",
                        )
                        .color(crate::theme::error_color(ui.visuals().dark_mode)),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Delete them").clicked() {
                            delete_views = true;
                        }
                        if ui.button("Keep them").clicked() {
                            self.drawing.confirm_delete_set = false;
                        }
                    });
                } else if theme::danger_button(ui, "Delete views")
                    .on_hover_text("Delete the picked views from the model and every sheet")
                    .clicked()
                {
                    self.drawing.confirm_delete_set = true;
                }
            }
            SheetPick::View => {
                let v = &placed[self.current_view(&sheet).min(placed.len() - 1)];
                crate::app::panels::title(
                    ui,
                    None,
                    v.title(),
                    &format!(
                        "On {label} · {}",
                        drift_core::sheets::scale_label(v.scale, self.imperial())
                    ),
                );
                typing |= self.views_section(ui, &sheet, &mut view);
                // The model's notes and dimensions this view shows, onto the sheet.
                let count = self.view_annotation_count();
                if count > 0 {
                    use drift_edit::sheet_annotations::ToSheet;
                    ui.add_space(12.0);
                    theme::caps_header(ui, "Model annotations");
                    ui.label(
                        egui::RichText::new(format!(
                            "This view shows {count} of the model's notes and dimensions. On \
                             the sheet they become its own: placed for this sheet, and off \
                             the other views."
                        ))
                        .small()
                        .weak(),
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .button("Move onto sheet")
                            .on_hover_text(
                                "Each becomes this sheet's own, where it is on the paper, and \
                                 leaves the model. A dimension that drives the geometry stays \
                                 in the model and stops printing in this view. Right-click one \
                                 in the view to move it alone",
                            )
                            .clicked()
                        {
                            bring = Some(ToSheet::Move);
                        }
                        if ui
                            .button("Copy onto sheet")
                            .on_hover_text(
                                "Each gets a copy that is this sheet's own; the model keeps \
                                 its own, which still prints in this view",
                            )
                            .clicked()
                        {
                            bring = Some(ToSheet::Copy);
                        }
                    });
                }
                ui.add_space(16.0);
                remove_view = theme::danger_button(ui, "Remove from sheet")
                    .on_hover_text("Take the picked view off this sheet (it stays in the model)")
                    .clicked();
            }
            SheetPick::Paper | SheetPick::Annotation => {
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
        // (After the sheet's own fields are written: this changes the sheet too.)
        if let Some(how) = bring {
            self.bring_view_annotations(how);
        }
        if remove_view {
            self.remove_picked_views(id);
            self.drawing.view.sheet_pick = SheetPick::Paper;
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
