//! Menu bar, toolbar, inspector (properties) and status bar.

use crate::app::palette::{Command, ViewFlag};
use crate::app::view::ShowRelations;
use crate::commands::{self, Category};
use crate::icons::{self, Glyph};
use crate::*;

/// What the Inspector shows for the picks that depends only on what is picked and on the
/// drawing's make-up apart from its fills ([`Document::make_up`](drift_core::Document::make_up):
/// entities added, removed, rewired or restyled, relations, dimensions, variables, layers,
/// walls), not on where anything is: the relations offered, the picked curves' kinds and
/// layers, and the relations and dimensions on them. With tens of thousands of curves
/// picked, finding them again every frame took most of a frame (#253). None of them reads a
/// fill, so a drag carrying a hatch along (which rewrites it every frame) keeps them.
#[derive(Default)]
pub(crate) struct PickMemo {
    key: Option<PickKey>,
    relations: Option<Vec<&'static str>>,
    constraints: Option<Vec<drift_core::ConstraintId>>,
    summary: Option<String>,
    curve_layers: Option<CurveLayers>,
    /// How many times it was emptied (tests).
    made: usize,
}

/// What a [`PickMemo`] was made for.
struct PickKey {
    lineage: u64,
    make_up: u64,
    selection: std::collections::BTreeSet<drift_core::EntityId>,
    midpoints: std::collections::BTreeSet<drift_core::EntityId>,
}

/// The layer of the first picked curve (not a wall's), and whether the picked curves are on
/// more than one layer.
type CurveLayers = Option<(drift_core::LayerId, bool)>;

impl PickMemo {
    /// Itself for `ed`'s picks and make-up: emptied if either changed. (Compared where they
    /// are: copying tens of thousands of picks every frame would cost what it saves.)
    fn for_picks(&mut self, ed: &Editor) -> &mut Self {
        let current = self.key.as_ref().is_some_and(|k| {
            k.lineage == ed.doc.lineage()
                && k.make_up == ed.doc.make_up()
                && k.selection == ed.selection
                && k.midpoints == ed.selected_midpoints
        });
        if !current {
            *self = Self {
                made: self.made + 1,
                key: Some(PickKey {
                    lineage: ed.doc.lineage(),
                    make_up: ed.doc.make_up(),
                    selection: ed.selection.clone(),
                    midpoints: ed.selected_midpoints.clone(),
                }),
                ..Default::default()
            };
        }
        self
    }

    /// The relations offered for the picks, by name (see [`Editor::relation_options`]; what
    /// one adds is found when it is clicked).
    pub(crate) fn relations(&mut self, ed: &Editor) -> Vec<&'static str> {
        let labels = self
            .for_picks(ed)
            .relations
            .get_or_insert_with(|| relation_labels(ed))
            .clone();
        debug_assert_eq!(labels, relation_labels(ed), "the relations offered, kept");
        labels
    }

    /// The relations and dimensions on the picks (all of them with nothing picked), in the
    /// drawing's order.
    pub(crate) fn constraints(&mut self, ed: &Editor) -> Vec<drift_core::ConstraintId> {
        let ids = self
            .for_picks(ed)
            .constraints
            .get_or_insert_with(|| picked_constraints(ed))
            .clone();
        debug_assert_eq!(ids, picked_constraints(ed), "the picks' relations, kept");
        ids
    }

    /// How many of each kind are picked, as the title's line says it.
    pub(crate) fn summary(&mut self, ed: &Editor) -> String {
        let text = self
            .for_picks(ed)
            .summary
            .get_or_insert_with(|| picked_summary(ed))
            .clone();
        debug_assert_eq!(text, picked_summary(ed), "the picks' summary, kept");
        text
    }

    /// See [`CurveLayers`].
    pub(crate) fn curve_layers(&mut self, ed: &Editor) -> CurveLayers {
        let layers = *self
            .for_picks(ed)
            .curve_layers
            .get_or_insert_with(|| picked_curve_layers(ed));
        debug_assert_eq!(layers, picked_curve_layers(ed), "the picks' layers, kept");
        layers
    }
}

pub(crate) fn relation_labels(ed: &Editor) -> Vec<&'static str> {
    ed.relation_options().iter().map(|o| o.label).collect()
}

pub(crate) fn picked_constraints(ed: &Editor) -> Vec<drift_core::ConstraintId> {
    if ed.selection.is_empty() && ed.selected_midpoints.is_empty() {
        ed.doc.constraints().map(|(id, _)| id).collect()
    } else {
        // Constraints on the selection's points too: a line's length is a distance between
        // its end points.
        let mut relevant = ed.doc.defining_points(ed.selection.iter().copied());
        relevant.extend(ed.selection.iter().copied());
        relevant.extend(ed.selected_midpoints.iter().copied());
        ed.doc
            .constraints()
            .filter(|(_, c)| c.entities().any(|e| relevant.contains(&e)))
            .map(|(id, _)| id)
            .collect()
    }
}

pub(crate) fn picked_summary(ed: &Editor) -> String {
    let mut counts = std::collections::BTreeMap::new();
    for id in &ed.selection {
        if let Some(e) = ed.doc.get(*id) {
            *counts.entry(e.kind.type_name()).or_insert(0) += 1;
        }
    }
    let summary: Vec<String> = counts.iter().map(|(name, n)| plural(*n, name)).collect();
    summary.join(" · ")
}

pub(crate) fn picked_curve_layers(ed: &Editor) -> CurveLayers {
    let walls = ed.selected_walls();
    let wall_curves: std::collections::BTreeSet<drift_core::EntityId> =
        walls.iter().flat_map(|w| ed.wall_curves(*w)).collect();
    let mut layers = ed
        .selection
        .iter()
        .filter(|id| !wall_curves.contains(*id))
        .filter_map(|id| ed.doc.get(*id))
        .filter(|e| !e.kind.is_point())
        .map(|e| e.layer);
    let first = layers.next()?;
    Some((first, layers.any(|l| l != first)))
}

/// A shortcut as the platform writes it: ⌘ / ⇧ on macOS, Ctrl / Shift elsewhere.
pub(crate) fn shortcut(key: &str, shift: bool) -> String {
    commands::cmd_text(key, shift, cfg!(target_os = "macos"))
}

/// An inspector's title: the thing's icon on a soft tile, its name, and a line of detail.
pub(crate) fn title(ui: &mut egui::Ui, icon: Option<ToolKind>, name: &str, detail: &str) {
    let glyph = |painter: &egui::Painter, rect: egui::Rect, color: egui::Color32| match icon {
        Some(kind) => icons::tool(painter, rect, kind, color),
        None => icons::glyph(painter, rect, Glyph::Inspector, color),
    };
    title_card(ui, &glyph, name, detail, None);
}

/// [`title`] with the object's own glyph and, at the right, a "…" button whose menu holds
/// the object's actions (`menu`), so the panel below is all settings. `tip` names the menu.
pub(crate) fn title_with_actions(
    ui: &mut egui::Ui,
    glyph: Glyph,
    name: &str,
    detail: &str,
    tip: &str,
    menu: &mut dyn FnMut(&mut egui::Ui),
) {
    let paint = |painter: &egui::Painter, rect: egui::Rect, color: egui::Color32| {
        icons::glyph(painter, rect, glyph, color);
    };
    title_card(ui, &paint, name, detail, Some((tip, menu)));
}

type GlyphPaint<'a> = &'a dyn Fn(&egui::Painter, egui::Rect, egui::Color32);

/// A title's actions menu: the button's tip, and what draws the menu.
type TitleMenu<'a> = (&'a str, &'a mut dyn FnMut(&mut egui::Ui));

fn title_card(
    ui: &mut egui::Ui,
    glyph: GlyphPaint<'_>,
    name: &str,
    detail: &str,
    menu: Option<TitleMenu<'_>>,
) {
    const MORE: f32 = 24.0;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let accent = theme::accent(ui.visuals().dark_mode);
        let tile = theme::icon_tile(ui, 28.0);
        glyph(ui.painter(), tile, accent);
        // (Room at the right for the actions button, so a long name truncates before it.)
        let room = if menu.is_some() { MORE + 6.0 } else { 0.0 };
        let text_width = (ui.available_width() - room).max(40.0);
        ui.allocate_ui(egui::vec2(text_width, 34.0), |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                // (One line alone sits in the middle of the tile.)
                if detail.is_empty() {
                    ui.add_space(4.0);
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(name)
                            .family(theme::bold())
                            .size(theme::size::TITLE)
                            .color(ui.visuals().strong_text_color()),
                    )
                    .truncate(),
                );
                if !detail.is_empty() {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(detail)
                                .size(theme::size::CAPTION)
                                .color(ui.visuals().weak_text_color()),
                        )
                        .truncate(),
                    );
                }
            });
        });
        if let Some((tip, menu)) = menu {
            let right = ui.max_rect().right();
            let rect = egui::Rect::from_center_size(
                egui::pos2(right - MORE / 2.0, tile.center().y),
                egui::Vec2::splat(MORE),
            );
            let more = crate::app::dock_ui::icon_at(
                ui,
                rect,
                ui.id().with("title_actions"),
                Glyph::More,
                tip,
            );
            egui::Popup::menu(&more).show(|ui| menu(ui));
        }
    });
    ui.add_space(4.0);
}

/// "1 line", "4 lines": a count with the kind's name, lower case and plural past one.
fn plural(n: usize, name: &str) -> String {
    let name = name.to_lowercase();
    if n == 1 {
        format!("1 {name}")
    } else if name.ends_with('s') {
        format!("{n} {name}es")
    } else {
        format!("{n} {name}s")
    }
}

/// The tool that draws an entity kind (for its icon).
fn kind_icon(kind: &drift_core::EntityKind) -> ToolKind {
    use drift_core::EntityKind as K;
    match kind {
        K::Point(_) => ToolKind::Point,
        K::Line { .. } => ToolKind::Line,
        K::Circle { .. } => ToolKind::Circle,
        K::Arc { .. } => ToolKind::Arc,
        K::Ellipse { .. } => ToolKind::Ellipse,
        K::EllipseArc { .. } => ToolKind::EllipseArc,
        K::Bezier { .. } => ToolKind::Pen,
    }
}

/// A value typed into the inspector.
enum SizeEdit {
    /// Set (or add) this dimension to the typed value.
    Dimension(Constraint, String),
    /// Move a point: its id, the axis (0 = X, 1 = Y) and the typed coordinate.
    Point(drift_core::EntityId, usize, String),
    /// Turn a line about its start to the typed angle.
    Angle(drift_core::EntityId, String),
}

/// A value field in the inspector: shows `shown`, and returns what was typed once the field
/// is left (Return, Tab or a click elsewhere) with something different in it. Esc puts the
/// old value back. The text being typed lives in egui's memory.
pub(crate) fn value_field(
    ui: &mut egui::Ui,
    key: egui::Id,
    shown: &str,
    width: f32,
    tip: &str,
) -> Option<String> {
    let mut text = ui
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| shown.to_owned());
    let response = theme::field(
        ui,
        egui::TextEdit::singleline(&mut text)
            .id(key.with("field"))
            .desired_width(width)
            .min_size(egui::vec2(0.0, theme::ROW_HEIGHT))
            .vertical_align(egui::Align::Center),
    )
    .on_hover_text(tip);
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text.clone()));
    }
    if response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(key));
        let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let typed = text.trim();
        if !cancelled && !typed.is_empty() && typed != shown {
            return Some(typed.to_owned());
        }
    }
    None
}

/// The values of a picked entity that can be typed. Sizes (a line's length, a circle's or
/// arc's radius and diameter) set the dimension that drives them, or add one. Positions (a
/// point, a line's ends, a centre) and a line's angle move the geometry, as far as its
/// relations allow.
fn size_rows(ui: &mut egui::Ui, ed: &Editor, id: drift_core::EntityId) -> Option<SizeEdit> {
    use drift_core::EntityKind as K;
    const UNITS: &str = "units and variables work, e.g. 2in or =width/2";
    let entity = ed.doc.get(id)?;
    let units = ed.doc.units();
    let out = std::cell::RefCell::new(None);
    let dimension = |ui: &mut egui::Ui, label: &str, wanted: Constraint, measured: f64| {
        let existing = ed.size_dimension(&wanted);
        let driven = existing.is_some_and(|c| ed.doc.is_driven(c));
        // A linked dimension shows its expression; otherwise the value as it would be typed.
        let same_kind = existing
            .and_then(|c| ed.doc.constraint(c))
            .is_some_and(|c| std::mem::discriminant(c) == std::mem::discriminant(&wanted));
        let shown = existing
            .filter(|_| same_kind)
            .and_then(|c| ed.dimension_edit_text(c))
            .unwrap_or_else(|| units.format_length(measured));
        theme::row(ui, label, |ui| {
            if driven {
                ui.label(format!("({shown})"))
                    .on_hover_text("A reference dimension: it follows the geometry");
                return;
            }
            let tip = if existing.is_some() {
                format!("Set by a dimension: type a new value ({UNITS})")
            } else {
                format!("Type a value to dimension it ({UNITS})")
            };
            let key = egui::Id::new(("size", id.0, label));
            let at = ui.cursor().left_top();
            let width = ui.available_width();
            if let Some(text) = value_field(ui, key, &shown, width, &tip) {
                *out.borrow_mut() = Some(SizeEdit::Dimension(wanted, text));
            }
            if existing.is_some() {
                // A small dimension glyph at the field's end: the value comes from a
                // dimension.
                let r = egui::Rect::from_center_size(
                    egui::pos2(at.x + width - 13.0, at.y + theme::ROW_HEIGHT / 2.0),
                    egui::Vec2::splat(14.0),
                );
                let color = theme::neutrals(ui.visuals().dark_mode).tertiary;
                icons::tool(ui.painter(), r, ToolKind::Dimension, color);
            }
        });
    };
    // A point's coordinates, as two fields on one row.
    let position = |ui: &mut egui::Ui, label: &str, point: drift_core::EntityId| {
        let Ok(p) = ed.doc.point(point) else { return };
        theme::row(ui, label, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // The two share the width.
            let width = ((ui.available_width() - 4.0) / 2.0).floor();
            for (axis, name, v) in [(0, "X", p.x), (1, "Y", p.y)] {
                let key = egui::Id::new(("coordinate", point.0, axis));
                let tip = format!("{name}: type a coordinate to move it ({UNITS})");
                if let Some(text) = value_field(ui, key, &units.format_length(v), width, &tip) {
                    *out.borrow_mut() = Some(SizeEdit::Point(point, axis, text));
                }
            }
        });
    };
    match entity.kind {
        K::Line { a, b } => {
            let (pa, pb) = (ed.doc.point(a).ok()?, ed.doc.point(b).ok()?);
            let length = pa.distance(pb);
            let wanted = Constraint::Distance {
                a,
                b,
                value: length,
            };
            dimension(ui, "Length", wanted, length);
            let d = pb - pa;
            let shown = units.format_angle(d.y.atan2(d.x).to_degrees());
            theme::row(ui, "Angle", |ui| {
                let key = egui::Id::new(("angle", id.0));
                let tip =
                    "Type an angle to turn the line about its start (counterclockwise from +X)";
                let width = ui.available_width();
                if let Some(text) = value_field(ui, key, &shown, width, tip) {
                    *out.borrow_mut() = Some(SizeEdit::Angle(id, text));
                }
            });
            position(ui, "Start", a);
            position(ui, "End", b);
        }
        K::Circle { center, .. } | K::Arc { center, .. } => {
            let radius = drift_solver::radius_of(&ed.doc, id)?;
            let wanted = Constraint::Radius {
                curve: id,
                value: radius,
            };
            dimension(ui, "Radius", wanted, radius);
            if matches!(entity.kind, K::Circle { .. }) {
                let wanted = Constraint::Diameter {
                    curve: id,
                    value: 2.0 * radius,
                };
                dimension(ui, "Diameter", wanted, 2.0 * radius);
            }
            position(ui, "Center", center);
        }
        K::Point(_) => position(ui, "Position", id),
        K::Ellipse { center, .. } | K::EllipseArc { center, .. } => position(ui, "Center", center),
        K::Bezier { a, b, .. } => {
            position(ui, "Start", a);
            position(ui, "End", b);
        }
    }
    out.into_inner()
}

/// The window width from which the top bar has a search field; under it, a search button
/// (so the sheet tabs keep some room).
const SEARCH_FIELD_FROM: f32 = 960.0;

/// The common divisions of a fixed grid's major square.
const GRID_DIVISION_PRESETS: [u32; 7] = [2, 4, 5, 8, 10, 12, 16];

/// The menus' names, in their order.
const MENUS: [&str; 8] = [
    "File", "Edit", "View", "Draw", "Build", "Modify", "Annotate", "Help",
];

/// Modify's Join submenu: the commands that make points one.
const JOIN_MENU: [Command; 3] = [
    Command::JoinCorners,
    Command::Weld(drift_edit::weld::WeldAt::Active),
    Command::Weld(drift_edit::weld::WeldAt::Centre),
];

impl DriftApp {
    /// A menu item for a command of the registry: its label, keys and description; greyed
    /// out (saying why) when it can't run; a check mark for a setting. Returns true if it
    /// ran.
    pub(crate) fn command_item(&mut self, ui: &mut egui::Ui, command: Command) -> bool {
        self.command_item_in(ui, command, false)
    }

    /// [`Self::command_item`] in a menu of switches: a plain command is set in by a check
    /// mark's room too, so every row starts at one edge.
    fn checked_menu_item(&mut self, ui: &mut egui::Ui, command: Command) -> bool {
        self.command_item_in(ui, command, true)
    }

    fn command_item_in(&mut self, ui: &mut egui::Ui, command: Command, room: bool) -> bool {
        let Some(spec) = commands::spec(command) else {
            return false;
        };
        if spec.when.hidden() {
            return false;
        }
        let label = self.menu_label(spec);
        // A menu's shortcut runs that very item, so a cycle's later tools (Ellipse after
        // Circle's C) show none: the key is on the cycle's first tool, and the tooltip says
        // what pressing it again does.
        let later_in_cycle = matches!(command, Command::Tool(kind) if kind.cycle_head() != kind && spec.shortcuts().is_empty());
        let keys = if later_in_cycle {
            String::new()
        } else {
            spec.key_text(cfg!(target_os = "macos"))
        };
        let enabled = spec.when.holds(self);
        let response = match self.command_state(command) {
            Some(on) => {
                ui.add_enabled_ui(enabled, |ui| theme::menu_check(ui, on, &label, &keys))
                    .inner
            }
            None if room => {
                let check = egui::Atom::custom(egui::Id::new("menu_check"), egui::vec2(14.0, 14.0));
                let button = egui::Button::new((check, label));
                let button = if keys.is_empty() {
                    button
                } else {
                    button.shortcut_text(keys)
                };
                ui.add_enabled(enabled, button)
            }
            // (Shortcut text, even empty, becomes part of the button's accessible label.)
            None if keys.is_empty() => ui.add_enabled(enabled, egui::Button::new(label)),
            None => ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)),
        };
        let response = response
            .on_hover_text(spec.does())
            .on_disabled_hover_text(spec.when.reason());
        if !response.clicked() {
            return false;
        }
        ui.close();
        let ctx = ui.ctx().clone();
        self.run_command(&ctx, command);
        true
    }

    /// View → Relations: which relations show their symbols (Alt+R steps through them).
    /// View → Grid: Auto or a fixed spacing, and the divisions of a major square.
    fn grid_menu(&mut self, ui: &mut egui::Ui) {
        let room = egui::Atom::custom(egui::Id::new("menu_check"), egui::vec2(14.0, 14.0));
        ui.menu_button((room, "Grid spacing"), |ui| {
            if theme::menu_check(ui, !self.grid_fixed, "Auto", "").clicked() {
                self.grid_fixed = false;
            }
            if theme::menu_check(ui, self.grid_fixed, "Fixed spacing", "").clicked() {
                self.grid_fixed = true;
            }
            ui.separator();
            ui.add_enabled_ui(self.grid_fixed, |ui| {
                let id = egui::Id::new("grid_spacing_text");
                let mut text = ui
                    .data(|d| d.get_temp::<String>(id))
                    .unwrap_or_else(|| self.grid_spacing_text.clone());
                let mut bad = false;
                ui.horizontal(|ui| {
                    ui.label("Spacing");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .desired_width(80.0)
                            .hint_text("600, 1', 3.6 m"),
                    );
                    if r.lost_focus() {
                        bad = !self.set_grid_spacing(&text);
                        text.clone_from(&self.grid_spacing_text);
                    }
                    r.on_hover_text("The major square: a length with units (a bare number is in the drawing's unit)");
                });
                if bad {
                    ui.label(egui::RichText::new("Not a length").weak());
                }
                ui.data_mut(|d| d.insert_temp(id, text));
                ui.horizontal(|ui| {
                    ui.label("Divisions");
                    let mut n = self.grid_divisions;
                    ui.add(egui::DragValue::new(&mut n).range(1..=1000));
                    self.grid_divisions = n.max(1);
                });
                ui.horizontal_wrapped(|ui| {
                    for n in GRID_DIVISION_PRESETS {
                        if ui
                            .selectable_label(self.grid_divisions == n, n.to_string())
                            .clicked()
                        {
                            self.grid_divisions = n;
                        }
                    }
                });
            });
        });
    }

    fn relations_menu(&mut self, ui: &mut egui::Ui) {
        let Some(spec) = commands::spec(Command::Relations) else {
            return;
        };
        let keys = spec.key_text(cfg!(target_os = "macos"));
        // (Set in from the edge like the switches round it, which have a check mark's room.)
        let room = egui::Atom::custom(egui::Id::new("menu_check"), egui::vec2(14.0, 14.0));
        ui.menu_button((room, "Relations"), |ui| {
            for mode in ShowRelations::ALL {
                let on = self.show_relations == mode;
                if theme::menu_check(ui, on, mode.label(), "").clicked() {
                    self.show_relations = mode;
                    ui.close();
                }
            }
            if !keys.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(format!("{keys} steps through them")).weak());
            }
        })
        .response
        .on_hover_text(spec.description);
    }

    /// Menu items for commands, in order; `None` is a separator.
    pub(crate) fn command_items(&mut self, ui: &mut egui::Ui, items: &[Option<Command>]) {
        for item in items {
            match item {
                Some(command) => {
                    self.command_item(ui, *command);
                }
                None => {
                    ui.separator();
                }
            }
        }
    }

    /// A category's commands as a menu, in the registry's order (the rail's), with
    /// separators round the rail's families.
    pub(crate) fn category_menu(&mut self, ui: &mut egui::Ui, category: Category) {
        let breaks = commands::menu_breaks(category);
        for spec in commands::registry()
            .iter()
            .filter(|s| s.category == category)
        {
            if breaks.contains(&spec.command) {
                ui.separator();
            }
            // Joining points is one submenu in Modify, so the menu fits a laptop's height.
            if JOIN_MENU.contains(&spec.command) {
                if spec.command == JOIN_MENU[0] {
                    ui.menu_button("Join", |ui| {
                        for c in JOIN_MENU {
                            self.command_item(ui, c);
                        }
                    });
                }
                continue;
            }
            self.command_item(ui, spec.command);
        }
    }

    /// The top bar, one row: Drift's mark, the menus and undo / redo at the left; the model
    /// and the sheets as tabs in the middle; the palette's search field and the panels'
    /// switch at the right. In a narrow window the tabs take what is left between the two
    /// ends, the search field becomes a button, and the mark and undo / redo (which the Edit
    /// menu has too) give their room to the menus.
    pub(crate) fn top_bar(&mut self, ui: &mut egui::Ui) {
        let bar = ui.max_rect();
        let roomy = bar.width() >= 640.0;
        let pad = if roomy { 7.0 } else { 4.0 };
        // The menus take the room of their names, so what follows them sits right after.
        let font = egui::TextStyle::Button.resolve(ui.style());
        let menus_width: f32 = MENUS
            .iter()
            .map(|name| {
                let text = ui.painter().layout_no_wrap(
                    (*name).to_owned(),
                    font.clone(),
                    egui::Color32::WHITE,
                );
                text.size().x + 2.0 * pad
            })
            .sum();
        let side = if roomy { 10.0 } else { 4.0 };
        let leading = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(bar.shrink2(egui::vec2(side, 0.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if roomy {
                        let (r, _) =
                            ui.allocate_exact_size(egui::Vec2::splat(20.0), egui::Sense::hover());
                        icons::logo(ui.painter(), r);
                        ui.add_space(6.0);
                    }
                    ui.allocate_ui_with_layout(
                        egui::vec2(menus_width, 24.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            egui::MenuBar::new()
                                .style(move |s: &mut egui::Style| {
                                    egui::containers::menu::menu_style(s);
                                    s.spacing.button_padding = egui::vec2(pad, 3.0);
                                    s.spacing.item_spacing.x = 0.0;
                                })
                                .ui(ui, |ui| self.menus(ui));
                        },
                    );
                    if roomy {
                        ui.add_space(3.0);
                        theme::bar_divider(ui);
                        ui.add_space(3.0);
                        self.history_button(ui, Command::Undo);
                        ui.add_space(2.0);
                        self.history_button(ui, Command::Redo);
                    }
                    ui.min_rect().right()
                },
            )
            .inner;
        let trailing = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_max(
                        egui::pos2(leading.min(bar.right() - side), bar.top()),
                        egui::pos2(bar.right() - side, bar.bottom()),
                    ))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    self.panels_button(ui);
                    if self.search_field(ui, bar.width() >= SEARCH_FIELD_FROM) {
                        self.open_palette();
                    }
                    ui.min_rect().left()
                },
            )
            .inner;
        let gap = if roomy { 14.0 } else { 6.0 };
        let zone = egui::Rect::from_min_max(
            egui::pos2(leading + gap, bar.top()),
            egui::pos2(trailing - gap, bar.bottom()),
        );
        if zone.width() > 30.0 {
            ui.scope_builder(egui::UiBuilder::new().max_rect(zone), |ui| {
                // (Whatever still doesn't fit is cut off, not drawn over the bar's ends.)
                ui.set_clip_rect(zone.intersect(ui.clip_rect()));
                self.sheet_tabs(ui, bar.center().x);
            });
        }
    }

    /// Undo or redo, as a button in the top bar: greyed out when there is nothing to take
    /// back (or to bring back).
    fn history_button(&mut self, ui: &mut egui::Ui, command: Command) {
        let Some(spec) = commands::spec(command) else {
            return;
        };
        // (Not "Undo" and "Redo": those are the Edit menu's.)
        let (label, glyph) = if command == Command::Undo {
            ("Undo last change", Glyph::Undo)
        } else {
            ("Redo last change", Glyph::Redo)
        };
        let enabled = spec.when.holds(self);
        let name = self.menu_label(spec);
        let keys = spec.key_text(cfg!(target_os = "macos"));
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                icons::button(ui, 26.0, false, label, |p, r, c| {
                    icons::glyph(p, r, glyph, c)
                })
            })
            .inner
            .on_hover_ui(|ui| theme::command_tip(ui, &name, &keys, spec.description))
            .on_disabled_hover_ui(|ui| {
                theme::command_tip(ui, spec.title, &keys, spec.when.reason());
            });
        if response.clicked() {
            let ctx = ui.ctx().clone();
            self.run_command(&ctx, command);
        }
    }

    /// The top bar's switch for the panels: hides them all for a clear view, and brings them
    /// back.
    fn panels_button(&mut self, ui: &mut egui::Ui) {
        let Some(spec) = commands::spec(Command::HideAllPanels) else {
            return;
        };
        let shown = !self.dock_view.hide_all;
        let response = icons::button(ui, 26.0, false, "Show or hide panels", |p, r, c| {
            if shown {
                // The side pane filled in while the panels show (on the icon's 20-unit grid).
                let s = r.width().min(r.height()) / 20.0;
                let o = r.center() - egui::Vec2::splat(10.0 * s);
                let pane = egui::Rect::from_min_max(
                    o + egui::vec2(12.0, 4.5) * s,
                    o + egui::vec2(17.0, 15.5) * s,
                );
                p.rect_filled(pane, 1.0, c.gamma_multiply(0.3));
            }
            icons::glyph(p, r, Glyph::Panels, c);
        });
        let keys = spec.key_text(cfg!(target_os = "macos"));
        let name = if shown { spec.title } else { "Show the panels" };
        if response
            .on_hover_ui(|ui| theme::command_tip(ui, name, &keys, spec.description))
            .clicked()
        {
            let ctx = ui.ctx().clone();
            self.run_command(&ctx, Command::HideAllPanels);
        }
    }

    /// What opens the command palette: a field-like button showing its keys, or just a
    /// button where there is no room for a field.
    fn search_field(&mut self, ui: &mut egui::Ui, field: bool) -> bool {
        let keys = shortcut("K", false);
        let tip = |ui: &mut egui::Ui| {
            theme::command_tip(
                ui,
                "Search commands",
                &keys,
                "Find any tool, setting, panel or command",
            );
        };
        if !field {
            return icons::button(ui, 26.0, false, "Search commands", |p, r, c| {
                icons::glyph(p, r, Glyph::Search, c)
            })
            .on_hover_ui(tip)
            .clicked();
        }
        let (r, response) = ui.allocate_exact_size(egui::vec2(200.0, 26.0), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Search commands")
        });
        let n = theme::neutrals(ui.visuals().dark_mode);
        let (fill, edge) = if response.hovered() {
            (n.faint, n.control_border)
        } else {
            (n.field, n.border)
        };
        ui.painter().rect(
            r,
            theme::radius::CONTROL + 1,
            fill,
            egui::Stroke::new(1.0, edge),
            egui::StrokeKind::Inside,
        );
        icons::glyph(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(r.left() + 15.0, r.center().y),
                egui::Vec2::splat(13.0),
            ),
            Glyph::Search,
            n.secondary,
        );
        ui.painter().text(
            egui::pos2(r.left() + 28.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            "Search commands",
            egui::FontId::proportional(theme::size::LABEL),
            n.tertiary,
        );
        theme::keycap_at(ui, r.right_center() - egui::vec2(5.0, 0.0), &keys);
        response.on_hover_ui(tip).clicked()
    }

    /// The eight menus, in the top bar.
    fn menus(&mut self, ui: &mut egui::Ui) {
        use Command as C;
        ui.menu_button("File", |ui| {
            self.command_items(ui, &[Some(C::New), Some(C::Open)]);
            ui.menu_button("Open recent", |ui| {
                if self.recent.is_empty() {
                    ui.label(egui::RichText::new("No recent sketches").weak());
                }
                let mut chosen = None;
                for (i, path) in self.recent.iter().enumerate() {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if ui
                        .button(name)
                        .on_hover_text(path.to_string_lossy())
                        .clicked()
                    {
                        chosen = Some(i);
                    }
                }
                if !self.recent.is_empty() {
                    ui.separator();
                    if ui.button("Clear the list").clicked() {
                        self.recent.clear();
                    }
                }
                if let Some(i) = chosen {
                    self.request(Pending::Recent(i));
                    ui.close();
                }
            });
            self.command_items(
                ui,
                &[
                    Some(C::OpenExample),
                    Some(C::ShowStart),
                    None,
                    Some(C::Save),
                    Some(C::SaveAs),
                    None,
                    Some(C::DrawingSettings),
                    None,
                    Some(C::Import),
                ],
            );
            ui.menu_button("Export", |ui| {
                self.command_items(
                    ui,
                    &[
                        Some(C::ExportPdf),
                        Some(C::ExportSvg),
                        Some(C::ExportDxf),
                        None,
                        Some(C::ExportModelSvg),
                        Some(C::ExportModelPdf),
                    ],
                );
                theme::checkbox(ui, &mut self.model_annotations, "Dimensions and notes")
                    .on_hover_text("Model exports include dimensions and notes");
                ui.separator();
                self.command_item(ui, C::ExportProfiles);
            });
            self.command_item(ui, C::Print);
        });
        ui.menu_button("Edit", |ui| {
            self.command_items(
                ui,
                &[
                    Some(C::Undo),
                    Some(C::Redo),
                    None,
                    Some(C::Cut),
                    Some(C::Copy),
                    Some(C::Paste),
                    Some(C::PasteInPlace),
                    Some(C::Duplicate),
                    None,
                    Some(C::Delete),
                    None,
                    Some(C::SelectAll),
                    None,
                    Some(C::Preferences),
                ],
            );
        });
        ui.menu_button("View", |ui| {
            // Every row set in by a check mark's room, switch or not, so the menu has
            // one left edge.
            self.checked_menu_item(ui, C::Palette);
            let room = egui::Atom::custom(egui::Id::new("menu_check"), egui::vec2(14.0, 14.0));
            ui.menu_button((room, "Panels"), |ui| self.panels_menu(ui));
            ui.separator();
            for command in [C::SheetPreview, C::ZoomToFit, C::ZoomToSelection] {
                self.checked_menu_item(ui, command);
            }
            self.command_items(
                ui,
                &[
                    None,
                    Some(C::Toggle(ViewFlag::Grid)),
                    Some(C::Toggle(ViewFlag::Axes)),
                    Some(C::Toggle(ViewFlag::Snap)),
                    None,
                    Some(C::Toggle(ViewFlag::Thin)),
                    Some(C::Toggle(ViewFlag::Status)),
                    Some(C::Toggle(ViewFlag::Endpoints)),
                    None,
                    Some(C::Toggle(ViewFlag::Dimensions)),
                    Some(C::Toggle(ViewFlag::PrintSize)),
                    Some(C::Toggle(ViewFlag::Hidden)),
                ],
            );
            self.grid_menu(ui);
            self.relations_menu(ui);
            self.command_items(
                ui,
                &[
                    Some(C::Toggle(ViewFlag::Free)),
                    Some(C::Toggle(ViewFlag::Combs)),
                    Some(C::Toggle(ViewFlag::Crops)),
                ],
            );
            ui.separator();
            let room = egui::Atom::custom(egui::Id::new("menu_check"), egui::vec2(14.0, 14.0));
            ui.menu_button((room, "Appearance"), |ui| {
                let current = theme::Look::current(ui.ctx());
                for look in theme::Look::ALL {
                    if theme::menu_check(ui, look == current, look.label(), "").clicked() {
                        theme::set_look(ui.ctx(), look);
                        ui.close();
                    }
                }
            });
        });
        for category in [Category::Draw, Category::Build] {
            ui.menu_button(category.label(), |ui| self.category_menu(ui, category));
        }
        ui.menu_button("Modify", |ui| {
            self.category_menu(ui, Category::Modify);
            // Drift's object menu: grouping, blocks and construction belong with the
            // other commands that change what is picked (Illustrator's Object menu).
            self.command_items(
                ui,
                &[
                    None,
                    Some(C::Group),
                    Some(C::Ungroup),
                    Some(C::MakeBlock),
                    Some(C::BlockFromGroup),
                    Some(C::Explode),
                    Some(C::ExplodeToGroup),
                    None,
                    Some(C::Construction),
                    None,
                ],
            );
            ui.menu_button("Arrange", |ui| self.category_menu(ui, Category::Arrange));
            ui.separator();
            self.command_item(ui, C::CheckSketch);
        });
        ui.menu_button("Annotate", |ui| self.category_menu(ui, Category::Annotate));
        ui.menu_button("Help", |ui| {
            self.command_items(
                ui,
                &[
                    Some(C::UserGuide),
                    Some(C::ReportProblem),
                    Some(C::ShowLogs),
                    None,
                    Some(C::Shortcuts),
                    // For development: it tells users nothing (hidden in release builds).
                    Some(C::Benchmark),
                    None,
                ],
            );
            #[cfg(not(target_arch = "wasm32"))]
            self.updates_menu(ui);
            self.command_item(ui, C::About);
            ui.label(
                egui::RichText::new(format!(
                    "Drift {} · {}",
                    drift_platform::VERSION,
                    self.backend
                ))
                .small()
                .weak(),
            );
        });
    }

    /// Select the curves that lie on top of others, and the block copies placed on another
    /// copy (the later-made of each pair).
    pub(crate) fn select_overlapping(&mut self, now: f64) {
        // (First: cancelling a copy's move can take a copy away.)
        self.abort_operation();
        let found = self.drawing.editor.overlapping();
        let copies = self.drawing.editor.overlapping_instances();
        if found.is_empty() && copies.is_empty() {
            self.set_status("Nothing overlaps".into(), false, now);
            return;
        }
        if self.tool.kind() != ToolKind::Select {
            self.set_tool(ToolKind::Select);
        }
        self.drawing.editor.selection = found.iter().copied().collect();
        self.drawing.editor.selected_instances = copies.iter().copied().collect();
        // (Inside a block or group being edited, only its own.)
        self.drawing.editor.retain_in_scope();
        let what = match (found.len(), copies.len()) {
            (n, 0) => format!("{n} overlapping curve(s)"),
            (0, n) => format!("{n} stacked block cop{}", if n == 1 { "y" } else { "ies" }),
            (n, m) => format!(
                "{n} overlapping curve(s) and {m} stacked block cop{}",
                if m == 1 { "y" } else { "ies" }
            ),
        };
        self.set_status(
            format!("Selected {what}. Delete removes them and leaves the ones underneath"),
            false,
            now,
        );
    }

    /// Pick the relations that conflict (#305): their geometry is selected and their rows in
    /// the Relations list show picked, so the one to delete or change is in front of you.
    pub(crate) fn select_conflicts(&mut self, now: f64) {
        self.abort_operation();
        let ed = &self.drawing.editor;
        let conflicts: Vec<ConstraintId> = ed
            .analysis()
            .conflicts
            .iter()
            .copied()
            .filter(|id| ed.doc.constraint(*id).is_some())
            .collect();
        if conflicts.is_empty() {
            self.set_status(
                "The sketch doesn't solve, but no relation is to blame on its own".into(),
                false,
                now,
            );
            return;
        }
        let geometry = conflicts
            .iter()
            .filter_map(|id| ed.doc.constraint(*id))
            .flat_map(|c| c.entities())
            .collect();
        if self.tool.kind() != ToolKind::Select {
            self.set_tool(ToolKind::Select);
        }
        let ed = &mut self.drawing.editor;
        ed.selection = geometry;
        ed.selected_instances.clear();
        ed.selected_midpoints.clear();
        ed.selected_texts.clear();
        ed.selected_fills.clear();
        ed.annotation_picks.clear();
        ed.selected_annotation = None;
        self.drawing.selected_constraint = None;
        // (Inside a block or group being edited, only its own.)
        self.drawing.editor.retain_in_scope();
        if self.drawing.editor.selection.is_empty() {
            // All of them outside the block or group being edited.
            self.drawing.picked_relations = None;
            self.set_status(
                "The conflicting relations are outside what you're editing".into(),
                false,
                now,
            );
            return;
        }
        let n = conflicts.len();
        self.drawing.picked_relations = Some((conflicts, self.drawing.editor.selection.clone()));
        self.show_panel(crate::dock::Panel::Inspector);
        self.set_status(
            format!(
                "Picked {n} conflicting relation{}: delete or change one in the Relations list",
                if n == 1 { "" } else { "s" }
            ),
            false,
            now,
        );
    }

    /// The relations the conflicts pill picked, while the selection it made stands (any other
    /// pick ends it).
    pub(crate) fn picked_relations(&self) -> &[ConstraintId] {
        match &self.drawing.picked_relations {
            Some((ids, made)) if !made.is_empty() && *made == self.drawing.editor.selection => ids,
            _ => &[],
        }
    }

    /// Zoom so that 100% is life size on a 96 dpi screen, about the middle of the canvas.
    pub(crate) fn zoom_to_percent(&mut self, percent: f64) {
        let target = percent / 100.0 * 96.0 / drift_core::units::MM_PER_INCH;
        // (On a sheet it is the sheet that is zoomed, about the middle of what shows.)
        if self.drawing.view.sheet_preview {
            self.drawing.view.preview.zoom = (target as f32).clamp(0.05, 200.0);
            return;
        }
        self.drawing.view.fit_target = None;
        // The view's origin is the canvas centre.
        self.drawing
            .view
            .camera
            .zoom_at(DVec2::ZERO, target / self.drawing.view.camera.scale);
    }

    /// Fit the view to what is selected. False with nothing selected.
    pub(crate) fn zoom_to_selection(&mut self) -> bool {
        let ed = &self.drawing.editor;
        let mut bounds: Bounds = ed
            .selection
            .iter()
            .filter_map(|id| ed.doc.shape(*id))
            .map(|s| s.bounds())
            .collect();
        for id in &ed.selected_instances {
            if let Some(i) = ed.doc.instance(*id) {
                bounds.include(i.at);
            }
        }
        for o in ed.selected_texts.iter().filter_map(|t| ed.text_outline(*t)) {
            bounds.include(o.bounds.min);
            bounds.include(o.bounds.max);
        }
        for (_, a, label) in ed.riders().notes {
            // A section mark's far end too.
            let far = match a {
                drift_core::Annotation::Section { b, .. } => Some(b),
                _ => None,
            };
            for p in label.into_iter().chain(far) {
                bounds.include(p);
            }
        }
        if bounds.is_empty() {
            return false;
        }
        // A single point has no size to fit: show some room around it.
        if bounds.min == bounds.max {
            let pad = DVec2::splat(self.grid().major);
            bounds.min -= pad;
            bounds.max += pad;
        }
        self.drawing.view.fit_target = Some(bounds);
        true
    }

    /// The status bar's zoom menu.
    fn zoom_menu(&mut self, ui: &mut egui::Ui) {
        self.command_items(
            ui,
            &[
                Some(Command::ZoomToFit),
                Some(Command::ZoomToSelection),
                None,
            ],
        );
        for percent in [25.0, 50.0, 100.0, 200.0, 400.0] {
            let label = if percent == 100.0 {
                "100% (life size)".to_owned()
            } else {
                format!("{percent:.0}%")
            };
            if ui.button(label).clicked() {
                self.zoom_to_percent(percent);
                ui.close();
            }
        }
    }

    /// View → Panels: show or hide each panel; hide them all; reset the layout.
    pub(crate) fn panels_menu(&mut self, ui: &mut egui::Ui) {
        for p in crate::dock::Panel::ALL {
            self.command_item(ui, Command::Panel(p));
        }
        ui.separator();
        self.command_items(
            ui,
            &[Some(Command::HideAllPanels), Some(Command::ResetPanels)],
        );
    }

    /// The Layer row every inspector shows first, under its title (#300): the layer of what
    /// is picked (curves, walls, shapes, fills, block copies, texts), or "Mixed", and a pick
    /// moves all of it there in one undo step. A wall counts by its own layer. Nothing with
    /// a layer picked (dimensions, notes, points): no row.
    pub(crate) fn inspector_layer_row(&mut self, ui: &mut egui::Ui) {
        let curves = self.drawing.pick_memo.curve_layers(&self.drawing.editor);
        let ed = &self.drawing.editor;
        let walls = ed.selected_walls();
        let others: Vec<drift_core::LayerId> = std::iter::empty()
            .chain(
                walls
                    .iter()
                    .filter_map(|w| ed.doc.wall(*w))
                    .map(|w| w.layer),
            )
            .chain(
                ed.selected_fills
                    .iter()
                    .filter_map(|f| ed.doc.fill(*f))
                    .map(|f| f.layer),
            )
            .chain(
                ed.selected_instances
                    .iter()
                    .filter_map(|i| ed.doc.instance(*i))
                    .map(|i| i.layer),
            )
            .chain(
                ed.selected_texts
                    .iter()
                    .filter_map(|t| ed.doc.text(*t))
                    .map(|t| t.layer),
            )
            .collect();
        // The picked curves' layers first, then the others'.
        let (first, mixed) = match curves {
            Some((first, mixed)) => (first, mixed || others.iter().any(|l| *l != first)),
            None => {
                let Some(&first) = others.first() else {
                    return;
                };
                (first, others.iter().any(|l| *l != first))
            }
        };
        let text = if mixed {
            "Mixed".to_owned()
        } else {
            ed.doc.layer(first).map_or("?".into(), |l| l.name.clone())
        };
        let layers: Vec<(drift_core::LayerId, String)> = ed
            .doc
            .layers()
            .map(|(id, l)| (id, l.name.clone()))
            .collect();
        let mut target = None;
        ui.add_space(4.0);
        theme::row(ui, "Layer", |ui| {
            egui::ComboBox::from_id_salt("inspector_layer")
                .icon(crate::theme::combo_icon)
                .selected_text(text)
                .width(ui.available_width())
                .truncate()
                .show_ui(ui, |ui| {
                    for (id, name) in layers {
                        if ui.selectable_label(!mixed && id == first, name).clicked() {
                            target = Some(id);
                        }
                    }
                })
                .response
                .on_hover_text("The layer of what is picked: pick another to move it there");
        });
        if let Some(id) = target {
            self.drawing.editor.move_selection_to_layer(id);
        }
    }

    /// Several things picked, text or annotations among them: what they are, and where the
    /// annotations show. Returns true if it showed.
    pub(crate) fn mixed_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let ed = &self.drawing.editor;
        let annotations = self.picked_annotations();
        let n = ed.selection.len()
            + ed.selected_instances.len()
            + ed.selected_fills.len()
            + ed.selected_texts.len()
            + annotations.len();
        if n < 2 || (ed.selected_texts.is_empty() && annotations.is_empty()) {
            return false;
        }
        let mut counts: Vec<(&str, usize)> = Vec::new();
        let mut count = |name: &'static str| match counts.iter_mut().find(|c| c.0 == name) {
            Some(c) => c.1 += 1,
            None => counts.push((name, 1)),
        };
        for id in &ed.selection {
            if let Some(e) = ed.doc.get(*id) {
                count(e.kind.type_name());
            }
        }
        for _ in &ed.selected_instances {
            count("Block");
        }
        for _ in &ed.selected_fills {
            count("Fill");
        }
        for r in &annotations {
            match r {
                drift_core::AnnotationRef::Dimension(_) => count("Dimension"),
                drift_core::AnnotationRef::Annotation(id) => {
                    if let Some(a) = ed.doc.annotation(*id) {
                        count(a.type_name());
                    }
                }
            }
        }
        for _ in &ed.selected_texts {
            count("Text");
        }
        let summary: Vec<String> = counts.iter().map(|(name, n)| plural(*n, name)).collect();
        title(
            ui,
            Some(ToolKind::Select),
            &format!("{n} selected"),
            &summary.join(" · "),
        );
        self.inspector_layer_row(ui);
        theme::hint(
            ui,
            "Delete, Move (G), Rotate (R), Mirror, Array and Copy act on all of it. \
             Dimensions follow their geometry.",
        );
        self.visibility_section(ui);
        self.array_panel(ui, ui.input(|i| i.time));
        true
    }

    pub(crate) fn inspector(&mut self, ui: &mut egui::Ui) {
        // On a sheet, the Inspector shows what is picked there.
        if self.drawing.view.sheet_preview {
            self.sheet_inspector(ui);
            return;
        }
        self.block_edit_banner(ui);
        self.walls_from_lines_report(ui);
        if self.fill_tool_inspector(ui)
            || self.opening_inspector(ui)
            || self.wall_inspector(ui)
            || self.offset_inspector(ui)
            || self.mixed_inspector(ui)
            || self.annotation_inspector(ui)
            || self.text_inspector(ui)
            || self.instance_inspector(ui)
            || self.shape_inspector(ui)
            || self.fill_inspector(ui)
            || self.dimension_inspector(ui)
        {
            return;
        }
        if let Some(region) = self.drawing.editor.region() {
            let units = self.drawing.editor.doc.units();
            theme::section(ui, "Region");
            let mono = |ui: &mut egui::Ui, text: String| {
                ui.label(text);
            };
            theme::row(ui, "Area", |ui| mono(ui, units.format_area(region.area)));
            theme::row(ui, "Perimeter", |ui| {
                mono(ui, units.format_readout(region.perimeter))
            });
            theme::row(ui, "Centroid", |ui| {
                mono(
                    ui,
                    format!(
                        "{}, {}",
                        units.format_readout(region.centroid.x),
                        units.format_readout(region.centroid.y)
                    ),
                )
            });
            theme::row(ui, "Holes", |ui| mono(ui, region.holes.len().to_string()));
            self.region_fill_buttons(ui);
        }
        self.group_inspector(ui);
        let ed = &self.drawing.editor;
        let selection: Vec<_> = ed.selection.iter().copied().collect();
        for line in &ed.selected_midpoints {
            if let Some(drift_core::Shape::Line { a, b }) = ed.index.shape(*line) {
                let m = (*a + *b) * 0.5;
                let units = ed.doc.units();
                let of = ed.doc.get(*line).map_or(String::new(), |e| {
                    let layer = ed.doc.layer(e.layer).map_or("", |l| l.name.as_str());
                    format!("A line on layer {layer}")
                });
                theme::section(ui, "Midpoint");
                theme::row(ui, "Of", |ui| {
                    ui.label(of);
                });
                theme::row(ui, "At", |ui| {
                    ui.label(format!(
                        "{}, {}",
                        units.format_readout(m.x),
                        units.format_readout(m.y)
                    ));
                });
            }
        }
        let mut size_edit = None;
        match selection.as_slice() {
            [] if !ed.selected_midpoints.is_empty() => {}
            [] if ed.selected_region.is_some() => {}
            [] => {
                // A title saying so, a hint, and the sketch's totals.
                title(ui, Some(ToolKind::Select), "Nothing selected", "");
                let hint = if self.drawing.editor.regions_need_fill() {
                    "Click geometry, or a fill or hatch, to inspect it."
                } else {
                    "Click geometry to inspect it, or inside a closed area to measure it."
                };
                theme::hint(ui, hint);
                theme::section(ui, "Sketch");
                self.drawing.layer_counts.update(&self.drawing.editor.doc);
                let curves = self.drawing.layer_counts.total;
                let relations = ed.doc.constraint_count();
                let layers = ed.doc.layers().count();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let w = ((ui.available_width() - 12.0) / 3.0).floor().max(40.0);
                    theme::stat(ui, curves, "Curves", w);
                    theme::stat(ui, relations, "Relations", w);
                    theme::stat(ui, layers, "Layers", w);
                });
            }
            [id] => {
                let (Some(entity), Some(shape)) = (ed.doc.get(*id), ed.index.shape(*id)) else {
                    return;
                };
                let mut layer = ed
                    .doc
                    .layer(entity.layer)
                    .map_or(String::new(), |l| format!("On layer {}", l.name));
                // A kept corner or a fillet's centre says what it is (#501).
                if let Some(kind) = ed.muted_kind(*id) {
                    layer = format!("{} · {layer}", kind.label());
                }
                title(
                    ui,
                    Some(kind_icon(&entity.kind)),
                    entity.kind.type_name(),
                    &layer,
                );
                let shape = *shape;
                self.inspector_layer_row(ui);
                let ed = &self.drawing.editor;
                theme::section(ui, "Geometry");
                size_edit = size_rows(ui, ed, *id);
                property_rows(ui, &shape, ed.doc.units());
            }
            many => {
                let n = many.len();
                let summary = self.drawing.pick_memo.summary(&self.drawing.editor);
                title(
                    ui,
                    Some(ToolKind::Select),
                    &format!("{n} selected"),
                    &summary,
                );
                self.inspector_layer_row(ui);
            }
        }

        let ed = &self.drawing.editor;
        let curves: Vec<_> = selection
            .iter()
            .filter_map(|id| ed.doc.get(*id))
            .filter(|e| !e.kind.is_point())
            .collect();
        if !curves.is_empty() {
            theme::section(ui, "Appearance");
            let current = curves[0].style;
            let uniform = curves.iter().all(|e| e.style == current);
            let shown = if uniform { current } else { LineKind::Solid };
            if let Some(style) = theme::segmented(
                ui,
                shown,
                &[
                    (LineKind::Solid, "Geometry"),
                    (LineKind::Construction, "Construction"),
                    (LineKind::Center, "Center line"),
                ],
            ) {
                self.set_selection_style(style);
            }
            self.appearance_panel(ui);
            // Filled shapes among the picked curves (a group's, say): their fills too.
            let shapes = self.drawing.editor.shapes_in_selection();
            if !shapes.fills.is_empty() {
                let title = match shapes.fills.len() {
                    1 => "Fill".to_owned(),
                    n => format!("Fill · {n} shapes"),
                };
                theme::section(ui, &title);
                self.shapes_fill_section(ui, &shapes);
            }
            theme::section(ui, "Arrange");
            self.arrange_row(ui);
        }

        let now = ui.input(|i| i.time);
        match size_edit {
            Some(SizeEdit::Dimension(wanted, text)) => {
                // A dimension already there is edited. Otherwise the size is added as a
                // dimension, unless automatic relations are off: then it only resizes.
                let keeps = self.drawing.editor.prefs.auto_relations
                    || self.drawing.editor.size_dimension(&wanted).is_some();
                let result = if keeps {
                    self.drawing.editor.set_size(wanted, &text).map(|_| ())
                } else {
                    self.drawing.editor.resize_once(wanted, &text)
                };
                if let Err(e) = result {
                    self.set_status(format!("Can't use {text}: {e}"), true, now);
                }
            }
            Some(SizeEdit::Point(id, axis, text)) => {
                match self.drawing.editor.evaluate_text(&text, Kind::Length) {
                    Ok(v) => {
                        if let Ok(mut to) = self.drawing.editor.doc.point(id) {
                            to[axis] = v.value;
                            if !self.drawing.editor.move_point_to(id, to) {
                                self.set_status(
                                    "That point is held by its relations".into(),
                                    true,
                                    now,
                                );
                            }
                        }
                    }
                    Err(e) => self.set_status(format!("Can't use {text}: {e}"), true, now),
                }
            }
            Some(SizeEdit::Angle(line, text)) => {
                match self.drawing.editor.evaluate_text(&text, Kind::Angle) {
                    Ok(v) => {
                        if !self.drawing.editor.set_line_angle(line, v.value) {
                            self.set_status(
                                "That line's direction is held by its relations".into(),
                                true,
                                now,
                            );
                        }
                    }
                    Err(e) => self.set_status(format!("Can't use {text}: {e}"), true, now),
                }
            }
            None => {}
        }
        self.centre_mark_button(ui);
        self.constraint_buttons(ui);
        self.array_panel(ui, now);
        self.constraint_list(ui);

        if !self.drawing.editor.selection.is_empty() {
            ui.add_space(12.0);
            if theme::danger_button(ui, "Delete selection").clicked() {
                self.drawing.editor.delete_selection();
            }
        }
        ui.add_space(10.0);
    }

    /// Add a relation or dimension offered for the selection. Returns a new dimension (to
    /// type its value into).
    pub(crate) fn apply_constraint_option(
        &mut self,
        option: &drift_edit::editor::ConstraintOption,
        is_dimension: bool,
        now: f64,
    ) -> Option<ConstraintId> {
        let added = match option.constraints.as_slice() {
            [c] if is_dimension => self
                .drawing
                .editor
                .add_dimension(*c, None)
                .map(|id| vec![id]),
            constraints if is_dimension => self
                .drawing
                .editor
                .add_constraints(option.label, constraints),
            // Only the relations that aren't there yet (some may already hold). The one picked
            // last stays where it can, and the others conform to it (#333).
            _ => match self.drawing.editor.missing_relations(option).as_slice() {
                [] if drift_edit::editor::Editor::is_fix_option(option) => Err(
                    drift_edit::editor::EditError::FullyDefined(self.drawing.editor.fixed_name()),
                ),
                [] => Err(drift_edit::editor::EditError::Redundant),
                missing => self
                    .drawing
                    .editor
                    .add_picked_constraints(option.label, missing),
            },
        };
        match added {
            Ok(ids) if is_dimension => ids.first().copied(),
            Ok(_) => None,
            Err(e) => {
                self.set_status(format!("{}: {e}", option.label), true, now);
                None
            }
        }
    }

    /// Relations and dimensions that fit the selection, as chips.
    pub(crate) fn constraint_buttons(&mut self, ui: &mut egui::Ui) {
        // (By name: what one adds is found when it is clicked.)
        let relations: Vec<drift_edit::editor::ConstraintOption> = self
            .drawing
            .pick_memo
            .relations(&self.drawing.editor)
            .into_iter()
            .map(|label| drift_edit::editor::ConstraintOption {
                label,
                constraints: Vec::new(),
            })
            .collect();
        let dimensions = self.drawing.editor.dimension_options();
        let now = ui.input(|i| i.time);
        let pins = dimensions.clone();
        for (heading, options, is_dimension) in [
            ("Add relation", relations, false),
            ("Add dimension", dimensions, true),
        ] {
            if options.is_empty() {
                continue;
            }
            theme::section(ui, heading);
            // Which of what is picked stays (#333).
            let hint = (!is_dimension)
                .then(|| self.drawing.editor.active_hint())
                .flatten();
            if let Some(hint) = hint {
                theme::hint(ui, hint);
            }
            let mut chosen = None;
            let ed = &self.drawing.editor;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for option in &options {
                    let mut response = theme::chip(ui, option.label);
                    if !is_dimension && let Some(role) = ed.relation_role(option.label) {
                        response = response.on_hover_text(role);
                    }
                    if response.clicked() {
                        // (A relation offered by name: what it adds is found now, and
                        // nothing if it is no longer offered.)
                        chosen = if is_dimension {
                            Some(option.clone())
                        } else {
                            ed.relation_options()
                                .into_iter()
                                .find(|o| o.label == option.label)
                        };
                    }
                }
            });
            if let Some(option) = chosen
                && let Some(id) = self.apply_constraint_option(&option, is_dimension, now)
            {
                // Let the user type the value straight away.
                self.drawing.focus_dimension = Some(id);
            }
        }
        if !pins.is_empty() {
            let mut chosen = None;
            theme::fold(ui, "pin_measurement", "Pin measurement", false, |ui| {
                theme::hint(ui, "Keep a live measurement in the Parameters tab");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for option in &pins {
                        if theme::chip(ui, option.label).clicked() {
                            chosen = option.constraints.first().copied();
                        }
                    }
                });
            });
            if let Some(c) = chosen {
                match self.drawing.editor.pin_measurement(c) {
                    Ok(_) => self.set_status("Pinned (see the Parameters tab)".into(), false, now),
                    Err(e) => self.set_status(format!("Can't pin it: {e}"), true, now),
                }
            }
        }
    }

    /// Constraints on the selection (or all of them when nothing is selected), with editable
    /// dimension values. Each row: the name (click to select its geometry), the value, a
    /// "reference" toggle for dimensions, and a delete button that shows on hover.
    pub(crate) fn constraint_list(&mut self, ui: &mut egui::Ui) {
        let ids = self.drawing.pick_memo.constraints(&self.drawing.editor);
        let ed = &self.drawing.editor;
        let constraints: Vec<(ConstraintId, Constraint)> = ids
            .into_iter()
            .filter_map(|id| ed.doc.constraint(id).map(|c| (id, *c)))
            .collect();
        if constraints.is_empty() {
            return;
        }
        let conflicts = ed.analysis().conflicts.clone();
        let redundant = ed.analysis().redundant.clone();
        let picked = self.picked_relations().to_vec();
        let everything = ed.selection.is_empty() && ed.selected_midpoints.is_empty();
        theme::section(
            ui,
            &format!("Relations & dimensions · {}", constraints.len()),
        );
        // With nothing selected this lists the whole sketch: folded away unless asked for (or
        // something conflicts).
        if everything && conflicts.is_empty() {
            let key = ui.make_persistent_id("all_constraints_open");
            let mut open = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
            if ui
                .button(if open { "Hide list" } else { "Show all" })
                .clicked()
            {
                open = !open;
                ui.data_mut(|d| d.insert_temp(key, open));
            }
            if !open {
                return;
            }
        }
        let now = ui.input(|i| i.time);
        let mut remove = None;
        let mut select = None;
        let mut apply = None;
        let mut toggle_driven = None;
        let weak = ui.visuals().weak_text_color();
        // Laying out thousands of rows every frame stalls the UI; the rest are summarised.
        const MAX_ROWS: usize = 100;
        let hidden_rows = constraints.len().saturating_sub(MAX_ROWS);
        // One grid for every row: the name takes what the value, the reference toggle and the
        // delete button leave, so the values and the buttons line up down the list.
        const REF_WIDTH: f32 = 32.0;
        const DELETE_WIDTH: f32 = 20.0;
        const GAP: f32 = 4.0;
        const INSET: f32 = 6.0;
        let room = ui.available_width();
        let value_width = (room * 0.32).clamp(64.0, 96.0).floor();
        let dark = ui.visuals().dark_mode;
        let gap_y = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 2.0;
        for (id, c) in constraints.iter().take(MAX_ROWS) {
            // (Filled in under the row once its size is known.)
            let backdrop = ui.painter().add(egui::Shape::Noop);
            let row = ui.horizontal(|ui| {
                ui.set_min_height(theme::ROW_HEIGHT);
                ui.spacing_mut().item_spacing.x = GAP;
                let has_value = c.value().is_some();
                let tail = if has_value {
                    value_width + REF_WIDTH + DELETE_WIDTH + 3.0 * GAP
                } else {
                    DELETE_WIDTH + GAP
                };
                let name_width = (ui.available_width() - INSET - tail).max(24.0);
                ui.add_space(INSET - GAP);
                let mut name = egui::RichText::new(constraint_label(&ed.doc, c));
                if conflicts.contains(id) {
                    name = name.color(ui.visuals().error_fg_color);
                }
                let is_redundant = redundant.contains(id);
                let label = ui
                    .allocate_ui_with_layout(
                        egui::vec2(name_width, theme::ROW_HEIGHT),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(name_width);
                            ui.spacing_mut().item_spacing.x = GAP;
                            // A redundant one's marker follows its name, inside the name's
                            // column, so the values stay in line.
                            let width = if is_redundant {
                                name_width - 18.0
                            } else {
                                name_width
                            };
                            let label = ui
                                .allocate_ui(egui::vec2(width, theme::ROW_HEIGHT), |ui| {
                                    ui.add(
                                        egui::Label::new(name)
                                            .sense(egui::Sense::click())
                                            .truncate(),
                                    )
                                })
                                .inner;
                            if is_redundant {
                                redundant_marker(ui);
                            }
                            label
                        },
                    )
                    .inner
                    .on_hover_text(format!("{id}: click to select its geometry"));
                if label.clicked() {
                    select = Some(*c);
                }
                let reference = |ui: &mut egui::Ui, driven: bool| {
                    let text = egui::RichText::new("Ref").size(theme::size::CAPTION);
                    ui.add_sized([REF_WIDTH, 20.0], egui::Button::selectable(driven, text))
                        .on_hover_text(if driven {
                            "Make it drive the geometry"
                        } else {
                            "Make it a reference that follows the geometry"
                        })
                        .clicked()
                };
                match c.value() {
                    Some(_) if ed.doc.is_driven(*id) => {
                        let value = ed.dimension_value(*id).unwrap_or_default();
                        ui.allocate_ui_with_layout(
                            egui::vec2(value_width, theme::ROW_HEIGHT),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(value_width);
                                ui.add_space(4.0);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(format!(
                                            "({})",
                                            ed.doc
                                                .units()
                                                .format(drift_core::dimension_kind(c), value)
                                        ))
                                        .color(weak),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text("Reference (driven): follows the geometry");
                            },
                        );
                        if reference(ui, true) {
                            toggle_driven = Some((*id, false));
                        }
                    }
                    Some(_) => {
                        let field = egui::Id::new(("dimension", id.0));
                        let mut text = self
                            .drawing
                            .dimension_text
                            .get(id)
                            .cloned()
                            .or_else(|| ed.dimension_edit_text(*id))
                            .unwrap_or_default();
                        let response = crate::theme::field(
                            ui,
                            egui::TextEdit::singleline(&mut text)
                                .id(field)
                                .desired_width(value_width)
                                .hint_text(c.unit()),
                        );
                        if self.drawing.focus_dimension == Some(*id) {
                            response.request_focus();
                            self.drawing.focus_dimension = None;
                        }
                        if response.has_focus() {
                            self.drawing.dimension_text.insert(*id, text.clone());
                        }
                        if response.lost_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                apply = Some((*id, text.clone()));
                            }
                            self.drawing.dimension_text.remove(id);
                        }
                        if reference(ui, false) {
                            toggle_driven = Some((*id, true));
                        }
                    }
                    None => {}
                }
                // The delete button ends every row, clear of the scroll bar.
                ui.add_space((ui.available_width() - DELETE_WIDTH - GAP).max(0.0));
                if theme::delete_button(ui, "×", "Delete").clicked() {
                    remove = Some(*id);
                }
            });
            // The picked relation's row, and the one under the pointer.
            let r = row.response.rect;
            if let Some(fill) =
                theme::row_fill(dark, picked.contains(id), ui.rect_contains_pointer(r))
            {
                ui.painter().set(
                    backdrop,
                    egui::epaint::RectShape::filled(
                        r.expand2(egui::vec2(0.0, 1.0)),
                        theme::radius::CONTROL,
                        fill,
                    ),
                );
            }
        }
        ui.spacing_mut().item_spacing.y = gap_y;
        if hidden_rows > 0 {
            ui.label(
                egui::RichText::new(format!("… and {hidden_rows} more (pick less to see them)"))
                    .weak(),
            );
        }
        // Clearing all of them at once (on a selection).
        if !everything {
            let (relations, dims) = self.drawing.editor.selection_constraints();
            let mut clear = None;
            ui.horizontal_wrapped(|ui| {
                if !relations.is_empty()
                    && ui
                        .button(format!("Clear relations ({})", relations.len()))
                        .on_hover_text(format!(
                            "Remove the relations on the selection, including those holding it \
                             to what isn't picked: {}. Shared corners stay joined; dimensions \
                             stay",
                            crate::constraint_summary(&self.drawing.editor.doc, &relations)
                        ))
                        .clicked()
                {
                    clear = Some(false);
                }
                if !dims.is_empty()
                    && ui
                        .button(format!("Clear dimensions ({})", dims.len()))
                        .on_hover_text(format!(
                            "Remove the dimensions on the selection: {}",
                            crate::constraint_summary(&self.drawing.editor.doc, &dims)
                        ))
                        .clicked()
                {
                    clear = Some(true);
                }
            });
            if let Some(dimensions) = clear {
                self.clear_selection_constraints(dimensions, now);
                return;
            }
        }
        if let Some(c) = select {
            self.drawing.editor.selection = c.entities().collect();
            self.drawing.editor.retain_in_scope();
        }
        if let Some(id) = remove {
            self.drawing.editor.remove_constraint(id);
        }
        if let Some((id, driven)) = toggle_driven
            && let Err(e) = self.drawing.editor.set_driven(id, driven)
        {
            self.set_status(format!("Can't make it driving: it {e}"), true, now);
        }
        if let Some((id, text)) = apply
            && Some(&text) != self.drawing.editor.dimension_edit_text(id).as_ref()
            && let Err(e) = self.drawing.editor.set_dimension_text(id, &text)
        {
            self.set_status(format!("Can't use {text}: {e}"), true, now);
        }
    }

    /// Zoom by a factor about the middle of the canvas (the sheet's, on a sheet).
    fn zoom_by(&mut self, factor: f64) {
        if self.drawing.view.sheet_preview {
            let zoom = &mut self.drawing.view.preview.zoom;
            *zoom = (*zoom * factor as f32).clamp(0.05, 200.0);
        } else {
            self.drawing.view.fit_target = None;
            self.drawing.view.camera.zoom_at(DVec2::ZERO, factor);
        }
    }

    /// On a sheet, its paper and scale (`A3 landscape · 1:50`) in the status bar, beside the
    /// zoom: the tab above names the sheet, and nothing sits over the paper.
    fn sheet_readout(&self, ui: &mut egui::Ui) {
        if !self.drawing.view.sheet_preview {
            return;
        }
        let doc = &self.drawing.editor.doc;
        let Some(sheet) = self.current_sheet().and_then(|id| doc.sheet(id)) else {
            return;
        };
        let info = format!(
            "{} {} · {}",
            sheet.paper.label(),
            if sheet.portrait {
                "portrait"
            } else {
                "landscape"
            },
            doc.sheet_scale_text(sheet)
        );
        theme::bar_divider(ui);
        theme::bar_caption(ui, &info)
            .on_hover_text("This sheet's paper and the scale of its views");
        theme::bar_divider(ui);
    }

    /// The status bar's zoom: out, the percentage (which opens the zoom menu) and in. Laid
    /// out from the right.
    fn zoom_control(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let step = 1.25;
            if icons::button(ui, 22.0, false, "Zoom in", |p, r, c| {
                icons::glyph(p, r, Glyph::Plus, c)
            })
            .on_hover_text("Zoom in")
            .clicked()
            {
                self.zoom_by(step);
            }
            // Zoom as a percentage of real size on a 96 dpi screen.
            let scale = if self.drawing.view.sheet_preview {
                f64::from(self.drawing.view.preview.zoom)
            } else {
                self.drawing.view.camera.scale
            };
            let zoom = scale * drift_core::units::MM_PER_INCH / 96.0 * 100.0;
            let button = ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(format!("{zoom:.0}%"))
                            .size(theme::size::LABEL)
                            .color(ui.visuals().weak_text_color()),
                    )
                    .frame_when_inactive(false)
                    .min_size(egui::vec2(48.0, 22.0)),
                )
                .on_hover_text("Zoom: click for fit, selection and fixed sizes");
            egui::Popup::menu(&button).show(|ui| self.zoom_menu(ui));
            if icons::button(ui, 22.0, false, "Zoom out", |p, r, c| {
                icons::glyph(p, r, Glyph::Minus, c)
            })
            .on_hover_text("Zoom out")
            .clicked()
            {
                self.zoom_by(1.0 / step);
            }
        });
    }

    /// Where the pointer is on the drawing: X and Y, each a label and 70 pt of value,
    /// right-aligned so the digits don't jump. With the pointer off the drawing there is
    /// nothing to read: it takes no room (the hint has it), and is still there to be read
    /// out as "off the drawing".
    fn pointer_readout(&self, ui: &mut egui::Ui) {
        const AXIS: f32 = 13.0;
        const CELL: f32 = 70.0;
        const GAP: f32 = 12.0;
        let units = self.drawing.editor.doc.units();
        let values = self.drawing.view.pointer.map(|p| {
            let at = p.pos();
            [units.format_readout(at.x), units.format_readout(at.y)]
        });
        let Some(values) = values else {
            let was = ui.spacing().item_spacing.x;
            ui.spacing_mut().item_spacing.x = 0.0;
            let (_, response) = ui.allocate_exact_size(egui::vec2(0.0, 20.0), egui::Sense::hover());
            ui.spacing_mut().item_spacing.x = was;
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Pointer off the drawing")
            });
            return;
        };
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(2.0 * (AXIS + CELL) + GAP, 20.0),
            egui::Sense::hover(),
        );
        let n = theme::neutrals(ui.visuals().dark_mode);
        let font = egui::FontId::proportional(theme::size::LABEL);
        let painter = ui.painter_at(rect);
        let mut left = rect.left();
        for (k, axis) in ["X", "Y"].into_iter().enumerate() {
            painter.text(
                egui::pos2(left, rect.center().y),
                egui::Align2::LEFT_CENTER,
                axis,
                font.clone(),
                n.tertiary,
            );
            painter.text(
                egui::pos2(left + AXIS + CELL, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                &values[k],
                font.clone(),
                n.secondary,
            );
            left += AXIS + CELL + GAP;
        }
        response.widget_info(|| {
            let [x, y] = &values;
            let text = format!("Pointer at X {x}, Y {y}");
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text)
        });
    }

    /// The status bar: the tool's hint at the left; then, right-aligned, the pointer's
    /// position, what snaps, what the drawing shows, the units, what is worth knowing about
    /// the sketch (and its definition status), and zoom. Messages appear as toasts over the
    /// canvas.
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui, _now: f64) {
        let bar = ui.max_rect();
        // The right-hand cluster first (laid out from the right), so the hint takes what is
        // left.
        let cluster_left = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(bar)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    // One control height across the bar.
                    ui.spacing_mut().interact_size.y = 22.0;
                    self.zoom_control(ui);
                    self.sheet_readout(ui);
                    #[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
                    self.link_pill(ui);
                    if !self.drawing.editor.doc.is_empty() {
                        let analysis = self.drawing.editor.analysis();
                        let dark = ui.visuals().dark_mode;
                        let conflicting = !analysis.converged;
                        let (text, color) = if conflicting {
                            (
                                format!("{} conflicting", analysis.conflicts.len()),
                                ui.visuals().error_fg_color,
                            )
                        } else if analysis.dof == Some(0) {
                            ("Fully defined".to_owned(), theme::ok_color(dark))
                        } else {
                            (
                                format!("{} DOF free", analysis.dof.unwrap_or(0)),
                                theme::under_color(dark),
                            )
                        };
                        let pill = theme::pill(ui, &text, color);
                        if conflicting {
                            // Like the overlaps pill: a click picks them (#305).
                            let click = ui
                                .interact(
                                    pill.rect,
                                    ui.id().with("conflicts"),
                                    egui::Sense::click(),
                                )
                                .on_hover_text(
                                    "Relations and dimensions that can't all hold at once. \
                                     Click to pick them; delete or change one in the Relations \
                                     list to free the sketch.",
                                );
                            click.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &text)
                            });
                            if click.clicked() {
                                self.select_conflicts(_now);
                            }
                        } else {
                            pill.on_hover_text(
                                "Degrees of freedom: how much the sketch can still move",
                            );
                        }
                    }
                    // Curves drawn on top of others, and block copies stacked on another copy
                    // (#264): a warning that selects them when clicked. (Curves aren't
                    // re-counted in the middle of a drag; the few copies are.)
                    let overlaps = if self.tool.is_busy() {
                        self.drawing.editor.overlapping_as_last_found()
                    } else {
                        self.drawing.editor.overlapping()
                    };
                    let stacked = self.drawing.editor.overlapping_instances().len();
                    if !overlaps.is_empty() || stacked > 0 {
                        let amber = crate::theme::amber(ui.visuals().dark_mode);
                        let text = format!("{} overlapping", overlaps.len() + stacked);
                        let pill = theme::pill(ui, &text, amber);
                        let click = ui
                            .interact(pill.rect, ui.id().with("overlaps"), egui::Sense::click())
                            .on_hover_text(
                                "Curves drawn on top of other curves (the same line twice, or the \
                             touching sides of two shapes), and block copies placed exactly on \
                             another copy. Click to select the ones on top; Delete then removes \
                             them and leaves those underneath.",
                            );
                        click.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &text)
                        });
                        if click.clicked() {
                            self.select_overlapping(_now);
                        }
                    }
                    // Dimensions or relations switched off for now: say so, and show them
                    // again on a click.
                    let grey = ui.visuals().weak_text_color();
                    for (shown, text, keys) in [
                        (
                            self.show_relations != ShowRelations::Off,
                            "Relations off · Show",
                            "Alt+R",
                        ),
                        (self.show_dimensions, "Dims & notes off · Show", "Alt+D"),
                    ] {
                        if shown {
                            continue;
                        }
                        let pill = theme::pill(ui, text, grey);
                        let click =
                            ui.interact(pill.rect, ui.id().with(text), egui::Sense::click());
                        click.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text)
                        });
                        if click
                            .on_hover_text(format!("Click (or {keys}) to show them again"))
                            .clicked()
                        {
                            if text.starts_with("Rel") {
                                self.show_relations = ShowRelations::All;
                            } else {
                                self.show_dimensions = true;
                            }
                        }
                    }
                    let hidden = self.hidden_annotation_count();
                    if hidden > 0 {
                        let grey = ui.visuals().weak_text_color();
                        let text = if self.show_hidden_annotations {
                            format!("Showing {hidden} hidden")
                        } else {
                            format!(
                                "{hidden} hidden annotation{}",
                                if hidden == 1 { "" } else { "s" }
                            )
                        };
                        let pill = theme::pill(ui, &text, grey);
                        let click =
                            ui.interact(pill.rect, ui.id().with("hidden"), egui::Sense::click());
                        click.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &text)
                        });
                        let click = click.on_hover_text(if self.show_hidden_annotations {
                            "Dimensions and annotations hidden on the canvas, shown faded. Click \
                             (or Alt+H) to leave them out again"
                        } else {
                            "Dimensions and annotations hidden on the canvas. Click (or Alt+H) \
                             to show them faded, so they can be picked and shown again"
                        });
                        if click.clicked() {
                            self.show_hidden_annotations = !self.show_hidden_annotations;
                        }
                    }
                    let mut units = self.drawing.editor.doc.units();
                    let short = |f: drift_core::LengthFormat| match f {
                        drift_core::LengthFormat::Architectural => "ft-in",
                        drift_core::LengthFormat::FractionalInch => "in ½",
                        f => f.symbol(),
                    };
                    egui::ComboBox::from_id_salt("status_units")
                        .icon(theme::combo_icon)
                        .selected_text(short(units.length))
                        .width(56.0)
                        .show_ui(ui, |ui| {
                            for f in drift_core::LengthFormat::METRIC
                                .into_iter()
                                .chain(drift_core::LengthFormat::IMPERIAL)
                            {
                                ui.selectable_value(&mut units.length, f, f.label());
                            }
                        })
                        .response
                        .on_hover_text("Display units (more in File → Drawing settings)");
                    if units != self.drawing.editor.doc.units() {
                        self.drawing.editor.set_units(units);
                    }
                    theme::bar_divider(ui);
                    let minor = self.grid().minor;
                    let grid = if units.length.is_metric() {
                        format!("{} mm", format_length(minor))
                    } else {
                        Units {
                            decimals: 4,
                            fraction: 64,
                            ..units
                        }
                        .format_length(minor)
                    };
                    // Two groups, laid out from the right: what the drawing shows, then (at
                    // the left) what snaps and what is added as you draw.
                    let toggle = |ui: &mut egui::Ui,
                                  glyph: Glyph,
                                  name: &str,
                                  value: &mut bool,
                                  tip: &str| {
                        if icons::button(ui, 22.0, *value, name, |p, r, c| {
                            icons::glyph(p, r, glyph, c)
                        })
                        .on_hover_text(tip)
                        .clicked()
                        {
                            *value = !*value;
                        }
                    };
                    toggle(
                        ui,
                        Glyph::Status,
                        "Status colors",
                        &mut self.status_colors,
                        &format!("Sketch status colors (on) or layer and line colors (off){}", crate::commands::key_suffix("view.status_colours")),
                    );
                    toggle(
                        ui,
                        Glyph::Thin,
                        "Thin lines",
                        &mut self.thin_lines,
                        "Thin lines (Shift+Alt+W)",
                    );
                    toggle(
                        ui,
                        Glyph::Endpoints,
                        "End points",
                        &mut self.drawing.editor.prefs.always_endpoints,
                        &format!("Always show end points (off: only on hovered or picked curves){}", crate::commands::key_suffix("view.endpoints")),
                    );
                    toggle(
                        ui,
                        Glyph::Axes,
                        "Origin axes",
                        &mut self.show_axes,
                        "Origin axes",
                    );
                    toggle(
                        ui,
                        Glyph::Grid,
                        "Grid",
                        &mut self.show_grid,
                        &format!("Grid: {grid} spacing · {}", shortcut("'", false)),
                    );
                    theme::bar_divider(ui);
                    toggle(
                        ui,
                        Glyph::AutoRelations,
                        "Auto relations",
                        &mut self.drawing.editor.prefs.auto_relations,
                        &format!("Auto relations: add relations as you draw and modify (horizontal, coincident, tangent...). Off: plain geometry{}", crate::commands::key_suffix("view.auto_relations")),
                    );
                    toggle(
                        ui,
                        Glyph::AutoMerge,
                        "Auto merge",
                        &mut self.drawing.editor.prefs.auto_merge,
                        &format!("Auto merge: a point dropped on another point joins it, and lines and arcs drawn on a point share it (on the same layer only). Off: only placed there. Ctrl flips it for one drop{}", crate::commands::key_suffix("view.auto_merge")),
                    );
                    // Angle snapping sits with grid snapping: a click opens its steps.
                    let angle_snap = &mut self.angle_snap;
                    let tip = match *angle_snap {
                        Some(step) => {
                            format!("Angle snap: lines are drawn every {step}° (Alt places freely)")
                        }
                        None => "Angle snap: draw lines at every 1°, 5°, 15° or 45°".to_owned(),
                    };
                    let on = angle_snap.is_some();
                    let button = icons::button(ui, 22.0, on, "Angle snap", |p, r, c| {
                        icons::glyph(p, r, Glyph::Angle, c)
                    })
                    .on_hover_text(tip);
                    egui::Popup::menu(&button).show(|ui| {
                        ui.set_min_width(130.0);
                        if theme::menu_check(ui, angle_snap.is_none(), "Off", "").clicked() {
                            *angle_snap = None;
                        }
                        for step in crate::ANGLE_STEPS {
                            let picked = *angle_snap == Some(step);
                            if theme::menu_check(ui, picked, &format!("Every {step}°"), "").clicked() {
                                *angle_snap = Some(step);
                            }
                        }
                    });
                    let mut perp_tangent = self.perp_tangent_snap;
                    toggle(
                        ui,
                        Glyph::PerpTangent,
                        "Perpendicular & tangent",
                        &mut perp_tangent,
                        "Perpendicular & tangent: lines snap square onto lines, circles and arcs, \
                         or tangent to circles and arcs, and follow guides square to a line or \
                         along an arc from the end you draw on from",
                    );
                    if perp_tangent != self.perp_tangent_snap {
                        self.set_perp_tangent_snap(perp_tangent);
                    }
                    toggle(
                        ui,
                        Glyph::Snap,
                        "Snap to grid",
                        &mut self.grid_snap,
                        &format!("Snap to grid ({grid}){}", crate::commands::key_suffix("view.snap")),
                    );
                    self.snap_kinds_button(ui);
                    // The pointer's position, leftmost of the cluster: the first to go when
                    // the window is narrow, so the hint keeps its room.
                    if bar.width() >= 1000.0 {
                        if self.drawing.view.pointer.is_some() {
                            theme::bar_divider(ui);
                        }
                        self.pointer_readout(ui);
                    }
                    // Where the cluster's leftmost control starts.
                    ui.min_rect().left()
                },
            )
            .inner;
        let selecting = self.tool.kind() == ToolKind::Select && !self.tool.is_busy();
        // How to get at a picked shape's fill on its own, until it has been done a few times
        // (#223).
        let tool_hint = if selecting
            && self.drawing.editor.prefs.fill_alone_picks < crate::FILL_ALONE_HINTS
            && self.drawing.editor.picked_shape_fill().is_some()
        {
            format!(
                "Double-click inside to edit its fill on its own · {}",
                self.tool.hint()
            )
        } else if self.tool.kind() == ToolKind::Fill {
            // Which fill the next click makes, and that H switches it (#497).
            let other = match self.drawing.editor.prefs.fill_style.kind {
                drift_core::FillKind::Solid => "hatch",
                drift_core::FillKind::Hatch(_) => "solid",
            };
            format!(
                "{} · H: {other} · {}",
                self.fill_tool_mode(),
                self.tool.hint()
            )
        } else {
            self.tool.hint()
        };
        // The Selection filter, while it is narrowed, so a click that picks nothing isn't a mystery
        // (#496).
        let filter = self.drawing.editor.prefs.pick_filter;
        let tool_hint = if selecting && !filter.is_all() {
            format!(
                "{} · {tool_hint}",
                crate::app::pick_filter_ui::describe(filter)
            )
        } else {
            tool_hint
        };
        // Inside a block or group open for editing, a click outside it does nothing: only Esc
        // (or Finish editing) leaves.
        let tool_hint = match (
            &self.drawing.editor.block_edit,
            self.drawing.editor.edit_group(),
        ) {
            (Some(_), Some(_)) if selecting => {
                format!("Editing the block · Esc: finish editing · {tool_hint}")
            }
            (None, Some(_)) if selecting => {
                format!("Editing the group · Esc: finish editing · {tool_hint}")
            }
            _ => tool_hint,
        };
        let hint = if self.drawing.align_edge.is_some() {
            "Align to edge: click one of the shape's edges (the pattern runs along it from its \
             start) · Esc cancels"
                .to_owned()
        } else if self.drawing.add_view_on.is_some() && !self.tool.is_busy() {
            "Drag a rectangle round what the new view shows · Esc cancels".to_owned()
        } else if self.drawing.view.sheet_preview {
            "Drag a view to move it · Alt-drag the drawing inside it · its handles crop it · the \
             Model tab returns to the model"
                .to_owned()
        } else if let Some(f) = self.drawing.editor.hovered_fill.filter(|_| selecting) {
            // What the fill under the pointer is: a shape, or an area fill and why (#202).
            let kind = match self.drawing.editor.fill_kind(f) {
                Ok(()) => "Shape".to_owned(),
                Err(why) => format!("Area fill: {}", why.reason()),
            };
            format!("{kind} · {tool_hint}")
        } else {
            tool_hint
        };
        // The prompt, leftmost (as AutoCAD's command line), in whatever room is left before
        // the cluster (as wide as it actually is, so the hint never lies over its controls).
        let right = cluster_left - 14.0;
        if right - bar.left() > 40.0 {
            let area = egui::Rect::from_min_max(bar.left_top(), egui::pos2(right, bar.bottom()));
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(area)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&hint)
                                .size(theme::size::LABEL)
                                .color(ui.visuals().weak_text_color()),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&hint);
                },
            );
        }
    }
}

/// The marker on a redundant relation's row (#305): an equals sign, already implied.
fn redundant_marker(ui: &mut egui::Ui) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    let color = crate::theme::amber(ui.visuals().dark_mode);
    let c = rect.center();
    for dy in [-2.0, 2.0] {
        ui.painter().line_segment(
            [c + egui::vec2(-4.0, dy), c + egui::vec2(4.0, dy)],
            egui::Stroke::new(1.5, color),
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Redundant"));
    response.on_hover_text(
        "Redundant: already implied by the others. Deleting it leaves the sketch as it is.",
    );
}

#[cfg(test)]
mod pick_memo_tests {
    use super::*;
    use drift_core::{Constraint, DVec2, Document, EntityId, LineKind};

    /// The memo's answers are what finding them afresh gives.
    fn check(memo: &mut PickMemo, ed: &Editor, step: usize, what: &str) {
        assert_eq!(memo.relations(ed), relation_labels(ed), "{step}: {what}");
        assert_eq!(
            memo.constraints(ed),
            picked_constraints(ed),
            "{step}: {what}"
        );
        assert_eq!(memo.summary(ed), picked_summary(ed), "{step}: {what}");
        assert_eq!(
            memo.curve_layers(ed),
            picked_curve_layers(ed),
            "{step}: {what}"
        );
    }

    #[test]
    fn the_inspectors_kept_answers_are_found_afresh_ones() {
        // Rectangles with a length dimension driven by a variable, a hatch, a circle and
        // loose lines on two layers. Random picks, moves (and frames of one, carrying the
        // hatch), relations added, a variable and a dimension edited, restyles, layer
        // changes, undo and redo: after each, the memo answers as finding afresh does.
        let mut doc = Document::new();
        let mut lines: Vec<EntityId> = Vec::new();
        for k in 0..3 {
            let x = k as f64 * 40.0;
            let p = [(0.0, 0.0), (30.0, 0.0), (30.0, 20.0), (0.0, 20.0)]
                .map(|(a, b)| doc.add_point(DVec2::new(x + a, b)));
            for i in 0..4 {
                lines.push(doc.add_line(p[i], p[(i + 1) % 4]).unwrap());
            }
        }
        let c = doc.add_point(DVec2::new(200.0, 0.0));
        let circle = doc.add_circle(c, 8.0).unwrap();
        for k in 0..6 {
            let a = doc.add_point(DVec2::new(k as f64 * 7.0, 60.0));
            let b = doc.add_point(DVec2::new(k as f64 * 7.0 + 5.0, 70.0));
            lines.push(doc.add_line(a, b).unwrap());
        }
        let mut ed = Editor::new(doc);
        ed.sync();
        let other = ed.add_layer("Other");
        ed.prefs.fill_style.kind = drift_core::FillKind::Hatch(drift_core::Hatch::new("ANSI37"));
        ed.add_fill_at(DVec2::new(5.0, 5.0)).unwrap();
        ed.settle_areas();
        let width = ed
            .add_variable("W", "30", drift_core::units::Kind::Length)
            .unwrap();
        let (a, b) = match ed.doc.get(lines[0]).unwrap().kind {
            drift_core::EntityKind::Line { a, b } => (a, b),
            _ => unreachable!(),
        };
        let dim = ed
            .add_dimension(Constraint::Distance { a, b, value: 30.0 }, None)
            .unwrap();
        ed.set_dimension_text(dim, "W").unwrap();
        let mut seed: u64 = 0x1234_5678_9abc_def1;
        let mut rnd = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let all: Vec<EntityId> = ed.doc.iter().map(|(id, _)| id).collect();
        let hatch = ed.doc.fills().next().map(|(id, _)| id).unwrap();
        assert!(matches!(
            ed.doc.fill(hatch).unwrap().kind,
            drift_core::FillKind::Hatch(_)
        ));
        let mut kept_through_moves = 0;
        let mut kept_through_hatched_moves = 0;
        let mut memo = PickMemo::default();
        check(&mut memo, &ed, 0, "at the start");
        for step in 1..300 {
            let what = match rnd(12) {
                0..=2 => "picks",
                3 => "a move",
                4 => "frames of a move",
                5 => "a relation added",
                6 => "the variable",
                7 => "the dimension",
                8 => "a restyle",
                9 => "a layer",
                10 => "undo",
                _ => "redo",
            };
            match what {
                "picks" => {
                    let n = rnd(8);
                    let mut picks = std::collections::BTreeSet::new();
                    for _ in 0..n {
                        picks.insert(all[rnd(all.len())]);
                    }
                    if rnd(4) == 0 {
                        picks.insert(circle);
                    }
                    ed.selection = picks;
                    ed.selected_midpoints = if rnd(5) == 0 {
                        [lines[rnd(lines.len())]].into()
                    } else {
                        Default::default()
                    };
                }
                "a move" => {
                    let _ = ed.nudge(DVec2::new(rnd(5) as f64, 1.0));
                }
                "frames of a move" => {
                    // The hatched rectangle, or the loose lines.
                    let hatched = rnd(2) == 0;
                    let ids: Vec<EntityId> = if hatched {
                        lines[..4].to_vec()
                    } else {
                        lines[12..].to_vec()
                    };
                    let dragged: Vec<drift_solver::Param> = ed
                        .doc
                        .defining_points(ids.iter().copied())
                        .iter()
                        .copied()
                        .flat_map(drift_solver::Param::point)
                        .collect();
                    ed.doc.begin();
                    check(&mut memo, &ed, step, "a move begun");
                    let made = memo.made;
                    let origin = |ed: &Editor| match &ed.doc.fill(hatch).unwrap().kind {
                        drift_core::FillKind::Hatch(h) => h.origin,
                        _ => unreachable!(),
                    };
                    let was = origin(&ed);
                    for k in 1..4 {
                        ed.drag(&ids, DVec2::new(k as f64, 0.5), &dragged);
                        check(&mut memo, &ed, step, "a frame of a move");
                    }
                    // Kept through the move, a hatch carried along (its fill rewritten every
                    // frame) or not.
                    assert_eq!(memo.made, made, "{step}: found again mid-move");
                    if hatched {
                        // (The rectangle may have been reshaped by a relation: carried only
                        // when its outline moved as one.)
                        if origin(&ed) != was {
                            kept_through_hatched_moves += 1;
                        }
                    } else {
                        kept_through_moves += 1;
                    }
                    ed.commit("Move");
                }
                "a relation added" => {
                    // Not on the hatched rectangle (lines 0..4): a relation that reshapes it
                    // decides whether later moves carry the hatch, and where a 300-step walk
                    // of solves ends that up is round-off, not the memo (Windows and macOS
                    // parted by 0.46 mm by step 171 on main), so the count of hatched moves
                    // below would hang on the platform.
                    let rest = &lines[4..];
                    let (a, b) = (rest[rnd(rest.len())], rest[rnd(rest.len())]);
                    if a != b {
                        let _ = ed.add_constraints("Parallel", &[Constraint::Parallel { a, b }]);
                    }
                }
                "the variable" => {
                    let _ = ed.set_variable(width, &format!("{}", 25 + rnd(10)));
                }
                "the dimension" => {
                    let _ = ed.set_dimension_text(dim, &format!("{}", 26 + rnd(8)));
                }
                "a restyle" => {
                    let id = lines[rnd(lines.len())];
                    ed.doc.begin();
                    ed.doc.set_style(
                        id,
                        if rnd(2) == 0 {
                            LineKind::Construction
                        } else {
                            LineKind::Solid
                        },
                    );
                    ed.commit("Style");
                }
                "a layer" => {
                    let id = lines[rnd(lines.len())];
                    ed.doc.begin();
                    let _ = ed.doc.set_entity_layer(id, other);
                    ed.commit("Layer");
                }
                "undo" => {
                    ed.undo();
                }
                _ => {
                    ed.redo();
                }
            }
            ed.settle_areas();
            check(&mut memo, &ed, step, what);
        }
        assert!(kept_through_moves > 5);
        assert!(
            kept_through_hatched_moves > 5,
            "hatch carried in {kept_through_hatched_moves} moves"
        );
    }
}
