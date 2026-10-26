//! The command palette (Ctrl/⌘+K): type a few letters of any tool, view setting, panel or
//! command and press Return. Arrow keys move, Esc closes; recent commands come first. The
//! entries are the command registry's ([`crate::commands`]), plus the drawing's options.

use crate::commands::{self, Icon, Spec, registry};
pub(crate) use crate::commands::{Command, ViewFlag};
use crate::dock::Panel;
use crate::icons::{self, Glyph};
use crate::*;

/// The palette's state while open.
#[derive(Default)]
pub(crate) struct Palette {
    query: String,
    /// The highlighted row.
    selected: usize,
    /// Focus the field on the first frame.
    fresh: bool,
}

/// A row in the list.
pub(crate) struct Entry {
    pub(crate) command: Command,
    pub(crate) title: String,
    group: &'static str,
    keys: String,
    /// The registry's entry (`None` for an option of the drawing).
    spec: Option<&'static Spec>,
    /// Whether it can run now; if not, why.
    enabled: bool,
    reason: &'static str,
    /// For toggles: whether it's on.
    on: Option<bool>,
}

impl DriftApp {
    pub(crate) fn open_palette(&mut self) {
        self.palette = Some(Palette {
            fresh: true,
            ..Default::default()
        });
    }

    fn flag(&mut self, f: ViewFlag) -> &mut bool {
        match f {
            ViewFlag::Grid => &mut self.show_grid,
            ViewFlag::Axes => &mut self.show_axes,
            ViewFlag::Snap => &mut self.grid_snap,
            ViewFlag::PerpTangent => &mut self.perp_tangent_snap,
            ViewFlag::Thin => &mut self.thin_lines,
            ViewFlag::Status => &mut self.status_colors,
            ViewFlag::Endpoints => &mut self.always_endpoints,
            ViewFlag::Dimensions => &mut self.show_dimensions,
            ViewFlag::Relations => &mut self.show_relations,
            ViewFlag::Hidden => &mut self.show_hidden_annotations,
            ViewFlag::Free => &mut self.params.show_free,
            ViewFlag::Combs => &mut self.show_combs,
            ViewFlag::Crops => &mut self.show_view_crops,
            ViewFlag::Centrelines => &mut self.show_wall_centrelines,
            ViewFlag::AutoRelations => &mut self.auto_relations,
        }
    }

    /// For a setting: whether it's on (`None` for a command that isn't a setting).
    pub(crate) fn command_state(&mut self, command: Command) -> Option<bool> {
        Some(match command {
            Command::Toggle(f) => *self.flag(f),
            Command::Panel(p) => self.layout.is_shown(p) && !self.dock_view.hide_all,
            Command::HideAllPanels => self.dock_view.hide_all,
            Command::SheetPreview => self.sheet_preview,
            _ => return None,
        })
    }

    /// A command's title as things stand: the counts of what it clears.
    fn command_title(&self, spec: &Spec, menu: bool) -> String {
        let base = if menu { spec.menu } else { spec.title };
        let count = match spec.command {
            Command::ClearRelations => self.editor.selection_constraints().0.len(),
            Command::ClearDimensions => self.editor.selection_constraints().1.len(),
            _ => 0,
        };
        let history = &self.editor.history;
        match spec.command {
            _ if count > 0 => format!("{base} ({count})"),
            Command::Undo if menu => history
                .undo_label()
                .map_or_else(|| base.to_owned(), |l| format!("Undo {l}")),
            Command::Redo if menu => history
                .redo_label()
                .map_or_else(|| base.to_owned(), |l| format!("Redo {l}")),
            _ => base.to_owned(),
        }
    }

    /// A menu's label for a command.
    pub(crate) fn menu_label(&self, spec: &Spec) -> String {
        self.command_title(spec, true)
    }

    /// Everything the palette offers, as things stand: every command in the registry (the
    /// tools first, in toolbar order), then the drawing's options.
    pub(crate) fn palette_entries(&mut self) -> Vec<Entry> {
        let mac = cfg!(target_os = "macos");
        let mut out = Vec::new();
        for spec in registry() {
            if spec.when.hidden() {
                continue;
            }
            let enabled = spec.when.holds(self);
            out.push(Entry {
                command: spec.command,
                title: self.command_title(spec, false),
                group: spec.category.label(),
                keys: spec.key_text(mac),
                spec: Some(spec),
                enabled,
                reason: spec.when.reason(),
                on: self.command_state(spec.command),
            });
        }
        let toolbar = |e: &Entry| match e.command {
            Command::Tool(kind) => ToolKind::ALL.iter().position(|k| *k == kind),
            _ => None,
        };
        out.sort_by_key(|e| toolbar(e).unwrap_or(usize::MAX));
        let options: Vec<_> = self
            .editor
            .doc
            .variants()
            .map(|(id, v)| (id, v.name.clone()))
            .collect();
        for (id, name) in options {
            let ghosted = self.params.ghosted.contains(&id);
            for (command, title, on) in [
                (
                    Command::ApplyOption(id),
                    format!("Apply option {name}"),
                    None,
                ),
                (
                    Command::GhostOption(id),
                    format!("Ghost of option {name}"),
                    Some(ghosted),
                ),
            ] {
                out.push(Entry {
                    command,
                    title,
                    group: "Options",
                    keys: String::new(),
                    spec: None,
                    enabled: true,
                    reason: "",
                    on,
                });
            }
        }
        out
    }

    /// Entries matching the query, best first (recent ones first when it's empty). What can't
    /// run now comes after what can.
    pub(crate) fn palette_matches(&mut self, query: &str) -> Vec<Entry> {
        let mut scored: Vec<(i32, usize, Entry)> = self
            .palette_entries()
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let recent = self.recent_commands.iter().position(|c| *c == e.command);
                let best = match e.spec {
                    Some(spec) => commands::rank(query, spec),
                    None => commands::score(query, &e.title)
                        .max(commands::score(query, e.group).map(|s| s / 3)),
                }?;
                let bonus = recent.map_or(0, |r| 400 - 20 * r as i32);
                let disabled = if e.enabled { 0 } else { 500 };
                Some((best + bonus - disabled, i, e))
            })
            .collect();
        // With no query: recent commands, then the tools in toolbar order.
        scored.sort_by_key(|(s, i, _)| (-*s, *i));
        scored.into_iter().map(|(_, _, e)| e).collect()
    }

    /// Do a command (from a menu, the palette, a key or Enter to repeat it).
    pub(crate) fn run_command(&mut self, ctx: &egui::Context, command: Command) {
        if commands::spec(command).is_some_and(|s| s.repeatable) {
            self.last_command = Some(command);
        }
        let painter = ctx.layer_painter(egui::LayerId::background());
        let now = ctx.input(|i| i.time);
        match command {
            Command::Tool(kind) => self.switch_tool(kind),
            Command::Toggle(f) => {
                let v = self.flag(f);
                *v = !*v;
                if f == ViewFlag::AutoRelations {
                    let text = if self.auto_relations {
                        "Auto relations on"
                    } else {
                        "Auto relations off: plain geometry, nothing added for you"
                    };
                    self.set_status(text.into(), false, now);
                }
            }
            Command::Panel(p) => self.toggle_panel(p),
            Command::HideAllPanels => self.dock_view.hide_all = !self.dock_view.hide_all,
            Command::ResetPanels => {
                self.layout = Default::default();
                self.dock_view.hide_all = false;
            }
            Command::New => self.request(Pending::New),
            Command::Open => self.request(Pending::Open),
            Command::OpenExample => self.request(Pending::Example),
            Command::ShowStart => self.show_start = true,
            Command::Save => self.save(),
            Command::SaveAs => self.save_as(),
            Command::Import => self.pick_import_file(false),
            Command::ExportPdf => self.export_pdf(&painter),
            Command::ExportSvg => self.export_svg(&painter),
            Command::ExportDxf => self.export_dxf(&painter),
            Command::ExportProfiles => self.export_profiles(),
            Command::ExportModelSvg => self.export_model(&painter, false),
            Command::ExportModelPdf => self.export_model(&painter, true),
            Command::Print => self.print(&painter),
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::SelectAll => {
                self.set_tool(ToolKind::Select);
                self.editor.select_all();
            }
            Command::SelectOverlapping => self.select_overlapping(now),
            Command::CheckSketch => self.open_sketch_check(),
            Command::PickFilter(f) => {
                if self.tool.kind() != ToolKind::Select {
                    self.set_tool(ToolKind::Select);
                }
                self.set_pick_filter(f);
            }
            Command::ClearRelations => self.clear_selection_constraints(false, now),
            Command::ClearDimensions => self.clear_selection_constraints(true, now),
            Command::AlignDimensions => self.align_picked_dimensions(false, now),
            Command::SpaceDimensions => self.align_picked_dimensions(true, now),
            Command::HideAnnotations => {
                let refs = self.picked_annotations();
                self.set_annotations_shown(&refs, Some(false), None, now);
            }
            Command::AnnotationsOffSheets => {
                let refs = self.picked_annotations();
                self.set_annotations_shown(&refs, None, Some(false), now);
            }
            Command::Cut => self.cut(ctx),
            Command::Copy => self.copy(ctx),
            Command::Paste => self.paste_copied(false, now),
            Command::PasteInPlace => self.paste_copied(true, now),
            Command::Delete => self.delete_picked(),
            Command::Duplicate => self.duplicate_and_grab(),
            Command::Group => {
                self.editor.make_group();
            }
            Command::Arrange(how) => self.arrange(how, now),
            Command::Rename => self.start_rename(),
            Command::Ungroup => {
                self.editor.ungroup();
            }
            Command::MakeBlock => self.start_make_block(""),
            Command::Explode => self.editor.explode_selected(),
            Command::BlockFromGroup => {
                if let Some(g) = self.editor.convertible_group() {
                    self.block_from_group(g, now);
                }
            }
            Command::ExplodeToGroup => {
                self.editor.explode_to_groups();
            }
            Command::WallsFromCurves => {
                let picked: Vec<_> = self.editor.selection.iter().copied().collect();
                match self.editor.walls_from_curves(&picked) {
                    Ok(made) => {
                        let n = made.len();
                        let what = if n == 1 { "a wall" } else { "walls" };
                        self.set_status(format!("Made {what} along the curves"), false, now);
                    }
                    Err(why) => self.editor.notice = Some(why),
                }
            }
            Command::ExplodeWalls => {
                let walls: Vec<_> = self.editor.selected_walls().into_iter().collect();
                self.editor.explode_walls(&walls);
            }
            Command::JoinWalls => {
                let walls: Vec<_> = self.editor.selected_walls().into_iter().collect();
                self.editor.join_walls(&walls);
            }
            Command::UnjoinWalls => {
                let walls: Vec<_> = self.editor.selected_walls().into_iter().collect();
                self.editor.unjoin_walls(&walls);
            }
            Command::Move => self.start_transform(ToolKey::G, now),
            Command::Rotate => self.start_transform(ToolKey::R, now),
            Command::Scale => {
                if self.tool.kind() != ToolKind::Select {
                    self.set_tool(ToolKind::Select);
                }
                let text = "Scale: drag a handle of the selection's box · Shift keeps the \
                            shape, Ctrl scales about the middle";
                self.set_status(text.into(), false, now);
            }
            Command::Pattern(_) => {
                self.patterns.reveal = true;
                self.show_panel(Panel::Inspector);
            }
            Command::SaveOption(whole) => {
                let n = self.editor.doc.variants().count() + 1;
                self.editor.save_option(&format!("Option {n}"), whole);
                self.show_panel(Panel::Parameters);
            }
            Command::ApplyOption(id) => {
                if let Err(e) = self.editor.apply_variant(id) {
                    self.set_status(format!("Can't apply it: {e}"), true, now);
                }
            }
            Command::GhostOption(id) => {
                if !self.params.ghosted.remove(&id) {
                    self.params.ghosted.insert(id);
                }
            }
            Command::Construction => self.toggle_construction(now),
            Command::ZoomToFit => self.fit_target = Some(self.fit_bounds()),
            Command::ZoomToSelection => {
                self.zoom_to_selection();
            }
            Command::ZoomLifeSize => self.zoom_to_percent(100.0),
            Command::SheetPreview => {
                if self.current_sheet().is_none() {
                    self.add_sheet();
                }
                self.sheet_preview = !self.sheet_preview;
                self.preview.zoom = 0.0;
            }
            Command::Palette => self.open_palette(),
            Command::Shortcuts => self.show_shortcuts = true,
            Command::Preferences => self.show_settings = true,
            Command::DrawingSettings => self.show_drawing_settings = true,
            Command::About => self.show_about = true,
            Command::UserGuide => ctx.open_url(egui::OpenUrl::new_tab(crate::links::GUIDE)),
            Command::ReportProblem => ctx.open_url(egui::OpenUrl::new_tab(crate::links::REPORT)),
            Command::Benchmark => self.benchmark = Some(run_benchmark()),
            Command::Theme(pref) => ctx.set_theme(pref),
        }
    }

    /// Enter or Space with nothing in hand: the last command or tool again, if it can run.
    pub(crate) fn repeat_last_command(&mut self, ctx: &egui::Context) -> bool {
        let Some(command) = self.last_command else {
            return false;
        };
        if let Some(spec) = commands::spec(command)
            && !spec.when.holds(self)
        {
            let now = ctx.input(|i| i.time);
            self.set_status(
                format!("{}: {}", spec.title, spec.when.reason()),
                false,
                now,
            );
            return true;
        }
        self.run_command(ctx, command);
        true
    }

    /// Move (G) or rotate (R) what is picked, as the keys do.
    fn start_transform(&mut self, key: ToolKey, now: f64) {
        if self.tool.kind() != ToolKind::Select {
            self.set_tool(ToolKind::Select);
        }
        // From a menu or the palette the pointer is off the canvas: start where it last was.
        let pointer = self.pointer.or(self.last_pointer);
        if !self.tool.key(key, pointer.as_ref(), &mut self.editor) {
            let (what, letter) = if key == ToolKey::G {
                ("Move", "G")
            } else {
                ("Rotate", "R")
            };
            let text = format!("{what}: point at the canvas and press {letter}");
            self.set_status(text, false, now);
        }
    }

    /// The palette, when open: a search field over a list of commands, near the top of the
    /// window.
    pub(crate) fn palette_ui(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.palette.take() else {
            return;
        };
        // Keys the list handles, taken before the text field sees them.
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if escape {
            return;
        }
        let entries = self.palette_matches(state.query.trim());
        let shown = entries.len().min(12);
        if down && shown > 0 {
            state.selected = (state.selected + 1) % shown;
        }
        if up && shown > 0 {
            state.selected = (state.selected + shown - 1) % shown;
        }
        state.selected = state.selected.min(shown.saturating_sub(1));
        let mut chosen = (enter && shown > 0).then_some(state.selected);
        let mut close = false;

        let content = ctx.content_rect();
        // A light scrim; clicking it closes the palette.
        let scrim = egui::Area::new(egui::Id::new("palette_scrim"))
            .order(egui::Order::Foreground)
            .fixed_pos(content.min)
            .show(ctx, |ui| {
                ui.allocate_exact_size(content.size(), egui::Sense::click())
                    .1
            });
        ctx.layer_painter(scrim.response.layer_id).rect_filled(
            content,
            0.0,
            egui::Color32::from_black_alpha(90),
        );
        if scrim.inner.clicked() {
            close = true;
        }
        let width = 540.0_f32.min(content.width() - 32.0);
        let dark = ctx.theme() == egui::Theme::Dark;
        let accent = theme::accent(dark);
        egui::Area::new(egui::Id::new("command_palette"))
            .order(egui::Order::Tooltip)
            .fixed_pos(egui::pos2(
                content.center().x - width / 2.0,
                content.top() + 72.0,
            ))
            .show(ctx, |ui| {
                let style = ui.style().clone();
                egui::Frame::popup(&style)
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(width - 16.0);
                        ui.horizontal(|ui| {
                            let (r, _) = ui
                                .allocate_exact_size(egui::vec2(22.0, 28.0), egui::Sense::hover());
                            icons::glyph(
                                ui.painter(),
                                egui::Rect::from_center_size(r.center(), egui::Vec2::splat(15.0)),
                                Glyph::Search,
                                ui.visuals().weak_text_color(),
                            );
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut state.query)
                                    .id(egui::Id::new("palette_query"))
                                    .hint_text("Type a tool, setting, panel or command…")
                                    .frame(egui::Frame::NONE)
                                    .font(egui::FontId::proportional(15.0))
                                    .desired_width(f32::INFINITY),
                            );
                            if state.fresh {
                                field.request_focus();
                                state.fresh = false;
                            }
                            if field.changed() {
                                state.selected = 0;
                            }
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        if entries.is_empty() {
                            ui.add_space(8.0);
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("Nothing matches").weak());
                            });
                            ui.add_space(8.0);
                        }
                        for (k, e) in entries.iter().take(shown).enumerate() {
                            let (r, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 30.0),
                                egui::Sense::click(),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    e.enabled,
                                    &e.title,
                                )
                            });
                            if response.hovered()
                                && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO)
                            {
                                state.selected = k;
                            }
                            let selected = k == state.selected;
                            let painter = ui.painter();
                            let (fg, weak) = if selected {
                                painter.rect_filled(r, 7.0, accent);
                                (egui::Color32::WHITE, egui::Color32::from_white_alpha(180))
                            } else {
                                (ui.visuals().text_color(), ui.visuals().weak_text_color())
                            };
                            // What can't run now is dimmed, and says why on the right.
                            let fg = if e.enabled { fg } else { weak };
                            let icon = egui::Rect::from_center_size(
                                egui::pos2(r.left() + 18.0, r.center().y),
                                egui::Vec2::splat(16.0),
                            );
                            match e.spec.and_then(|s| s.icon) {
                                Some(Icon::Tool(kind)) => icons::tool(painter, icon, kind, fg),
                                Some(Icon::Glyph(g)) => icons::glyph(painter, icon, g, fg),
                                None => {
                                    if let Some(on) = e.on {
                                        // A check box for settings.
                                        let b = icon.shrink(2.0);
                                        painter.rect_stroke(
                                            b,
                                            3.0,
                                            egui::Stroke::new(1.2, weak),
                                            egui::StrokeKind::Inside,
                                        );
                                        if on {
                                            painter.add(egui::Shape::line(
                                                vec![
                                                    b.left_center() + egui::vec2(2.5, 0.0),
                                                    b.center_bottom() + egui::vec2(-1.0, -3.0),
                                                    b.right_top() + egui::vec2(-2.5, 3.0),
                                                ],
                                                egui::Stroke::new(1.6, fg),
                                            ));
                                        }
                                    }
                                }
                            }
                            painter.text(
                                egui::pos2(r.left() + 36.0, r.center().y),
                                egui::Align2::LEFT_CENTER,
                                &e.title,
                                egui::FontId::proportional(13.5),
                                fg,
                            );
                            let mut right = r.right() - 10.0;
                            if !e.keys.is_empty() {
                                let g = painter.layout_no_wrap(
                                    e.keys.clone(),
                                    egui::FontId::proportional(theme::size::CAPTION),
                                    weak,
                                );
                                right -= g.size().x;
                                painter.galley(
                                    egui::pos2(right, r.center().y - g.size().y / 2.0),
                                    g,
                                    weak,
                                );
                                right -= 12.0;
                            }
                            let group = if e.enabled { e.group } else { e.reason };
                            painter.text(
                                egui::pos2(right, r.center().y),
                                egui::Align2::RIGHT_CENTER,
                                group,
                                egui::FontId::proportional(11.5),
                                weak,
                            );
                            if response.clicked() {
                                chosen = Some(k);
                            }
                        }
                    });
            });
        if let Some(e) = chosen.and_then(|k| entries.get(k)) {
            if e.enabled {
                self.recent_commands.retain(|c| *c != e.command);
                self.recent_commands.insert(0, e.command);
                self.recent_commands.truncate(6);
                self.run_command(ctx, e.command);
            } else {
                let now = ctx.input(|i| i.time);
                self.set_status(format!("{}: {}", e.title, e.reason), false, now);
            }
            return;
        }
        if !close {
            self.palette = Some(state);
        }
    }
}
