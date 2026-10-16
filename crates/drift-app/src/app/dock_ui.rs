//! The docks at either side of the canvas, their tabs, floating panels, and dragging panels
//! between them.
//!
//! - Dragging a tab along its tab bar moves it there, the others making room as it goes.
//! - Dragged out, a tab follows the pointer: onto another dock's tabs to dock it there (a
//!   bar shows where), onto the top or bottom of a floating panel to stack it there, or onto
//!   the canvas to float it.
//! - A floating panel on its own is moved by its header; over a dock, the dock lights up to
//!   take it. In a stack, a panel's header pulls it out, and the strip along the top moves
//!   the whole stack.
//!
//! Right-click a tab or use a header's menu for the same choices.

use std::collections::HashMap;

use crate::dock::{Panel, Place, Side};
use crate::icons::{self, Glyph};
use crate::*;

/// Height of a dock's tab bar.
const TAB_BAR: f32 = 34.0;
/// Width of a collapsed dock's icon rail.
const RAIL: f32 = 42.0;
/// How far into the canvas a drop still docks to that side (where there's no dock yet).
const DROP_BAND: f32 = 76.0;

/// Height of the strip that moves a stack of floating panels.
const GRIP: f32 = 12.0;
/// Space between stacked panels (it resizes them).
const DIVIDER: f32 = 7.0;

/// Panel interaction that lasts between frames but isn't saved.
#[derive(Default)]
pub(crate) struct DockView {
    /// A panel being dragged: by its tab or pulled from a stack (`false`), or as its own
    /// floating window, which moves with it (`true`).
    drag: Option<(Panel, bool)>,
    /// Where on the dragged tab it was grabbed (from its left edge).
    grab_dx: f32,
    /// Tabs, tab bars and docks as laid out this frame, for dropping.
    tabs: Vec<(Side, Panel, egui::Rect)>,
    bars: [Option<egui::Rect>; 2],
    docks: [Option<egui::Rect>; 2],
    /// Each floating panel as last drawn, and each floating window (by its top panel).
    stacked: Vec<(Panel, egui::Rect)>,
    windows: HashMap<Panel, egui::Rect>,
    /// Each stacked panel's share of its window's height (1 unless resized).
    weights: HashMap<Panel, f32>,
    /// The header of each floating panel as last drawn (the part that docks it when dragged).
    headers: HashMap<Panel, egui::Rect>,
    /// Where a panel just floated by dropping should appear.
    float_at: HashMap<Panel, egui::Pos2>,
    was_wide: Option<bool>,
    /// Every panel hidden for a clear view (Ctrl+\).
    pub hide_all: bool,
}

pub(crate) fn glyph(p: Panel) -> Glyph {
    match p {
        Panel::Inspector => Glyph::Inspector,
        Panel::Parameters => Glyph::Parameters,
        Panel::Layers => Glyph::Layers,
        Panel::Sheets => Glyph::Sheets,
        Panel::Blocks => Glyph::Blocks,
    }
}

/// A small icon button drawn at `rect`.
fn icon_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    g: Glyph,
    tip: &str,
) -> egui::Response {
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    let color = if response.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, ui.visuals().widgets.hovered.bg_fill);
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    icons::glyph(ui.painter(), rect.shrink(5.0), g, color);
    response.on_hover_text(tip)
}

impl DriftApp {
    /// Open a panel and bring it to the front (also undoes "hide all panels").
    pub(crate) fn show_panel(&mut self, p: Panel) {
        self.dock_view.hide_all = false;
        self.layout.show(p);
    }

    /// Show or hide a panel (Ctrl+L for the layers).
    pub(crate) fn toggle_panel(&mut self, p: Panel) {
        if self.dock_view.hide_all {
            self.show_panel(p);
        } else {
            self.layout.toggle(p);
        }
    }

    /// Start of a frame: forget last frame's tab positions; collapse the docks to rails when
    /// the window gets narrow (and open them again when it widens).
    pub(crate) fn begin_docks(&mut self, wide: bool) {
        self.dock_view.tabs.clear();
        self.dock_view.bars = [None, None];
        self.dock_view.docks = [None, None];
        if self.dock_view.was_wide != Some(wide) {
            if !wide {
                self.layout.collapsed = [true, true];
            } else if self.dock_view.was_wide == Some(false) {
                self.layout.collapsed = [false, false];
            }
            self.dock_view.was_wide = Some(wide);
        }
    }

    /// One dock (call between the toolbar and the canvas).
    pub(crate) fn dock(&mut self, ui: &mut egui::Ui, side: Side) {
        let panels = self.layout.dock(side).to_vec();
        if self.dock_view.hide_all || panels.is_empty() {
            return;
        }
        let i = side.index();
        let style = ui.style().clone();
        let make = |id: &str| match side {
            Side::Left => egui::Panel::left(format!("{id}_left")),
            Side::Right => egui::Panel::right(format!("{id}_right")),
        };
        let response = if self.layout.collapsed[i] {
            make("dock_rail")
                .resizable(false)
                .exact_size(RAIL)
                .frame(
                    egui::Frame::side_top_panel(&style).inner_margin(egui::Margin::symmetric(4, 6)),
                )
                .show(ui, |ui| self.rail(ui, side, &panels))
                .response
        } else {
            make("dock")
                .resizable(true)
                .default_size(310.0)
                .min_size(240.0)
                .frame(egui::Frame::side_top_panel(&style).inner_margin(egui::Margin::ZERO))
                .show(ui, |ui| {
                    let front = self.layout.active[i]
                        .filter(|p| panels.contains(p))
                        .unwrap_or(panels[0]);
                    self.tab_bar(ui, side, &panels, front);
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 14,
                            right: 8,
                            top: 2,
                            bottom: 0,
                        })
                        .show(ui, |ui| self.panel_body(ui, front));
                })
                .response
        };
        self.dock_view.docks[i] = Some(response.rect);
    }

    /// A panel's contents.
    pub(crate) fn panel_body(&mut self, ui: &mut egui::Ui, p: Panel) {
        let now = ui.input(|i| i.time);
        match p {
            Panel::Inspector => {
                egui::ScrollArea::vertical()
                    .id_salt("inspector_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.inspector(ui));
            }
            Panel::Parameters => self.parameters_panel(ui, now),
            Panel::Layers => self.layers_panel(ui, now),
            Panel::Sheets => self.sheets_panel(ui),
            Panel::Blocks => self.blocks_panel(ui),
        }
    }

    /// The tabs across the top of a dock: icon and name (just the icon when they don't fit),
    /// the front one underlined; then collapse and menu buttons.
    fn tab_bar(&mut self, ui: &mut egui::Ui, side: Side, panels: &[Panel], front: Panel) {
        let i = side.index();
        let (bar, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), TAB_BAR),
            egui::Sense::hover(),
        );
        let dark = ui.visuals().dark_mode;
        let accent = theme::accent(dark);
        self.dock_view.bars[i] = Some(bar);
        ui.painter().line_segment(
            [bar.left_bottom(), bar.right_bottom()],
            egui::Stroke::new(1.0, theme::hairline(ui)),
        );
        // A tab of this bar dragged along it: drawn under the pointer (below), its place
        // left empty.
        let pointer = ui.input(|inp| inp.pointer.latest_pos());
        let sliding = self.dock_view.drag.and_then(|(d, window)| {
            (!window && panels.contains(&d) && pointer.is_some_and(|at| in_bar(bar, at)))
                .then_some(d)
        });
        let font = egui::FontId::new(12.5, egui::FontFamily::Proportional);
        let widths: Vec<f32> = panels
            .iter()
            .map(|p| {
                ui.painter()
                    .layout_no_wrap(p.title().to_owned(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
                    + 37.0
            })
            .collect();
        let controls = 58.0;
        let room = bar.width() - controls - 8.0;
        // Every tab named if they fit; otherwise the front one named and the rest as icons.
        let all = widths.iter().sum::<f32>() + 2.0 * panels.len() as f32 <= room;
        let front_at = panels.iter().position(|p| *p == front).unwrap_or(0);
        let front_only = widths[front_at] + 34.0 * (panels.len() - 1) as f32 <= room;
        let mut x = bar.left() + 6.0;
        for (k, &p) in panels.iter().enumerate() {
            let labelled = all || (front_only && p == front);
            let w = if labelled { widths[k] } else { 32.0 };
            let r = egui::Rect::from_min_size(egui::pos2(x, bar.top() + 4.0), egui::vec2(w, 26.0));
            x += w + 2.0;
            let id = ui.id().with(("tab", p.key()));
            let response = ui.interact(r, id, egui::Sense::click_and_drag());
            let selected = p == front;
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, p.title())
            });
            self.dock_view.tabs.push((side, p, r));
            if response.drag_started() {
                self.dock_view.drag = Some((p, false));
                self.dock_view.grab_dx = response
                    .interact_pointer_pos()
                    .map_or(w / 2.0, |at| at.x - r.left());
            }
            response.context_menu(|ui| self.panel_menu(ui, p));
            if response.clicked() {
                self.layout.active[i] = Some(p);
            }
            let r = match (sliding, pointer) {
                (Some(d), Some(at)) if d == p => {
                    // Its place stays empty; the tab itself follows the pointer along the bar.
                    let x = (at.x - self.dock_view.grab_dx)
                        .clamp(bar.left() + 2.0, (bar.right() - w - 60.0).max(bar.left()));
                    let moved = egui::Rect::from_min_size(egui::pos2(x, r.top()), r.size());
                    ui.painter().rect(
                        moved,
                        6.0,
                        ui.visuals().widgets.active.bg_fill,
                        egui::Stroke::new(1.0, accent),
                        egui::StrokeKind::Inside,
                    );
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    moved
                }
                _ => r,
            };
            let dragging = self.dock_view.drag.is_some_and(|(d, _)| d == p);
            if (response.hovered() && !selected) || (dragging && sliding.is_none()) {
                ui.painter()
                    .rect_filled(r, 6.0, ui.visuals().widgets.hovered.bg_fill);
            }
            if selected {
                ui.painter().line_segment(
                    [
                        egui::pos2(r.left() + 8.0, bar.bottom() - 1.0),
                        egui::pos2(r.right() - 8.0, bar.bottom() - 1.0),
                    ],
                    egui::Stroke::new(2.0, accent),
                );
            }
            let color = if selected {
                ui.visuals().strong_text_color()
            } else if response.hovered() {
                ui.visuals().text_color()
            } else {
                ui.visuals().weak_text_color()
            };
            let icon_x = if labelled {
                r.left() + 17.0
            } else {
                r.center().x
            };
            icons::glyph(
                ui.painter(),
                egui::Rect::from_center_size(
                    egui::pos2(icon_x, r.center().y),
                    egui::Vec2::splat(15.0),
                ),
                glyph(p),
                color,
            );
            if labelled {
                let family = if selected {
                    theme::bold()
                } else {
                    egui::FontFamily::Proportional
                };
                ui.painter().text(
                    egui::pos2(r.left() + 29.0, r.center().y),
                    egui::Align2::LEFT_CENTER,
                    p.title(),
                    egui::FontId::new(12.5, family),
                    color,
                );
            }
            let _ = response.on_hover_ui(|ui| {
                ui.label(egui::RichText::new(p.title()).family(theme::bold()));
                ui.label(egui::RichText::new(p.description()).weak());
                ui.label(
                    egui::RichText::new(
                        "Drag to float it or dock it elsewhere; right-click for more",
                    )
                    .small()
                    .weak(),
                );
            });
        }
        // Collapse to a rail, and the dock's menu.
        let more = egui::Rect::from_center_size(
            egui::pos2(bar.right() - 20.0, bar.center().y),
            egui::Vec2::splat(24.0),
        );
        let collapse = more.translate(egui::vec2(-26.0, 0.0));
        let toward_edge = match side {
            Side::Left => Glyph::ChevronsLeft,
            Side::Right => Glyph::ChevronsRight,
        };
        if icon_at(
            ui,
            collapse,
            ui.id().with("collapse"),
            toward_edge,
            "Collapse to icons",
        )
        .clicked()
        {
            self.layout.collapsed[i] = true;
        }
        let menu = icon_at(ui, more, ui.id().with("more"), Glyph::More, "Panel options");
        egui::Popup::menu(&menu).show(|ui| {
            self.panel_menu(ui, front);
            let hidden: Vec<Panel> = Panel::ALL
                .into_iter()
                .filter(|p| self.layout.place(*p) == Place::Hidden)
                .collect();
            if !hidden.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new("Add to this dock").small().weak());
                for p in hidden {
                    if ui.button(p.title()).clicked() {
                        self.layout.move_to(p, Place::Dock(side), None);
                        ui.close();
                    }
                }
            }
        });
    }

    /// Where a panel can go: dock left or right, float, or close.
    fn panel_menu(&mut self, ui: &mut egui::Ui, p: Panel) {
        ui.label(egui::RichText::new(p.title()).small().weak());
        let place = self.layout.place(p);
        for (target, label) in [
            (Place::Dock(Side::Left), "Dock left"),
            (Place::Dock(Side::Right), "Dock right"),
            (Place::Float, "Float over the canvas"),
        ] {
            if ui
                .add(egui::Button::selectable(place == target, label))
                .clicked()
            {
                if place != target {
                    self.layout.move_to(p, target, None);
                }
                ui.close();
            }
        }
        ui.separator();
        let close = if p == Panel::Layers {
            egui::Button::new("Close").shortcut_text(app::panels::shortcut("L", false))
        } else {
            egui::Button::new("Close")
        };
        if ui.add(close).clicked() {
            self.layout.move_to(p, Place::Hidden, None);
            ui.close();
        }
    }

    /// A collapsed dock: an icon per panel; clicking one opens the dock on it.
    fn rail(&mut self, ui: &mut egui::Ui, side: Side, panels: &[Panel]) {
        let i = side.index();
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let away = match side {
                Side::Left => Glyph::ChevronsRight,
                Side::Right => Glyph::ChevronsLeft,
            };
            if icons::button(ui, 30.0, false, "Expand", |p, r, c| {
                icons::glyph(p, r, away, c.gamma_multiply(0.7))
            })
            .on_hover_text("Expand")
            .clicked()
            {
                self.layout.collapsed[i] = false;
            }
            ui.add_space(2.0);
            for &p in panels {
                let response = icons::button(ui, 32.0, false, p.title(), |painter, r, c| {
                    icons::glyph(painter, r, glyph(p), c)
                })
                .on_hover_text(p.title());
                if response.clicked() {
                    self.layout.active[i] = Some(p);
                    self.layout.collapsed[i] = false;
                }
                response.context_menu(|ui| self.panel_menu(ui, p));
            }
        });
    }

    /// Floating panels over the canvas, each stack in a window of its own (call after the
    /// canvas).
    pub(crate) fn floating_panels(&mut self, ctx: &egui::Context) {
        self.dock_view.stacked.clear();
        self.dock_view.windows.clear();
        if self.dock_view.hide_all {
            return;
        }
        let canvas = self.canvas_rect;
        if !canvas.is_positive() {
            return;
        }
        let dark = ctx.theme() == egui::Theme::Dark;
        let fill = if dark {
            egui::Color32::from_rgba_unmultiplied(0x26, 0x28, 0x2d, 244)
        } else {
            egui::Color32::from_rgba_unmultiplied(0xfb, 0xfb, 0xfc, 248)
        };
        let frame = egui::Frame::new()
            .fill(fill)
            .corner_radius(12.0)
            .stroke(egui::Stroke::new(
                1.0,
                if dark {
                    egui::Color32::from_white_alpha(18)
                } else {
                    egui::Color32::from_black_alpha(22)
                },
            ))
            .shadow(egui::epaint::Shadow {
                offset: [0, 8],
                blur: 26,
                spread: 0,
                color: egui::Color32::from_black_alpha(if dark { 100 } else { 45 }),
            })
            .inner_margin(egui::Margin::ZERO);
        for (k, stack) in self.layout.floating.clone().into_iter().enumerate() {
            let top = stack[0];
            let n = stack.len();
            let step = 26.0 * k as f32;
            let tall = (300.0 * n as f32).max(440.0).min(canvas.height() - 28.0);
            // A window per stack, named by its top panel and how many it holds, so a stack
            // that changes starts at its new size (and at the place it was, see `float_at`).
            let mut window = egui::Window::new(top.title())
                .id(egui::Id::new(("floating_panel", top.key(), n)))
                .title_bar(false)
                .resizable(true)
                .movable(true)
                .frame(frame)
                .default_size([300.0, tall])
                .min_width(240.0)
                .min_height(140.0 * n as f32)
                .pivot(egui::Align2::RIGHT_TOP)
                .default_pos(canvas.right_top() + egui::vec2(-14.0 - step, 14.0 + step))
                .constrain_to(canvas);
            if let Some(at) = self.dock_view.float_at.remove(&top) {
                window = window.pivot(egui::Align2::LEFT_TOP).current_pos(at);
            }
            let shown = window.show(ctx, |ui| {
                if n > 1 {
                    // The strip that moves the whole stack (it senses nothing itself, so the
                    // window takes the drag).
                    let (strip, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), GRIP),
                        egui::Sense::hover(),
                    );
                    let c = strip.center() + egui::vec2(0.0, 1.0);
                    let dots = ui.visuals().weak_text_color();
                    for dx in [-8.0, -3.0, 2.0, 7.0] {
                        ui.painter()
                            .circle_filled(c + egui::vec2(dx, 0.0), 1.2, dots);
                    }
                }
                let weights: Vec<f32> = stack
                    .iter()
                    .map(|p| self.dock_view.weights.get(p).copied().unwrap_or(1.0))
                    .collect();
                let total: f32 = weights.iter().sum();
                let room = (ui.available_height() - DIVIDER * (n - 1) as f32).max(60.0 * n as f32);
                for (j, &p) in stack.iter().enumerate() {
                    let h = (room * weights[j] / total).max(60.0);
                    let width = ui.available_width();
                    let inner = ui.allocate_ui_with_layout(
                        egui::vec2(width, h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_height(h);
                            ui.set_max_height(h);
                            self.float_header(ui, p, n > 1);
                            egui::Frame::new()
                                .inner_margin(egui::Margin {
                                    left: 12,
                                    right: 6,
                                    top: 0,
                                    bottom: 8,
                                })
                                .show(ui, |ui| self.panel_body(ui, p));
                        },
                    );
                    self.dock_view.stacked.push((p, inner.response.rect));
                    if j + 1 < n {
                        self.divider(ui, &stack, j, room);
                    }
                }
            });
            let Some(shown) = shown else {
                continue;
            };
            self.dock_view.windows.insert(top, shown.response.rect);
            // A panel floating alone, dragged by its header, can be docked or stacked.
            // Dragging inside it (a slider, a list, empty space) is just using it.
            let by_header = ctx.input(|i| i.pointer.press_origin()).is_some_and(|at| {
                self.dock_view
                    .headers
                    .get(&top)
                    .is_some_and(|bar| bar.contains(at))
            });
            if n == 1 && shown.response.dragged() && by_header && self.dock_view.drag.is_none() {
                self.dock_view.drag = Some((top, true));
            }
        }
    }

    /// A floating panel's header as last drawn (for tests).
    #[cfg(test)]
    pub(crate) fn header_rect(&self, p: Panel) -> Option<egui::Rect> {
        self.dock_view.headers.get(&p).copied()
    }

    /// The bar between two stacked panels: drag it to share the height differently.
    fn divider(&mut self, ui: &mut egui::Ui, stack: &[Panel], j: usize, room: f32) {
        let (r, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), DIVIDER),
            egui::Sense::drag(),
        );
        let hot = response.hovered() || response.dragged();
        let color = if hot {
            theme::accent(ui.visuals().dark_mode)
        } else {
            theme::hairline(ui)
        };
        ui.painter().line_segment(
            [
                egui::pos2(r.left() + 10.0, r.center().y),
                egui::pos2(r.right() - 10.0, r.center().y),
            ],
            egui::Stroke::new(if hot { 2.0 } else { 1.0 }, color),
        );
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        let dy = response.drag_delta().y;
        if dy != 0.0 {
            let w = &mut self.dock_view.weights;
            let total: f32 = stack.iter().map(|p| *w.get(p).unwrap_or(&1.0)).sum();
            let shift = dy / room * total;
            let (a, b) = (stack[j], stack[j + 1]);
            let (wa, wb) = (*w.get(&a).unwrap_or(&1.0), *w.get(&b).unwrap_or(&1.0));
            let min = 0.15 * total / stack.len() as f32;
            let shift = shift.clamp(min - wa, wb - min);
            w.insert(a, wa + shift);
            w.insert(b, wb - shift);
        }
    }

    /// A floating panel's header: its icon and name, a menu and a close button. Alone in its
    /// window, dragging it moves the window (or docks or stacks it); in a stack, dragging it
    /// pulls the panel out.
    fn float_header(&mut self, ui: &mut egui::Ui, p: Panel, in_stack: bool) {
        let sense = if in_stack {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::hover()
        };
        let (bar, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), sense);
        if in_stack && response.drag_started() {
            self.dock_view.drag = Some((p, false));
            self.dock_view.grab_dx = 30.0;
        }
        if in_stack && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let text = ui.visuals().strong_text_color();
        icons::glyph(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(bar.left() + 20.0, bar.center().y),
                egui::Vec2::splat(15.0),
            ),
            glyph(p),
            text,
        );
        ui.painter().text(
            egui::pos2(bar.left() + 34.0, bar.center().y),
            egui::Align2::LEFT_CENTER,
            p.title(),
            egui::FontId::new(13.0, theme::bold()),
            text,
        );
        ui.painter().line_segment(
            [
                bar.left_bottom() + egui::vec2(10.0, 0.0),
                bar.right_bottom() - egui::vec2(10.0, 0.0),
            ],
            egui::Stroke::new(1.0, theme::hairline(ui)),
        );
        let close = egui::Rect::from_center_size(
            egui::pos2(bar.right() - 20.0, bar.center().y),
            egui::Vec2::splat(24.0),
        );
        let more = close.translate(egui::vec2(-26.0, 0.0));
        // Closing a floating panel puts it back in the dock it came from (the menu beside it
        // can hide it altogether).
        let tip = format!("Back to the {} dock", self.layout.home(p).name());
        if icon_at(
            ui,
            close,
            ui.id().with(("close", p.key())),
            Glyph::Close,
            &tip,
        )
        .clicked()
        {
            self.redock_floating(p);
        }
        self.dock_view.headers.insert(p, bar);
        let menu = icon_at(
            ui,
            more,
            ui.id().with(("more", p.key())),
            Glyph::More,
            "Dock or close",
        );
        egui::Popup::menu(&menu).show(|ui| self.panel_menu(ui, p));
    }

    /// Put a floating panel back in its dock; the rest of its stack keeps its place.
    fn redock_floating(&mut self, p: Panel) {
        let keep = self.stack_position(p);
        self.layout.redock(p);
        self.restore_stack_position(keep);
    }

    /// Before a panel leaves its stack: the panel that will be on top of what's left, and
    /// where the window is, so what's left stays there (its window is a new one).
    fn stack_position(&self, p: Panel) -> Option<(Panel, egui::Pos2)> {
        let stack = self.layout.stack_of(p)?;
        if stack.len() < 2 {
            return None;
        }
        let at = self.dock_view.windows.get(&stack[0])?.min;
        let top = if stack[0] == p { stack[1] } else { stack[0] };
        Some((top, at))
    }

    fn restore_stack_position(&mut self, keep: Option<(Panel, egui::Pos2)>) {
        if let Some((top, at)) = keep {
            self.dock_view.float_at.insert(top, at);
        }
    }

    /// Where a dragged panel would dock: each side's dock, or a band along that edge of the
    /// canvas when the side has no dock.
    fn drop_zones(&self) -> Vec<(Side, egui::Rect)> {
        let canvas = self.canvas_rect;
        [Side::Left, Side::Right]
            .into_iter()
            .map(|side| {
                let zone = self.dock_view.docks[side.index()].unwrap_or_else(|| match side {
                    Side::Left => egui::Rect::from_min_max(
                        canvas.left_top(),
                        egui::pos2(canvas.left() + DROP_BAND, canvas.bottom()),
                    ),
                    Side::Right => egui::Rect::from_min_max(
                        egui::pos2(canvas.right() - DROP_BAND, canvas.top()),
                        canvas.right_bottom(),
                    ),
                });
                (side, zone)
            })
            .collect()
    }

    /// Where a dragged panel would go if let go at `pos`: onto the top or bottom of a floating
    /// panel (to stack there), or into a dock (among its tabs, when over its tab bar).
    fn drop_target(&self, panel: Panel, window: bool, pos: egui::Pos2) -> Option<Drop> {
        // A window being moved carries its whole stack: not onto itself.
        let own: Vec<Panel> = if window {
            self.layout
                .stack_of(panel)
                .map_or(vec![panel], <[Panel]>::to_vec)
        } else {
            vec![panel]
        };
        if let Some((onto, rect)) = self
            .dock_view
            .stacked
            .iter()
            .rev()
            .find(|(p, r)| !own.contains(p) && r.contains(pos))
        {
            return Some(Drop::Stack {
                onto: *onto,
                below: pos.y > rect.center().y,
                rect: *rect,
            });
        }
        let (side, zone) = self
            .drop_zones()
            .into_iter()
            .find(|(_, r)| r.contains(pos))?;
        let bar = self.dock_view.bars[side.index()];
        let index = bar.filter(|b| in_bar(*b, pos)).map(|_| {
            self.dock_view
                .tabs
                .iter()
                .filter(|(s, p, r)| *s == side && *p != panel && r.center().x < pos.x)
                .count()
        });
        Some(Drop::Dock { side, index, zone })
    }

    /// While a panel is dragged: show where it would go, and put it there on release (call
    /// last in the frame).
    pub(crate) fn dock_drag(&mut self, ctx: &egui::Context) {
        let Some((panel, window)) = self.dock_view.drag else {
            return;
        };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.dock_view.drag = None;
            return;
        }
        let released = ctx.input(|i| i.pointer.any_released() || !i.pointer.any_down());
        let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else {
            if released {
                self.dock_view.drag = None;
            }
            return;
        };
        // A tab slid along its own tab bar takes its new place as it goes (drawn by the bar).
        if !window
            && let Place::Dock(side) = self.layout.place(panel)
            && let Some(bar) = self.dock_view.bars[side.index()]
            && in_bar(bar, pos)
        {
            let i = side.index();
            let slot = self
                .dock_view
                .tabs
                .iter()
                .filter(|(s, p, r)| *s == side && *p != panel && r.center().x < pos.x)
                .count();
            if let Some(from) = self.layout.docks[i].iter().position(|p| *p == panel)
                && slot != from
            {
                let index = if slot > from { slot + 1 } else { slot };
                self.layout.move_to(panel, Place::Dock(side), Some(index));
            }
            if released {
                self.dock_view.drag = None;
            }
            return;
        }
        let dark = ctx.theme() == egui::Theme::Dark;
        let accent = theme::accent(dark);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("dock_drop"),
        ));
        let target = self.drop_target(panel, window, pos);
        let mark = egui::Stroke::new(3.0, accent);
        match target {
            Some(Drop::Stack { below, rect, .. }) => {
                // A bar along the edge it would stack against.
                let y = if below {
                    rect.bottom() - 1.0
                } else {
                    rect.top() + 1.0
                };
                painter.line_segment(
                    [
                        egui::pos2(rect.left() + 8.0, y),
                        egui::pos2(rect.right() - 8.0, y),
                    ],
                    mark,
                );
            }
            Some(Drop::Dock { side, index, zone }) => {
                if window {
                    // A floating panel over a dock: the dock lights up to take it.
                    let r = zone.shrink(3.0);
                    painter.rect(
                        r,
                        8.0,
                        accent.gamma_multiply(0.18),
                        egui::Stroke::new(1.5, accent),
                        egui::StrokeKind::Inside,
                    );
                    painter.text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("Dock {}", side.name()),
                        egui::FontId::new(13.0, theme::bold()),
                        accent,
                    );
                } else if let (Some(k), Some(bar)) = (index, self.dock_view.bars[side.index()]) {
                    // A tab over another dock's tabs: a bar where it would go.
                    let tabs: Vec<egui::Rect> = self
                        .dock_view
                        .tabs
                        .iter()
                        .filter(|(s, p, _)| *s == side && *p != panel)
                        .map(|(_, _, r)| *r)
                        .collect();
                    let x = match (k.checked_sub(1).and_then(|j| tabs.get(j)), tabs.get(k)) {
                        (Some(before), _) => before.right() + 1.0,
                        (None, Some(after)) => after.left() - 1.0,
                        (None, None) => bar.left() + 6.0,
                    };
                    painter.line_segment(
                        [
                            egui::pos2(x, bar.top() + 5.0),
                            egui::pos2(x, bar.bottom() - 5.0),
                        ],
                        mark,
                    );
                } else if self.layout.place(panel) != Place::Dock(side) {
                    // Over another dock (or the edge where one would be): its outline.
                    painter.rect_stroke(
                        zone.shrink(3.0),
                        8.0,
                        egui::Stroke::new(1.5, accent),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            None => {}
        }
        if !window {
            self.paint_dragged_tab(ctx, &painter, panel, pos);
        }
        if !released {
            return;
        }
        self.dock_view.drag = None;
        match target {
            Some(Drop::Stack { onto, below, .. }) => {
                // The window it joins stays where it is (it becomes a new window).
                let joined = self
                    .layout
                    .stack_of(onto)
                    .and_then(|s| self.dock_view.windows.get(&s[0]))
                    .map(|r| r.min);
                let keep = self.stack_position(panel);
                self.layout.stack(panel, onto, below);
                self.restore_stack_position(keep);
                if let (Some(at), Some(stack)) = (joined, self.layout.stack_of(onto)) {
                    self.dock_view.float_at.insert(stack[0], at);
                }
            }
            Some(Drop::Dock { side, index, .. }) => {
                // Dropped on its own dock away from the tabs: nothing to do.
                if index.is_none() && self.layout.place(panel) == Place::Dock(side) {
                    return;
                }
                let keep = self.stack_position(panel);
                self.layout.move_to(panel, Place::Dock(side), index);
                self.restore_stack_position(keep);
            }
            None if !window && self.canvas_rect.contains(pos) => {
                let keep = self.stack_position(panel);
                self.layout.move_to(panel, Place::Float, None);
                self.restore_stack_position(keep);
                self.dock_view
                    .float_at
                    .insert(panel, pos - egui::vec2(30.0, 14.0));
            }
            None => {}
        }
    }

    /// A dragged tab (or a panel pulled from a stack) under the pointer.
    fn paint_dragged_tab(
        &self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        panel: Panel,
        pos: egui::Pos2,
    ) {
        let visuals = ctx.style_of(ctx.theme()).visuals.clone();
        let accent = theme::accent(visuals.dark_mode);
        let text = visuals.strong_text_color();
        let galley = painter.layout_no_wrap(
            panel.title().to_owned(),
            egui::FontId::new(12.5, theme::bold()),
            text,
        );
        let r = egui::Rect::from_min_size(
            pos - egui::vec2(self.dock_view.grab_dx.min(40.0), 13.0),
            egui::vec2(galley.size().x + 40.0, 28.0),
        );
        painter.rect(
            r,
            7.0,
            visuals.window_fill,
            egui::Stroke::new(1.0, accent),
            egui::StrokeKind::Inside,
        );
        icons::glyph(
            painter,
            egui::Rect::from_center_size(
                egui::pos2(r.left() + 16.0, r.center().y),
                egui::Vec2::splat(14.0),
            ),
            glyph(panel),
            text,
        );
        painter.galley(
            egui::pos2(r.left() + 28.0, r.center().y - galley.size().y / 2.0),
            galley,
            text,
        );
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }
}

/// Where a dragged panel would go.
#[derive(Clone, Copy, Debug)]
enum Drop {
    /// Into the floating stack `onto` is in, above or below it (`rect` is `onto`'s).
    Stack {
        onto: Panel,
        below: bool,
        rect: egui::Rect,
    },
    /// Into a dock: among its tabs at `index`, or (`None`) at the end.
    Dock {
        side: Side,
        index: Option<usize>,
        zone: egui::Rect,
    },
}

/// Whether `at` is on (or just above or below) a tab bar: a dragged tab there slides along it.
fn in_bar(bar: egui::Rect, at: egui::Pos2) -> bool {
    bar.expand2(egui::vec2(0.0, TAB_BAR * 0.6)).contains(at)
}
