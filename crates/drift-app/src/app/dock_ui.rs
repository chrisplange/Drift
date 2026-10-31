//! The docks at either side of the canvas, their tabs, floating panels, and dragging panels
//! between them.
//!
//! - Dragging a tab along its tab bar moves it there, the others making room as it goes.
//! - Dragged out, a tab follows the pointer: onto another dock's tabs to dock it there (a
//!   bar shows where), onto the top or bottom of a floating panel to stack it there, or onto
//!   the canvas to float it.
//! - A floating panel on its own is moved by its header; over a dock, the dock lights up to
//!   take it. In a stack, the top header moves the whole stack (with Alt it pulls the top
//!   panel out) and the other headers pull their panel out.
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

/// Height of a floating panel's header.
const HEADER: f32 = 34.0;
/// The least height a stacked panel is given (room allowing): its header and a little more.
const MIN_PANEL: f32 = HEADER + 64.0;
/// Space around a panel's contents, docked or floating.
const BODY_MARGIN: egui::Margin = egui::Margin {
    left: 14,
    right: 8,
    top: 2,
    bottom: 0,
};
/// How far past its dock (or its stack's window) a dragged tab must go before it comes
/// away: let go nearer and nothing changes.
const DETACH: f32 = 40.0;
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
    /// The floating windows as drawn the frame before (`windows` fills as they are drawn).
    last_windows: HashMap<Panel, egui::Rect>,
    /// How tall each panel's contents were when last drawn (nothing cut off).
    content: HashMap<Panel, f32>,
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
                        .inner_margin(BODY_MARGIN)
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
                let output = egui::ScrollArea::vertical()
                    .id_salt("inspector_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.inspector(ui));
                self.panel_scrolled(ui, p, &output);
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
        // In a stack, floating is coming out of it into a window of its own.
        let stacked = self.layout.stack_of(p).is_some_and(|s| s.len() > 1);
        let float = if stacked {
            "Float on its own"
        } else {
            "Float over the canvas"
        };
        for (target, label) in [
            (Place::Dock(Side::Left), "Dock left"),
            (Place::Dock(Side::Right), "Dock right"),
            (Place::Float, float),
        ] {
            let here = place == target && !stacked;
            if ui.add(egui::Button::selectable(here, label)).clicked() {
                if !here {
                    let keep = self.stack_position(p);
                    self.layout.move_to(p, target, None);
                    self.restore_stack_position(keep);
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

    /// A collapsed dock: an icon per panel, the one in front marked; clicking one opens the
    /// dock on it.
    fn rail(&mut self, ui: &mut egui::Ui, side: Side, panels: &[Panel]) {
        let i = side.index();
        let front = self.layout.active[i]
            .filter(|p| panels.contains(p))
            .unwrap_or(panels[0]);
        let accent = theme::accent(ui.visuals().dark_mode);
        let edge = ui.max_rect();
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
                    let c = if p == front {
                        c
                    } else {
                        c.gamma_multiply(0.55)
                    };
                    icons::glyph(painter, r, glyph(p), c)
                });
                if p == front {
                    // A bar on the canvas side, like the front tab's underline.
                    let r = response.rect;
                    let x = match side {
                        Side::Left => edge.right() + 2.0,
                        Side::Right => edge.left() - 2.0,
                    };
                    ui.painter().line_segment(
                        [
                            egui::pos2(x, r.top() + 7.0),
                            egui::pos2(x, r.bottom() - 7.0),
                        ],
                        egui::Stroke::new(2.0, accent),
                    );
                }
                let response = response.on_hover_text(if p == front {
                    format!("{} (in front)", p.title())
                } else {
                    p.title().to_owned()
                });
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
        self.dock_view.last_windows = std::mem::take(&mut self.dock_view.windows);
        if self.dock_view.hide_all {
            return;
        }
        let canvas = self.drawing.view.canvas_rect;
        if !canvas.is_positive() {
            return;
        }
        // Opaque, like the dialogs: nothing underneath shows through.
        let frame =
            egui::Frame::window(&ctx.style_of(ctx.theme())).inner_margin(egui::Margin::ZERO);
        // Never taller than the canvas: egui lets a window taller than the area it is kept in
        // slide up past its top, where the header would be cut off under the bar above.
        let most = canvas.height().floor();
        for (k, stack) in self.layout.floating.clone().into_iter().enumerate() {
            let top = stack[0];
            let n = stack.len();
            let step = 26.0 * k as f32;
            let tall = (300.0 * n as f32).max(440.0).min(most - 28.0);
            let least = MIN_PANEL * n as f32 + DIVIDER * (n - 1) as f32;
            // Every panel folded: a window just the headers, as wide as it was.
            let shut = stack.iter().all(|p| self.layout.folded.contains(p));
            let headers = (HEADER * n as f32 + DIVIDER * (n - 1) as f32).min(most);
            // A window per stack, named by its top panel and how many it holds, so a stack
            // that changes starts at its new size (and at the place it was, see `float_at`).
            let mut window = egui::Window::new(top.title())
                .id(egui::Id::new(("floating_panel", top.key(), n, shut)))
                .title_bar(false)
                .resizable(true)
                .movable(true)
                .frame(frame)
                .default_size([300.0, tall])
                .min_width(240.0)
                .min_height(least.min(most))
                .max_height(most)
                // Placed by its top left (see `float_at`), at first in the canvas's top right.
                .default_pos(canvas.right_top() + egui::vec2(-314.0 - step, 14.0 + step))
                .constrain_to(canvas);
            // As wide as it was last frame (a window new to this stack: the default width).
            let width = self
                .dock_view
                .last_windows
                .get(&top)
                .map_or(300.0, |r| r.width());
            if shut {
                window = window
                    .resizable(false)
                    .min_height(headers)
                    .fixed_size([width, headers]);
            }
            if let Some(at) = self.dock_view.float_at.remove(&top) {
                window = window.current_pos(at);
            }
            let alt = ctx.input(|i| i.modifiers.alt);
            let shown = window.show(ctx, |ui| {
                // Each panel gets a fixed share of the window and scrolls within it, so what
                // it holds never makes the window taller.
                // (Nothing between them but the dividers.)
                let spacing = ui.spacing().item_spacing;
                ui.spacing_mut().item_spacing.y = 0.0;
                let room = (ui.available_height() - DIVIDER * (n - 1) as f32).max(0.0);
                let folded: Vec<bool> = stack
                    .iter()
                    .map(|p| self.layout.folded.contains(p))
                    .collect();
                let heights = self.stack_heights(&stack, &folded, room);
                let width = ui.available_width();
                for (j, &p) in stack.iter().enumerate() {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, heights[j]), egui::Sense::hover());
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    child.set_clip_rect(rect.intersect(ui.clip_rect()));
                    child.spacing_mut().item_spacing = spacing;
                    // The top header moves the window (Alt pulls the top panel out); the
                    // others pull their panel out of the stack.
                    let role = match (n, j) {
                        (1, _) => Header::Alone,
                        (_, 0) if !alt => Header::Top,
                        _ => Header::Pulls,
                    };
                    self.float_header(&mut child, p, role, folded[j]);
                    if !folded[j] {
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                bottom: 8,
                                ..BODY_MARGIN
                            })
                            .show(&mut child, |ui| self.panel_body(ui, p));
                    }
                    self.dock_view.stacked.push((p, rect));
                    if j + 1 < n {
                        self.divider(ui, &stack, j, &heights, &folded);
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

    /// A floating panel (header and contents) as last drawn, and the window it is in (for
    /// tests).
    #[cfg(test)]
    pub(crate) fn floating_rects(&self, p: Panel) -> Option<(egui::Rect, egui::Rect)> {
        let panel = self.dock_view.stacked.iter().find(|(q, _)| *q == p)?.1;
        let top = self.layout.stack_of(p)?[0];
        Some((panel, *self.dock_view.windows.get(&top)?))
    }

    /// After a panel's scrolled contents: fade the bottom when there is more below, and note
    /// how tall the contents are (nothing cut off), for sharing a stack's height by them.
    pub(crate) fn panel_scrolled(
        &mut self,
        ui: &egui::Ui,
        p: Panel,
        output: &egui::scroll_area::ScrollAreaOutput<()>,
    ) {
        let behind = if self.layout.place(p) == Place::Float {
            ui.visuals().window_fill
        } else {
            ui.visuals().panel_fill
        };
        theme::fade_bottom(ui, output, behind);
        self.dock_view.content.insert(p, output.content_size.y);
    }

    /// How a stack's panels share `room`: a folded one just its header, and the open ones
    /// the rest (see [`Self::share_heights`]).
    fn stack_heights(&self, stack: &[Panel], folded: &[bool], room: f32) -> Vec<f32> {
        let open: Vec<Panel> = stack
            .iter()
            .zip(folded)
            .filter(|(_, f)| !**f)
            .map(|(p, _)| *p)
            .collect();
        let shut = (stack.len() - open.len()) as f32;
        let mut shares = self
            .share_heights(&open, (room - HEADER * shut).max(0.0))
            .into_iter();
        folded
            .iter()
            .map(|f| {
                if *f {
                    HEADER
                } else {
                    shares.next().unwrap_or(0.0)
                }
            })
            .collect()
    }

    /// How open stacked panels share `room`: as the stack's dividers were last dragged, or
    /// else by what they hold. A panel that needs less than an even share gets just that, and
    /// the others share the rest; what is left when everything fits goes to the Inspector (or
    /// the bottom panel). Each gets at least its header and a little more, room allowing.
    fn share_heights(&self, stack: &[Panel], room: f32) -> Vec<f32> {
        let n = stack.len();
        if n == 0 {
            return Vec::new();
        }
        let n = stack.len();
        let least = MIN_PANEL.min(room / n as f32);
        let mut heights = if let Some(weights) = self.layout.stack_weights(stack) {
            let total: f32 = weights.iter().sum();
            weights.iter().map(|w| room * w / total).collect()
        } else {
            // What each would take to show everything (unknown until drawn once).
            let wants: Vec<f32> = stack
                .iter()
                .map(|p| {
                    // (With the margins, and a little to spare.)
                    self.dock_view.content.get(p).map_or(f32::INFINITY, |h| {
                        HEADER + h + f32::from(BODY_MARGIN.top) + 8.0 + 8.0
                    })
                })
                .collect();
            let mut heights = vec![0.0; n];
            let mut open: Vec<usize> = (0..n).collect();
            let mut left = room;
            loop {
                if open.is_empty() {
                    let gets = stack
                        .iter()
                        .position(|p| *p == Panel::Inspector)
                        .unwrap_or(n - 1);
                    heights[gets] += left;
                    break;
                }
                let share = left / open.len() as f32;
                let (fits, rest): (Vec<usize>, Vec<usize>) =
                    open.iter().partition(|&&i| wants[i].max(least) <= share);
                if fits.is_empty() {
                    for &i in &rest {
                        heights[i] = share;
                    }
                    break;
                }
                for i in fits {
                    heights[i] = wants[i].max(least);
                    left -= heights[i];
                }
                open = rest;
            }
            heights
        };
        // At least the least each (taken from the tallest), whole pixels, and exactly `room`
        // in all.
        for i in 0..n {
            let short = least - heights[i];
            if short > 0.0
                && let Some(tallest) = (0..n).max_by(|a, b| heights[*a].total_cmp(&heights[*b]))
            {
                heights[tallest] -= short;
                heights[i] = least;
            }
        }
        for h in heights.iter_mut().take(n - 1) {
            *h = h.floor();
        }
        heights[n - 1] = room - heights[..n - 1].iter().sum::<f32>();
        heights
    }

    /// The bar between two stacked panels: drag it to share the height differently;
    /// double-click it to share by what they hold again. Beside a folded panel it is just a
    /// line.
    fn divider(
        &mut self,
        ui: &mut egui::Ui,
        stack: &[Panel],
        j: usize,
        heights: &[f32],
        folded: &[bool],
    ) {
        let fixed = folded[j] || folded[j + 1];
        let (r, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), DIVIDER),
            if fixed {
                egui::Sense::hover()
            } else {
                egui::Sense::click_and_drag()
            },
        );
        if fixed {
            ui.painter().line_segment(
                [
                    egui::pos2(r.left() + 10.0, r.center().y),
                    egui::pos2(r.right() - 10.0, r.center().y),
                ],
                egui::Stroke::new(1.0, theme::hairline(ui)),
            );
            return;
        }
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
        let response = response.on_hover_text("Drag to resize · double-click to fit");
        if response.double_clicked() {
            for p in stack {
                self.layout.weights.remove(p);
            }
            return;
        }
        let dy = response.drag_delta().y;
        if dy != 0.0 {
            // From here on the stack keeps the shares it is dragged to (the open panels';
            // a folded one keeps what it had).
            let open: Vec<usize> = (0..stack.len()).filter(|i| !folded[*i]).collect();
            let room: f32 = open.iter().map(|i| heights[*i]).sum();
            let least = MIN_PANEL.min(room / open.len() as f32);
            let mut h = heights.to_vec();
            let dy = dy.clamp(least - h[j], h[j + 1] - least);
            h[j] += dy;
            h[j + 1] -= dy;
            for i in open.iter().copied() {
                self.layout
                    .weights
                    .insert(stack[i], h[i] / room * open.len() as f32);
            }
        }
    }

    /// A floating panel's header: its icon and name (in a stack, a chevron: click to fold),
    /// a menu and a button that docks it. Dragging it moves its window, or pulls the panel
    /// out of its stack (see [`Header`]).
    fn float_header(&mut self, ui: &mut egui::Ui, p: Panel, role: Header, folded: bool) {
        let sense = match role {
            Header::Alone | Header::Top => egui::Sense::click(),
            Header::Pulls => egui::Sense::click_and_drag(),
        };
        let (bar, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER), sense);
        if role == Header::Pulls && response.drag_started() {
            self.dock_view.drag = Some((p, false));
            self.dock_view.grab_dx = 30.0;
        }
        let dock = egui::Rect::from_center_size(
            egui::pos2(bar.right() - 20.0, bar.center().y),
            egui::Vec2::splat(24.0),
        );
        let more = dock.translate(egui::vec2(-26.0, 0.0));
        // Over the header itself (not its buttons): what dragging it does.
        let on_bar = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|at| bar.contains(at) && at.x < more.left() - 2.0);
        if on_bar && response.hovered() {
            response.clone().on_hover_text(match role {
                Header::Alone => {
                    "Click to fold · drag to move it; onto a dock to dock it, or onto another \
                                  panel to stack them"
                }
                Header::Top => {
                    "Click to fold · drag to move the stack · Alt-drag to pull this \
                                panel out"
                }
                Header::Pulls => "Click to fold · drag to pull it out of the stack",
            });
        }
        if response.clicked() {
            // Folding the last open panel, or opening one of a folded stack, changes its
            // window's size (a new one): keep it where it is.
            if let Some(top) = self.layout.stack_of(p).map(|s| s[0])
                && let Some(r) = self.dock_view.last_windows.get(&top)
            {
                self.dock_view.float_at.insert(top, r.left_top());
            }
            self.layout.toggle_fold(p);
        }
        let text = ui.visuals().strong_text_color();
        icons::glyph(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(
                    bar.left() + f32::from(BODY_MARGIN.left) + 7.5,
                    bar.center().y,
                ),
                egui::Vec2::splat(15.0),
            ),
            glyph(p),
            text,
        );
        let title = ui.painter().text(
            egui::pos2(
                bar.left() + f32::from(BODY_MARGIN.left) + 22.0,
                bar.center().y,
            ),
            egui::Align2::LEFT_CENTER,
            p.title(),
            egui::FontId::new(13.0, theme::bold()),
            text,
        );
        {
            // Down when open, right when folded.
            let c = egui::pos2(title.right() + 10.0, bar.center().y);
            let color = if on_bar {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().weak_text_color()
            };
            let pts = if folded {
                vec![
                    c + egui::vec2(-1.8, -3.5),
                    c + egui::vec2(1.8, 0.0),
                    c + egui::vec2(-1.8, 3.5),
                ]
            } else {
                vec![
                    c + egui::vec2(-3.5, -1.8),
                    c + egui::vec2(0.0, 1.8),
                    c + egui::vec2(3.5, -1.8),
                ]
            };
            ui.painter()
                .add(egui::Shape::line(pts, egui::Stroke::new(1.4, color)));
        }
        if !folded {
            ui.painter().line_segment(
                [
                    bar.left_bottom() + egui::vec2(10.0, 0.0),
                    bar.right_bottom() - egui::vec2(10.0, 0.0),
                ],
                egui::Stroke::new(1.0, theme::hairline(ui)),
            );
        }
        // Back to the dock it came from (closing it is in the menu beside).
        let home = self.layout.home(p);
        let g = match home {
            Side::Left => Glyph::DockLeft,
            Side::Right => Glyph::DockRight,
        };
        let tip = format!("Dock {}", home.name());
        if icon_at(ui, dock, ui.id().with(("dock", p.key())), g, &tip).clicked() {
            self.redock_floating(p);
        }
        self.dock_view.headers.insert(p, bar);
        let menu = icon_at(
            ui,
            more,
            ui.id().with(("more", p.key())),
            Glyph::More,
            "Dock, float or close",
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
        let canvas = self.drawing.view.canvas_rect;
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
        // A tab (or a stacked panel) hasn't come away until it's well clear of its dock (or
        // its window): until then nothing is shown, and letting go changes nothing. Moving
        // within its own stack still works.
        if !window && !matches!(target, Some(Drop::Stack { .. })) && self.still_attached(panel, pos)
        {
            if released {
                self.dock_view.drag = None;
            }
            return;
        }
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
            None if !window && self.drawing.view.canvas_rect.contains(pos) => {
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

    /// Whether a dragged tab is still within reach of where it came from: its dock, or its
    /// stack's window, and [`DETACH`] around it.
    fn still_attached(&self, panel: Panel, pos: egui::Pos2) -> bool {
        let home = match self.layout.place(panel) {
            Place::Dock(side) => self.dock_view.docks[side.index()],
            Place::Float => self
                .layout
                .stack_of(panel)
                .and_then(|s| self.dock_view.windows.get(&s[0]).copied()),
            Place::Hidden => None,
        };
        home.is_some_and(|r| r.expand(DETACH).contains(pos))
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

/// What dragging a floating panel's header does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Header {
    /// Alone in its window: moves the window (which can then dock or stack).
    Alone,
    /// Top of a stack: moves the whole stack.
    Top,
    /// In a stack (or the top one with Alt): pulls the panel out.
    Pulls,
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
