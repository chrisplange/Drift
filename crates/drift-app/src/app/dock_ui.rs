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
/// What a named tab is wider than its name by: its icon and the space around the two.
const TAB_PAD: f32 = 37.0;
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
    left: theme::PANE_MARGIN as i8,
    right: theme::PANE_MARGIN as i8,
    top: 2,
    bottom: 0,
};
/// How far past its dock (or its stack's window) a dragged tab must go before it comes
/// away: let go nearer and nothing changes.
const DETACH: f32 = 40.0;
/// Space between stacked panels (it resizes them).
const DIVIDER: f32 = 7.0;
/// Space kept between a floating window Drift places itself and the canvas edge.
const EDGE_GAP: f32 = 14.0;
/// Space kept between floating windows Drift places itself.
const WINDOW_GAP: f32 = 10.0;
/// How near a dragged floating window's edge comes to another edge before it snaps there.
const SNAP: f32 = 8.0;
/// A floating window's width until it is resized.
const FLOAT_WIDTH: f32 = 300.0;
/// The least width of a dock or a floating window.
const MIN_WIDTH: f32 = 240.0;
/// The most a dock or floating window widens itself to fit what it holds; past this, the
/// rest is cut off.
const MOST_NEEDED: f32 = 480.0;

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
    /// Each floating window as it stood when the pointer went down, held until it comes up:
    /// what a resize measures its room from (#505). And which of its edges the press was on.
    pressed_at: HashMap<Panel, (egui::Rect, Grip)>,
    /// The least width each floating window was found to need while its left edge is held
    /// (learnt from what was cut off, as a dock's is: see `dock_needs`).
    /// And the width it was given last frame, which what was cut off then was measured at
    /// (the window's own rect reports a frame late while it is sized this way).
    float_needs: HashMap<Panel, (f32, f32)>,
    /// How much wider than shown each panel's contents were when last drawn.
    overflow: HashMap<Panel, f32>,
    /// The least width each dock was found to need while its edge is held (see `dock`).
    dock_needs: [Option<f32>; 2],
    /// Each dock's width as last drawn (`docks` is cleared at the start of a frame).
    dock_widths: [f32; 2],
    /// How wide each floating panel's contents were when last drawn: never wider than its
    /// window, which can't be resized narrower than this.
    content_width: HashMap<Panel, f32>,
    /// How tall each panel's contents were when last drawn (nothing cut off).
    content: HashMap<Panel, f32>,
    /// The header of each floating panel as last drawn (the part that docks it when dragged).
    headers: HashMap<Panel, egui::Rect>,
    /// Where a panel just floated by dropping should appear.
    float_at: HashMap<Panel, egui::Pos2>,
    was_wide: Option<bool>,
    /// Whether the floating windows have been drawn once: the first frame puts back the
    /// windows of the last session, not panels just floated.
    drawn: bool,
    /// The panels floating when the windows were last drawn.
    was_floating: std::collections::HashSet<Panel>,
    /// Each floating window's egui id as last drawn (by its top panel).
    ids: HashMap<Panel, egui::Id>,
    /// Whether the sketch check was open last frame: opened afresh, it is placed afresh.
    pub(crate) checker_open: bool,
    /// Every panel hidden for a clear view (Ctrl+\).
    pub hide_all: bool,
    /// The panel whose contents are being drawn, while they are: what a click in a panel
    /// asks for never sends its own dock to another panel (see [`DriftApp::show_panel`]).
    in_panel: Option<Panel>,
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

/// The front panel's name in a row of its own under the tab bar: for a dock too narrow for
/// its tabs' names, so the panel is always named. (Painted: the tab is what it is called.)
fn front_title(ui: &mut egui::Ui, front: Panel) {
    let (row, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    ui.painter().text(
        egui::pos2(row.left() + theme::PANE_MARGIN, row.center().y + 1.0),
        egui::Align2::LEFT_CENTER,
        front.title(),
        egui::FontId::new(theme::size::SECTION, theme::bold()),
        theme::neutrals(ui.visuals().dark_mode).secondary,
    );
}

/// A small icon button drawn at `rect`.
pub(crate) fn icon_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    g: Glyph,
    tip: &str,
) -> egui::Response {
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    let n = theme::neutrals(ui.visuals().dark_mode);
    let color = if response.hovered() {
        let fill = if response.is_pointer_button_down_on() {
            n.pressed
        } else {
            n.hover
        };
        ui.painter().rect_filled(rect, theme::radius::CONTROL, fill);
        n.text
    } else {
        n.secondary
    };
    icons::glyph(ui.painter(), rect.shrink(5.0), g, color);
    response.on_hover_text(tip)
}

impl DriftApp {
    /// Open a panel and bring it to the front (also undoes "hide all panels").
    ///
    /// Asked for by a click inside another panel, it never takes that panel's place: a
    /// panel the user is working in stays in front, and one in another dock or floating
    /// comes forward as usual (#425).
    pub(crate) fn show_panel(&mut self, p: Panel) {
        if let Some(from) = self.dock_view.in_panel
            && from != p
            && let Place::Dock(side) = self.layout.place(from)
        {
            let to = self.layout.place(p);
            if to == Place::Hidden || to == Place::Dock(side) {
                return;
            }
        }
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
                .show_separator_line(false)
                .resizable(false)
                .exact_size(RAIL)
                .frame(
                    egui::Frame::side_top_panel(&style).inner_margin(egui::Margin::symmetric(4, 6)),
                )
                .show(ui, |ui| self.rail(ui, side, &panels))
                .response
        } else {
            let front = self.layout.active[i]
                .filter(|p| panels.contains(p))
                .unwrap_or(panels[0]);
            // Never narrower than what the front panel holds. A dock can't measure that
            // before it is drawn, so it learns it from what was cut off: while the edge is
            // held, the least width found holds the edge there (from the next frame); let go,
            // it is forgotten, so contents that have since become narrower can be narrowed
            // to. What was cut off when nothing is held widens the dock to fit.
            let over = self.dock_view.overflow.get(&front).copied().unwrap_or(0.0);
            let needs = &mut self.dock_view.dock_needs[i];
            if !ui.input(|i| i.pointer.primary_down()) {
                *needs = None;
            }
            if over > 0.5 {
                let wide = self.dock_view.dock_widths[i] + over;
                *needs = Some(needs.unwrap_or(0.0).max(wide));
            }
            let least = needs.map_or(MIN_WIDTH, |w| w.clamp(MIN_WIDTH, MOST_NEEDED));
            make("dock")
                .show_separator_line(false)
                .resizable(true)
                .default_size(310.0)
                .min_size(least)
                .frame(egui::Frame::side_top_panel(&style).inner_margin(egui::Margin::ZERO))
                .show(ui, |ui| {
                    if !self.tab_bar(ui, side, &panels, front) {
                        front_title(ui, front);
                    }
                    egui::Frame::new()
                        .inner_margin(BODY_MARGIN)
                        .show(ui, |ui| self.panel_body(ui, front));
                })
                .response
        };
        // The line between the dock and the canvas: a pane's edge.
        let edge = match side {
            Side::Left => egui::Align2::RIGHT_CENTER,
            Side::Right => egui::Align2::LEFT_CENTER,
        };
        theme::pane_edge(ui, response.rect, edge);
        self.dock_view.docks[i] = Some(response.rect);
        self.dock_view.dock_widths[i] = response.rect.width();
    }

    /// A panel's contents.
    pub(crate) fn panel_body(&mut self, ui: &mut egui::Ui, p: Panel) {
        let now = ui.input(|i| i.time);
        let outer = self.dock_view.in_panel.replace(p);
        match p {
            Panel::Inspector => {
                let area = egui::ScrollArea::vertical()
                    .id_salt("inspector_scroll")
                    .auto_shrink([false, false]);
                let output = theme::pane_scroll(ui, area, |ui| self.inspector(ui));
                self.panel_scrolled(ui, p, &output);
            }
            Panel::Parameters => self.parameters_panel(ui, now),
            Panel::Layers => self.layers_panel(ui, now),
            Panel::Sheets => self.sheets_panel(ui),
            Panel::Blocks => self.blocks_panel(ui),
        }
        self.dock_view.in_panel = outer;
    }

    /// The tabs across the top of a dock: icon and name (just the icon when they don't fit),
    /// the front one underlined; then collapse and menu buttons. Returns whether the front
    /// tab shows its name (when it doesn't, the dock names the panel under the bar).
    fn tab_bar(&mut self, ui: &mut egui::Ui, side: Side, panels: &[Panel], front: Panel) -> bool {
        let i = side.index();
        let (bar, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), TAB_BAR),
            egui::Sense::hover(),
        );
        let dark = ui.visuals().dark_mode;
        let accent = theme::accent(dark);
        let n = theme::neutrals(dark);
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
        // (Measured in the front tab's bold, so a tab keeps its width when it comes to the
        // front.)
        let font = egui::FontId::new(theme::size::BODY, theme::bold());
        let widths: Vec<f32> = panels
            .iter()
            .map(|p| {
                ui.painter()
                    .layout_no_wrap(p.title().to_owned(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
                    + TAB_PAD
            })
            .collect();
        let controls = 58.0;
        let room = bar.width() - controls - 8.0;
        // Every tab named if they fit; otherwise the front one named and the rest as icons.
        let all = widths.iter().sum::<f32>() + 2.0 * panels.len() as f32 <= room;
        let front_at = panels.iter().position(|p| *p == front).unwrap_or(0);
        let front_only = widths[front_at] + 34.0 * (panels.len() - 1) as f32 <= room;
        // Icons alone give way too where even they don't fit, so the tabs never run under
        // the buttons at the bar's end.
        let icon = ((room - 2.0 * panels.len() as f32) / panels.len() as f32).clamp(20.0, 32.0);
        let mut x = bar.left() + 6.0;
        for (k, &p) in panels.iter().enumerate() {
            let labelled = all || (front_only && p == front);
            let w = if labelled { widths[k] } else { icon };
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
                        theme::radius::CONTROL,
                        n.pressed,
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
                ui.painter().rect_filled(r, theme::radius::CONTROL, n.hover);
            }
            if selected {
                // The front tab's underline, on the bar's own rule.
                ui.painter().hline(
                    r.left() + 6.0..=r.right() - 6.0,
                    bar.bottom() - 1.0,
                    egui::Stroke::new(2.0, accent),
                );
            }
            let color = if selected || response.hovered() {
                n.text
            } else {
                n.secondary
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
                    egui::FontId::new(theme::size::BODY, family),
                    color,
                );
            }
            let _ = response.on_hover_ui(|ui| {
                ui.label(egui::RichText::new(p.title()).family(theme::bold()));
                ui.label(egui::RichText::new(p.description()).weak());
                theme::hint(
                    ui,
                    "Drag to float it or dock it elsewhere; right-click for more",
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
        all || front_only
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
            self.close_panel(p);
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
        let dark = ui.visuals().dark_mode;
        let accent = theme::accent(dark);
        let n = theme::neutrals(dark);
        let edge = ui.max_rect();
        let at_rest = ui.visuals().text_color();
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let away = match side {
                Side::Left => Glyph::ChevronsRight,
                Side::Right => Glyph::ChevronsLeft,
            };
            if icons::button(ui, 30.0, false, "Expand", |p, r, _| {
                icons::glyph(p, r, away, n.secondary)
            })
            .on_hover_text("Expand")
            .clicked()
            {
                self.layout.collapsed[i] = false;
            }
            ui.add_space(4.0);
            for &p in panels {
                let response = icons::button(ui, 32.0, false, p.title(), |painter, r, c| {
                    // The one in front in the text's tone (the pointed-at one too), the
                    // others in the secondary: as the tabs are.
                    let c = if p == front {
                        n.text
                    } else if c == at_rest {
                        n.secondary
                    } else {
                        c
                    };
                    icons::glyph(painter, r, glyph(p), c)
                });
                if p == front {
                    // A bar on the canvas side: the front tab's underline, turned.
                    let r = response.rect;
                    let x = match side {
                        Side::Left => edge.right() + 2.0,
                        Side::Right => edge.left() - 2.0,
                    };
                    ui.painter().vline(
                        x,
                        r.top() + 6.0..=r.bottom() - 6.0,
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
        // A frame that draws none (every panel hidden, no canvas) keeps the windows of the
        // last that did: shown again, they are where they were, not new ones to place.
        let canvas = self.drawing.view.canvas_rect;
        if self.dock_view.hide_all || !canvas.is_positive() {
            return;
        }
        self.dock_view.last_windows = std::mem::take(&mut self.dock_view.windows);
        let was_floating = std::mem::take(&mut self.dock_view.was_floating);
        self.dock_view.was_floating = self.layout.floating.iter().flatten().copied().collect();
        // Opaque, like the dialogs: nothing underneath shows through.
        let frame =
            egui::Frame::window(&ctx.style_of(ctx.theme())).inner_margin(egui::Margin::ZERO);
        // Never taller than the canvas: egui lets a window taller than the area it is kept in
        // slide up past its top, where the header would be cut off under the bar above.
        let most = canvas.height().floor();
        let down = ctx.input(|i| i.pointer.primary_down());
        // The presses just let go of.
        if !down {
            self.dock_view.float_needs.clear();
        }
        let released = if down {
            HashMap::new()
        } else {
            std::mem::take(&mut self.dock_view.pressed_at)
        };
        // What a window Drift places itself keeps off: the picks, unless they fill the view.
        let avoid = self.selection_on_screen(canvas);
        let narrow = self.dock_view.was_wide == Some(false);
        for stack in self.layout.floating.clone() {
            let top = stack[0];
            let n = stack.len();
            let tall = (300.0 * n as f32).max(440.0).min(most - 28.0);
            let least = MIN_PANEL * n as f32 + DIVIDER * (n - 1) as f32;
            // The other windows, as drawn this frame or (not drawn yet) the frame before.
            let others: Vec<egui::Rect> = self
                .dock_view
                .windows
                .iter()
                .chain(self.dock_view.last_windows.iter())
                .filter(|(p, _)| !stack.contains(p))
                .map(|(_, r)| *r)
                .collect();
            // A window new this frame that nobody put anywhere (a drop, a fold or a stack
            // that changed sets `float_at`): where it goes is worked out here.
            let new = !self.dock_view.last_windows.contains_key(&top)
                && !self.dock_view.float_at.contains_key(&top);
            if new
                && let Some(r) = stack
                    .iter()
                    .find_map(|p| self.dock_view.last_windows.get(p))
            {
                // The stack was drawn last frame under another top panel: it stays put.
                self.dock_view.float_at.insert(top, r.left_top());
            } else if new
                && narrow
                && n == 1
                && self.dock_view.drawn
                && !was_floating.contains(&top)
            {
                // In a narrow window a panel just floated opens folded to its header, out of
                // the way.
                self.layout.folded.insert(top);
            }
            // Every panel folded: a window just the headers, as wide as it was.
            let shut = stack.iter().all(|p| self.layout.folded.contains(p));
            let headers = (HEADER * n as f32 + DIVIDER * (n - 1) as f32).min(most);
            let id = egui::Id::new(("floating_panel", top.key(), n, shut));
            // The same stack in a new window (it gained or lost a panel, or folded) that
            // nothing put anywhere: where the old one was, never egui's default place.
            if !new
                && !self.dock_view.float_at.contains_key(&top)
                && self.dock_view.ids.get(&top).is_some_and(|was| *was != id)
                && let Some(r) = self.dock_view.last_windows.get(&top)
            {
                self.dock_view.float_at.insert(top, r.left_top());
            }
            self.dock_view.ids.insert(top, id);
            if new && !self.dock_view.float_at.contains_key(&top) {
                // As big as egui brings it back (it keeps a resized window's size), else the
                // default.
                let size = ctx
                    .memory(|m| m.area_rect(id))
                    .map_or(egui::vec2(FLOAT_WIDTH, tall), |r| {
                        egui::vec2(r.width().max(MIN_WIDTH), r.height().min(most))
                    });
                let size = egui::vec2(size.x, if shut { headers } else { size.y });
                let at = self.float_spot(
                    ctx,
                    id,
                    top,
                    canvas,
                    size,
                    least.min(size.y),
                    &others,
                    &avoid,
                );
                self.dock_view.float_at.insert(top, at);
            }
            // A window put somewhere starts no taller than the room below it.
            let room = self.dock_view.float_at.get(&top).map_or(tall, |at| {
                (canvas.bottom() - EDGE_GAP - at.y)
                    .floor()
                    .max(least.min(most))
            });
            // A window per stack, named by its top panel and how many it holds, so a stack
            // that changes starts at its new size (and at the place it was, see `float_at`).
            let mut window = egui::Window::new(top.title())
                .id(id)
                .title_bar(false)
                .resizable(true)
                .movable(true)
                .frame(frame)
                .default_size([FLOAT_WIDTH, tall.min(room)])
                .min_width(MIN_WIDTH)
                .min_height(least.min(most))
                .max_height(most)
                // Placed by its top left (see `float_at`).
                .default_pos(canvas.right_top() + egui::vec2(-FLOAT_WIDTH - EDGE_GAP, EDGE_GAP))
                .constrain_to(canvas);
            // As wide as it was last frame (a window new to this stack: the default width).
            let width = self
                .dock_view
                .last_windows
                .get(&top)
                .map_or(300.0, |r| r.width());
            // Never wider or taller than the room right of and below where it stands: a resize
            // past the canvas edge would otherwise be pushed back by `constrain_to`, so dragging
            // the right edge out would grow the window to the left. The room is measured from
            // where it stood when the pointer went down, not from last frame (#505). And while
            // its right or bottom edge is dragged, the area it is kept in reaches past those
            // edges: egui sizes the window to the pointer and then fits that rect into the
            // area by moving it, so with the canvas as the area it slid left (or up) by however
            // far the pointer went past the edge, and kept the pointer's size on release. The
            // cap below keeps what is drawn inside the canvas.
            if let Some(r) = self.dock_view.last_windows.get(&top) {
                let (from, grip) = if down {
                    *self.dock_view.pressed_at.entry(top).or_insert_with(|| {
                        let origin = ctx.input(|i| i.pointer.press_origin());
                        let reach = ctx.style_of(ctx.theme()).interaction.clone();
                        let (side, corner) = (
                            reach.resize_grab_radius_side,
                            reach.resize_grab_radius_corner,
                        );
                        // (Folded, it can't be resized: a press anywhere on it moves it.)
                        let mut grip = match origin {
                            Some(o) if !shut => Grip::at(*r, o, side, corner),
                            _ => Grip::default(),
                        };
                        // A left or top edge counts only when egui gave the press to that
                        // edge of this window: a press on a neighbour's edge snapped against
                        // this one, or on a window or popup over it, leaves it alone.
                        let edge = egui::Id::new(egui::LayerId::new(egui::Order::Middle, id))
                            .with("edge_drag");
                        let held = ctx.dragged_id();
                        let on =
                            |sides: [&str; 3]| sides.iter().any(|s| held == Some(edge.with(*s)));
                        grip.left &= on(["left", "left_top", "left_bottom"]);
                        grip.top &= on(["top", "left_top", "right_top"]);
                        (*r, grip)
                    })
                } else {
                    (*r, Grip::default())
                };
                // A left (or top) edge dragged inward past the least width (or height) moves
                // the window on with the pointer in egui, its right (or bottom) edge walking
                // away; an area that ends at that edge holds it there.
                if grip != Grip::default() {
                    let mut room = canvas;
                    if grip.right || grip.bottom {
                        room.max += egui::vec2(1.0e5, 1.0e5);
                    }
                    if grip.left {
                        room.max.x = from.right();
                    }
                    if grip.top {
                        room.max.y = from.bottom();
                    }
                    window = window.constrain_to(room);
                }
                // A left (or top) edge dragged grows the window from its right (or bottom)
                // edge, which stays put: the room is on the other side (measured from where
                // the window stood when the pointer went down, else it can't grow until it
                // is let go).
                let wide = if grip.left {
                    from.right() - canvas.left()
                } else {
                    canvas.right() - from.left()
                };
                let high = if grip.top {
                    from.bottom() - canvas.top()
                } else {
                    canvas.bottom() - from.top()
                };
                let wide = wide.floor().max(MIN_WIDTH);
                let high = high.floor().min(most).max(least.min(most));
                window = window.max_width(wide).max_height(high);
            } else if let Some(at) = self.dock_view.float_at.get(&top) {
                // Put somewhere new: no bigger than the room right of and below that place,
                // so it isn't pushed back over what it was placed clear of.
                window = window
                    .max_width((canvas.right() - EDGE_GAP - at.x).floor().max(MIN_WIDTH))
                    .max_height(room);
            }
            // Moved by dragging: its edges snap to the canvas's and to the other windows'.
            // (egui puts a dragged window where it was when the drag began plus how far the
            // pointer has gone, so that starting point is what is moved.)
            if ctx.is_being_dragged(id.with("move"))
                && let Some(r) = self.dock_view.last_windows.get(&top)
                && let Some((from, grip)) = self.dock_view.pressed_at.get(&top).copied()
                && grip == Grip::default()
                && let Some(moved) = ctx.input(|i| i.pointer.total_drag_delta())
            {
                let raw = egui::Rect::from_min_size(from.min + moved, r.size());
                let at = raw.min + snap_offset(raw, canvas, &others);
                ctx.data_mut(|d| d.insert_temp(id.with("pivot_at_drag_start"), at - moved));
            }
            if shut {
                window = window
                    .resizable(false)
                    .min_height(headers)
                    .fixed_size([width, headers]);
            }
            // A left (or top) edge just let go of: egui keeps the window where the pointer
            // put its edge, even past the least width, where it was only held in place (see
            // above) while the button was down. Its right (or bottom) edge goes back where it
            // was.
            if let Some((from, grip)) = released.get(&top)
                && (grip.left || grip.top)
                && let Some(r) = self.dock_view.last_windows.get(&top)
            {
                let x = if grip.left {
                    from.right() - r.width()
                } else {
                    r.left()
                };
                let y = if grip.top {
                    from.bottom() - r.height()
                } else {
                    r.top()
                };
                self.dock_view
                    .float_at
                    .entry(top)
                    .or_insert(egui::pos2(x, y));
            }
            if let Some(at) = self.dock_view.float_at.remove(&top) {
                window = window.current_pos(at);
            }
            // A left or top edge (or a corner with one) held: the window is sized here, from
            // where it stood when the pointer went down and how far the pointer has gone, the
            // far edges staying put. Left to egui, the left edge followed the pointer a frame
            // before the width did, so the right edge wobbled by each frame's move, and past
            // the least width the whole window walked on with the pointer.
            let mut held = None;
            if down
                && !shut
                && let Some((from, grip)) = self.dock_view.pressed_at.get(&top).copied()
                && (grip.left || grip.top)
                && let (Some(o), Some(at)) =
                    ctx.input(|i| (i.pointer.press_origin(), i.pointer.latest_pos()))
            {
                let d = at - o;
                // Never narrower than what it holds: what was cut off while held raises the
                // least width, from the next frame.
                let over = stack
                    .iter()
                    .filter(|p| !self.layout.folded.contains(p))
                    .filter_map(|p| self.dock_view.overflow.get(p).copied())
                    .fold(0.0_f32, f32::max);
                let (needs, given) = self
                    .dock_view
                    .float_needs
                    .entry(top)
                    .or_insert((MIN_WIDTH, from.width()));
                if over > 0.5 {
                    *needs = needs.max(*given + over);
                }
                let least_w = needs.clamp(MIN_WIDTH, MOST_NEEDED);
                let least_h = least.min(most);
                let mut r = from;
                if grip.left {
                    r.min.x = (from.min.x + d.x)
                        .round()
                        .min(from.max.x - least_w)
                        .max(canvas.left());
                } else if grip.right {
                    r.max.x = (from.max.x + d.x)
                        .round()
                        .min(canvas.right())
                        .max(from.min.x + least_w);
                }
                if grip.top {
                    r.min.y = (from.min.y + d.y)
                        .round()
                        .min(from.max.y - least_h)
                        .max(from.max.y - most)
                        .max(canvas.top());
                } else if grip.bottom {
                    r.max.y = (from.max.y + d.y)
                        .round()
                        .min(canvas.bottom())
                        .min(from.min.y + most)
                        .max(from.min.y + least_h);
                }
                *given = r.width();
                // (Already inside the canvas. egui's own fitting would use last frame's size
                // with this frame's place, and push the left edge back by a frame's move.)
                window = window.fixed_rect(r).constrain(false);
                held = Some(grip);
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
                    // What it holds may need more width than the window gives it: the
                    // window is told, so it grows to fit and a resize stops there (egui
                    // measures the least width a window's contents take when a resize
                    // starts, and holds the resize to it).
                    // (A panel scrolls up and down within the window, so what it holds past
                    // its right edge is cut off there and told apart.)
                    let over = if folded[j] {
                        0.0
                    } else {
                        self.dock_view.overflow.get(&p).copied().unwrap_or(0.0)
                    };
                    let needs = (child.min_rect().right() - rect.left())
                        .max(rect.width() + over)
                        .min(MOST_NEEDED.max(rect.width()));
                    self.dock_view.content_width.insert(p, needs);
                    // (Sized here while an edge is held: the window is that size, else it
                    // would be drawn as wide as what it holds for the frame before the least
                    // width is learnt, its right edge bulging out. What is cut off for that
                    // frame raises the least width for the next.)
                    if held.is_none() {
                        ui.expand_to_include_x(rect.left() + needs);
                    }
                    self.dock_view.stacked.push((p, rect));
                    if j + 1 < n {
                        self.divider(ui, &stack, j, &heights, &folded);
                    }
                }
            });
            // Sized here, the window has no resize grips of its own while held, so the
            // resize cursor is kept up here until it is let go.
            if let Some(g) = held {
                ctx.set_cursor_icon(g.cursor());
            }
            let Some(shown) = shown else {
                continue;
            };
            self.dock_view.windows.insert(top, shown.response.rect);
            // Where it stands, once let go: a drag that ends in a dock or a stack leaves
            // where it stood before.
            if !down && self.dock_view.drag.is_none() {
                let at = shown.response.rect.min;
                self.layout.spots.insert(top, (at.x, at.y));
            }
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
        self.dock_view.drawn = true;
    }

    /// The floating windows as last drawn: what the quick-constrain bar and the sketch check
    /// keep clear of.
    pub(crate) fn floating_windows(&self) -> Vec<egui::Rect> {
        if self.dock_view.hide_all {
            return Vec::new();
        }
        self.dock_view.windows.values().copied().collect()
    }

    /// Where a window new to `top` goes: where its panel last floated (as remembered, or as
    /// egui remembers the window), if that is in the canvas and clear of the other windows
    /// and of the picks;
    /// else the first free place down the edge on its home dock's side (see [`free_spot`]).
    #[allow(clippy::too_many_arguments)]
    fn float_spot(
        &self,
        ctx: &egui::Context,
        id: egui::Id,
        top: Panel,
        canvas: egui::Rect,
        size: egui::Vec2,
        least: f32,
        others: &[egui::Rect],
        avoid: &[egui::Rect],
    ) -> egui::Pos2 {
        let saved = self
            .layout
            .spots
            .get(&top)
            .map(|(x, y)| egui::pos2(*x, *y))
            .or_else(|| ctx.memory(|m| m.area_rect(id)).map(|r| r.min));
        if let Some(at) = saved {
            let h = size.y.min(canvas.bottom() - at.y);
            let r = egui::Rect::from_min_size(at, egui::vec2(size.x, h));
            let clear = |rects: &[egui::Rect]| !rects.iter().any(|o| overlap(*o, r) > 1.0);
            if canvas.contains_rect(r) && h >= least && clear(others) && clear(avoid) {
                return at;
            }
        }
        free_spot(canvas, size, least, others, avoid, self.layout.home(top))
    }

    /// Where the picks show on the canvas (as one box), unless they fill much of it or the
    /// canvas shows a sheet.
    fn selection_on_screen(&self, canvas: egui::Rect) -> Vec<egui::Rect> {
        if self.drawing.view.sheet_preview {
            return Vec::new();
        }
        let map = ScreenMap {
            rect: canvas,
            camera: self.drawing.view.camera,
        };
        let ed = &self.drawing.editor;
        ed.selection
            .iter()
            .chain(ed.selected_midpoints.iter())
            // (Plenty to tell by; a pick of thousands isn't walked every frame.)
            .take(2000)
            .filter_map(|id| ed.doc.shape(*id))
            .map(|shape| shape.bounds())
            .filter(|b| !b.is_empty())
            .map(|b| egui::Rect::from_two_pos(map.to_screen(b.min), map.to_screen(b.max)))
            .reduce(|a, b| a.union(b))
            .map(|r| r.expand(8.0).intersect(canvas))
            .filter(|r| r.is_positive() && r.area() < canvas.area() * 0.5)
            .into_iter()
            .collect()
    }

    /// How much wider than shown a panel's contents were when last drawn (for tests).
    #[cfg(test)]
    pub(crate) fn panel_overflow(&self, p: Panel) -> f32 {
        self.dock_view.overflow.get(&p).copied().unwrap_or(0.0)
    }

    /// How wide a floating panel's contents were when last drawn (for tests).
    #[cfg(test)]
    pub(crate) fn floating_content_width(&self, p: Panel) -> Option<f32> {
        self.dock_view.content_width.get(&p).copied()
    }

    /// Put a floating panel's window with its top left at `at` next frame (for tests).
    #[cfg(test)]
    pub(crate) fn place_floating(&mut self, p: Panel, at: egui::Pos2) {
        self.dock_view.float_at.insert(p, at);
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
        // Wider than it is shown (scrolled only up and down, the rest is cut off).
        let over = (output.content_size.x - output.inner_rect.width()).max(0.0);
        self.dock_view.overflow.insert(p, over);
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
            egui::FontId::new(theme::size::BODY, theme::bold()),
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
            theme::chevron(ui.painter(), c, !folded, color);
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

    /// Dock (or close) a stacked panel as its header button (or menu) does, from inside its
    /// window while the frame draws it: when this frame's rect of the window isn't known yet
    /// (for tests).
    #[cfg(test)]
    pub(crate) fn leave_stack_mid_frame(&mut self, p: Panel, close: bool) {
        self.dock_view.last_windows = std::mem::take(&mut self.dock_view.windows);
        if close {
            self.close_panel(p);
        } else {
            self.redock_floating(p);
        }
        self.dock_view.windows = self.dock_view.last_windows.clone();
    }

    /// Hide a panel; the rest of its stack, if any, keeps its place.
    fn close_panel(&mut self, p: Panel) {
        let keep = self.stack_position(p);
        self.layout.move_to(p, Place::Hidden, None);
        self.restore_stack_position(keep);
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
        // (Asked from inside the stack's own window, it isn't drawn yet this frame.)
        let at = self
            .dock_view
            .windows
            .get(&stack[0])
            .or_else(|| self.dock_view.last_windows.get(&stack[0]))?
            .min;
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
                        theme::radius::MENU,
                        theme::accent_soft(dark),
                        egui::Stroke::new(1.5, accent),
                        egui::StrokeKind::Inside,
                    );
                    painter.text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("Dock {}", side.name()),
                        egui::FontId::new(theme::size::BODY, theme::bold()),
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
                        theme::radius::MENU,
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
            egui::FontId::new(theme::size::BODY, theme::bold()),
            text,
        );
        let r = egui::Rect::from_min_size(
            pos - egui::vec2(self.dock_view.grab_dx.min(40.0), 13.0),
            egui::vec2(galley.size().x + 40.0, 28.0),
        );
        painter.rect(
            r,
            theme::radius::CONTROL,
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

/// Which edges of a floating window a press was on (none: inside it, or on its header).
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(crate) struct Grip {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

impl Grip {
    /// The resize cursor for these edges, as egui shows over a window's grips.
    fn cursor(self) -> egui::CursorIcon {
        use egui::CursorIcon::*;
        if (self.left && self.top) || (self.right && self.bottom) {
            ResizeNwSe
        } else if (self.right && self.top) || (self.left && self.bottom) {
            ResizeNeSw
        } else if self.left || self.right {
            ResizeHorizontal
        } else {
            ResizeVertical
        }
    }

    /// Which edges of `r` a press at `o` was on, as egui's resize grips reach: `side` from
    /// an edge, and `corner` both ways from a corner (where both edges move). The right and
    /// bottom edges reach a little further, as #505 measured; the left and top only as far
    /// as egui's, since the top edge is also the header, which moves the window.
    fn at(r: egui::Rect, o: egui::Pos2, side: f32, corner: f32) -> Self {
        const FAR: f32 = 8.0;
        if !r.expand(FAR.max(corner)).contains(o) {
            return Self::default();
        }
        let near = |a: f32, b: f32, reach: f32| (a - b).abs() <= reach;
        let mut grip = Self {
            left: near(o.x, r.left(), side),
            right: near(o.x, r.right(), FAR),
            top: near(o.y, r.top(), side),
            bottom: near(o.y, r.bottom(), FAR),
        };
        for (x, y, left, top) in [
            (r.left(), r.top(), true, true),
            (r.right(), r.top(), false, true),
            (r.left(), r.bottom(), true, false),
            (r.right(), r.bottom(), false, false),
        ] {
            if near(o.x, x, corner) && near(o.y, y, corner) {
                grip = Self {
                    left,
                    right: !left,
                    top,
                    bottom: !top,
                };
            }
        }
        grip
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

/// How much of `a` and `b` overlap (0 when they don't).
fn overlap(a: egui::Rect, b: egui::Rect) -> f32 {
    let r = a.intersect(b);
    if r.is_positive() { r.area() } else { 0.0 }
}

/// Where a floating window of `size` goes when nobody put it anywhere: down the canvas edge
/// on `side` (its home dock's), [`EDGE_GAP`] in, below the windows already there, then in
/// the next column in; never on another window (`taken`) or on what `avoid` holds (the
/// picks). It may be shorter than `size`, down to `least`, to end above the canvas's foot.
/// With nowhere free: the place that covers the least.
pub(crate) fn free_spot(
    canvas: egui::Rect,
    size: egui::Vec2,
    least: f32,
    taken: &[egui::Rect],
    avoid: &[egui::Rect],
    side: Side,
) -> egui::Pos2 {
    let inner = canvas.shrink(EDGE_GAP);
    let w = size.x.min(inner.width()).max(1.0);
    let covers = |r: egui::Rect| {
        2.0 * taken.iter().map(|t| overlap(*t, r)).sum::<f32>()
            + avoid.iter().map(|t| overlap(*t, r)).sum::<f32>()
    };
    let mut tried = Vec::new();
    for col in 0..64 {
        let x = match side {
            Side::Right => inner.right() - w - col as f32 * (w + WINDOW_GAP),
            Side::Left => inner.left() + col as f32 * (w + WINDOW_GAP),
        };
        if x < inner.left() - 0.5 || x + w > inner.right() + 0.5 {
            break;
        }
        // At the top, or just below something in this column.
        let mut tops: Vec<f32> = taken
            .iter()
            .chain(avoid)
            .filter(|r| r.max.x > x && r.min.x < x + w)
            .map(|r| r.bottom() + WINDOW_GAP)
            .filter(|y| *y > inner.top())
            .chain(std::iter::once(inner.top()))
            .collect();
        tops.sort_by(f32::total_cmp);
        for y in tops {
            let h = size.y.min(inner.bottom() - y);
            if h < least {
                continue;
            }
            let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
            if covers(r) <= 1.0 {
                return r.min.round();
            }
            tried.push(r);
        }
    }
    tried
        .into_iter()
        .min_by(|a, b| covers(*a).total_cmp(&covers(*b)))
        .map_or(inner.min, |r| r.min)
        .round()
}

/// How far to move a dragged floating window (`rect`, where the pointer would put it) so
/// that an edge within [`SNAP`] of the canvas's edge, or of the edge of another window
/// beside it, lies on it.
pub(crate) fn snap_offset(
    rect: egui::Rect,
    canvas: egui::Rect,
    others: &[egui::Rect],
) -> egui::Vec2 {
    let nearest = |moves: Vec<f32>| {
        moves
            .into_iter()
            .filter(|d| d.abs() <= SNAP)
            .min_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap_or(0.0)
    };
    let mut dx = vec![canvas.left() - rect.left(), canvas.right() - rect.right()];
    let mut dy = vec![canvas.top() - rect.top(), canvas.bottom() - rect.bottom()];
    for o in others {
        // Side by side (their rows meet, give or take the snap): sides abut or line up.
        if o.top() - SNAP < rect.bottom() && o.bottom() + SNAP > rect.top() {
            dx.extend([
                o.right() - rect.left(),
                o.left() - rect.right(),
                o.left() - rect.left(),
                o.right() - rect.right(),
            ]);
        }
        // One above the other: tops and bottoms abut or line up.
        if o.left() - SNAP < rect.right() && o.right() + SNAP > rect.left() {
            dy.extend([
                o.bottom() - rect.top(),
                o.top() - rect.bottom(),
                o.top() - rect.top(),
                o.bottom() - rect.bottom(),
            ]);
        }
    }
    egui::vec2(nearest(dx), nearest(dy))
}

#[cfg(test)]
mod placing {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    const CANVAS: egui::Rect = egui::Rect {
        min: egui::pos2(100.0, 50.0),
        max: egui::pos2(1900.0, 1050.0),
    };

    #[test]
    fn new_windows_go_down_the_edge_then_inward_never_on_each_other() {
        let size = egui::vec2(300.0, 440.0);
        let first = free_spot(CANVAS, size, 98.0, &[], &[], Side::Right);
        assert_eq!(
            first,
            egui::pos2(1900.0 - EDGE_GAP - 300.0, 50.0 + EDGE_GAP)
        );
        let a = egui::Rect::from_min_size(first, size);
        // Below the first, in the same column.
        let second = free_spot(CANVAS, size, 98.0, &[a], &[], Side::Right);
        assert_eq!(second, egui::pos2(first.x, a.bottom() + WINDOW_GAP));
        // The column full: the next one in, at the top.
        let b = r(second.x, second.y, 300.0, 1050.0 - EDGE_GAP - second.y);
        let third = free_spot(CANVAS, size, 98.0, &[a, b], &[], Side::Right);
        assert_eq!(
            third,
            egui::pos2(first.x - 300.0 - WINDOW_GAP, 50.0 + EDGE_GAP)
        );
        // On the left dock's side they start at the left.
        let left = free_spot(CANVAS, size, 98.0, &[], &[], Side::Left);
        assert_eq!(left, egui::pos2(100.0 + EDGE_GAP, 50.0 + EDGE_GAP));
    }

    #[test]
    fn a_new_window_keeps_off_the_picks() {
        let size = egui::vec2(300.0, 440.0);
        // The picks lie under the top of the right edge's column.
        let picks = r(1500.0, 40.0, 400.0, 300.0);
        let at = free_spot(CANVAS, size, 98.0, &[], &[picks], Side::Right);
        let placed = egui::Rect::from_min_size(at, size);
        assert!(!placed.intersects(picks), "{placed:?}");
        assert!(CANVAS.contains_rect(placed), "{placed:?}");
    }

    #[test]
    fn with_no_room_it_covers_the_least() {
        let small = r(0.0, 0.0, 500.0, 300.0);
        let taken = [r(0.0, 0.0, 500.0, 300.0)];
        let at = free_spot(
            small,
            egui::vec2(300.0, 200.0),
            98.0,
            &taken,
            &[],
            Side::Right,
        );
        assert!(small.contains(at));
    }

    #[test]
    fn dragged_windows_snap_to_edges_near_them() {
        // Within the snap of the canvas's right edge: flush with it.
        let w = r(1595.0, 300.0, 300.0, 400.0);
        assert_eq!(snap_offset(w, CANVAS, &[]), egui::vec2(5.0, 0.0));
        // Beside another window: abuts it, tops lined up.
        let other = r(1200.0, 200.0, 300.0, 400.0);
        let w = r(1506.0, 204.0, 300.0, 400.0);
        assert_eq!(snap_offset(w, CANVAS, &[other]), egui::vec2(-6.0, -4.0));
        // Far from everything: stays.
        let w = r(700.0, 400.0, 300.0, 300.0);
        assert_eq!(snap_offset(w, CANVAS, &[other]), egui::Vec2::ZERO);
        // A window well below doesn't pull it sideways.
        let below = r(1200.0, 900.0, 300.0, 100.0);
        let w = r(1504.0, 100.0, 300.0, 300.0);
        assert_eq!(snap_offset(w, CANVAS, &[below]).x, 0.0);
    }
}
