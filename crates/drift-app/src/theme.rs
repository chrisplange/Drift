//! Look and feel: the platform's UI font, a restrained palette with one accent colour, soft
//! corners and comfortable spacing, plus small helpers for consistent panels.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme,
    Vec2, vec2,
};

/// The accent colour: a calm blue that holds white text in both themes. Used sparingly: the
/// one primary button, focus, and the icon or text of whatever is current.
pub fn accent(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x3d, 0x8b, 0xfd)
    } else {
        Color32::from_rgb(0x1f, 0x6f, 0xeb)
    }
}

/// A faint tint of the accent, behind the current item in a list or rail.
pub fn accent_soft(dark: bool) -> Color32 {
    accent(dark).gamma_multiply(if dark { 0.22 } else { 0.14 })
}

/// The interface's neutrals, from the canvas to the strongest text.
#[derive(Clone, Copy)]
pub struct Neutrals {
    /// Docked panels, bars and the tool rail.
    pub panel: Color32,
    /// Dialogs, floating panels, menus and popups: a step above the panels.
    pub raised: Color32,
    /// Text fields.
    pub field: Color32,
    /// Quiet fills (code, key caps, stat tiles).
    pub faint: Color32,
    /// Buttons and other controls, at rest, hovered and pressed.
    pub control: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    /// Hairlines between sections and around panels.
    pub border: Color32,
    /// The outline of a control at rest.
    pub control_border: Color32,
    pub text: Color32,
    /// Labels and hints.
    pub secondary: Color32,
    /// Captions, counts and disabled text.
    pub tertiary: Color32,
}

pub fn neutrals(dark: bool) -> Neutrals {
    let c = |r, g, b| Color32::from_rgb(r, g, b);
    if dark {
        Neutrals {
            panel: c(0x20, 0x22, 0x26),
            raised: c(0x2a, 0x2d, 0x32),
            field: c(0x16, 0x18, 0x1b),
            faint: c(0x28, 0x2b, 0x30),
            control: c(0x2e, 0x31, 0x36),
            hover: c(0x38, 0x3c, 0x42),
            pressed: c(0x43, 0x47, 0x4e),
            border: c(0x33, 0x36, 0x3c),
            control_border: c(0x3b, 0x3f, 0x46),
            text: c(0xe6, 0xe7, 0xe9),
            secondary: c(0xa3, 0xa7, 0xae),
            tertiary: c(0x6f, 0x74, 0x7c),
        }
    } else {
        Neutrals {
            panel: c(0xee, 0xef, 0xf1),
            raised: Color32::WHITE,
            field: Color32::WHITE,
            faint: c(0xe6, 0xe8, 0xeb),
            control: c(0xe2, 0xe4, 0xe8),
            hover: c(0xd7, 0xda, 0xdf),
            pressed: c(0xc9, 0xcd, 0xd3),
            border: c(0xd9, 0xdb, 0xdf),
            control_border: c(0xc6, 0xca, 0xd0),
            text: c(0x1b, 0x1d, 0x21),
            secondary: c(0x5d, 0x62, 0x6b),
            tertiary: c(0x8a, 0x90, 0x99),
        }
    }
}

/// Corner radii: controls (checkboxes included) and chips, menus and tooltips, dialogs and
/// floating panels.
pub mod radius {
    pub const CONTROL: u8 = 4;
    pub const MENU: u8 = 6;
    pub const DIALOG: u8 = 8;
}

/// The type scale (points): captions and shortcuts, labels, body and controls, titles, and
/// the start panel's and About's name.
pub mod size {
    pub const CAPTION: f32 = 11.0;
    pub const LABEL: f32 = 12.5;
    pub const BODY: f32 = 13.0;
    pub const TITLE: f32 = 15.0;
    pub const DISPLAY: f32 = 20.0;
}

/// Status colours for the definition pill.
pub fn under_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x5f, 0xa8, 0xf5)
    } else {
        Color32::from_rgb(0x15, 0x65, 0xc0)
    }
}

pub fn ok_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x30, 0xd1, 0x58)
    } else {
        Color32::from_rgb(0x24, 0x8a, 0x3d)
    }
}

/// Family for headings and emphasis (a bold system face when one is available).
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

/// Use the platform's own UI font on desktop (Segoe UI, SF Pro, a standard Linux sans), with
/// egui's fonts kept as fallbacks for symbols. Fonts are read from the system at run time:
/// nothing is bundled or downloaded. The web build keeps egui's default fonts.
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

/// The interface's fonts (see [`install_fonts`]).
pub fn font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let defaults = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let load = |paths: &[&str]| {
            paths
                .iter()
                .find_map(|p| std::fs::read(p).ok())
                .map(egui::FontData::from_owned)
        };
        let ui_font = load(&[
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
            "/usr/share/fonts/truetype/inter/Inter-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ]);
        let bold_font = load(&[
            "C:\\Windows\\Fonts\\seguisb.ttf",
            "C:\\Windows\\Fonts\\segoeuib.ttf",
            "/System/Library/Fonts/SFNS.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        ]);
        let mono_font = load(&[
            "C:\\Windows\\Fonts\\consola.ttf",
            "/System/Library/Fonts/SFNSMono.ttf",
            "/System/Library/Fonts/Menlo.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        ]);
        for (name, data, family) in [
            ("system-ui", ui_font, FontFamily::Proportional),
            ("system-mono", mono_font, FontFamily::Monospace),
        ] {
            if let Some(data) = data {
                fonts.font_data.insert(name.into(), data.into());
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, name.into());
            }
        }
        let mut bold_list = defaults.clone();
        if let Some(data) = bold_font {
            fonts.font_data.insert("system-bold".into(), data.into());
            bold_list.insert(0, "system-bold".into());
        } else if fonts.font_data.contains_key("system-ui") {
            bold_list.insert(0, "system-ui".into());
        }
        fonts.families.insert(bold(), bold_list);
    }
    #[cfg(target_arch = "wasm32")]
    {
        fonts.families.insert(bold(), defaults);
    }
    fonts
}

/// Apply the light and dark styles.
pub fn apply(ctx: &egui::Context) {
    ctx.style_mut_of(Theme::Dark, |s| style(s, true));
    ctx.style_mut_of(Theme::Light, |s| style(s, false));
}

fn style(s: &mut egui::Style, dark: bool) {
    // Typography: the platform's UI font throughout (numbers included); monospace only for
    // typed expressions and diagnostics.
    s.text_styles = [
        (
            TextStyle::Small,
            FontId::new(size::CAPTION, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(size::BODY, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(size::BODY, FontFamily::Proportional),
        ),
        (TextStyle::Heading, FontId::new(size::TITLE, bold())),
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
    ]
    .into();

    // Spacing on a 4-point grid: 24-point controls, 16 points of padding in dialogs.
    let sp = &mut s.spacing;
    sp.item_spacing = vec2(8.0, 6.0);
    sp.button_padding = vec2(10.0, 4.0);
    sp.interact_size = vec2(24.0, 24.0);
    sp.icon_width = 14.0;
    sp.icon_width_inner = 8.0;
    sp.menu_margin = Margin::same(6);
    sp.window_margin = Margin::same(16);
    sp.combo_width = 120.0;
    sp.icon_spacing = 6.0;
    // Thin scroll bars that widen on hover, in a strip of their own so they never sit on
    // the content.
    sp.scroll = egui::style::ScrollStyle {
        floating_allocated_width: 8.0,
        ..egui::style::ScrollStyle::floating()
    };
    s.interaction.tooltip_delay = 0.35;

    let n = neutrals(dark);
    let accent = accent(dark);
    let v = &mut s.visuals;
    v.panel_fill = n.panel;
    v.window_fill = n.raised;
    v.extreme_bg_color = n.field;
    v.text_edit_bg_color = Some(n.field);
    v.faint_bg_color = n.faint;
    v.code_bg_color = n.faint;
    v.weak_text_color = Some(n.secondary);
    v.override_text_color = None;
    v.hyperlink_color = accent;
    // A quiet highlight for picked list items and selected text.
    v.selection.bg_fill = accent.gamma_multiply(if dark { 0.38 } else { 0.2 });
    v.selection.stroke = Stroke::new(1.0, if dark { n.text } else { accent });
    v.window_corner_radius = CornerRadius::same(radius::DIALOG);
    v.menu_corner_radius = CornerRadius::same(radius::MENU);
    v.window_stroke = Stroke::new(1.0, if dark { n.control_border } else { n.border });
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 36 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 90 } else { 28 }),
    };
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;

    let radius = CornerRadius::same(radius::CONTROL);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = n.panel;
    w.noninteractive.weak_bg_fill = n.panel;
    // Separators and panel edges: a hairline in the panel's own tone, never white.
    w.noninteractive.bg_stroke = Stroke::new(1.0, n.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, n.text);
    w.noninteractive.corner_radius = radius;
    for (wv, bg) in [
        (&mut w.inactive, n.control),
        (&mut w.hovered, n.hover),
        (&mut w.active, n.pressed),
        (&mut w.open, n.hover),
    ] {
        wv.bg_fill = bg;
        wv.weak_bg_fill = bg;
        wv.bg_stroke = Stroke::NONE;
        wv.fg_stroke = Stroke::new(1.0, n.text);
        wv.corner_radius = radius;
        wv.expansion = 0.0;
    }
    // Controls at rest carry a faint outline, so checkboxes and fields read on any fill.
    w.inactive.bg_stroke = Stroke::new(1.0, n.control_border);
    w.hovered.bg_stroke = Stroke::new(1.0, n.control_border);
    w.inactive.fg_stroke = Stroke::new(1.0, n.text.gamma_multiply(0.92));
}

/// Letter-spaced caps for section titles, on one line.
fn caps_job(ui: &egui::Ui, title: &str, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::single_section(
        title.to_uppercase(),
        egui::TextFormat {
            font_id: FontId::new(10.5, bold()),
            color,
            extra_letter_spacing: 0.7,
            ..Default::default()
        },
    );
    job.wrap.max_rows = 1;
    job.wrap.max_width = ui.available_width();
    job
}

/// The hairline colour used between sections and rows.
pub fn hairline(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.noninteractive.bg_stroke.color
}

/// A quiet section header: letter-spaced caps followed by a hairline to the edge, with space
/// above it.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    let color = ui.visuals().weak_text_color();
    let galley = ui.painter().layout_job(caps_job(ui, title, color));
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), egui::Sense::hover());
    let y = rect.center().y;
    ui.painter().galley(
        egui::pos2(rect.left(), y - galley.size().y / 2.0),
        galley.clone(),
        color,
    );
    let x = rect.left() + galley.size().x + 8.0;
    if x < rect.right() {
        ui.painter().line_segment(
            [egui::pos2(x, y), egui::pos2(rect.right(), y)],
            Stroke::new(1.0, hairline(ui)),
        );
    }
    ui.add_space(2.0);
}

/// A section that folds away: a chevron, the caps title and a hairline. Whether it is open
/// is remembered (across runs). Returns the body's result when open.
pub fn fold<R>(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    default_open: bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    ui.add_space(14.0);
    let id = ui.make_persistent_id(("fold", id_salt));
    let mut open = ui.data_mut(|d| *d.get_persisted_mut_or(id, default_open));
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 18.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, title)
    });
    if response.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_persisted(id, open));
    }
    let color = if response.hovered() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let y = rect.center().y;
    // The chevron: right when folded, down when open.
    let c = egui::pos2(rect.left() + 4.0, y);
    let pts = if open {
        vec![
            c + vec2(-3.5, -1.8),
            c + vec2(0.0, 1.8),
            c + vec2(3.5, -1.8),
        ]
    } else {
        vec![
            c + vec2(-1.8, -3.5),
            c + vec2(1.8, 0.0),
            c + vec2(-1.8, 3.5),
        ]
    };
    ui.painter()
        .add(egui::Shape::line(pts, Stroke::new(1.4, color)));
    let galley = ui.painter().layout_job(caps_job(ui, title, color));
    let left = rect.left() + 14.0;
    ui.painter().galley(
        egui::pos2(left, y - galley.size().y / 2.0),
        galley.clone(),
        color,
    );
    let x = left + galley.size().x + 8.0;
    if x < rect.right() {
        ui.painter().line_segment(
            [egui::pos2(x, y), egui::pos2(rect.right(), y)],
            Stroke::new(1.0, hairline(ui)),
        );
    }
    ui.add_space(2.0);
    open.then(|| body(ui))
}

/// A soft, full-width button for destructive actions.
pub fn danger_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let red = ui.visuals().error_fg_color;
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(red))
            .fill(red.gamma_multiply(0.12))
            .min_size(vec2(ui.available_width(), 28.0)),
    )
}

/// A small, letter-spaced all-caps header (as in macOS sidebars).
pub fn caps_header(ui: &mut egui::Ui, title: &str) {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &title.to_uppercase(),
        6.0,
        egui::TextFormat {
            font_id: egui::FontId::new(10.0, bold()),
            color: neutrals(ui.visuals().dark_mode).tertiary,
            extra_letter_spacing: 0.9,
            ..Default::default()
        },
    );
    ui.label(job);
}

/// A segmented control (macOS-style): one rounded track with the selected segment raised.
/// Returns the clicked segment.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: T,
    options: &[(T, &str)],
) -> Option<T> {
    let dark = ui.visuals().dark_mode;
    let n = neutrals(dark);
    let track = n.control;
    let raised = if dark {
        Color32::from_rgb(0x4a, 0x4e, 0x56)
    } else {
        Color32::WHITE
    };
    let width = ui.available_width();
    let height = 26.0;
    let (rect, track_response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, radius::MENU, track);
    // Segments share the width in proportion to their labels (so long ones fit).
    let font = FontId::new(size::LABEL, bold());
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x
                + 12.0
        })
        .collect();
    let total: f32 = widths.iter().sum();
    let room = rect.width() - 4.0;
    let mut x = rect.min.x + 2.0;
    let mut clicked = None;
    for (i, (value, label)) in options.iter().enumerate() {
        let seg = widths[i] / total * room;
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.min.y + 2.0), vec2(seg, height - 4.0));
        x += seg;
        // Ids from the track's own (unique) id, so several controls can share a parent.
        let response = ui.interact(r, track_response.id.with(("seg", i)), egui::Sense::click());
        let selected = *value == current;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, *label)
        });
        if selected {
            ui.painter().rect_filled(r, radius::CONTROL, raised);
            if !dark {
                ui.painter().rect_stroke(
                    r,
                    radius::CONTROL,
                    Stroke::new(1.0, n.control_border),
                    egui::StrokeKind::Inside,
                );
            }
        } else if response.hovered() {
            ui.painter().rect_filled(r, radius::CONTROL, n.hover);
        }
        let color = if selected {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().text_color()
        };
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            FontId::new(
                size::LABEL,
                if selected {
                    bold()
                } else {
                    FontFamily::Proportional
                },
            ),
            color,
        );
        if response.clicked() {
            clicked = Some(*value);
        }
    }
    clicked
}

/// A rounded "pill" label with a coloured dot, for status.
pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    let font = FontId::new(12.0, FontFamily::Proportional);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, ui.visuals().text_color());
    let size = vec2(galley.size().x + 26.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 10.0, color.gamma_multiply(0.16));
    let dot = rect.left_center() + vec2(10.0, 0.0);
    ui.painter().circle_filled(dot, 3.5, color);
    ui.painter().galley(
        rect.left_center() + vec2(18.0, -galley.size().y / 2.0),
        galley,
        ui.visuals().text_color(),
    );
    response
}

/// Space between a row's label column and its value.
pub const LABEL_WIDTH: f32 = 88.0;

/// A label / value row in an inspector grid.
pub fn row(
    ui: &mut egui::Ui,
    label: &str,
    add: impl FnOnce(&mut egui::Ui),
) -> egui::InnerResponse<()> {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(LABEL_WIDTH, 20.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(LABEL_WIDTH);
                ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color()));
            },
        );
        add(ui);
    })
}

/// A fade over the bottom of a scrolled list when there is more below it, so a cut-off
/// row reads as "scroll for more" rather than a clipped edge. `solid` is the colour behind
/// the list.
pub fn fade_bottom(
    ui: &egui::Ui,
    output: &egui::scroll_area::ScrollAreaOutput<()>,
    solid: Color32,
) {
    let view = output.inner_rect;
    let below = output.content_size.y - (output.state.offset.y + view.height());
    if below < 1.0 {
        return;
    }
    let height = 28.0_f32.min(below + 4.0);
    let rect = egui::Rect::from_min_max(egui::pos2(view.left(), view.bottom() - height), view.max);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(rect.right_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(rect.right_bottom(), solid);
    mesh.colored_vertex(rect.left_bottom(), solid);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// A row in a list (sheets, blocks): the name left-aligned; the picked row on a faint
/// accent tint with strong text.
pub fn list_row(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 26.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });
    let dark = ui.visuals().dark_mode;
    let n = neutrals(dark);
    if selected {
        ui.painter()
            .rect_filled(rect, radius::CONTROL, accent_soft(dark));
    } else if response.hovered() {
        ui.painter().rect_filled(rect, radius::CONTROL, n.hover);
    }
    let font = FontId::new(
        size::BODY,
        if selected {
            bold()
        } else {
            FontFamily::Proportional
        },
    );
    ui.painter().text(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        n.text,
    );
    response
}

/// A menu item that is on or off: a check in a column at its left (blank when off), its
/// label and its shortcut, the whole row highlighting on hover like any menu item.
pub fn menu_check(ui: &mut egui::Ui, on: bool, label: &str, keys: &str) -> egui::Response {
    let check = egui::Atom::custom(egui::Id::new("menu_check"), vec2(14.0, 14.0));
    let response = ui.add(egui::Button::new((check, label)).shortcut_text(keys));
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, label));
    if on {
        let x = response.rect.left() + ui.spacing().button_padding.x + 7.0;
        let c = egui::pos2(x, response.rect.center().y);
        let color = ui.style().interact(&response).text_color();
        ui.painter().add(egui::Shape::line(
            vec![
                c + vec2(-4.0, 0.0),
                c + vec2(-1.2, 3.0),
                c + vec2(4.0, -3.5),
            ],
            Stroke::new(1.6, color),
        ));
    }
    response
}

/// The frame every dialog shares (windows and modal prompts alike).
pub fn dialog_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style).inner_margin(Margin::same(16))
}

/// A dialog window: the shared frame, without egui's title bar (each dialog starts with
/// [`dialog_title`]), centred on the window.
pub fn dialog(ctx: &egui::Context, title: &str) -> egui::Window<'static> {
    egui::Window::new(title.to_owned())
        .frame(dialog_frame(&ctx.style_of(ctx.theme())))
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
}

/// A small close button (an ×), as dialogs and panels use.
pub fn close_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close window"));
    let n = neutrals(ui.visuals().dark_mode);
    if response.hovered() {
        ui.painter().rect_filled(rect, radius::CONTROL, n.hover);
    }
    let color = if response.hovered() {
        n.text
    } else {
        n.secondary
    };
    let c = rect.center();
    let d = 4.0;
    let stroke = Stroke::new(1.4, color);
    ui.painter()
        .line_segment([c + vec2(-d, -d), c + vec2(d, d)], stroke);
    ui.painter()
        .line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
    response.on_hover_text("Close")
}

/// A dialog's title, left-aligned, with a close button at the right when `closable`.
/// Returns whether the close button was clicked. Call it after setting the dialog's width.
pub fn dialog_title(ui: &mut egui::Ui, title: &str, closable: bool) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).text_style(TextStyle::Heading));
        if closable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = close_button(ui).clicked();
            });
        }
    });
    ui.add_space(6.0);
    close
}

/// A dialog's row of buttons, right-aligned at its foot. Buttons are laid out right to
/// left: add the one that dismisses (Cancel, Close) first and the primary action last, so
/// the primary sits leftmost, as on Windows.
pub fn dialog_buttons<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.add_space(12.0);
    // A fixed height: a centred layout would otherwise stretch to fill a growing window.
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 28.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().interact_size.y = 28.0;
            ui.spacing_mut().button_padding.x = 14.0;
            add(ui)
        },
    )
    .inner
}

/// The primary action: the one accent-filled button in a dialog or panel.
pub fn primary_button(label: &str, dark: bool) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(label.to_owned()).color(Color32::WHITE))
        .fill(accent(dark))
        .stroke(Stroke::NONE)
}

/// The appearance (follow the system, dark or light), as a segmented control.
pub fn appearance(ui: &mut egui::Ui) {
    use egui::ThemePreference as P;
    let current = ui.ctx().options(|o| o.theme_preference);
    let options = [
        (P::System, "System"),
        (P::Dark, "Dark"),
        (P::Light, "Light"),
    ];
    if let Some(pref) = segmented(ui, current, &options) {
        ui.ctx().set_theme(pref);
    }
}
