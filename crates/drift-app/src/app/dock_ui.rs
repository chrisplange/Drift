//! The docks at either side of the canvas, their tabs, floating panels, and dragging panels
//! between them: drag a tab (or a floating panel's header) onto a dock to dock it there, or
//! a tab onto the canvas to float it. Right-click a tab for the same choices.

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

/// Panel interaction that lasts between frames but isn't saved.
#[derive(Default)]
pub(crate) struct DockView {
    /// A panel being dragged: by its tab (`false`) or its floating window (`true`).
    drag: Option<(Panel, bool)>,
    /// Tabs and docks as laid out this frame, for dropping.
    tabs: Vec<(Side, egui::Rect)>,
    docks: [Option<egui::Rect>; 2],
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
        ui.painter().line_segment(
            [bar.left_bottom(), bar.right_bottom()],
            egui::Stroke::new(1.0, theme::hairline(ui)),
        );
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
            let dragging = self.dock_view.drag.is_some_and(|(d, _)| d == p);
            if (response.hovered() && !selected) || dragging {
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
            let response = response.on_hover_ui(|ui| {
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
            if response.clicked() {
                self.layout.active[i] = Some(p);
            }
            if response.drag_started() {
                self.dock_view.drag = Some((p, false));
            }
            response.context_menu(|ui| self.panel_menu(ui, p));
            self.dock_view.tabs.push((side, r));
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

    /// Floating panels over the canvas (call after the canvas).
    pub(crate) fn floating_panels(&mut self, ctx: &egui::Context) {
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
        for (k, p) in self.layout.floating.clone().into_iter().enumerate() {
            let step = 26.0 * k as f32;
            let mut window = egui::Window::new(p.title())
                .id(egui::Id::new(("floating_panel", p.key())))
                .title_bar(false)
                .resizable(true)
                .movable(true)
                .frame(frame)
                .default_size([300.0, 440.0_f32.min(canvas.height() - 28.0)])
                .min_width(240.0)
                .min_height(140.0)
                .pivot(egui::Align2::RIGHT_TOP)
                .default_pos(canvas.right_top() + egui::vec2(-14.0 - step, 14.0 + step))
                .constrain_to(canvas);
            if let Some(at) = self.dock_view.float_at.remove(&p) {
                window = window.pivot(egui::Align2::LEFT_TOP).current_pos(at);
            }
            let shown = window.show(ctx, |ui| {
                self.float_header(ui, p);
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 6,
                        top: 0,
                        bottom: 8,
                    })
                    .show(ui, |ui| self.panel_body(ui, p));
            });
            if let Some(shown) = shown
                && shown.response.dragged()
                && self.dock_view.drag.is_none()
            {
                self.dock_view.drag = Some((p, true));
            }
        }
    }

    /// A floating panel's header: drag it to move (or onto a dock to dock it).
    fn float_header(&mut self, ui: &mut egui::Ui, p: Panel) {
        let (bar, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::hover());
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
        let tip = if p == Panel::Layers {
            format!("Close ({})", app::panels::shortcut("L", false))
        } else {
            "Close".to_owned()
        };
        if icon_at(ui, close, ui.id().with("close"), Glyph::Close, &tip).clicked() {
            self.layout.move_to(p, Place::Hidden, None);
        }
        let menu = icon_at(ui, more, ui.id().with("more"), Glyph::More, "Dock or close");
        egui::Popup::menu(&menu).show(|ui| self.panel_menu(ui, p));
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

    /// While a panel is dragged: highlight where it would dock, and dock or float it on
    /// release (call last in the frame).
    pub(crate) fn dock_drag(&mut self, ctx: &egui::Context) {
        let Some((panel, window)) = self.dock_view.drag else {
            return;
        };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.dock_view.drag = None;
            return;
        }
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let released = ctx.input(|i| i.pointer.any_released() || !i.pointer.any_down());
        let zones = self.drop_zones();
        let target = pointer.and_then(|pos| zones.iter().find(|(_, r)| r.contains(pos)).copied());
        let dark = ctx.theme() == egui::Theme::Dark;
        let accent = theme::accent(dark);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("dock_drop"),
        ));
        for (side, zone) in &zones {
            let hot = target.is_some_and(|(s, _)| s == *side);
            let r = zone.shrink(3.0);
            if hot {
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
            } else {
                painter.rect(
                    r,
                    8.0,
                    accent.gamma_multiply(0.05),
                    egui::Stroke::new(1.0, accent.gamma_multiply(0.35)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if !window && let Some(pos) = pointer {
            // The tab follows the pointer.
            let text = ctx.style_of(ctx.theme()).visuals.strong_text_color();
            let galley = painter.layout_no_wrap(
                panel.title().to_owned(),
                egui::FontId::new(12.5, theme::bold()),
                text,
            );
            let r = egui::Rect::from_min_size(
                pos + egui::vec2(10.0, 8.0),
                egui::vec2(galley.size().x + 40.0, 28.0),
            );
            painter.rect(
                r,
                7.0,
                ctx.style_of(ctx.theme()).visuals.window_fill,
                egui::Stroke::new(1.0, accent),
                egui::StrokeKind::Inside,
            );
            icons::glyph(
                &painter,
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
        if !released {
            return;
        }
        self.dock_view.drag = None;
        let Some(pos) = pointer else {
            return;
        };
        match target {
            Some((side, _)) => {
                // Over a tab bar, the drop picks the place among the tabs.
                let tabs: Vec<egui::Rect> = self
                    .dock_view
                    .tabs
                    .iter()
                    .filter(|(s, _)| *s == side)
                    .map(|(_, r)| *r)
                    .collect();
                let on_bar = tabs.iter().any(|r| (r.top()..=r.bottom()).contains(&pos.y));
                let index = on_bar.then(|| tabs.iter().filter(|r| r.center().x < pos.x).count());
                // Dropped on its own dock away from the tabs: nothing to do.
                if index.is_none() && self.layout.place(panel) == Place::Dock(side) {
                    return;
                }
                self.layout.move_to(panel, Place::Dock(side), index);
            }
            None if !window && self.canvas_rect.contains(pos) => {
                self.layout.move_to(panel, Place::Float, None);
                self.dock_view
                    .float_at
                    .insert(panel, pos - egui::vec2(30.0, 14.0));
            }
            None => {}
        }
    }
}
