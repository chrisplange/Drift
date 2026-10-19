//! The command palette (Ctrl/⌘+K): type a few letters of any tool, view setting, panel or
//! command and press Return. Arrow keys move, Esc closes; recent commands come first.

use crate::dock::Panel;
use crate::icons::{self, Glyph};
use crate::*;

/// Something the palette can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Tool(ToolKind),
    Toggle(ViewFlag),
    Panel(Panel),
    HideAllPanels,
    ResetPanels,
    New,
    Open,
    OpenExample,
    Save,
    SaveAs,
    ImportDxf,
    ImportUnderlay,
    ExportPdf,
    ExportSvg,
    ExportDxf,
    Print,
    Undo,
    Redo,
    SelectAll,
    SelectOverlapping,
    CheckSketch,
    HideAnnotations,
    AnnotationsOffSheets,
    AlignDimensions,
    SpaceDimensions,
    ClearRelations,
    ClearDimensions,
    PickFilter(crate::editor::PickFilter),
    Cut,
    Copy,
    Paste,
    PasteInPlace,
    Delete,
    Duplicate,
    Group,
    Ungroup,
    MakeBlock,
    Explode,
    Construction,
    ZoomToFit,
    ZoomToSelection,
    ZoomLifeSize,
    SheetPreview,
    Shortcuts,
    Preferences,
    DrawingSettings,
    About,
    Theme(egui::ThemePreference),
}

/// On/off view settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewFlag {
    Grid,
    Axes,
    Snap,
    PerpTangent,
    Thin,
    Status,
    Endpoints,
    Dimensions,
    Relations,
    Hidden,
    Free,
    Ghosts,
    Combs,
    Crops,
    AutoRelations,
}

impl ViewFlag {
    const ALL: [ViewFlag; 15] = [
        ViewFlag::Grid,
        ViewFlag::Axes,
        ViewFlag::Snap,
        ViewFlag::PerpTangent,
        ViewFlag::Thin,
        ViewFlag::Status,
        ViewFlag::Endpoints,
        ViewFlag::Dimensions,
        ViewFlag::Relations,
        ViewFlag::Hidden,
        ViewFlag::Free,
        ViewFlag::Ghosts,
        ViewFlag::Combs,
        ViewFlag::Crops,
        ViewFlag::AutoRelations,
    ];

    fn label(self) -> &'static str {
        match self {
            ViewFlag::Grid => "Grid",
            ViewFlag::Axes => "Origin axes",
            ViewFlag::Snap => "Snap to grid",
            ViewFlag::PerpTangent => "Perpendicular and tangent snaps",
            ViewFlag::Thin => "Thin lines",
            ViewFlag::Status => "Constraint status colours",
            ViewFlag::Endpoints => "Always show end points",
            ViewFlag::Dimensions => "Dimensions and notes",
            ViewFlag::Relations => "Relations",
            ViewFlag::Hidden => "Hidden annotations (faded)",
            ViewFlag::Free => "Free directions",
            ViewFlag::Ghosts => "Variant ghosts",
            ViewFlag::Combs => "Curvature combs",
            ViewFlag::Crops => "View crops",
            ViewFlag::AutoRelations => "Automatic relations",
        }
    }

    fn keys(self) -> String {
        match self {
            ViewFlag::Grid => app::panels::shortcut("'", false),
            ViewFlag::Snap => "Shift+G".into(),
            ViewFlag::Thin => "W".into(),
            ViewFlag::Status => "Shift+C".into(),
            ViewFlag::Dimensions => "Alt+D".into(),
            ViewFlag::Relations => "Alt+R".into(),
            ViewFlag::Hidden => "Alt+H".into(),
            ViewFlag::AutoRelations => "Shift+Alt+R".into(),
            ViewFlag::Endpoints => "Shift+Alt+V".into(),
            ViewFlag::Crops => "Shift+Alt+B".into(),
            _ => String::new(),
        }
    }
}

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
struct Entry {
    command: Command,
    title: String,
    group: &'static str,
    keys: String,
    /// Extra words to match (a tool's description).
    words: &'static str,
    /// For toggles: whether it's on.
    on: Option<bool>,
}

/// How well `query` matches `text`: whole-word and prefix matches first, then substrings,
/// then letters in order. `None` when it doesn't match.
fn score(query: &str, text: &str) -> Option<i32> {
    let q = query.to_lowercase();
    let t = text.to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    if t.starts_with(&q) {
        return Some(1000 - t.len() as i32);
    }
    if let Some(at) = t.find(&q) {
        let word_start = at == 0 || !t.as_bytes()[at - 1].is_ascii_alphanumeric();
        return Some(if word_start { 800 } else { 500 } - at as i32);
    }
    // Letters in order, preferring those that start words.
    let mut chars = t.char_indices().peekable();
    let mut total = 0;
    let mut prev = ' ';
    for qc in q.chars() {
        loop {
            let (_, c) = chars.next()?;
            let starts = !prev.is_alphanumeric();
            prev = c;
            if c == qc {
                total += if starts { 12 } else { 2 };
                break;
            }
        }
    }
    Some(100 + total - t.len() as i32 / 4)
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
            ViewFlag::Ghosts => &mut self.params.show_ghosts,
            ViewFlag::Combs => &mut self.show_combs,
            ViewFlag::Crops => &mut self.show_view_crops,
            ViewFlag::AutoRelations => &mut self.auto_relations,
        }
    }

    /// Everything the palette offers, as things stand.
    fn palette_entries(&mut self) -> Vec<Entry> {
        let mut out = Vec::new();
        let mut add = |command, title: &str, group, keys: String| {
            out.push(Entry {
                command,
                title: title.to_owned(),
                group,
                keys,
                words: "",
                on: None,
            });
        };
        let ctrl = |k: &str| app::panels::shortcut(k, false);
        let ctrl_shift = |k: &str| app::panels::shortcut(k, true);
        add(Command::New, "New sketch", "File", ctrl("N"));
        add(Command::Open, "Open…", "File", ctrl("O"));
        add(Command::OpenExample, "Open example", "File", String::new());
        add(Command::Save, "Save", "File", ctrl("S"));
        add(Command::SaveAs, "Save as…", "File", ctrl_shift("S"));
        add(
            Command::ImportDxf,
            "Import DXF, SVG, PDF or Illustrator…",
            "File",
            String::new(),
        );
        add(
            Command::ImportUnderlay,
            "Import underlay…",
            "File",
            String::new(),
        );
        add(Command::ExportPdf, "Export PDF…", "File", String::new());
        add(Command::ExportSvg, "Export SVG…", "File", String::new());
        add(Command::ExportDxf, "Export DXF…", "File", String::new());
        add(Command::Print, "Print…", "File", ctrl("P"));
        add(Command::Undo, "Undo", "Edit", ctrl("Z"));
        add(Command::Redo, "Redo", "Edit", ctrl_shift("Z"));
        add(Command::SelectAll, "Select all", "Edit", ctrl("A"));
        add(
            Command::SelectOverlapping,
            "Select overlapping curves",
            "Edit",
            String::new(),
        );
        add(
            Command::CheckSketch,
            "Check sketch (gaps, short ends)…",
            "Edit",
            String::new(),
        );
        let (on_relations, on_dims) = self.editor.selection_constraints();
        if !on_relations.is_empty() {
            add(
                Command::ClearRelations,
                &format!("Clear relations on the selection ({})", on_relations.len()),
                "Edit",
                String::new(),
            );
        }
        if !on_dims.is_empty() {
            add(
                Command::ClearDimensions,
                &format!("Clear dimensions on the selection ({})", on_dims.len()),
                "Edit",
                String::new(),
            );
        }
        for (f, title) in [
            (crate::editor::PickFilter::Edges, "Select edges only"),
            (crate::editor::PickFilter::Points, "Select points only"),
            (
                crate::editor::PickFilter::Annotations,
                "Select annotations only",
            ),
            (crate::editor::PickFilter::Text, "Select text only"),
            (
                crate::editor::PickFilter::All,
                "Select anything (no filter)",
            ),
        ] {
            add(Command::PickFilter(f), title, "Edit", String::new());
        }
        let (align, space) = self.can_align_dimensions();
        if align {
            add(
                Command::AlignDimensions,
                "Align picked dimensions",
                "Edit",
                String::new(),
            );
        }
        if space {
            add(
                Command::SpaceDimensions,
                "Space picked dimensions evenly",
                "Edit",
                String::new(),
            );
        }
        if !self.picked_annotations().is_empty() {
            add(
                Command::HideAnnotations,
                "Hide picked dimensions and notes on the canvas",
                "Edit",
                String::new(),
            );
            add(
                Command::AnnotationsOffSheets,
                "Leave picked dimensions and notes off sheets",
                "Edit",
                String::new(),
            );
        }
        add(Command::Cut, "Cut", "Edit", ctrl("X"));
        add(Command::Copy, "Copy", "Edit", ctrl("C"));
        add(Command::Paste, "Paste", "Edit", ctrl("V"));
        add(
            Command::PasteInPlace,
            "Paste in place",
            "Edit",
            ctrl_shift("V"),
        );
        add(Command::Delete, "Delete selection", "Edit", "Del".into());
        add(Command::Duplicate, "Duplicate", "Edit", "Shift+D".into());
        add(Command::Group, "Group", "Edit", ctrl("G"));
        add(Command::Ungroup, "Ungroup", "Edit", ctrl_shift("G"));
        add(Command::MakeBlock, "Make block", "Edit", String::new());
        add(
            Command::Explode,
            "Explode block copies",
            "Edit",
            String::new(),
        );
        add(
            Command::Construction,
            "Toggle construction",
            "Edit",
            "Alt+C".into(),
        );
        add(Command::ZoomToFit, "Zoom to fit", "View", "Shift+F".into());
        add(
            Command::ZoomToSelection,
            "Zoom to selection",
            "View",
            "Shift+Z".into(),
        );
        add(
            Command::ZoomLifeSize,
            "Zoom to 100% (life size)",
            "View",
            String::new(),
        );
        add(
            Command::SheetPreview,
            "Sheet preview",
            "View",
            String::new(),
        );
        add(
            Command::HideAllPanels,
            "Hide all panels",
            "Panels",
            ctrl("\\"),
        );
        add(
            Command::ResetPanels,
            "Reset panel layout",
            "Panels",
            String::new(),
        );
        add(Command::Shortcuts, "Keyboard shortcuts", "Help", "?".into());
        add(Command::Preferences, "Preferences…", "Edit", ctrl(","));
        add(
            Command::DrawingSettings,
            "Drawing settings…",
            "File",
            String::new(),
        );
        add(Command::About, "About Drift", "Help", String::new());
        for (pref, label) in [
            (egui::ThemePreference::Dark, "Dark appearance"),
            (egui::ThemePreference::Light, "Light appearance"),
            (
                egui::ThemePreference::System,
                "Appearance follows the system",
            ),
        ] {
            add(Command::Theme(pref), label, "View", String::new());
        }
        for p in Panel::ALL {
            let shown = self.layout.is_shown(p) && !self.dock_view.hide_all;
            out.push(Entry {
                command: Command::Panel(p),
                title: format!("{} panel", p.title()),
                group: "Panels",
                keys: if p == Panel::Layers {
                    ctrl("L")
                } else {
                    String::new()
                },
                words: p.description(),
                on: Some(shown),
            });
        }
        for f in ViewFlag::ALL {
            let on = *self.flag(f);
            out.push(Entry {
                command: Command::Toggle(f),
                title: f.label().to_owned(),
                group: "View",
                keys: f.keys(),
                words: "",
                on: Some(on),
            });
        }
        for kind in ToolKind::ALL {
            if matches!(kind, ToolKind::Calibrate | ToolKind::MoveUnderlay) {
                continue;
            }
            out.insert(
                0,
                Entry {
                    command: Command::Tool(kind),
                    title: kind.label().to_owned(),
                    group: "Tool",
                    keys: kind.shortcut_label().unwrap_or_default(),
                    words: kind.description(),
                    on: None,
                },
            );
        }
        out
    }

    /// Entries matching the query, best first (recent ones first when it's empty).
    fn palette_matches(&mut self, query: &str) -> Vec<Entry> {
        let mut scored: Vec<(i32, usize, Entry)> = self
            .palette_entries()
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let recent = self.recent_commands.iter().position(|c| *c == e.command);
                let title = score(query, &e.title);
                let group = score(query, e.group).map(|s| s / 3);
                let in_words =
                    query.len() >= 3 && e.words.to_lowercase().contains(&query.to_lowercase());
                let words = in_words.then_some(150);
                let best = title.max(group).max(words)?;
                let bonus = recent.map_or(0, |r| 400 - 20 * r as i32);
                Some((best + bonus, i, e))
            })
            .collect();
        // With no query: recent commands, then the tools in toolbar order.
        scored.sort_by_key(|(s, i, _)| (-*s, *i));
        scored.into_iter().map(|(_, _, e)| e).collect()
    }

    /// Do a command.
    pub(crate) fn run_command(&mut self, ctx: &egui::Context, command: Command) {
        self.recent_commands.retain(|c| *c != command);
        self.recent_commands.insert(0, command);
        self.recent_commands.truncate(6);
        let painter = ctx.layer_painter(egui::LayerId::background());
        let now = ctx.input(|i| i.time);
        match command {
            Command::Tool(kind) => self.switch_tool(kind),
            Command::Toggle(f) => {
                let v = self.flag(f);
                *v = !*v;
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
            Command::Save => self.save(),
            Command::SaveAs => self.save_as(),
            Command::ImportDxf => self.files.open(app::sheets_ui::IMPORT_FILE),
            Command::ImportUnderlay => self.pick_underlay_file(),
            Command::ExportPdf => self.export_pdf(&painter),
            Command::ExportSvg => self.export_svg(&painter),
            Command::ExportDxf => self.export_dxf(&painter),
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
            Command::Delete => self.editor.delete_selection(),
            Command::Duplicate => {
                self.set_tool(ToolKind::Select);
                if self.editor.duplicate_selection() > 0 {
                    self.tool
                        .key(ToolKey::G, self.pointer.as_ref(), &mut self.editor);
                }
            }
            Command::Group => {
                self.editor.make_group();
            }
            Command::Ungroup => {
                self.editor.ungroup();
            }
            Command::MakeBlock => {
                if let Err(e) = self.editor.make_block("") {
                    self.set_status(e, true, now);
                }
            }
            Command::Explode => self.editor.explode_selected(),
            Command::Construction => self.editor.toggle_construction(),
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
            Command::Shortcuts => self.show_shortcuts = true,
            Command::Preferences => self.show_settings = true,
            Command::DrawingSettings => self.show_drawing_settings = true,
            Command::About => self.show_about = true,
            Command::Theme(pref) => ctx.set_theme(pref),
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
        let mut chosen = (enter && shown > 0).then(|| entries[state.selected].command);
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
                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &e.title)
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
                            let icon = egui::Rect::from_center_size(
                                egui::pos2(r.left() + 18.0, r.center().y),
                                egui::Vec2::splat(16.0),
                            );
                            match e.command {
                                Command::Tool(kind) => icons::tool(painter, icon, kind, fg),
                                Command::Panel(p) => {
                                    icons::glyph(painter, icon, app::dock_ui::glyph(p), fg)
                                }
                                _ => {
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
                            painter.text(
                                egui::pos2(right, r.center().y),
                                egui::Align2::RIGHT_CENTER,
                                e.group,
                                egui::FontId::proportional(11.5),
                                weak,
                            );
                            if response.clicked() {
                                chosen = Some(e.command);
                            }
                        }
                    });
            });
        if let Some(command) = chosen {
            self.run_command(ctx, command);
            return;
        }
        if !close {
            self.palette = Some(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn prefixes_beat_substrings_beat_scattered_letters() {
        let prefix = score("rec", "Rectangle").unwrap();
        let word = score("arc", "3-pt arc").unwrap();
        let inner = score("ang", "Rectangle").unwrap();
        let scattered = score("rct", "Rectangle").unwrap();
        assert!(prefix > word && word > inner && inner > scattered);
        assert!(score("xyz", "Rectangle").is_none());
        assert_eq!(score("", "Anything"), Some(0));
    }
}
