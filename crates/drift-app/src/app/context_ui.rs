//! The context bar under the menu (the active tool and its options, or quick actions for the
//! selection, as in Affinity's context toolbar) and the canvas's right-click menu.

use crate::app::palette::{Command, ViewFlag};
use crate::icons::{self, Glyph};
use crate::tools::SlotMode;
use crate::*;

/// A thin vertical divider.
fn divider(ui: &mut egui::Ui) {
    ui.add_space(2.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(1.0, 18.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 0.0, theme::hairline(ui));
    ui.add_space(2.0);
}

/// A small caption before a control.
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).weak());
}

/// A segmented choice sized to its labels (for the bar, where the full width isn't ours).
pub(super) fn choice<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: T,
    options: &[(T, &str)],
) -> Option<T> {
    let font = egui::FontId::proportional(12.0);
    let width: f32 = options
        .iter()
        .map(|(_, l)| {
            ui.painter()
                .layout_no_wrap((*l).to_owned(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
                + 22.0
        })
        .sum();
    let mut out = None;
    ui.allocate_ui(egui::vec2(width + 4.0, 24.0), |ui| {
        out = theme::segmented(ui, current, options);
    });
    out
}

/// The select tool's filter: a toggle for each kind it picks, and All. Alt-click or
/// double-click a toggle for that kind alone. Returns the filter clicked to.
fn pick_filter_toggles(
    ui: &mut egui::Ui,
    filter: crate::editor::PickFilter,
) -> Option<crate::editor::PickFilter> {
    use crate::editor::{PickFilter, PickKind};
    let mut out = None;
    ui.spacing_mut().item_spacing.x = 2.0;
    caption(ui, "Pick");
    for kind in PickKind::ALL {
        let on = filter.has(kind);
        let label = match kind {
            // (Short, so the bar fits; the tooltip has the whole name.)
            PickKind::Blocks => "Blocks",
            k => k.label(),
        };
        let response = ui
            .add(
                egui::Button::new(egui::RichText::new(label).size(12.0))
                    .selected(on)
                    .min_size(egui::vec2(0.0, 22.0)),
            )
            .on_hover_text(format!(
                "{}: picks {} · Alt-click or double-click: pick only {}",
                kind.label(),
                kind.picks(),
                kind.label().to_lowercase()
            ));
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                true,
                on,
                format!("Pick {}", kind.label()),
            )
        });
        let alone =
            response.double_clicked() || (response.clicked() && ui.input(|i| i.modifiers.alt));
        if alone {
            out = Some(PickFilter::only(kind));
        } else if response.clicked() {
            out = Some(filter.with(kind, !on));
        }
    }
    ui.add_space(2.0);
    if ui
        .add_enabled(
            !filter.is_all(),
            egui::Button::new(egui::RichText::new("All").size(12.0))
                .min_size(egui::vec2(0.0, 22.0)),
        )
        .on_hover_text("Pick everything again")
        .clicked()
    {
        out = Some(PickFilter::ALL);
    }
    out
}

/// [`choice`], with a tooltip on the whole control (for the key that switches it).
fn keyed_choice<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: T,
    options: &[(T, &str)],
    tip: &str,
) -> Option<T> {
    let mut out = None;
    ui.scope(|ui| out = choice(ui, current, options))
        .response
        .on_hover_text(tip);
    out
}

/// A colour change made from a swatch: to the selection's own colour, or its layer's
/// (`None` is "by layer" / automatic).
enum Edit {
    Own(Option<drift_core::Rgb>),
    Layer(Option<drift_core::Rgb>),
}

/// A colour swatch on the bar.
struct Swatch<'a> {
    name: &'a str,
    /// The colour in effect.
    shown: egui::Color32,
    /// Filled (a fill) or a ring (a stroke).
    solid: bool,
    /// The layer's name.
    layer: &'a str,
    /// The selection's own colour, if it can have one (`Some(None)` = follows its layer).
    own: Option<Option<drift_core::Rgb>>,
    layer_color: Option<drift_core::Rgb>,
    /// What the layer's colour is called when it isn't set.
    layer_auto: &'a str,
}

/// A captioned swatch that opens a colour editor: for the selection or for its layer.
fn swatch(ui: &mut egui::Ui, s: Swatch<'_>) -> Option<Edit> {
    caption(ui, s.name);
    let (r, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, s.name));
    let inner = r.shrink(2.0);
    if s.solid {
        ui.painter().rect_filled(inner, 5.0, s.shown);
    } else {
        ui.painter().rect_stroke(
            inner.shrink(1.5),
            4.0,
            egui::Stroke::new(3.0, s.shown),
            egui::StrokeKind::Middle,
        );
    }
    let edge = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        theme::hairline(ui)
    };
    ui.painter().rect_stroke(
        inner,
        5.0,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Outside,
    );
    let response = response.on_hover_text(format!(
        "{} colour: of the selection, or of its layer",
        s.name
    ));
    let mut out = None;
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(220.0);
            // Which colour is being edited: the selection's own, or its layer's.
            let key = ui.id().with(("swatch_layer", s.name));
            let mut on_layer =
                s.own.is_none() || ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
            if s.own.is_some() {
                let tabs = [(false, "Selection"), (true, "Layer")];
                if let Some(v) = theme::segmented(ui, on_layer, &tabs) {
                    on_layer = v;
                    ui.data_mut(|d| d.insert_temp(key, v));
                }
                ui.add_space(4.0);
            }
            let (value, auto_label) = if on_layer {
                ui.label(
                    egui::RichText::new(format!(
                        "Layer {}: everything on it follows unless it has its own colour",
                        s.layer
                    ))
                    .small()
                    .weak(),
                );
                (s.layer_color, s.layer_auto)
            } else {
                (s.own.flatten(), "By layer")
            };
            let mut edited = None;
            let mut auto = value.is_none();
            if ui.checkbox(&mut auto, auto_label).changed() {
                edited = Some(if auto {
                    None
                } else {
                    Some(drift_core::Rgb([s.shown.r(), s.shown.g(), s.shown.b()]))
                });
            }
            if let Some(drift_core::Rgb([r, g, b])) = value {
                let mut c = egui::Color32::from_rgb(r, g, b);
                if egui::color_picker::color_picker_color32(
                    ui,
                    &mut c,
                    egui::color_picker::Alpha::Opaque,
                ) {
                    edited = Some(Some(drift_core::Rgb([c.r(), c.g(), c.b()])));
                }
            }
            if let Some(c) = edited {
                out = Some(if on_layer {
                    Edit::Layer(c)
                } else {
                    Edit::Own(c)
                });
            }
        });
    out
}

/// Dash lengths of a line type's sample (points; empty for a continuous line).
fn sample_dashes(t: drift_core::LineType) -> &'static [f32] {
    use drift_core::LineType as T;
    match t {
        T::Continuous => &[],
        T::Dashed => &[6.0, 3.0],
        T::Hidden => &[3.0, 2.0],
        T::Center => &[9.0, 2.0, 2.0, 2.0],
        T::Phantom => &[9.0, 2.0, 2.0, 2.0, 2.0, 2.0],
        // No stroke: faint dots, as the canvas shows it picked.
        T::None => &[1.0, 3.0],
    }
}

/// A line drawn across `rect`, as a sample of a weight (mm; 0 is the hairline) and type.
fn line_sample(ui: &egui::Ui, rect: egui::Rect, mm: f64, t: drift_core::LineType) {
    let width = ((mm as f32) * crate::annotations::PT_PER_MM).clamp(0.6, 7.0);
    let stroke = egui::Stroke::new(width, ui.visuals().text_color());
    let (a, b) = (rect.left_center(), rect.right_center());
    let dashes = sample_dashes(t);
    if dashes.is_empty() {
        ui.painter().line_segment([a, b], stroke);
        return;
    }
    let mut x = a.x;
    'row: loop {
        for pair in dashes.chunks(2) {
            if x >= b.x {
                break 'row;
            }
            let end = (x + pair[0]).min(b.x);
            ui.painter()
                .line_segment([egui::pos2(x, a.y), egui::pos2(end, a.y)], stroke);
            x = end + pair.get(1).copied().unwrap_or(2.0);
        }
    }
}

/// A menu of stroke settings on the bar: "By layer" (showing what the layer gives), then
/// each option with a sample of the line. Returns the pick.
#[allow(clippy::too_many_arguments)]
fn stroke_menu<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    width: f32,
    current: Option<T>,
    from_layer: T,
    options: &[T],
    label: impl Fn(T) -> String,
    sample: impl Fn(&egui::Ui, egui::Rect, T),
    hover: &str,
) -> Option<Option<T>> {
    let mut picked = None;
    let shown = current.unwrap_or(from_layer);
    let closed = egui::ComboBox::from_id_salt(id)
        .width(width)
        // Every option at once: nothing to scroll for.
        .height(420.0)
        .selected_text("")
        .show_ui(ui, |ui| {
            ui.set_min_width(170.0);
            let mut row = |ui: &mut egui::Ui, value: Option<T>, text: String| {
                let r = ui.selectable_label(current == value, text);
                let room = egui::Rect::from_min_max(
                    egui::pos2(r.rect.right() - 62.0, r.rect.top()),
                    egui::pos2(r.rect.right() - 8.0, r.rect.bottom()),
                );
                sample(ui, room, value.unwrap_or(from_layer));
                if r.clicked() {
                    picked = Some(value);
                }
            };
            row(ui, None, "By layer".to_owned());
            ui.separator();
            for &option in options {
                row(ui, Some(option), label(option));
            }
        })
        .response;
    // The closed menu shows the line itself, dimmed while it comes from the layer.
    let room = closed.rect.shrink2(egui::vec2(8.0, 0.0));
    let room = egui::Rect::from_min_max(room.min, egui::pos2(room.right() - 14.0, room.bottom()));
    if current.is_none() {
        ui.scope(|ui| {
            ui.set_opacity(0.55);
            sample(ui, room, shown);
        });
    } else {
        sample(ui, room, shown);
    }
    closed.on_hover_text(format!(
        "{hover}: {}{}",
        label(shown),
        if current.is_none() { " (by layer)" } else { "" }
    ));
    picked
}

impl DriftApp {
    /// The selection's layer, stroke and fill, as in a drawing program's control bar: a
    /// layer menu and two colour swatches. Each swatch edits the colour of the selection
    /// itself or of its layer (which everything on the layer follows unless overridden).
    fn selection_look(&mut self, ui: &mut egui::Ui) {
        let ed = &self.editor;
        let dark = ui.visuals().dark_mode;
        let curves: Vec<drift_core::EntityId> = ed
            .selection
            .iter()
            .copied()
            .filter(|id| ed.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
            .collect();
        let fills: Vec<drift_core::FillId> = ed
            .selected_fills
            .iter()
            .copied()
            .filter(|id| ed.doc.fill(*id).is_some())
            .collect();
        let first = curves.first().and_then(|id| ed.doc.get(*id)).copied();
        let first_fill = fills.first().and_then(|id| ed.doc.fill(*id)).cloned();
        let Some(layer_id) = first
            .map(|e| e.layer)
            .or(first_fill.as_ref().map(|f| f.layer))
        else {
            return;
        };
        let Some(layer) = ed.doc.layer(layer_id).cloned() else {
            return;
        };
        let mixed = curves
            .iter()
            .any(|id| ed.doc.get(*id).is_some_and(|e| e.layer != layer_id));
        let layers: Vec<(drift_core::LayerId, String)> = ed
            .doc
            .layers()
            .map(|(id, l)| (id, l.name.clone()))
            .collect();

        // Layer.
        caption(ui, "Layer");
        let mut target = None;
        egui::ComboBox::from_id_salt("bar_layer")
            .selected_text(if mixed {
                "(mixed)".to_owned()
            } else {
                layer.name.clone()
            })
            .width(92.0)
            .show_ui(ui, |ui| {
                for (id, name) in layers {
                    if ui
                        .selectable_label(!mixed && id == layer_id, name)
                        .clicked()
                    {
                        target = Some(id);
                    }
                }
            })
            .response
            .on_hover_text("The selection's layer: pick another to move it there");
        if let Some(id) = target {
            self.editor.move_selection_to_layer(id);
        }

        // Stroke: the curves' own colour, or their layer's.
        if let Some(entity) = first {
            let own = entity.appearance.color;
            let shown = crate::layers_ui::to_color32(own.or(layer.color), dark);
            let edit = swatch(
                ui,
                Swatch {
                    name: "Stroke",
                    shown,
                    solid: false,
                    layer: &layer.name,
                    own: Some(own),
                    layer_color: layer.color,
                    layer_auto: "Automatic (black or white)",
                },
            );
            match edit {
                Some(Edit::Own(c)) => self.editor.update_selection_appearance(|a| a.color = c),
                Some(Edit::Layer(c)) => {
                    let mut l = layer.clone();
                    l.color = c;
                    self.editor.update_layer(layer_id, l);
                }
                None => {}
            }
            // Its weight and line type, beside it: the curves' own, or the layer's.
            let look = entity.appearance;
            caption(ui, "Weight");
            let weight = stroke_menu(
                ui,
                "bar_weight",
                58.0,
                look.weight,
                layer.weight,
                &drift_core::LineWeight::ISO,
                |w: drift_core::LineWeight| w.label(),
                |ui, rect, w| line_sample(ui, rect, w.mm(), drift_core::LineType::Continuous),
                "Stroke weight",
            );
            if let Some(w) = weight {
                self.editor.update_selection_appearance(|a| a.weight = w);
            }
            caption(ui, "Type");
            let kind = stroke_menu(
                ui,
                "bar_line_type",
                58.0,
                look.line_type,
                layer.line_type,
                &drift_core::LineType::ALL,
                |t: drift_core::LineType| t.label().to_owned(),
                |ui, rect, t| line_sample(ui, rect, 0.35, t),
                "Line type",
            );
            if let Some(t) = kind {
                self.editor.update_selection_appearance(|a| a.line_type = t);
            }
        }

        // Fill: the picked fills' own colour, or the layer's fill colour.
        let own = first_fill.as_ref().and_then(|f| f.color);
        let shown = crate::layers_ui::to_color32(own.or(layer.fill_colour()), dark);
        let edit = swatch(
            ui,
            Swatch {
                name: "Fill",
                shown,
                solid: true,
                layer: &layer.name,
                // Without a fill picked there is only the layer's colour to set.
                own: first_fill.as_ref().map(|_| own),
                layer_color: layer.fill_color,
                layer_auto: "Same as the layer's stroke",
            },
        );
        match edit {
            Some(Edit::Own(c)) => {
                for id in fills {
                    if let Some(mut f) = self.editor.doc.fill(id).cloned() {
                        f.color = c;
                        self.editor.update_fill(id, f);
                    }
                }
            }
            Some(Edit::Layer(c)) => {
                let mut l = layer.clone();
                l.fill_color = c;
                self.editor.update_layer(layer_id, l);
            }
            None => {}
        }
    }

    /// The context bar: the tool's icon and name, then its options (or, with the select tool,
    /// actions for the selection), and a search field for the command palette at the right.
    pub(crate) fn context_bar(&mut self, ui: &mut egui::Ui) {
        let kind = self.tool.kind();
        ui.horizontal(|ui| {
            ui.set_height(28.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            // The palette's search field, from the right.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.search_field(ui) {
                    self.open_palette();
                }
                divider(ui);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // Whatever doesn't fit is cut off rather than drawn over the right side.
                    ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                    let accent = theme::accent(ui.visuals().dark_mode);
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                    icons::tool(ui.painter(), r.shrink(1.0), kind, accent);
                    // Painted rather than a label, so the toolbar keeps the only "Select"
                    // (etc.) button for assistive tech.
                    let name = ui.painter().layout_no_wrap(
                        kind.label().to_owned(),
                        egui::FontId::new(13.0, theme::bold()),
                        ui.visuals().strong_text_color(),
                    );
                    let (r, _) = ui.allocate_exact_size(name.size(), egui::Sense::hover());
                    ui.painter().galley(r.min, name, egui::Color32::WHITE);
                    divider(ui);
                    self.tool_options(ui, kind);
                });
            });
        });
    }

    /// A field-like button that opens the command palette.
    fn search_field(&mut self, ui: &mut egui::Ui) -> bool {
        let (r, response) = ui.allocate_exact_size(egui::vec2(180.0, 24.0), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Search commands")
        });
        let visuals = ui.visuals();
        let fill = if response.hovered() {
            visuals.widgets.hovered.bg_fill
        } else {
            visuals.extreme_bg_color
        };
        ui.painter().rect(
            r,
            7.0,
            fill,
            egui::Stroke::new(1.0, theme::hairline(ui)),
            egui::StrokeKind::Inside,
        );
        let weak = ui.visuals().weak_text_color();
        icons::glyph(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(r.left() + 14.0, r.center().y),
                egui::Vec2::splat(13.0),
            ),
            Glyph::Search,
            weak,
        );
        ui.painter().text(
            egui::pos2(r.left() + 26.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            "Search commands",
            egui::FontId::proportional(12.5),
            weak,
        );
        ui.painter().text(
            egui::pos2(r.right() - 8.0, r.center().y),
            egui::Align2::RIGHT_CENTER,
            app::panels::shortcut("K", false),
            egui::FontId::proportional(theme::size::CAPTION),
            weak.gamma_multiply(0.8),
        );
        response
            .on_hover_text("Find any tool, setting, panel or command")
            .clicked()
    }

    /// Options for the active tool.
    fn tool_options(&mut self, ui: &mut egui::Ui, kind: ToolKind) {
        let units = self.editor.doc.units();
        // What a drawing tool draws: geometry, or construction guides.
        if kind.draws_curves() {
            if let Some(c) = keyed_choice(
                ui,
                self.editor.draw_construction,
                &[(false, "Geometry"), (true, "Construction")],
                "Draw geometry, or construction guides that don't print (Alt+C)",
            ) {
                self.editor.draw_construction = c;
            }
            divider(ui);
        }
        match kind {
            // On a sheet the model's selection isn't what is being worked on.
            ToolKind::Select if self.sheet_preview => {}
            ToolKind::Select => self.selection_actions(ui),
            ToolKind::Circle | ToolKind::Ellipse | ToolKind::EllipseArc => {
                if let Some(k) = keyed_choice(
                    ui,
                    kind,
                    &[
                        (ToolKind::Circle, "Circle"),
                        (ToolKind::Ellipse, "Ellipse"),
                        (ToolKind::EllipseArc, "Partial ellipse"),
                    ],
                    "C again switches to the next",
                ) {
                    self.switch_tool(k);
                }
            }
            ToolKind::Arc | ToolKind::ThreePointArc | ToolKind::TangentArc => {
                if let Some(k) = keyed_choice(
                    ui,
                    kind,
                    &[
                        (ToolKind::Arc, "Centre"),
                        (ToolKind::ThreePointArc, "3-point"),
                        (ToolKind::TangentArc, "Tangent"),
                    ],
                    "A again switches to the next",
                ) {
                    self.switch_tool(k);
                }
            }
            ToolKind::Polygon => {
                caption(ui, "Sides");
                let mut sides = if self.editor.polygon_sides == 0 {
                    6
                } else {
                    self.editor.polygon_sides
                };
                if ui
                    .add(egui::DragValue::new(&mut sides).range(3..=64).speed(0.1))
                    .changed()
                {
                    self.editor.polygon_sides = sides;
                }
                if let Some(c) = keyed_choice(
                    ui,
                    self.editor.polygon_circumscribed,
                    &[(false, "Inscribed"), (true, "Circumscribed")],
                    "X switches while drawing",
                ) {
                    self.editor.polygon_circumscribed = c;
                }
            }
            ToolKind::Slot => {
                if let Some(m) = keyed_choice(
                    ui,
                    self.editor.slot_mode,
                    &[
                        (SlotMode::Straight, "Straight"),
                        (SlotMode::Centre, "Centre"),
                        (SlotMode::Arc, "Arc"),
                    ],
                    "U again switches to the next",
                ) {
                    self.editor.slot_mode = m;
                    self.tool.cancel(&mut self.editor);
                }
            }
            ToolKind::Fillet | ToolKind::Chamfer => {
                if let Some(k) = keyed_choice(
                    ui,
                    kind,
                    &[(ToolKind::Fillet, "Fillet"), (ToolKind::Chamfer, "Chamfer")],
                    "F again switches",
                ) {
                    self.switch_tool(k);
                }
            }
            ToolKind::Trim | ToolKind::Extend | ToolKind::Split => {
                if let Some(k) = keyed_choice(
                    ui,
                    kind,
                    &[
                        (ToolKind::Trim, "Trim"),
                        (ToolKind::Extend, "Extend"),
                        (ToolKind::Split, "Split"),
                    ],
                    "T trims, E extends; Split has no key",
                ) {
                    self.switch_tool(k);
                }
            }
            ToolKind::Text => {
                caption(ui, "Height");
                let unit = units.length.mm_per_unit();
                let mut shown = if self.editor.text_height > 0.0 {
                    self.editor.text_height
                } else {
                    10.0
                } / unit;
                if ui
                    .add(
                        egui::DragValue::new(&mut shown)
                            .speed(0.1)
                            .range(0.001..=1e6)
                            .max_decimals(3)
                            .suffix(format!(" {}", units.length.symbol())),
                    )
                    .on_hover_text("Cap height of new text")
                    .changed()
                {
                    self.editor.text_height = shown * unit;
                }
            }
            ToolKind::Fill => self.fill_tool_bar(ui),
            ToolKind::Wall => self.wall_tool_bar(ui),
            ToolKind::Opening => self.opening_tool_bar(ui),
            ToolKind::Pen => {
                ui.checkbox(&mut self.show_combs, "Curvature combs");
            }
            ToolKind::Dimension | ToolKind::Ordinate | ToolKind::Note | ToolKind::Callout => {
                if kind == ToolKind::Dimension {
                    // How horizontal and vertical dimensions go on.
                    if let Some(m) = choice(
                        ui,
                        self.editor.dim_mode,
                        &crate::editor::DimMode::ALL.map(|m| (m, m.label())),
                    ) {
                        self.editor.dim_mode = m;
                    }
                    ui.add_enabled(
                        self.editor.dim_mode == crate::editor::DimMode::Chain,
                        egui::Checkbox::new(&mut self.editor.dim_overall, "Overall"),
                    )
                    .on_hover_text(
                        "When a chain ends, add the overall dimension outside it (as a \
                         reference: the chain already holds it)",
                    );
                    divider(ui);
                }
                // Dimensioning goes better with the dimensions and relations in view.
                if matches!(kind, ToolKind::Dimension | ToolKind::Ordinate) {
                    ui.checkbox(&mut self.show_dimensions, "Show dimensions")
                        .on_hover_text("Alt+D");
                    ui.checkbox(&mut self.show_relations, "Show relations")
                        .on_hover_text("Alt+R");
                }
            }
            // Tools without options: the bar names the tool, the status bar says what to do.
            _ => {}
        }
        // Tools that draw closed shapes can fill them as they go.
        if matches!(
            kind,
            ToolKind::Rectangle
                | ToolKind::Polygon
                | ToolKind::Circle
                | ToolKind::Ellipse
                | ToolKind::Slot
                | ToolKind::Pen
        ) {
            divider(ui);
            self.new_shape_fill_bar(ui);
        }
    }

    /// Set what the select tool picks; what is picked now keeps only what the filter allows.
    pub(crate) fn set_pick_filter(&mut self, filter: crate::editor::PickFilter) {
        let ed = &mut self.editor;
        ed.pick_filter = filter;
        let doc = &ed.doc;
        ed.selection.retain(|id| filter.allows(doc, *id));
        if !filter.fills() {
            ed.selected_fills.clear();
        }
        if !filter.blocks() {
            ed.selected_instances.clear();
        }
        if !filter.areas() {
            ed.selected_region = None;
        }
        if !filter.midpoints() {
            ed.selected_midpoints.clear();
        }
        if !filter.texts() {
            ed.selected_texts.clear();
        }
        if !filter.annotations() {
            ed.selected_annotation = None;
            ed.annotation_picks.clear();
            self.selected_leader = None;
            // A picked relation stays with the geometry filters.
            if !filter.geometry()
                || self
                    .selected_constraint
                    .and_then(|id| self.editor.doc.constraint(id))
                    .is_some_and(|c| c.is_dimension())
            {
                self.selected_constraint = None;
            }
        } else if !filter.geometry() {
            // Annotations only: a picked relation goes, a picked dimension stays.
            if self
                .selected_constraint
                .and_then(|id| self.editor.doc.constraint(id))
                .is_some_and(|c| !c.is_dimension())
            {
                self.selected_constraint = None;
            }
        }
    }

    /// Quick actions for the selection (the select tool's options).
    fn selection_actions(&mut self, ui: &mut egui::Ui) {
        // What a click or a box picks: always offered with nothing picked; with something
        // picked (the bar is full), only while a filter is on.
        let picked = !self.editor.selection.is_empty()
            || !self.editor.selected_instances.is_empty()
            || !self.editor.selected_fills.is_empty();
        if !picked || !self.editor.pick_filter.is_all() {
            if let Some(f) = pick_filter_toggles(ui, self.editor.pick_filter) {
                self.set_pick_filter(f);
            }
            divider(ui);
        }
        let ed = &self.editor;
        let n = ed.selection.len() + ed.selected_instances.len();
        if n == 0 && !ed.selected_fills.is_empty() {
            caption(ui, "Fill");
            divider(ui);
            self.selection_look(ui);
            return;
        }
        if n == 0
            && let Some(id) = self.picked_view.filter(|id| ed.doc.view(*id).is_some())
        {
            self.view_actions(ui, id);
            return;
        }
        if n == 0 {
            return;
        }
        let grouped = ed.selection.iter().any(|id| ed.doc.group_of(*id).is_some());
        let curves = !ed.selection.is_empty();
        // Construction shows as on when everything picked is construction.
        let construction = ed.selection.iter().all(|id| {
            ed.doc
                .get(*id)
                .is_none_or(|e| e.kind.is_point() || e.style == drift_core::LineKind::Construction)
        });
        // (Text and annotations picked with the geometry count too.)
        let all = n + ed.selected_texts.len() + self.picked_annotations().len();
        caption(ui, &format!("{all} selected"));
        divider(ui);
        self.selection_look(ui);
        divider(ui);
        // Construction is a property of the curves; the things to do with them are in a menu
        // (and on the right-click menu and their keys), so the bar stays about what is picked.
        if curves
            && ui
                .add(
                    egui::Button::selectable(
                        construction,
                        egui::RichText::new("Construction").size(12.5),
                    )
                    .min_size(egui::vec2(0.0, 24.0)),
                )
                .on_hover_text(
                    "Construction geometry: guides, off sheets unless a view shows them (Alt+C)",
                )
                .clicked()
        {
            self.editor.toggle_construction();
        }
        ui.menu_button(egui::RichText::new("Actions").size(12.5), |ui| {
            ui.set_min_width(190.0);
            self.command_items(
                ui,
                &[
                    Some(Command::Duplicate),
                    Some(Command::Move),
                    Some(Command::Rotate),
                ],
            );
            if curves {
                self.command_items(
                    ui,
                    &[
                        Some(Command::Scale),
                        Some(Command::Tool(ToolKind::Mirror)),
                        None,
                    ],
                );
            }
            if curves && self.editor.selection.len() >= 2 {
                self.command_item(ui, Command::Group);
            }
            if grouped {
                self.command_item(ui, Command::Ungroup);
            }
            if curves {
                self.command_item(ui, Command::MakeBlock);
            }
            if self.editor.convertible_group().is_some() {
                self.command_item(ui, Command::BlockFromGroup);
            }
        })
        .response
        .on_hover_text("Duplicate, move, rotate, scale, mirror, group, make a block");
        divider(ui);
        if icons::button(ui, 24.0, false, "Delete", |p, r, c| {
            icons::glyph(p, r, Glyph::Delete, c)
        })
        .on_hover_text("Delete the selection (Del)")
        .clicked()
        {
            self.editor.delete_selection();
        }
    }

    /// Duplicate the selection in place and grab the copy (Shift+D).
    pub(crate) fn duplicate_and_grab(&mut self) {
        if self.tool.kind() != ToolKind::Select {
            self.set_tool(ToolKind::Select);
        }
        if self.editor.duplicate_selection() > 0 {
            self.tool
                .key(ToolKey::G, self.pointer.as_ref(), &mut self.editor);
        }
    }

    /// The canvas's right-click menu (with the select tool): actions for the selection, or
    /// for the view when nothing is picked.
    pub(crate) fn canvas_menu(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        ui.set_min_width(210.0);
        let n = self.editor.selection.len() + self.editor.selected_instances.len();
        // Picked dimensions and notes: where they show.
        let refs = self.picked_annotations();
        if n == 0 && !refs.is_empty() {
            let doc = &self.editor.doc;
            let all = |f: fn(drift_core::Shown) -> bool| refs.iter().all(|r| f(doc.shown(*r)));
            let (canvas, sheets) = (all(|s| s.canvas), all(|s| s.sheets));
            let what = if refs.len() == 1 {
                "1 picked".to_owned()
            } else {
                format!("{} picked", refs.len())
            };
            ui.label(egui::RichText::new(what).small().weak());
            let canvas_label = if canvas {
                "Hide on canvas"
            } else {
                "Show on canvas"
            };
            if ui.button(canvas_label).clicked() {
                self.set_annotations_shown(&refs, Some(!canvas), None, now);
                ui.close();
            }
            let sheets_label = if sheets {
                "Leave off sheets"
            } else {
                "Show on sheets"
            };
            if ui
                .button(sheets_label)
                .on_hover_text("Sheets, PDF, SVG, DXF and print")
                .clicked()
            {
                self.set_annotations_shown(&refs, None, Some(!sheets), now);
                ui.close();
            }
            // A section or elevation mark: a view of what it looks at.
            if let [drift_core::AnnotationRef::Annotation(id)] = refs[..]
                && self.editor.doc.annotation(id).is_some_and(|a| {
                    matches!(
                        a,
                        drift_core::Annotation::Section { .. }
                            | drift_core::Annotation::Elevation { .. }
                    )
                })
            {
                ui.separator();
                if ui
                    .button("Make view")
                    .on_hover_text(
                        "A model view of what the mark looks at, linked to the mark: its flag \
                         reads the view's number and sheet once it is placed",
                    )
                    .clicked()
                {
                    self.make_view_from_mark(id);
                    ui.close();
                }
            }
            let (align, space) = self.can_align_dimensions();
            if align || space {
                ui.separator();
                self.command_items(
                    ui,
                    &[
                        Some(Command::AlignDimensions),
                        Some(Command::SpaceDimensions),
                        None,
                    ],
                );
            }
            self.command_items(ui, &[Some(Command::Toggle(ViewFlag::Hidden)), None]);
        }
        if n == 0 {
            if self.clipboard.is_some() {
                self.command_item(ui, Command::Paste);
            }
            self.command_items(
                ui,
                &[
                    Some(Command::SelectAll),
                    Some(Command::ZoomToFit),
                    None,
                    Some(Command::Toggle(ViewFlag::Grid)),
                    Some(Command::Toggle(ViewFlag::Snap)),
                    Some(Command::Toggle(ViewFlag::PerpTangent)),
                    None,
                    Some(Command::Palette),
                ],
            );
            return;
        }
        ui.label(egui::RichText::new(format!("{n} selected")).small().weak());
        if self.command_item(ui, Command::Cut) {
            return;
        }
        self.command_item(ui, Command::Copy);
        if self.clipboard.is_some() && self.command_item(ui, Command::Paste) {
            return;
        }
        self.command_items(
            ui,
            &[
                Some(Command::Duplicate),
                Some(Command::Move),
                Some(Command::Rotate),
            ],
        );
        let curves = !self.editor.selection.is_empty();
        if curves {
            self.command_items(
                ui,
                &[Some(Command::Scale), Some(Command::Tool(ToolKind::Mirror))],
            );
        }
        if self.editor.can_arrange() {
            self.arrange_menu(ui);
        }
        let (on_relations, on_dims) = self.editor.selection_constraints();
        if !on_relations.is_empty() {
            self.command_item(ui, Command::ClearRelations);
        }
        if !on_dims.is_empty() {
            self.command_item(ui, Command::ClearDimensions);
        }
        // Relations and dimensions that fit the selection.
        let relations = self.editor.relation_options();
        let dimensions = self.editor.dimension_options();
        if !relations.is_empty() || !dimensions.is_empty() {
            ui.separator();
        }
        for (title, options, is_dimension) in [
            ("Add relation", relations, false),
            ("Add dimension", dimensions, true),
        ] {
            if options.is_empty() {
                continue;
            }
            ui.menu_button(title, |ui| {
                for option in &options {
                    if ui.button(option.label).clicked() {
                        if let Some(id) = self.apply_constraint_option(option, is_dimension, now) {
                            self.open_dimension_edit(id);
                        }
                        ui.close();
                    }
                }
            });
        }
        if curves {
            ui.separator();
            let grouped = self
                .editor
                .selection
                .iter()
                .any(|id| self.editor.doc.group_of(*id).is_some());
            if self.editor.selection.len() >= 2 {
                self.command_item(ui, Command::Group);
            }
            if grouped {
                self.command_item(ui, Command::Ungroup);
            }
            let cut = (self.editor.selection.len() == 1)
                .then(|| self.editor.selection.iter().next().copied())
                .flatten()
                .and_then(|id| {
                    if self.editor.fillet_at(id).is_some() {
                        Some((id, "Remove fillet"))
                    } else if self.editor.chamfer_at(id).is_some() {
                        Some((id, "Remove chamfer"))
                    } else {
                        None
                    }
                });
            if let Some((id, label)) = cut
                && ui.button(label).clicked()
            {
                ui.close();
                if let Err(e) = self.editor.remove_corner_cut(id) {
                    self.set_status(format!("{label}: {e}"), true, now);
                }
            }
            self.command_item(ui, Command::Construction);
            if ui
                .add(egui::Button::new("Select chain").shortcut_text("Tab"))
                .clicked()
            {
                self.editor.select_chain();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Relations…").shortcut_text("Q"))
                .clicked()
            {
                ui.close();
                self.open_pie(ui.ctx());
            }
            let layers: Vec<(drift_core::LayerId, String)> = self
                .editor
                .doc
                .layers()
                .map(|(id, l)| (id, l.name.clone()))
                .collect();
            ui.menu_button("Move to layer", |ui| {
                for (id, name) in layers {
                    if ui.button(name).clicked() {
                        self.editor.move_selection_to_layer(id);
                        ui.close();
                    }
                }
            });
            self.command_item(ui, Command::MakeBlock);
            if self.editor.convertible_group().is_some() {
                self.command_item(ui, Command::BlockFromGroup);
            }
        }
        if !self.editor.selected_instances.is_empty() {
            self.command_items(ui, &[Some(Command::Explode), Some(Command::ExplodeToGroup)]);
        }
        ui.separator();
        // Delete in red (its label and keys from the registry).
        if let Some(spec) = crate::commands::spec(Command::Delete) {
            let red = ui.visuals().error_fg_color;
            let keys = spec.key_text(cfg!(target_os = "macos"));
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(spec.menu).color(red))
                        .shortcut_text(keys),
                )
                .on_hover_text(spec.description)
                .clicked()
            {
                ui.close();
                let ctx = ui.ctx().clone();
                self.run_command(&ctx, Command::Delete);
            }
        }
    }
}
