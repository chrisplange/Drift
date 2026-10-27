//! The tool rail: the registry's [`commands::RAIL`] as rows with names and keys (icons in two
//! columns in a narrow window). A family (the circle's ellipses, Trim's Extend and Split)
//! takes one place, showing the one used last; right-click it, or hold the button down, for
//! the rest in a flyout.

use crate::app::palette::Command;
use crate::commands::{self, Icon, RAIL, Slot};
use crate::icons;
use crate::*;

/// How long a press on a family opens its flyout.
const HOLD: f64 = 0.35;
/// Row heights: normal, and in a window too short for the rail.
const ROW: f32 = 24.0;
const SHORT_ROW: f32 = 22.0;
/// The narrow rail's icon tiles.
const TILE: f32 = 32.0;

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

impl DriftApp {
    /// The tool rail: icon rows in sections (Draw, Build, Modify, Annotate), with tooltips
    /// that name the tool, its keys and what it does.
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui, wide: bool) {
        // On a sheet, the rail holds what can be done on paper.
        if self.sheet_preview {
            self.sheet_rail(ui, wide);
            return;
        }
        self.note_family_tool();
        // Too short for the rail's rows: shorter rows, and hairlines for the headers.
        let rows: usize = RAIL.iter().map(|(_, slots)| slots.len()).sum();
        let headers = RAIL.iter().filter(|(c, _)| c.is_some()).count();
        let short =
            wide && ui.available_height() < rows as f32 * ROW + headers as f32 * 26.0 + 12.0;
        let mut picked = None;
        let output = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                for (i, (category, slots)) in RAIL.iter().enumerate() {
                    if wide && !short {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if let Some(c) = category {
                            ui.add_space(if i > 0 { 7.0 } else { 2.0 });
                            let color = theme::neutrals(ui.visuals().dark_mode).secondary;
                            theme::caps_header_in(ui, c.label(), color);
                            ui.add_space(2.0);
                        }
                        for slot in *slots {
                            picked = picked.or(self.rail_place(ui, slot, Some(ROW)));
                        }
                    } else {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if i > 0 {
                            ui.add_space(3.0);
                            ui.separator();
                            ui.add_space(3.0);
                        }
                        if wide {
                            for slot in *slots {
                                picked = picked.or(self.rail_place(ui, slot, Some(SHORT_ROW)));
                            }
                        } else {
                            // Two to a row.
                            for pair in slots.chunks(2) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    for slot in pair {
                                        picked = picked.or(self.rail_place(ui, slot, None));
                                    }
                                });
                                ui.add_space(2.0);
                            }
                        }
                    }
                }
                ui.add_space(6.0);
            });
        theme::fade_bottom(ui, &output, ui.visuals().panel_fill);
        picked = picked.or(self.rail_flyout_ui(ui.ctx()));
        if let Some(command) = picked {
            self.rail_flyout = None;
            let ctx = ui.ctx().clone();
            self.run_command(&ctx, command);
            // A family shows what was picked from it (a command as much as a tool).
            if let Some(slot) = commands::rail_slot(command)
                && !slot.more.is_empty()
            {
                self.show_in_family(slot.head, command);
            }
        }
    }

    /// The current tool, if it's in a family, is what that family shows from now on (picked
    /// by key, menu or palette as much as on the rail).
    fn note_family_tool(&mut self) {
        let current = Command::Tool(self.tool.kind());
        if let Some(slot) = commands::rail_slot(current)
            && !slot.more.is_empty()
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

    /// One place on the rail: a row (`Some(height)`) or an icon tile. Returns the command
    /// to run if it was clicked.
    fn rail_place(&mut self, ui: &mut egui::Ui, slot: &Slot, row: Option<f32>) -> Option<Command> {
        let command = self.rail_shows(slot);
        let spec = commands::spec(command)?;
        let family = !slot.more.is_empty();
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
            .add_enabled_ui(enabled, |ui| match row {
                Some(height) => icons::tool_row_sized(
                    ui,
                    height,
                    selected,
                    label,
                    Some(key.as_str()).filter(|k| !k.is_empty()),
                    draw,
                ),
                None => icons::button(ui, TILE, selected, label, draw),
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
            let open = |held| Flyout {
                head: slot.head,
                at: response.rect.right_top() + egui::vec2(6.0, -4.0),
                held,
                opened: frame,
            };
            if response.secondary_clicked() {
                self.rail_flyout = Some(open(false));
            } else if held && self.rail_flyout.is_none_or(|f| f.head != slot.head) {
                self.rail_flyout = Some(open(true));
            }
        }
        // No row's tooltip while a flyout is open: it would draw over the flyout's rows.
        let response = if self.rail_flyout.is_none() {
            response
                .on_hover_ui(|ui| self.rail_tooltip(ui, slot, command))
                .on_disabled_hover_ui(|ui| {
                    self.rail_tooltip(ui, slot, command);
                    ui.label(egui::RichText::new(spec.when.reason()).small().weak());
                })
        } else {
            response
        };
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

    /// The tooltip: the name and keys, what it does, and for a family what else is in it.
    fn rail_tooltip(&self, ui: &mut egui::Ui, slot: &Slot, command: Command) {
        let Some(spec) = commands::spec(command) else {
            return;
        };
        ui.horizontal(|ui| {
            let name = spec.title.trim_end_matches('…');
            ui.label(egui::RichText::new(name).family(theme::bold()));
            let keys = spec.all_keys(cfg!(target_os = "macos"));
            let keys = if keys.is_empty() {
                spec.key_text(cfg!(target_os = "macos"))
            } else {
                keys
            };
            if !keys.is_empty() {
                ui.label(egui::RichText::new(keys).small().weak());
            }
        });
        ui.label(egui::RichText::new(spec.description).weak());
        if let Command::Tool(kind) = command
            && let Some(again) = kind.cycle_hint()
        {
            ui.label(egui::RichText::new(again).small().weak());
        }
        if !slot.more.is_empty() {
            let others: Vec<&str> = slot
                .commands()
                .filter(|c| *c != command)
                .filter_map(commands::spec)
                .map(|s| s.title.trim_end_matches('…'))
                .collect();
            ui.label(
                egui::RichText::new(format!("Right-click or hold for {}", others.join(", ")))
                    .small()
                    .weak(),
            );
        }
    }

    /// A filter on: a badge on the Select tool, so it isn't forgotten.
    fn pick_filter_badge(&self, ui: &mut egui::Ui, response: &egui::Response) {
        let filter = self.editor.pick_filter;
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
                    for command in slot.commands() {
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

/// A command's icon from the registry.
fn draw_icon(p: &egui::Painter, r: egui::Rect, icon: Option<Icon>, c: egui::Color32) {
    match icon {
        Some(Icon::Tool(kind)) => icons::tool(p, r, kind, c),
        Some(Icon::Glyph(g)) => icons::glyph(p, r.shrink(1.0), g, c),
        None => {}
    }
}

/// A family's mark: a small triangle at the icon's lower right.
fn family_mark(p: &egui::Painter, icon: egui::Rect, c: egui::Color32) {
    let b = icon.right_bottom() + egui::vec2(2.0, 2.0);
    p.add(egui::Shape::convex_polygon(
        vec![b, b - egui::vec2(5.0, 0.0), b - egui::vec2(0.0, 5.0)],
        c,
        egui::Stroke::NONE,
    ));
}
