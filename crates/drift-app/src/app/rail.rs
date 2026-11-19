//! The tool rail: the registry's [`commands::RAIL`] as a slim column of icon tiles (two
//! columns in a window too short for one), or, with "Show tool names" at its foot, as rows
//! with names and keys. A family (the circle's ellipses, Trim's Extend and Split) takes one
//! place, showing the one used last; right-click it, or hold the button down, for the rest in
//! a flyout.

use crate::app::palette::Command;
use crate::commands::{self, Icon, RAIL, Slot};
use crate::icons;
use crate::*;

/// How long a press on a family opens its flyout.
const HOLD: f64 = 0.35;
/// Row heights with names shown: normal, and in a window too short for the rail.
const ROW: f32 = 24.0;
const SHORT_ROW: f32 = 22.0;
/// The slim rail's tiles: their width, and their height with room and without.
pub(crate) const TILE_WIDTH: f32 = 36.0;
pub(crate) const TILE: f32 = 30.0;
const SHORT_TILE: f32 = 26.0;
/// Two columns of tiles: each a little narrower, with a gap between.
const PAIR_WIDTH: f32 = 35.0;
const PAIR_GAP: f32 = 2.0;
/// The hairline between the slim rail's groups, with the room either side of it.
const GROUP_GAP: f32 = 9.0;
/// The foot of the rail: a hairline and the "Show tool names" toggle.
const FOOT_GAP: f32 = 7.0;
const FOOT_BUTTON: f32 = 22.0;
const FOOT: f32 = FOOT_GAP + FOOT_BUTTON;
/// The rail's margin above and below, and at its sides (slim, and with names).
pub(crate) const MARGIN_Y: i8 = 6;
const SLIM_MARGIN: i8 = 6;
const NAMES_MARGIN: i8 = 8;

/// How the rail is laid out this frame: decided before the panel is shown, since its width
/// follows from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RailLayout {
    /// The labelled list (icon, name and key) rather than icon tiles.
    pub(crate) names: bool,
    /// Columns of tiles (the slim rail in a short window has two).
    columns: usize,
    /// A tile's height.
    tile: f32,
}

impl RailLayout {
    /// The rail's width.
    pub(crate) fn width(self) -> f32 {
        match (self.names, self.columns) {
            (true, _) => 168.0,
            (false, 1) => 2.0 * f32::from(SLIM_MARGIN) + TILE_WIDTH,
            (false, _) => 2.0 * f32::from(SLIM_MARGIN) + 2.0 * PAIR_WIDTH + PAIR_GAP,
        }
    }

    /// The margin at the rail's sides.
    pub(crate) fn margin(self) -> i8 {
        if self.names {
            NAMES_MARGIN
        } else {
            SLIM_MARGIN
        }
    }
}

/// A family's flyout, open beside its place on the rail.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Flyout {
    head: Command,
    at: egui::Pos2,
    /// Opened by holding the button down: letting go over a row picks it, and letting go on
    /// the rail doesn't count as a click.
    held: bool,
    /// The frame it opened in (that frame's press doesn't close it).
    opened: u64,
}

/// The tooltip of a place on the rail, to the right of the rail (never over its neighbours):
/// `edge` is the rail's right edge.
pub(crate) fn tip_beside(response: &egui::Response, edge: f32, add: impl FnOnce(&mut egui::Ui)) {
    let r = response.rect;
    let beside = egui::Rect::from_x_y_ranges(r.left()..=edge.max(r.right()), r.y_range());
    let mut tooltip = if response.enabled() {
        egui::Tooltip::for_enabled(response)
    } else {
        egui::Tooltip::for_disabled(response)
    };
    tooltip.popup = tooltip
        .popup
        .anchor(beside)
        .align(egui::RectAlign::RIGHT_START)
        .align_alternatives(&[egui::RectAlign::RIGHT_START])
        .gap(6.0);
    tooltip.show(add);
}

/// The hairline between two groups on the rail.
pub(crate) fn group_line(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), GROUP_GAP),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.left() + 4.0..=rect.right() - 4.0,
        rect.center().y,
        egui::Stroke::new(1.0, theme::hairline(ui)),
    );
}

impl DriftApp {
    /// The rail's layout in a panel `height` high: tiles as tall as there is room for, and a
    /// second column before any of them would need scrolling to.
    pub(crate) fn rail_layout(&self, height: f32) -> RailLayout {
        let one = |tile| RailLayout {
            names: self.rail_names,
            columns: 1,
            tile,
        };
        if self.rail_names || self.drawing.view.sheet_preview {
            return one(TILE);
        }
        let room = height - 2.0 * f32::from(MARGIN_Y) - FOOT;
        let lines = RAIL.len().saturating_sub(1) as f32 * GROUP_GAP;
        let slots = RAIL.iter().map(|(_, slots)| slots.len()).sum::<usize>() as f32;
        let tile = ((room - lines) / slots).floor();
        if tile >= SHORT_TILE {
            return one(tile.min(TILE));
        }
        let rows = RAIL
            .iter()
            .map(|(_, slots)| slots.len().div_ceil(2))
            .sum::<usize>() as f32;
        RailLayout {
            names: false,
            columns: 2,
            tile: if rows * TILE + lines <= room {
                TILE
            } else {
                SHORT_TILE
            },
        }
    }

    /// The tool rail: icon tiles in groups (Draw, Build, Modify, Annotate), with tooltips
    /// that name the tool, its keys and what it does; or the same as rows with names.
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui, layout: RailLayout) {
        ui.spacing_mut().item_spacing.y = 0.0;
        // The rail's right edge: where its flyouts and tooltips start.
        let edge = ui.max_rect().right() + f32::from(layout.margin());
        let room = (ui.available_height() - FOOT).max(0.0);
        // On a sheet, the rail holds what can be done on paper.
        if self.drawing.view.sheet_preview {
            let (rect, _) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), room), egui::Sense::hover());
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            self.sheet_rail(&mut inner, layout.names, edge);
            self.rail_foot(ui, edge);
            return;
        }
        self.note_family_tool();
        let mut picked = None;
        let output = egui::ScrollArea::vertical()
            .max_height(room)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                picked = if layout.names {
                    self.rail_rows(ui, room, edge)
                } else {
                    self.rail_tiles(ui, layout, edge)
                };
            });
        theme::fade_bottom(ui, &output, ui.visuals().panel_fill);
        self.rail_foot(ui, edge);
        picked = picked.or(self.rail_flyout_ui(ui.ctx()));
        if let Some(command) = picked {
            self.rail_flyout = None;
            let ctx = ui.ctx().clone();
            self.run_command(&ctx, command);
            // A family shows what was picked from it (a command as much as a tool).
            if let Some(slot) = commands::rail_slot(command)
                && slot.is_family()
            {
                self.show_in_family(slot.head, command);
            }
        }
    }

    /// The slim rail: a tile per place, a hairline between groups.
    fn rail_tiles(&mut self, ui: &mut egui::Ui, layout: RailLayout, edge: f32) -> Option<Command> {
        let mut picked = None;
        for (i, (_, slots)) in RAIL.iter().enumerate() {
            if i > 0 {
                group_line(ui);
            }
            if layout.columns == 1 {
                let place = Place::Tile(egui::vec2(TILE_WIDTH, layout.tile));
                for slot in *slots {
                    picked = picked.or(self.rail_place(ui, slot, place, edge));
                }
            } else {
                let place = Place::Tile(egui::vec2(PAIR_WIDTH, layout.tile));
                for pair in slots.chunks(2) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = PAIR_GAP;
                        for slot in pair {
                            picked = picked.or(self.rail_place(ui, slot, place, edge));
                        }
                    });
                }
            }
        }
        picked
    }

    /// The rail with names: a row per place under its group's caption. Too short for the
    /// rows: first shorter rows and tighter captions, and only when that still doesn't fit,
    /// hairlines for the captions.
    fn rail_rows(&mut self, ui: &mut egui::Ui, room: f32, edge: f32) -> Option<Command> {
        let rows = RAIL.iter().map(|(_, slots)| slots.len()).sum::<usize>() as f32;
        let headers = RAIL.iter().filter(|(c, _)| c.is_some()).count() as f32;
        let tight = room < rows * ROW + headers * 24.0 + 8.0;
        let short = tight && room < rows * SHORT_ROW + headers * 18.0 + 8.0;
        let mut picked = None;
        for (i, (category, slots)) in RAIL.iter().enumerate() {
            if short {
                if i > 0 {
                    group_line(ui);
                }
            } else if let Some(c) = category {
                let gap = match (i > 0, tight) {
                    (false, _) => 2.0,
                    (true, false) => 7.0,
                    (true, true) => 3.0,
                };
                ui.add_space(gap);
                let color = theme::neutrals(ui.visuals().dark_mode).secondary;
                theme::caps_header_in(ui, c.label(), color);
                if !tight {
                    ui.add_space(2.0);
                }
            }
            let height = if tight { SHORT_ROW } else { ROW };
            for slot in *slots {
                picked = picked.or(self.rail_place(ui, slot, Place::Row(height), edge));
            }
        }
        picked
    }

    /// The foot of the rail: the switch between the slim rail and the one with names.
    pub(crate) fn rail_foot(&mut self, ui: &mut egui::Ui, edge: f32) {
        let names = self.rail_names;
        let (line, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), FOOT_GAP),
            egui::Sense::hover(),
        );
        ui.painter().hline(
            line.x_range(),
            line.center().y,
            egui::Stroke::new(1.0, theme::hairline(ui)),
        );
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), FOOT_BUTTON),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, names, "Show tool names")
        });
        let visuals = ui.visuals();
        let n = theme::neutrals(visuals.dark_mode);
        let (bg, fg) = if response.is_pointer_button_down_on() {
            (Some(visuals.widgets.active.bg_fill), n.text)
        } else if response.hovered() {
            (Some(visuals.widgets.hovered.bg_fill), n.text)
        } else {
            (None, n.secondary)
        };
        if let Some(bg) = bg {
            ui.painter().rect_filled(rect, theme::radius::CONTROL, bg);
        }
        // A painted chevron: out to open the names, back to fold them away.
        let (glyph, x) = if names {
            (icons::Glyph::ChevronsLeft, rect.left() + 17.0)
        } else {
            (icons::Glyph::ChevronsRight, rect.center().x)
        };
        let icon =
            egui::Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(14.0, 14.0));
        icons::glyph(ui.painter(), icon, glyph, fg);
        if names {
            let galley = ui.painter().layout_no_wrap(
                "Hide names".to_owned(),
                egui::FontId::proportional(theme::size::LABEL),
                fg,
            );
            let y = rect.center().y - galley.mesh_bounds.center().y;
            ui.painter()
                .galley(egui::pos2(rect.left() + 34.0, y), galley, fg);
        }
        tip_beside(&response, edge, |ui| {
            if names {
                theme::command_tip(
                    ui,
                    "Icons only",
                    "",
                    "The rail as icons alone. Each tool's name and key are in its tooltip",
                );
            } else {
                theme::command_tip(
                    ui,
                    "Tool names",
                    "",
                    "The rail as a list, with each tool's name and key",
                );
            }
        });
        if response.clicked() {
            self.rail_names = !names;
            self.rail_flyout = None;
        }
    }

    /// The current tool, if it's in a family, is what that family shows from now on (picked
    /// by key, menu or palette as much as on the rail).
    fn note_family_tool(&mut self) {
        let current = Command::Tool(self.tool.kind());
        if let Some(slot) = commands::rail_slot(current)
            && slot.is_family()
        {
            self.show_in_family(slot.head, current);
        }
    }

    fn show_in_family(&mut self, head: Command, command: Command) {
        match self.rail_shown.iter_mut().find(|(h, _)| *h == head) {
            Some(entry) => entry.1 = command,
            None => self.rail_shown.push((head, command)),
        }
    }

    /// What a place on the rail shows: the family's last used, else its head.
    pub(crate) fn rail_shows(&self, slot: &Slot) -> Command {
        self.rail_shown
            .iter()
            .find(|(h, _)| *h == slot.head)
            .map_or(slot.head, |(_, c)| *c)
    }

    /// One place on the rail: a row with its name, or an icon tile. Returns the command to
    /// run if it was clicked.
    fn rail_place(
        &mut self,
        ui: &mut egui::Ui,
        slot: &Slot,
        place: Place,
        edge: f32,
    ) -> Option<Command> {
        let command = self.rail_shows(slot);
        let spec = commands::spec(command)?;
        let family = slot.is_family();
        let selected = command == Command::Tool(self.tool.kind());
        let enabled = spec.when.holds(self);
        let label = spec.title.trim_end_matches('…');
        let mac = cfg!(target_os = "macos");
        let key = spec.key_text(mac);
        let dark = ui.visuals().dark_mode;
        let mark = theme::neutrals(dark).secondary;
        let icon = spec.icon;
        let draw = move |p: &egui::Painter, r: egui::Rect, c: egui::Color32| {
            draw_icon(p, r, icon, c);
            if family {
                family_mark(p, r, mark);
            }
        };
        let response = ui
            .add_enabled_ui(enabled, |ui| match place {
                Place::Row(height) => icons::tool_row_sized(
                    ui,
                    height,
                    selected,
                    label,
                    Some(key.as_str()).filter(|k| !k.is_empty()),
                    draw,
                ),
                Place::Tile(size) => icons::tool_tile(ui, size, selected, label, draw),
            })
            .inner;
        if command == Command::Tool(ToolKind::Select) {
            self.pick_filter_badge(ui, &response);
        }
        // A family's flyout: right-click, or hold the button down.
        let frame = ui.ctx().cumulative_frame_nr();
        if family {
            let held = response.is_pointer_button_down_on()
                && ui.input(|i| {
                    i.pointer.primary_down()
                        && i.pointer
                            .press_start_time()
                            .is_some_and(|t| i.time - t >= HOLD)
                });
            if response.is_pointer_button_down_on() && self.rail_flyout.is_none() {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(HOLD));
            }
            // To the right of the rail, clear of its edge.
            let open = |held| Flyout {
                head: slot.head,
                at: egui::pos2(edge + 4.0, response.rect.top() - 4.0),
                held,
                opened: frame,
            };
            if response.secondary_clicked() {
                self.rail_flyout = Some(open(false));
            } else if held && self.rail_flyout.is_none_or(|f| f.head != slot.head) {
                self.rail_flyout = Some(open(true));
            }
        }
        // No place's tooltip while a flyout is open: it would draw over the flyout's rows.
        if self.rail_flyout.is_none() {
            tip_beside(&response, edge, |ui| {
                self.rail_tooltip(ui, slot, command, enabled);
            });
        }
        if !response.clicked() || !enabled {
            return None;
        }
        // Letting go after holding it open isn't a click.
        if let Some(f) = self.rail_flyout
            && f.head == slot.head
            && f.held
        {
            self.rail_flyout = Some(Flyout { held: false, ..f });
            return None;
        }
        Some(command)
    }

    /// The tooltip: the name and its keys as key caps, what it does, for a family what else
    /// is in it, and why it can't be used when it can't.
    fn rail_tooltip(&self, ui: &mut egui::Ui, slot: &Slot, command: Command, enabled: bool) {
        let Some(spec) = commands::spec(command) else {
            return;
        };
        let mac = cfg!(target_os = "macos");
        let keys = spec.all_keys(mac);
        let keys = if keys.is_empty() {
            spec.key_text(mac)
        } else {
            keys
        };
        ui.set_max_width(280.0);
        ui.horizontal(|ui| {
            let name = spec.title.trim_end_matches('…');
            ui.label(
                egui::RichText::new(name)
                    .family(theme::bold())
                    .color(ui.visuals().strong_text_color()),
            );
            if !keys.is_empty() {
                theme::keycaps(ui, &keys);
            }
        });
        let n = theme::neutrals(ui.visuals().dark_mode);
        ui.label(egui::RichText::new(spec.description).color(n.secondary));
        let small = |text: String| egui::RichText::new(text).small().color(n.tertiary);
        if let Command::Tool(kind) = command
            && let Some(again) = crate::commands::cycle_hint(kind)
        {
            ui.label(small(again.to_string()));
        }
        if slot.is_family() {
            let others: Vec<&str> = slot
                .offered()
                .filter(|c| *c != command)
                .filter_map(commands::spec)
                .map(|s| s.title.trim_end_matches('…'))
                .collect();
            ui.label(small(format!(
                "Right-click or hold for {}",
                others.join(", ")
            )));
        }
        if !enabled {
            ui.label(
                egui::RichText::new(spec.when.reason())
                    .small()
                    .color(theme::warning_color(ui.visuals().dark_mode)),
            );
        }
    }

    /// A filter on: a badge on the Select tool, so it isn't forgotten.
    fn pick_filter_badge(&self, ui: &mut egui::Ui, response: &egui::Response) {
        let filter = self.drawing.editor.prefs.pick_filter;
        if filter.is_all() {
            return;
        }
        let r = response.rect;
        let at = egui::pos2(r.right() - 7.0, r.top() + 7.0);
        let dark = ui.visuals().dark_mode;
        ui.painter()
            .circle_filled(at, 4.0, theme::warning_color(dark));
        let kinds: Vec<&str> = filter.kinds().map(|k| k.label()).collect();
        let tip = if kinds.is_empty() {
            "Selection filter on: nothing can be picked".to_owned()
        } else {
            format!("Selection filter on: picks {}", kinds.join(", "))
        };
        let badge = ui
            .interact(
                egui::Rect::from_center_size(at, egui::vec2(10.0, 10.0)),
                ui.id().with("pick_filter_badge"),
                egui::Sense::hover(),
            )
            .on_hover_text(tip);
        badge.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Selection filter on")
        });
    }

    /// The open flyout: the family's commands with their keys. Returns the one picked.
    fn rail_flyout_ui(&mut self, ctx: &egui::Context) -> Option<Command> {
        let flyout = self.rail_flyout?;
        let slot = commands::rail_slot(flyout.head)?;
        let shown = self.rail_shows(slot);
        let mac = cfg!(target_os = "macos");
        let mut picked = None;
        let mut rows = Vec::new();
        let mut tips = Vec::new();
        let area = egui::Area::new(egui::Id::new("rail_flyout"))
            .fixed_pos(flyout.at)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_width(190.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for command in slot.offered() {
                        let Some(spec) = commands::spec(command) else {
                            continue;
                        };
                        let enabled = spec.when.holds(self);
                        let key = spec.key_text(mac);
                        let icon = spec.icon;
                        let response = ui
                            .add_enabled_ui(enabled, |ui| {
                                icons::tool_row(
                                    ui,
                                    command == shown,
                                    spec.title.trim_end_matches('…'),
                                    Some(key.as_str()).filter(|k| !k.is_empty()),
                                    |p, r, c| draw_icon(p, r, icon, c),
                                )
                            })
                            .inner;
                        if response.clicked() {
                            picked = Some(command);
                        }
                        if enabled {
                            rows.push((response.rect, command));
                        }
                        let tip = if enabled {
                            spec.description
                        } else {
                            spec.when.reason()
                        };
                        tips.push((response, tip));
                    }
                });
            });
        // A row's tooltip goes beside the flyout (left of it if there's no room on the
        // right), so it never covers the other rows.
        let frame = area.response.rect;
        for (response, tip) in &tips {
            let beside = egui::Rect::from_x_y_ranges(frame.x_range(), response.rect.y_range());
            let mut tooltip = if response.enabled() {
                egui::Tooltip::for_enabled(response)
            } else {
                egui::Tooltip::for_disabled(response)
            };
            tooltip.popup = tooltip
                .popup
                .anchor(beside)
                .align(egui::RectAlign::RIGHT_START)
                .align_alternatives(&[egui::RectAlign::RIGHT_START, egui::RectAlign::LEFT_START]);
            tooltip.show(|ui| {
                ui.label(*tip);
            });
        }
        let (pointer, released, pressed, escape) = ctx.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_released(),
                i.pointer.any_pressed(),
                i.key_pressed(egui::Key::Escape),
            )
        });
        // Held open: letting go over a row picks it.
        if flyout.held
            && released
            && let Some(at) = pointer
            && let Some((_, command)) = rows.iter().find(|(r, _)| r.contains(at))
        {
            picked = Some(*command);
        }
        let outside = pointer.is_some_and(|p| !area.response.rect.contains(p));
        if escape || (pressed && outside && ctx.cumulative_frame_nr() != flyout.opened) {
            self.rail_flyout = None;
        }
        picked
    }
}

/// A place on the rail as it is drawn: a row of a height with its name and key, or an icon
/// tile of a size.
#[derive(Clone, Copy)]
enum Place {
    Row(f32),
    Tile(egui::Vec2),
}

/// A command's icon from the registry.
fn draw_icon(p: &egui::Painter, r: egui::Rect, icon: Option<Icon>, c: egui::Color32) {
    match icon {
        Some(Icon::Tool(kind)) => icons::tool(p, r, kind, c),
        Some(Icon::Glyph(g)) => icons::glyph(p, r.shrink(1.0), g, c),
        None => {}
    }
}

/// A family's mark: a small triangle at the icon's lower right (big enough to be seen on a
/// large screen, so the flyout isn't known from the tooltip alone).
fn family_mark(p: &egui::Painter, icon: egui::Rect, c: egui::Color32) {
    let b = icon.right_bottom() + egui::vec2(3.0, 3.0);
    p.add(egui::Shape::convex_polygon(
        vec![b, b - egui::vec2(7.0, 0.0), b - egui::vec2(0.0, 7.0)],
        c,
        egui::Stroke::NONE,
    ));
}
