//! What floats over the canvas to help: the quick-constrain bar, and the hint on an empty
//! canvas.
//!
//! The bar puts the relations and dimensions the selection offers where the work is: just
//! under what is picked (above it when there is no room under), while the Select tool is
//! idle; at the foot of the canvas when what is picked is out of view. Each relation is a
//! button showing the symbol the canvas shows once it is applied; each dimension is a chip.
//! They do what the Inspector's chips do. The bar gives way to the drawing: it is gone while
//! the pointer is down on the canvas, and it is never over what is picked.

use crate::annotations;
use crate::*;
use drift_edit::editor::ConstraintOption;

/// The bar's height, its gap from the canvas's edge, and how far under (or over) what is
/// picked it sits.
pub(crate) const HEIGHT: f32 = 36.0;
pub(crate) const MARGIN: f32 = 16.0;
pub(crate) const NEAR: f32 = 24.0;
/// The row inside the bar, and the room between the bar's edge and what is on it (with the
/// 1 pt edge: 12 at both ends, whatever the bar ends with).
const ROW: f32 = 28.0;
const PAD: i8 = 11;
/// A relation's tile, the symbol on it, and the gap between two tiles.
const TILE: egui::Vec2 = egui::vec2(28.0, 26.0);
const SYMBOL: f32 = 16.0;
const TILE_GAP: f32 = 2.0;
/// The room after the caption, and the hairline's between the relations and the dimensions.
const CAPTION_GAP: f32 = 8.0;
const DIVIDER: f32 = 13.0;
/// A dimension's chip, and the gap between two.
const CHIP: f32 = 24.0;
const CHIP_GAP: f32 = 4.0;
/// How many relations and dimensions show before the rest go under "More".
const MAX_ITEMS: usize = 9;
/// The canvas has to be this wide for the bar to show.
const MIN_CANVAS: f32 = 420.0;

/// Where the bar was drawn this frame (kept in egui's memory for the tests).
pub(crate) fn rect_id() -> egui::Id {
    egui::Id::new("quick_bar_rect")
}

/// Where the empty canvas's hint was painted this frame (for the tests).
pub(crate) fn hint_rect_id() -> egui::Id {
    egui::Id::new("empty_canvas_hint_rect")
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

/// The bar for this frame: where it goes and what it offers.
pub(crate) struct QuickBar {
    pub(crate) rect: egui::Rect,
    items: Vec<Item>,
}

/// What shows on the bar and what goes under "More".
fn split(items: &[Item]) -> (&[Item], &[Item]) {
    if items.len() > MAX_ITEMS {
        items.split_at(MAX_ITEMS - 1)
    } else {
        (items, &items[..0])
    }
}

/// One thing in the bar's row, left to right.
enum Step<'a> {
    Gap(f32),
    Caption,
    Relation(&'a Item),
    Divider,
    Chip(&'a Item),
    More,
}

/// The bar's row: the caption, the relations' tiles, a hairline, the dimensions' chips, and
/// "More" for what found no room. The one description of the row: it is measured from this
/// before the bar is placed, and drawn from this.
fn steps(shown: &[Item], more: bool) -> Vec<Step<'_>> {
    let mut row = vec![Step::Caption, Step::Gap(CAPTION_GAP)];
    // (Whether the one before was a dimension.)
    let mut before: Option<bool> = None;
    for item in shown {
        match (before, item.is_dimension) {
            (Some(false), false) => row.push(Step::Gap(TILE_GAP)),
            (Some(false), true) => row.push(Step::Divider),
            (Some(true), _) => row.push(Step::Gap(CHIP_GAP)),
            (None, _) => {}
        }
        row.push(if item.is_dimension {
            Step::Chip(item)
        } else {
            Step::Relation(item)
        });
        before = Some(item.is_dimension);
    }
    if more {
        row.push(Step::Gap(CHIP_GAP));
        row.push(Step::More);
    }
    row
}

fn caption_font() -> egui::FontId {
    egui::FontId::new(theme::size::CAPTION, theme::bold())
}

fn chip_font() -> egui::FontId {
    egui::FontId::proportional(theme::size::LABEL)
}

/// How wide `text` is set in `font`, in whole points.
fn text_width(painter: &egui::Painter, text: &str, font: egui::FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
        .size()
        .x
        .ceil()
}

/// How wide a step of the row is.
fn step_width(painter: &egui::Painter, step: &Step<'_>) -> f32 {
    match step {
        Step::Gap(width) => *width,
        Step::Caption => text_width(painter, "Constrain", caption_font()),
        Step::Relation(_) => TILE.x,
        Step::Divider => DIVIDER,
        Step::Chip(item) => text_width(painter, item.option.label, chip_font()) + 18.0,
        Step::More => text_width(painter, "More", chip_font()) + 30.0,
    }
}

/// How wide the bar is with these on it.
fn bar_width(painter: &egui::Painter, shown: &[Item], more: bool) -> f32 {
    let row: f32 = steps(shown, more)
        .iter()
        .map(|step| step_width(painter, step))
        .sum();
    row + 2.0 * (f32::from(PAD) + 1.0)
}

/// Where a bar of `size` goes on `canvas`, given where each pick shows on screen: centred
/// [`NEAR`] under all of them when they are in view and there is room, else as far above
/// them; and when they are out of view (or fill it), at the foot of the canvas, or at its
/// top when a pick lies there. Under or over the picks it never lies on what is clicked on
/// the canvas there (`keep_clear`: the dimensions' labels, the relations' symbols): it goes
/// to the foot instead. Nor under a floating panel (`covered`): it slides sideways clear of
/// one, and failing that goes to the foot or the top. Inside the canvas, never over a pick,
/// and the same every frame for the same picks and view.
pub(crate) fn place(
    canvas: egui::Rect,
    size: egui::Vec2,
    picks: &[egui::Rect],
    keep_clear: &[egui::Rect],
    covered: &[egui::Rect],
) -> egui::Rect {
    let (lo, hi) = (canvas.left() + MARGIN, canvas.right() - MARGIN - size.x);
    let left = |middle: f32| {
        if hi >= lo {
            (middle - size.x / 2.0).clamp(lo, hi)
        } else {
            canvas.center().x - size.x / 2.0
        }
    };
    let at = |x: f32, y: f32| egui::Rect::from_min_size(egui::pos2(x, y).round(), size);
    let fits = |top: f32| top >= canvas.top() + MARGIN && top + size.y <= canvas.bottom() - MARGIN;
    let covers = |bar: egui::Rect| keep_clear.iter().chain(covered).any(|r| r.intersects(bar));
    // At a height: where it would be, else just left or right of each floating panel it
    // would lie under (staying in the canvas).
    let along = |x: f32, top: f32| -> Vec<egui::Rect> {
        let first = at(x, top);
        let mut bars = vec![first];
        for r in covered.iter().filter(|r| r.intersects(first)) {
            for x in [r.left() - MARGIN - size.x, r.right() + MARGIN] {
                if x >= lo - 0.5 && x <= hi + 0.5 {
                    bars.push(at(x, top));
                }
            }
        }
        bars
    };
    let all = picks.iter().copied().reduce(|a, b| a.union(b));
    if let Some(all) = all.filter(|all| canvas.intersects(*all)) {
        let x = left(all.center().x);
        for top in [all.bottom() + NEAR, all.top() - NEAR - size.y] {
            if !fits(top) {
                continue;
            }
            if let Some(bar) = along(x, top).into_iter().find(|bar| !covers(*bar)) {
                return bar;
            }
        }
    }
    // At the foot, else at the top: whichever is clear of the picks, of what is clicked and
    // of the panels; failing both, the one clear of the picks.
    let x = left(canvas.center().x);
    let foot = at(x, canvas.bottom() - MARGIN - size.y);
    let top = at(x, canvas.top() + MARGIN);
    let on_pick = |bar: egui::Rect| picks.iter().any(|pick| pick.intersects(bar.expand(8.0)));
    along(foot.left(), foot.top())
        .into_iter()
        .chain(along(top.left(), top.top()))
        .find(|bar| !on_pick(*bar) && !covers(*bar))
        .unwrap_or(if on_pick(foot) { top } else { foot })
}

/// What a relation does, in a sentence, for its tooltip.
fn relation_about(label: &str) -> &'static str {
    match label {
        "Horizontal" => "Makes the picked lines level, or puts two points level with each other.",
        "Vertical" => "Makes the picked lines upright, or puts two points one above the other.",
        "Parallel" => "Makes the picked lines run in the same direction.",
        "Perpendicular" => "Makes the picked lines square to each other.",
        "Collinear" => "Puts the picked lines on one straight line.",
        "Equal" => "Gives the picked lines one length, or the picked circles and arcs one radius.",
        "Fix" => "Holds what is picked where it is.",
        "Coincident" => "Brings the picked points together, and keeps them together.",
        "Midpoint" => "Keeps the point at the middle of the line.",
        "On curve" => "Keeps the point on the curve.",
        "Tangent" => "Makes the two curves touch smoothly, with no corner where they meet.",
        "Concentric" => "Gives the picked circles and arcs one centre.",
        "Coradial" => "Gives the picked circles and arcs one centre and one radius.",
        "Symmetric" => "Keeps the two picks mirror images of each other about the line.",
        _ => "Adds this relation to what is picked.",
    }
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
    /// something picked that relations or dimensions fit, and nothing else going on (the
    /// start panel included). `canvas` is the canvas's own response: while the pointer is
    /// down on the canvas (a click, a drag, a pan) the bar is out of the way. Where it goes
    /// is worked out here, from what it holds and where the picks show: nothing of the
    /// frame before is used, so it is in its place from the first frame. `keep_clear` is
    /// what is clicked on the canvas (see [`place`]).
    pub(crate) fn quick_bar_wanted(
        &mut self,
        canvas: &egui::Response,
        map: &ScreenMap,
        painter: &egui::Painter,
        keep_clear: &[egui::Rect],
    ) -> Option<QuickBar> {
        let ed = &self.drawing.editor;
        if !self.show_quick_bar
            || self.show_start
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
        let (shown, more) = split(&items);
        let size = egui::vec2(bar_width(painter, shown, !more.is_empty()), HEIGHT);
        let ed = &self.drawing.editor;
        let picks: Vec<egui::Rect> = ed
            .selection
            .iter()
            .chain(ed.selected_midpoints.iter())
            // (Plenty to tell by; a pick of thousands isn't walked every frame.)
            .take(2000)
            .filter_map(|id| ed.doc.shape(*id))
            .map(|shape| shape.bounds())
            .filter(|b| !b.is_empty())
            .map(|b| egui::Rect::from_two_pos(map.to_screen(b.min), map.to_screen(b.max)))
            .collect();
        let rect = place(map.rect, size, &picks, keep_clear, &self.floating_windows());
        Some(QuickBar { rect, items })
    }

    /// Draw the quick-constrain bar on the canvas, and do what is clicked on it.
    pub(crate) fn quick_bar(&mut self, ctx: &egui::Context, canvas: egui::Rect, bar: QuickBar) {
        let QuickBar { rect, items } = bar;
        let dark = ctx.theme() == egui::Theme::Dark;
        let n = theme::neutrals(dark);
        let (shown, more) = split(&items);
        let mut chosen: Option<Item> = None;
        let style = ctx.style_of(ctx.theme());
        let area = egui::Area::new(egui::Id::new("quick_bar"))
            .order(egui::Order::Middle)
            .fixed_pos(rect.min)
            // (Where it was placed, to the point, from the first frame: it is clipped to the
            // canvas, and not moved by egui to fit a size it only knows a frame late.)
            .constrain_to(canvas)
            .constrain(false)
            .default_size(rect.size())
            .show(ctx, |ui| {
                egui::Frame::popup(&style)
                    .fill(n.raised)
                    .stroke(egui::Stroke::new(1.0, n.control_border))
                    .corner_radius(theme::radius::MENU)
                    // (With the 1 pt edge: 36 high around a row of 28.)
                    .inner_margin(egui::Margin {
                        left: PAD,
                        right: PAD,
                        top: 3,
                        bottom: 3,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.set_height(ROW);
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for step in steps(shown, !more.is_empty()) {
                                let width = step_width(ui.painter(), &step);
                                let (item, response) = match step {
                                    Step::Gap(_) => {
                                        ui.add_space(width);
                                        continue;
                                    }
                                    Step::Caption => {
                                        caption(ui, width);
                                        continue;
                                    }
                                    Step::Divider => {
                                        divider(ui);
                                        continue;
                                    }
                                    Step::More => {
                                        if let Some(item) = more_menu(ui, more, width) {
                                            chosen = Some(item);
                                        }
                                        continue;
                                    }
                                    Step::Relation(item) => (item, relation_button(ui, item)),
                                    Step::Chip(item) => (item, dimension_chip(ui, item, width)),
                                };
                                if self.quick_tip(response, item).clicked() {
                                    chosen = Some(item.clone());
                                }
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
            // is in view (its tab in front, and the panels not all hidden), or else on the
            // dimension itself.
            let inspector =
                self.layout.is_shown(crate::dock::Panel::Inspector) && !self.dock_view.hide_all;
            if inspector {
                self.drawing.focus_dimension = Some(id);
            } else if self.show_dimensions {
                self.open_dimension_edit(id);
            }
        }
    }

    /// What a button on the bar says when pointed at: its name, what it does, and which of
    /// the picks give way (as the Inspector's chip says), in whole sentences.
    fn quick_tip(&self, response: egui::Response, item: &Item) -> egui::Response {
        let name = item.option.label;
        let about = if item.is_dimension {
            "Adds this dimension to what is picked. Type its value straight away.".to_owned()
        } else {
            let what = relation_about(name);
            match self.drawing.editor.relation_role(name) {
                Some("They all move") => format!(
                    "{what} None of the picks is the active one, so all of them may move: \
                     Shift-click one to keep it where it is."
                ),
                Some(role) => format!("{what} {role}."),
                None => what.to_owned(),
            }
        };
        response.on_hover_ui(|ui| theme::command_tip(ui, name, "", &about))
    }

    /// Whether the empty canvas says how to start: nothing in the drawing at all (no curve
    /// or point, text, fill, block copy, note, underlay or wall), the start panel closed,
    /// the Select tool idle and nothing else up.
    pub(crate) fn empty_hint_wanted(&self, canvas: egui::Rect) -> bool {
        let doc = &self.drawing.editor.doc;
        !self.show_start
            && doc.is_empty()
            && doc.texts().next().is_none()
            && doc.fills().next().is_none()
            && doc.instances().next().is_none()
            && doc.annotations().next().is_none()
            && doc.underlays().next().is_none()
            && doc.walls().next().is_none()
            && self.tool.kind() == ToolKind::Select
            && !self.tool.is_busy()
            && !self.drawing.view.sheet_preview
            && !self.canvas_occupied()
            && canvas.width() >= 360.0
            && canvas.height() >= 220.0
    }

    /// The hint on an empty canvas: how to start, in three quiet lines. Painted only (key
    /// caps and words, nothing behind them): it takes no clicks and nothing is added to what
    /// a screen reader sees. It sits clear of the origin's axes, in the middle of the
    /// quarter of the canvas under and to the right of them (or of another quarter with the
    /// room). Gone as soon as anything exists.
    pub(crate) fn empty_canvas_hint(&self, ui: &egui::Ui, map: &ScreenMap) {
        let canvas = map.rect;
        ui.ctx()
            .data_mut(|d| d.remove::<egui::Rect>(hint_rect_id()));
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
        const GAP: f32 = 12.0;
        let font = egui::FontId::proportional(theme::size::BODY);
        let painter = ui.painter_at(canvas);
        let texts: Vec<_> = lines
            .iter()
            .map(|(_, what)| painter.layout_no_wrap((*what).to_owned(), font.clone(), n.tertiary))
            .collect();
        // The keys end and the words start either side of one line down the block.
        let keys_w = lines
            .iter()
            .map(|(keys, _)| theme::keycaps_width(ui, keys))
            .fold(0.0_f32, f32::max);
        let text_w = texts.iter().map(|g| g.size().x).fold(0.0_f32, f32::max);
        let size = egui::vec2(keys_w + GAP + text_w, LINE * lines.len() as f32);
        let Some(block) = hint_place(canvas, map.to_screen(DVec2::ZERO), size) else {
            return;
        };
        ui.ctx().data_mut(|d| d.insert_temp(hint_rect_id(), block));
        let spine = block.left() + keys_w;
        for (i, ((keys, _), text)) in lines.iter().zip(texts).enumerate() {
            let y = block.top() + LINE * (i as f32 + 0.5);
            theme::keycap_at(ui, egui::pos2(spine, y), keys);
            painter.galley(
                egui::pos2(spine + GAP, y - text.size().y / 2.0),
                text,
                n.tertiary,
            );
        }
    }
}

/// Where the empty canvas's hint (a block of `size`) goes: the middle of the quarter of the
/// canvas under and to the right of the origin's axes, or of the first other quarter with
/// the room for it and some air; nowhere when no quarter has. With the origin out of view
/// along a direction the canvas isn't divided that way.
pub(crate) fn hint_place(
    canvas: egui::Rect,
    origin: egui::Pos2,
    size: egui::Vec2,
) -> Option<egui::Rect> {
    /// The least air between the block and an axis or the canvas's edge.
    const AIR: f32 = 24.0;
    let halves = |at: f32, range: egui::Rangef| {
        if at > range.min && at < range.max {
            // (After the axis first: right of it, under it.)
            [
                egui::Rangef::new(at, range.max),
                egui::Rangef::new(range.min, at),
            ]
        } else {
            [range, range]
        }
    };
    let columns = halves(origin.x, canvas.x_range());
    let rows = halves(origin.y, canvas.y_range());
    [(0, 0), (1, 0), (0, 1), (1, 1)]
        .into_iter()
        .map(|(column, row)| egui::Rect::from_x_y_ranges(columns[column], rows[row]))
        .find(|quarter| {
            quarter.width() >= size.x + 2.0 * AIR && quarter.height() >= size.y + 2.0 * AIR
        })
        .map(|quarter| egui::Rect::from_min_size((quarter.center() - size / 2.0).round(), size))
}

/// The bar's caption: painted, so it is nothing to click and has no name of its own. Its
/// letters' middle is on the middle of the row, where the symbols' is.
fn caption(ui: &mut egui::Ui, width: f32) {
    let n = theme::neutrals(ui.visuals().dark_mode);
    let galley = ui
        .painter()
        .layout_no_wrap("Constrain".to_owned(), caption_font(), n.tertiary);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW), egui::Sense::hover());
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.mesh_bounds.center().y),
        galley,
        n.tertiary,
    );
}

/// A hairline between the relations and the dimensions.
fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(DIVIDER, 16.0), egui::Sense::hover());
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

/// A relation: a tile with the relation's own symbol, in the colour of the bar's text.
fn relation_button(ui: &mut egui::Ui, item: &Item) -> egui::Response {
    let n = theme::neutrals(ui.visuals().dark_mode);
    let (rect, response) = ui.allocate_exact_size(TILE, egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, item.label()));
    if let Some(fill) = button_fill(ui, &response) {
        ui.painter().rect_filled(rect, theme::radius::CONTROL, fill);
    }
    // (On whole pixels, so the symbol's strokes are as crisp as on the canvas.)
    let center = rect.center().round();
    annotations::paint_glyph_icon(
        ui.painter(),
        egui::Rect::from_center_size(center, egui::Vec2::splat(SYMBOL)),
        app::pie::glyph(item.option.label),
        n.text,
    );
    response
}

/// A dimension: a compact chip with its name.
fn dimension_chip(ui: &mut egui::Ui, item: &Item, width: f32) -> egui::Response {
    let n = theme::neutrals(ui.visuals().dark_mode);
    let galley = ui
        .painter()
        .layout_no_wrap(item.option.label.to_owned(), chip_font(), n.text);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, CHIP), egui::Sense::click());
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
fn more_menu(ui: &mut egui::Ui, items: &[Item], width: f32) -> Option<Item> {
    let n = theme::neutrals(ui.visuals().dark_mode);
    let galley = ui
        .painter()
        .layout_no_wrap("More".to_owned(), chip_font(), n.text);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, CHIP), egui::Sense::click());
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
    theme::chevron(
        ui.painter(),
        egui::pos2(rect.right() - 12.0, rect.center().y + 0.5),
        true,
        n.secondary,
    );
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
