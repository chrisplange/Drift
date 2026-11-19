//! What floats over the canvas to help: the quick-constrain bar, and the hint on an empty
//! canvas.
//!
//! The bar puts the relations and dimensions the selection offers where the work is: bottom
//! centre of the canvas, while the Select tool is idle. Each relation is a button showing the
//! symbol the canvas shows once it is applied; each dimension is a chip. They do what the
//! Inspector's chips do. The bar gives way to the drawing: it is gone while the pointer is
//! down on the canvas, and it moves to the top of the canvas when what is picked lies under
//! it.

use crate::annotations::{self, AnnotationColors};
use crate::*;
use drift_edit::editor::ConstraintOption;

/// The bar's height, its gap from the canvas's edge, and the gap the toast keeps above it.
pub(crate) const HEIGHT: f32 = 36.0;
pub(crate) const MARGIN: f32 = 16.0;
const TOAST_GAP: f32 = 8.0;
/// A relation's button, and the symbol on it.
const BUTTON: f32 = 28.0;
const SYMBOL: f32 = 20.0;
/// A dimension's chip.
const CHIP: f32 = 24.0;
/// How many relations and dimensions show before the rest go under "More".
const MAX_ITEMS: usize = 9;
/// The canvas has to be this wide for the bar to show.
const MIN_CANVAS: f32 = 420.0;

/// Where the bar was drawn this frame (kept in egui's memory for the frame after, and for
/// the tests).
pub(crate) fn rect_id() -> egui::Id {
    egui::Id::new("quick_bar_rect")
}

/// One thing the bar offers.
#[derive(Clone)]
struct Item {
    option: ConstraintOption,
    is_dimension: bool,
}

impl Item {
    /// What it is called: never the bare name, which is the Inspector's chip.
    fn label(&self) -> String {
        if self.is_dimension {
            format!("Dimension: {}", self.option.label)
        } else {
            format!("Constrain: {}", self.option.label)
        }
    }
}

/// Where the bar sits: at the foot of the canvas, or at the top when what is picked lies
/// under its usual place.
#[derive(Clone, Copy, PartialEq)]
enum Place {
    Bottom,
    Top,
}

/// The bar for this frame: where it goes and what it offers.
pub(crate) struct QuickBar {
    place: Place,
    items: Vec<Item>,
}

impl DriftApp {
    /// Whether anything is typed into, chosen from or asked on top of the canvas.
    fn canvas_occupied(&self) -> bool {
        let d = &self.drawing;
        d.editor.editing_text.is_some()
            || d.note_edit.is_some()
            || d.dimension_edit.is_some()
            || d.property_edit.is_some()
            || d.temp_dims.edit.is_some()
            || d.typing.is_some()
            || self.pie.is_some()
            || self.palette.is_some()
            || self.any_dialog_open()
    }

    /// What the bar would offer for the selection: relations first, as the Inspector lists
    /// them. (A relation is offered by name: what it adds is found when it is clicked.)
    fn quick_bar_items(&mut self) -> Vec<Item> {
        let relations = self
            .drawing
            .pick_memo
            .relations(&self.drawing.editor)
            .into_iter()
            .map(|label| Item {
                option: ConstraintOption {
                    label,
                    constraints: Vec::new(),
                },
                is_dimension: false,
            });
        let dimensions = self
            .drawing
            .editor
            .dimension_options()
            .into_iter()
            .map(|option| Item {
                option,
                is_dimension: true,
            });
        relations.chain(dimensions).collect()
    }

    /// The quick-constrain bar, if it shows this frame: the Select tool idle on the model,
    /// something picked that relations or dimensions fit, and nothing else going on.
    /// `canvas` is the canvas's own response: while the pointer is down on the canvas (a
    /// click, a drag, a pan) the bar is out of the way.
    pub(crate) fn quick_bar_wanted(
        &mut self,
        canvas: &egui::Response,
        map: &ScreenMap,
    ) -> Option<QuickBar> {
        let ed = &self.drawing.editor;
        if !self.show_quick_bar
            || self.tool.kind() != ToolKind::Select
            || self.tool.is_busy()
            || self.drawing.view.sheet_preview
            || map.rect.width() < MIN_CANVAS
            || map.rect.height() < 4.0 * (HEIGHT + MARGIN)
            || (ed.selection.is_empty() && ed.selected_midpoints.is_empty())
            || canvas.is_pointer_button_down_on()
            || canvas.dragged()
            || self.canvas_occupied()
        {
            return None;
        }
        let items = self.quick_bar_items();
        if items.is_empty() {
            return None;
        }
        // What is picked stays in reach: the bar moves to the top when its usual place would
        // cover any of it.
        let last: Option<egui::Rect> = canvas.ctx.data(|d| d.get_temp(rect_id()));
        let width = last.map_or(360.0, |r| r.width());
        let home = egui::Rect::from_min_max(
            egui::pos2(
                map.rect.center().x - width / 2.0,
                map.rect.bottom() - MARGIN - HEIGHT,
            ),
            egui::pos2(
                map.rect.center().x + width / 2.0,
                map.rect.bottom() - MARGIN,
            ),
        )
        .expand(8.0);
        let ed = &self.drawing.editor;
        let covered = ed
            .selection
            .iter()
            .chain(ed.selected_midpoints.iter())
            // (Plenty to tell by; a pick of thousands isn't walked every frame.)
            .take(2000)
            .filter_map(|id| ed.doc.shape(*id))
            .map(|shape| shape.bounds())
            .filter(|b| !b.is_empty())
            .any(|b| {
                let on_screen =
                    egui::Rect::from_two_pos(map.to_screen(b.min), map.to_screen(b.max));
                on_screen.intersects(home)
            });
        let place = if covered { Place::Top } else { Place::Bottom };
        Some(QuickBar { place, items })
    }

    /// Where a toast's bottom edge goes: above the bar while it shows at the foot of the
    /// canvas.
    pub(crate) fn toast_bottom(canvas: egui::Rect, bar: Option<&QuickBar>) -> f32 {
        match bar.map(|b| b.place) {
            Some(Place::Bottom) => canvas.bottom() - MARGIN - HEIGHT - TOAST_GAP,
            _ => canvas.bottom() - 24.0,
        }
    }

    /// Draw the quick-constrain bar on the canvas, and do what is clicked on it.
    pub(crate) fn quick_bar(&mut self, ctx: &egui::Context, canvas: egui::Rect, bar: QuickBar) {
        let QuickBar { place, items } = bar;
        let dark = ctx.theme() == egui::Theme::Dark;
        let n = theme::neutrals(dark);
        let (pivot, at) = match place {
            Place::Bottom => (
                egui::Align2::CENTER_BOTTOM,
                egui::pos2(canvas.center().x, canvas.bottom() - MARGIN),
            ),
            Place::Top => (
                egui::Align2::CENTER_TOP,
                egui::pos2(canvas.center().x, canvas.top() + MARGIN),
            ),
        };
        let (shown, more) = if items.len() > MAX_ITEMS {
            items.split_at(MAX_ITEMS - 1)
        } else {
            (&items[..], &items[..0])
        };
        let mut chosen: Option<Item> = None;
        let style = ctx.style_of(ctx.theme());
        let area = egui::Area::new(egui::Id::new("quick_bar"))
            .order(egui::Order::Middle)
            .pivot(pivot)
            .fixed_pos(at)
            .constrain_to(canvas)
            .show(ctx, |ui| {
                egui::Frame::popup(&style)
                    .fill(n.raised)
                    .stroke(egui::Stroke::new(1.0, n.control_border))
                    .corner_radius(theme::radius::MENU)
                    // (With the 1 pt edge: 36 high around a row of 28.)
                    .inner_margin(egui::Margin {
                        left: 11,
                        right: 3,
                        top: 3,
                        bottom: 3,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.set_height(HEIGHT - 8.0);
                            ui.spacing_mut().item_spacing.x = 2.0;
                            caption(ui, "Constrain");
                            let mut in_dimensions = false;
                            for item in shown {
                                if item.is_dimension && !in_dimensions {
                                    in_dimensions = true;
                                    // (Only between the two kinds.)
                                    if shown.iter().any(|i| !i.is_dimension) {
                                        divider(ui);
                                    }
                                } else if item.is_dimension {
                                    ui.add_space(2.0);
                                }
                                let response = if item.is_dimension {
                                    dimension_chip(ui, item)
                                } else {
                                    relation_button(ui, item)
                                };
                                let response = self.quick_tip(response, item);
                                if response.clicked() {
                                    chosen = Some(item.clone());
                                }
                            }
                            if !more.is_empty() {
                                ui.add_space(2.0);
                                if let Some(item) = more_menu(ui, more) {
                                    chosen = Some(item);
                                }
                            } else if shown.last().is_some_and(|i| i.is_dimension) {
                                // (A chip stands off the bar's edge as far as it does from
                                // the bar's top; a symbol's button has its own inset.)
                                ui.add_space(2.0);
                            }
                        });
                    });
            });
        ctx.data_mut(|d| d.insert_temp(rect_id(), area.response.rect));
        let Some(item) = chosen else {
            return;
        };
        let now = ctx.input(|i| i.time);
        // (A relation offered by name: what it adds is found now, and nothing if it is no
        // longer offered.)
        let option = if item.is_dimension {
            Some(item.option)
        } else {
            self.drawing
                .editor
                .relation_options()
                .into_iter()
                .find(|o| o.label == item.option.label)
        };
        if let Some(option) = option
            && let Some(id) = self.apply_constraint_option(&option, item.is_dimension, now)
        {
            // Let the user type the value straight away: in the Inspector's field when it
            // is in view, or else on the dimension itself.
            if self.layout.is_shown(crate::dock::Panel::Inspector) {
                self.drawing.focus_dimension = Some(id);
            } else if self.show_dimensions {
                self.open_dimension_edit(id);
            }
        }
    }

    /// What a button on the bar says when pointed at: its name, and which of the picks give
    /// way (as the Inspector's chip says).
    fn quick_tip(&self, response: egui::Response, item: &Item) -> egui::Response {
        let name = item.option.label;
        let about = if item.is_dimension {
            "Add this dimension and type its value"
        } else {
            self.drawing
                .editor
                .relation_role(name)
                .unwrap_or("Add this relation to what is picked")
        };
        response.on_hover_ui(|ui| {
            ui.set_max_width(260.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                egui::RichText::new(name)
                    .family(theme::bold())
                    .color(ui.visuals().strong_text_color()),
            );
            ui.label(egui::RichText::new(about).color(ui.visuals().weak_text_color()));
        })
    }

    /// Whether the empty canvas says how to start: nothing drawn, the start panel closed,
    /// the Select tool idle and nothing else up.
    pub(crate) fn empty_hint_wanted(&self, canvas: egui::Rect) -> bool {
        let doc = &self.drawing.editor.doc;
        !self.show_start
            && doc.is_empty()
            && doc.texts().next().is_none()
            && doc.underlays().next().is_none()
            && self.tool.kind() == ToolKind::Select
            && !self.tool.is_busy()
            && !self.drawing.view.sheet_preview
            && !self.canvas_occupied()
            && canvas.width() >= 360.0
            && canvas.height() >= 220.0
    }

    /// The hint on an empty canvas: how to start, in three quiet lines. Painted only: it
    /// takes no clicks and nothing is added to what a screen reader sees. Gone as soon as
    /// anything exists.
    pub(crate) fn empty_canvas_hint(&self, ui: &egui::Ui, canvas: egui::Rect) {
        if !self.empty_hint_wanted(canvas) {
            return;
        }
        let n = theme::neutrals(ui.visuals().dark_mode);
        let lines = [
            ("L".to_owned(), "Line"),
            ("R".to_owned(), "Rectangle"),
            (
                app::panels::shortcut("K", false),
                "Search every tool and command",
            ),
        ];
        const LINE: f32 = 28.0;
        let font = egui::FontId::proportional(theme::size::BODY);
        let painter = ui.painter_at(canvas);
        let texts: Vec<_> = lines
            .iter()
            .map(|(_, what)| painter.layout_no_wrap((*what).to_owned(), font.clone(), n.tertiary))
            .collect();
        // The keys end and the words start either side of one line down the middle, set so
        // that the widest row is centred on the canvas.
        let keys_w = lines
            .iter()
            .map(|(keys, _)| theme::keycaps_width(ui, keys))
            .fold(0.0_f32, f32::max);
        let text_w = texts.iter().map(|g| g.size().x).fold(0.0_f32, f32::max);
        const GAP: f32 = 12.0;
        let left = canvas.center().x - (keys_w + GAP + text_w) / 2.0;
        let spine = left + keys_w;
        // (A little under the middle, where a new drawing's axes cross; and on a patch of
        // the canvas's own colour, so no axis or grid line runs through the words.)
        let middle = canvas.center().y + (canvas.height() * 0.16).round();
        let top = middle - LINE * lines.len() as f32 / 2.0;
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(left, top),
                egui::vec2(keys_w + GAP + text_w, LINE * lines.len() as f32),
            )
            .expand2(egui::vec2(18.0, 10.0)),
            theme::radius::MENU,
            theme::canvas(ui.visuals().dark_mode),
        );
        for (i, ((keys, _), text)) in lines.iter().zip(texts).enumerate() {
            let y = top + LINE * (i as f32 + 0.5);
            theme::keycap_at(ui, egui::pos2(spine, y), keys);
            painter.galley(
                egui::pos2(spine + GAP, y - text.size().y / 2.0),
                text,
                n.tertiary,
            );
        }
    }
}

/// The bar's caption: painted, so it is nothing to click and has no name of its own.
fn caption(ui: &mut egui::Ui, text: &str) {
    let n = theme::neutrals(ui.visuals().dark_mode);
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::new(theme::size::CAPTION, theme::bold()),
        n.tertiary,
    );
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 8.0, HEIGHT - 8.0),
        egui::Sense::hover(),
    );
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        n.tertiary,
    );
}

/// A hairline between the relations and the dimensions.
fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(13.0, 16.0), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        egui::Stroke::new(1.0, theme::hairline(ui)),
    );
}

/// The fill of a button on the bar: nothing at rest, so the symbols read as one row.
fn button_fill(ui: &egui::Ui, response: &egui::Response) -> Option<egui::Color32> {
    let n = theme::neutrals(ui.visuals().dark_mode);
    if response.is_pointer_button_down_on() {
        Some(n.pressed)
    } else if response.hovered() || response.has_focus() {
        Some(n.hover)
    } else {
        None
    }
}

/// A relation: a square button with the relation's own symbol.
fn relation_button(ui: &mut egui::Ui, item: &Item) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(BUTTON), egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, item.label()));
    let fill = button_fill(ui, &response);
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, theme::radius::CONTROL, fill);
    }
    // The canvas's own colour for a relation, brighter under the pointer.
    let color = if fill.is_some() {
        theme::neutrals(dark).text
    } else {
        AnnotationColors::new(dark).normal
    };
    // (On whole pixels, so the symbol's strokes are as crisp as on the canvas.)
    let center = rect.center().round();
    annotations::paint_glyph_icon(
        ui.painter(),
        egui::Rect::from_center_size(center, egui::Vec2::splat(SYMBOL)),
        app::pie::glyph(item.option.label),
        color,
    );
    response
}

/// A dimension: a compact chip with its name.
fn dimension_chip(ui: &mut egui::Ui, item: &Item) -> egui::Response {
    let n = theme::neutrals(ui.visuals().dark_mode);
    let galley = ui.painter().layout_no_wrap(
        item.option.label.to_owned(),
        egui::FontId::proportional(theme::size::LABEL),
        n.text,
    );
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 18.0, CHIP),
        egui::Sense::click(),
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, item.label()));
    let fill = button_fill(ui, &response).unwrap_or(n.control);
    ui.painter().rect(
        rect,
        theme::radius::CONTROL,
        fill,
        egui::Stroke::new(1.0, n.control_border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        n.text,
    );
    response
}

/// What didn't fit on the bar, in a menu. Returns the one chosen.
fn more_menu(ui: &mut egui::Ui, items: &[Item]) -> Option<Item> {
    let n = theme::neutrals(ui.visuals().dark_mode);
    let galley = ui.painter().layout_no_wrap(
        "More".to_owned(),
        egui::FontId::proportional(theme::size::LABEL),
        n.text,
    );
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 30.0, CHIP),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More constraints")
    });
    if let Some(fill) = button_fill(ui, &response) {
        ui.painter().rect_filled(rect, theme::radius::CONTROL, fill);
    }
    ui.painter().galley(
        egui::pos2(rect.left() + 9.0, rect.center().y - galley.size().y / 2.0),
        galley,
        n.text,
    );
    // A chevron: there is more under it.
    let c = egui::pos2(rect.right() - 12.0, rect.center().y + 0.5);
    ui.painter().add(egui::Shape::line(
        vec![
            c + egui::vec2(-3.5, -1.8),
            c + egui::vec2(0.0, 1.8),
            c + egui::vec2(3.5, -1.8),
        ],
        egui::Stroke::new(1.4, n.secondary),
    ));
    let mut chosen = None;
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(170.0);
        for item in items {
            let (row, response) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, item.label())
            });
            if response.hovered() {
                ui.painter()
                    .rect_filled(row, theme::radius::CONTROL, n.hover);
            }
            let mut x = row.left() + 8.0;
            if !item.is_dimension {
                annotations::paint_glyph_icon(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(x + 8.0, row.center().y).round(),
                        egui::Vec2::splat(15.0),
                    ),
                    app::pie::glyph(item.option.label),
                    n.secondary,
                );
            }
            x += 24.0;
            ui.painter().text(
                egui::pos2(x, row.center().y),
                egui::Align2::LEFT_CENTER,
                item.option.label,
                egui::FontId::proportional(theme::size::BODY),
                n.text,
            );
            if response.clicked() {
                chosen = Some(item.clone());
                ui.close();
            }
        }
    });
    chosen
}
