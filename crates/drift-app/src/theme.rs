//! Look and feel: the platform's UI font, a restrained palette with one accent colour, soft
//! corners and comfortable spacing, plus small helpers for consistent panels.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme,
    Vec2, vec2,
};

/// The two dark looks: Midnight (the original, cool greys with a blue accent) and Obsidian
/// (neutral greys with an orange accent). The light look is a single one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DarkLook {
    Midnight,
    Obsidian,
}

impl DarkLook {
    pub fn key(self) -> &'static str {
        match self {
            DarkLook::Midnight => "midnight",
            DarkLook::Obsidian => "obsidian",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [DarkLook::Midnight, DarkLook::Obsidian]
            .into_iter()
            .find(|l| l.key() == s.trim())
    }
}

/// True for Obsidian (the default), false for Midnight.
static OBSIDIAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Which dark look is in use (it only matters while the appearance is dark).
pub fn dark_look() -> DarkLook {
    if OBSIDIAN.load(std::sync::atomic::Ordering::Relaxed) {
        DarkLook::Obsidian
    } else {
        DarkLook::Midnight
    }
}

pub fn set_dark_look(look: DarkLook) {
    OBSIDIAN.store(
        look == DarkLook::Obsidian,
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// What the appearance control offers: follow the system, one of the dark looks, or light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    System,
    Midnight,
    Obsidian,
    Light,
}

impl Look {
    pub const ALL: [Look; 4] = [Look::System, Look::Midnight, Look::Obsidian, Look::Light];

    pub fn label(self) -> &'static str {
        match self {
            Look::System => "System",
            Look::Midnight => "Midnight",
            Look::Obsidian => "Obsidian",
            Look::Light => "Light",
        }
    }

    /// The look in force now.
    pub fn current(ctx: &egui::Context) -> Look {
        match ctx.options(|o| o.theme_preference) {
            egui::ThemePreference::System => Look::System,
            egui::ThemePreference::Light => Look::Light,
            egui::ThemePreference::Dark => match dark_look() {
                DarkLook::Midnight => Look::Midnight,
                DarkLook::Obsidian => Look::Obsidian,
            },
        }
    }
}

/// Switch the look. Following the system keeps the dark look last chosen for its dark half.
pub fn set_look(ctx: &egui::Context, look: Look) {
    match look {
        Look::System => ctx.set_theme(egui::ThemePreference::System),
        Look::Light => ctx.set_theme(egui::ThemePreference::Light),
        Look::Midnight | Look::Obsidian => {
            set_dark_look(if look == Look::Midnight {
                DarkLook::Midnight
            } else {
                DarkLook::Obsidian
            });
            ctx.set_theme(egui::ThemePreference::Dark);
        }
    }
    // The styles are built from the palette: build them again.
    apply(ctx);
}

/// One of the three looks' set of colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Skin {
    Midnight,
    Obsidian,
    Light,
}

impl Skin {
    #[cfg(test)]
    const ALL: [Skin; 3] = [Skin::Midnight, Skin::Obsidian, Skin::Light];

    /// The one in force for the dark or the light appearance.
    fn current(dark: bool) -> Skin {
        match (dark, dark_look()) {
            (false, _) => Skin::Light,
            (true, DarkLook::Midnight) => Skin::Midnight,
            (true, DarkLook::Obsidian) => Skin::Obsidian,
        }
    }

    fn accent(self) -> Color32 {
        match self {
            Skin::Midnight => Color32::from_rgb(0x3d, 0x8b, 0xfd),
            Skin::Obsidian => Color32::from_rgb(0xff, 0x9d, 0x2a),
            Skin::Light => Color32::from_rgb(0x1f, 0x6f, 0xeb),
        }
    }

    /// One surface model in every look: the canvas is the darkest well, the panes sit above
    /// it, and what floats (menus, dialogs, popovers) sits above the panes.
    fn neutrals(self) -> Neutrals {
        let c = |hex: u32| Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        match self {
            Skin::Midnight => Neutrals {
                panel: c(0x21_23_29),
                raised: c(0x2b_2e_35),
                field: c(0x15_16_1a),
                faint: c(0x28_2a_31),
                control: c(0x31_34_3c),
                hover: c(0x3b_3f_48),
                pressed: c(0x47_4b_55),
                border: c(0x36_39_40),
                divider: c(0x0d_0e_11),
                control_border: c(0x41_45_4f),
                text: c(0xe7_e8_ec),
                secondary: c(0xb1_b5_be),
                tertiary: c(0x94_9a_a5),
            },
            Skin::Obsidian => Neutrals {
                panel: c(0x23_23_25),
                raised: c(0x2d_2d_30),
                field: c(0x17_17_18),
                faint: c(0x2a_2a_2c),
                control: c(0x33_33_36),
                hover: c(0x3d_3d_41),
                pressed: c(0x49_49_4d),
                border: c(0x38_38_3b),
                divider: c(0x0f_0f_10),
                control_border: c(0x43_43_47),
                text: c(0xe8_e8_ea),
                secondary: c(0xb4_b4_ba),
                tertiary: c(0x96_96_9e),
            },
            Skin::Light => Neutrals {
                panel: c(0xec_ec_ee),
                raised: c(0xff_ff_ff),
                field: c(0xff_ff_ff),
                faint: c(0xe2_e2_e5),
                control: c(0xde_de_e1),
                hover: c(0xd3_d3_d7),
                pressed: c(0xc7_c7_cc),
                border: c(0xd6_d6_da),
                divider: c(0xbc_bc_c2),
                control_border: c(0xc2_c2_c7),
                text: c(0x1d_1d_1f),
                secondary: c(0x4a_4a_50),
                tertiary: c(0x68_68_6f),
            },
        }
    }
}

/// The accent colour: blue in Midnight and Light, orange in Obsidian. Used sparingly: the
/// one primary button, focus, and the icon or text of whatever is current.
pub fn accent(dark: bool) -> Color32 {
    Skin::current(dark).accent()
}

/// A faint tint of the accent, behind the current item in a list or rail.
pub fn accent_soft(dark: bool) -> Color32 {
    accent(dark).gamma_multiply(if dark { 0.22 } else { 0.14 })
}

/// The interface's neutrals, from the deepest well to the strongest text.
#[derive(Clone, Copy)]
pub struct Neutrals {
    /// Docked panels, bars and the tool rail.
    pub panel: Color32,
    /// Dialogs, floating panels, menus and popups: a step above the panels.
    pub raised: Color32,
    /// Text fields and the tracks of segmented controls: wells in a pane.
    pub field: Color32,
    /// Quiet fills (code, stat tiles).
    pub faint: Color32,
    /// Buttons and other controls, at rest, hovered and pressed.
    pub control: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    /// Hairlines between sections and rows: quiet on a pane, and still there on what floats
    /// above one (a menu's separators, a dialog's sections).
    pub border: Color32,
    /// The line between two panes: darker than either (see [`divider`]).
    pub divider: Color32,
    /// The outline of a control at rest.
    pub control_border: Color32,
    pub text: Color32,
    /// Labels and hints.
    pub secondary: Color32,
    /// Captions, counts and disabled text. It still reads (4.5:1) on a pane and on what
    /// floats above one.
    pub tertiary: Color32,
}

pub fn neutrals(dark: bool) -> Neutrals {
    Skin::current(dark).neutrals()
}

/// The line between two panes (a bar and the canvas, a dock and the canvas): darker than the
/// panes, where [`hairline`] is the quiet line inside one.
pub fn divider(dark: bool) -> Color32 {
    neutrals(dark).divider
}

/// The drawing's background: the darkest surface of the interface, the same in both dark
/// looks. One source for the canvas itself and for whatever is painted to match it (the
/// active tab, handles, the gap behind dimension text): `drift_render`'s palette.
pub fn canvas(dark: bool) -> Color32 {
    let [r, g, b, _] = drift_render::Palette::themed(dark).background;
    Color32::from_rgb(r, g, b)
}

/// The desk a sheet's paper lies on.
pub fn desk(dark: bool) -> Color32 {
    let [r, g, b, _] = drift_render::Palette::themed(dark).desk;
    Color32::from_rgb(r, g, b)
}

/// A colour `t` of the way from `a` to `b` (both opaque): a step between two tokens.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
    )
}

/// Corner radii: controls (checkboxes included) and chips, menus and popovers, dialogs and
/// floating panels.
pub mod radius {
    pub const CONTROL: u8 = 5;
    pub const MENU: u8 = 8;
    pub const DIALOG: u8 = 10;
}

/// The type scale (points): captions and shortcuts, labels, body and controls, titles, and
/// the start panel's and About's name. `SECTION` is a section's title, in bold.
pub mod size {
    pub const CAPTION: f32 = 11.0;
    pub const SECTION: f32 = 11.5;
    pub const LABEL: f32 = 12.0;
    pub const BODY: f32 = 12.5;
    pub const TITLE: f32 = 14.0;
    pub const DISPLAY: f32 = 20.0;
    pub const MONO: f32 = 12.0;
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

/// Something to look at that isn't an error (a fill covering several areas).
pub fn warning_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xef, 0x9f, 0x27)
    } else {
        Color32::from_rgb(0x85, 0x4f, 0x0b)
    }
}

/// What went wrong or can't be done: an error's text, a destructive button, a lost fill.
pub fn error_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xff, 0x5d, 0x5d)
    } else {
        Color32::from_rgb(0xc6, 0x28, 0x28)
    }
}

/// Amber, for a caution that asks to be read: a setting that overrides the drawing's, an
/// import that left something out, the pie's slice that would conflict.
pub fn amber(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xff, 0x9f, 0x0a)
    } else {
        Color32::from_rgb(0x9a, 0x5b, 0x00)
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

/// Capital height over the em of the interface font (the first in the proportional family,
/// where annotation text without a drawing font is shown).
pub fn interface_cap_ratio() -> f64 {
    static RATIO: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *RATIO.get_or_init(|| {
        let fonts = font_definitions();
        fonts
            .families
            .get(&FontFamily::Proportional)
            .and_then(|list| list.first())
            .and_then(|name| fonts.font_data.get(name))
            .and_then(|data| {
                drift_draft::font::Face::new(std::sync::Arc::new(data.font.to_vec()), data.index)
            })
            .map(|face| face.cap_ratio())
            .filter(|r| *r > 0.0)
            .unwrap_or(drift_draft::page::CAP_HEIGHT)
    })
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
            FontId::new(size::MONO, FontFamily::Monospace),
        ),
    ]
    .into();

    // Dense, as an instrument is: 24-point controls close together, 16 points of padding in
    // dialogs.
    let sp = &mut s.spacing;
    sp.item_spacing = vec2(6.0, 5.0);
    sp.button_padding = vec2(9.0, 3.0);
    sp.interact_size = vec2(24.0, 24.0);
    sp.icon_width = 14.0;
    sp.icon_width_inner = 8.0;
    sp.slider_rail_height = 3.0;
    sp.menu_margin = Margin::same(5);
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
    // Quick: a fold or a hover settles before the eye follows it.
    s.animation_time = 0.12;

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
    v.error_fg_color = error_color(dark);
    v.warn_fg_color = amber(dark);
    // A quiet highlight for picked list items and selected text.
    v.selection.bg_fill = accent.gamma_multiply(if dark { 0.38 } else { 0.2 });
    v.selection.stroke = Stroke::new(1.0, if dark { n.text } else { accent });
    v.window_corner_radius = CornerRadius::same(radius::DIALOG);
    v.menu_corner_radius = CornerRadius::same(radius::MENU);
    // What floats is opaque, edged, and lifted by a soft shadow: a deep one under dialogs,
    // a short one under menus and popovers.
    v.window_stroke = Stroke::new(1.0, n.control_border);
    v.window_shadow = Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 130 } else { 44 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 32 }),
    };
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;

    let radius = CornerRadius::same(radius::CONTROL);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = n.panel;
    w.noninteractive.weak_bg_fill = n.panel;
    // Separators and the lines inside a pane: a hairline in the pane's own tone, never white.
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

/// The hairline colour used between sections and rows.
pub fn hairline(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.noninteractive.bg_stroke.color
}

/// A section's title: sentence case, bold, in the secondary tone, on one line.
fn title_galley(ui: &egui::Ui, title: &str, color: Color32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        title.to_owned(),
        egui::TextFormat {
            font_id: FontId::new(size::SECTION, bold()),
            color,
            ..Default::default()
        },
    );
    job.wrap.max_rows = 1;
    job.wrap.max_width = ui.available_width();
    ui.painter().layout_job(job)
}

/// Height of a section's title row.
const SECTION_ROW: f32 = 18.0;

/// Room for a section's title, under the rule a section starts with: a hairline across the
/// pane, 8 points below what came before and 6 above the title. The first thing in a pane
/// needs no rule. Returns the title's row and the response of the whole.
fn section_row(ui: &mut egui::Ui, sense: egui::Sense) -> (egui::Rect, egui::Response) {
    let first = ui.min_rect().height() < 1.0;
    // (The gap above the rule includes the spacing every row already leaves.)
    let above = (8.0 - ui.spacing().item_spacing.y).max(0.0);
    let lead = if first { 0.0 } else { above + 1.0 + 6.0 };
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), lead + SECTION_ROW), sense);
    if !first {
        ui.painter().hline(
            rect.x_range(),
            rect.top() + above + 0.5,
            Stroke::new(1.0, hairline(ui)),
        );
    }
    let row = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - SECTION_ROW),
        rect.max,
    );
    (row, response)
}

/// A section header: a hairline across the pane, then the title in sentence case.
pub fn section(ui: &mut egui::Ui, title: &str) {
    let color = ui.visuals().weak_text_color();
    let (row, _) = section_row(ui, egui::Sense::hover());
    let galley = title_galley(ui, title, color);
    ui.painter().galley(
        egui::pos2(row.left(), row.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// A section that folds away: the section's rule, a chevron and the title. Whether it is
/// open is remembered (across runs). Returns the body's result when open.
pub fn fold<R>(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    default_open: bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let id = ui.make_persistent_id(("fold", id_salt));
    let mut open = ui.data_mut(|d| *d.get_persisted_mut_or(id, default_open));
    let (row, response) = section_row(ui, egui::Sense::click());
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
    let y = row.center().y;
    // The chevron: right when folded, down when open.
    chevron(ui.painter(), egui::pos2(row.left() + 5.0, y), open, color);
    let galley = title_galley(ui, title, color);
    ui.painter().galley(
        egui::pos2(row.left() + 16.0, y - galley.size().y / 2.0),
        galley,
        color,
    );
    open.then(|| body(ui))
}

/// A button for destructive actions: as wide as its name, red on a faint red.
pub fn danger_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let red = error_color(ui.visuals().dark_mode);
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(red))
            .fill(red.gamma_multiply(0.12))
            .stroke(Stroke::new(1.0, red.gamma_multiply(0.3)))
            .min_size(vec2(0.0, ROW_HEIGHT)),
    )
}

/// A small header over a group in a list or rail: sentence case, bold, in the tertiary
/// tone.
pub fn caps_header(ui: &mut egui::Ui, title: &str) {
    let color = neutrals(ui.visuals().dark_mode).tertiary;
    caps_header_in(ui, title, color);
}

/// [`caps_header`] in a colour (the tool rail's, in the secondary tone for contrast).
pub fn caps_header_in(ui: &mut egui::Ui, title: &str, color: Color32) {
    let galley =
        ui.painter()
            .layout_no_wrap(title.to_owned(), FontId::new(size::CAPTION, bold()), color);
    let size = vec2(galley.size().x + 6.0, galley.size().y);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    // Read out as it always was (in capitals), so it is never taken for the menu, the tab
    // or the tool of the same name.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title.to_uppercase())
    });
    ui.painter()
        .galley(rect.min + vec2(6.0, 0.0), galley, color);
}

/// The track of a segmented control (and of the top bar's tabs): a well in the dark looks,
/// a control's grey in Light.
pub fn segment_track(dark: bool) -> Color32 {
    let n = neutrals(dark);
    if dark { n.field } else { n.control }
}

/// The picked segment standing in [`segment_track`]: a control in the dark looks, white in
/// Light (edged with the control border in both).
pub fn segment_picked(dark: bool) -> Color32 {
    let n = neutrals(dark);
    if dark { n.control } else { n.raised }
}

/// The least room either side of a segment's label.
const SEGMENT_PAD: f32 = 6.0;

/// How long a segmented control's picked segment takes to slide to a new pick.
const SEGMENT_SLIDE: f64 = 0.22;

/// Where a segmented control's picked segment is drawn this frame: at `target` (segment
/// `index`'s place), or on its way there from where it was when the pick changed, easing in
/// and out like a macOS segmented control. Kept per control (`id`). With animations off
/// (an animation time of zero) it moves at once.
fn segment_slide(ui: &egui::Ui, id: egui::Id, index: usize, target: egui::Rect) -> egui::Rect {
    #[derive(Clone, Copy)]
    struct Slide {
        /// Where it set off from, for segment `to`, at `start`.
        from: egui::Rect,
        to: usize,
        start: f64,
        /// Where it was drawn last frame: a new pick sets off from there.
        shown: egui::Rect,
    }
    let id = id.with("segment_slide");
    let now = ui.input(|i| i.time);
    let animate = ui.style().animation_time > 0.0;
    let slide = ui.data(|d| d.get_temp::<Slide>(id));
    let mut slide = match slide {
        Some(s) if s.to == index => s,
        // A new pick: from wherever it is drawn now.
        Some(s) if animate => Slide {
            from: s.shown,
            to: index,
            start: now,
            shown: s.shown,
        },
        _ => Slide {
            from: target,
            to: index,
            start: f64::NEG_INFINITY,
            shown: target,
        },
    };
    let t = ((now - slide.start) / SEGMENT_SLIDE).clamp(0.0, 1.0) as f32;
    let e = egui::emath::easing::cubic_in_out(t);
    // (Towards the target as laid out this frame: a label turning bold moves it a little.)
    slide.shown = egui::Rect::from_min_max(
        slide.from.min.lerp(target.min, e),
        slide.from.max.lerp(target.max, e),
    );
    ui.data_mut(|d| d.insert_temp(id, slide));
    if t < 1.0 {
        ui.ctx().request_repaint();
    }
    slide.shown
}

/// A segmented control: a track with the picked segment standing in it, sliding to a new
/// pick. Returns the clicked segment.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: T,
    options: &[(T, &str)],
) -> Option<T> {
    let dark = ui.visuals().dark_mode;
    let n = neutrals(dark);
    let width = ui.available_width();
    let height = 24.0;
    let (rect, track_response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    ui.painter().rect(
        rect,
        radius::CONTROL + 1,
        segment_track(dark),
        Stroke::new(1.0, n.border),
        egui::StrokeKind::Inside,
    );
    let room = rect.width() - 4.0;
    let family = |selected: bool| {
        if selected {
            bold()
        } else {
            FontFamily::Proportional
        }
    };
    // Each label in its own face (the picked one bold), smaller where the track is too
    // narrow for them all with room either side; a label is never cut.
    let mut size = size::LABEL;
    let mut widths: Vec<f32> = Vec::new();
    for candidate in [size::LABEL, size::SECTION, size::CAPTION] {
        size = candidate;
        widths = options
            .iter()
            .map(|(value, label)| {
                ui.painter()
                    .layout_no_wrap(
                        (*label).to_owned(),
                        FontId::new(size, family(*value == current)),
                        Color32::WHITE,
                    )
                    .size()
                    .x
            })
            .collect();
        let needed = widths.iter().sum::<f32>() + 2.0 * SEGMENT_PAD * options.len() as f32;
        if needed <= room {
            break;
        }
    }
    // The room left over is shared evenly, so every label has the same space around it.
    let spare = ((room - widths.iter().sum::<f32>()) / options.len().max(1) as f32).max(0.0);
    let mut x = rect.min.x + 2.0;
    let rects: Vec<egui::Rect> = widths
        .iter()
        .map(|w| {
            let r = egui::Rect::from_min_size(
                egui::pos2(x, rect.min.y + 2.0),
                vec2(w + spare, height - 4.0),
            );
            x += w + spare;
            r
        })
        .collect();
    // The picked segment slides to a new pick, easing in and out (see [`segment_slide`]).
    let picked = options.iter().position(|(value, _)| *value == current);
    let shown = picked.map(|i| segment_slide(ui, track_response.id, i, rects[i]));
    if let Some(r) = shown {
        ui.painter().rect(
            r,
            radius::CONTROL - 1,
            segment_picked(dark),
            Stroke::new(1.0, n.control_border),
            egui::StrokeKind::Inside,
        );
    }
    let mut clicked = None;
    for (i, (value, label)) in options.iter().enumerate() {
        let r = rects[i];
        // Ids from the track's own (unique) id, so several controls can share a parent.
        let response = ui.interact(r, track_response.id.with(("seg", i)), egui::Sense::click());
        let selected = *value == current;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, *label)
        });
        if !selected && response.hovered() && shown.is_none_or(|s| !s.intersects(r.shrink(1.0))) {
            ui.painter()
                .rect_filled(r, radius::CONTROL - 1, if dark { n.faint } else { n.hover });
        }
        let color = if selected || response.hovered() {
            n.text
        } else {
            n.secondary
        };
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            FontId::new(size, family(selected)),
            color,
        );
        if response.clicked() {
            clicked = Some(*value);
        }
    }
    clicked
}

/// A status label with a coloured dot, on a tint of its colour.
pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    let font = FontId::new(size::LABEL, FontFamily::Proportional);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, ui.visuals().text_color());
    let size = vec2(galley.size().x + 25.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, radius::CONTROL, color.gamma_multiply(0.16));
    let dot = rect.left_center() + vec2(10.0, 0.0);
    ui.painter().circle_filled(dot, 3.5, color);
    ui.painter().galley(
        rect.left_center() + vec2(18.0, -galley.size().y / 2.0),
        galley,
        ui.visuals().text_color(),
    );
    response
}

/// A chip: a small button for one of several things on offer (a relation, a dimension).
/// `label` is what it shows and what it is called.
pub fn chip(ui: &mut egui::Ui, label: &str) -> egui::Response {
    chip_as(ui, None, label)
}

/// A [`chip`] that is on or off (a quick text that is on a dimension or isn't): on, the
/// accent's tint and edge.
pub fn select_chip(ui: &mut egui::Ui, on: bool, label: &str) -> egui::Response {
    chip_as(ui, Some(on), label)
}

fn chip_as(ui: &mut egui::Ui, on: Option<bool>, label: &str) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let n = neutrals(dark);
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::new(size::LABEL, FontFamily::Proportional),
        n.text,
    );
    let size = vec2(galley.size().x + 16.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| match on {
        Some(on) => egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, label),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label),
    });
    let fill = if !enabled {
        n.faint
    } else if response.is_pointer_button_down_on() {
        n.pressed
    } else if response.hovered() {
        n.hover
    } else {
        n.control
    };
    ui.painter().rect(
        rect,
        radius::CONTROL,
        fill,
        Stroke::new(1.0, n.control_border),
        egui::StrokeKind::Inside,
    );
    if on == Some(true) {
        ui.painter().rect(
            rect,
            radius::CONTROL,
            accent_soft(dark),
            Stroke::new(1.0, accent(dark).gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
    }
    let color = if enabled { n.text } else { n.tertiary };
    ui.painter().galley(
        egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response
}

/// The keys of a shortcut ("Ctrl+Shift+Z" is Ctrl, Shift and Z; "Ctrl++" is Ctrl and +).
fn key_names(keys: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut start = 0;
    for (i, ch) in keys.char_indices() {
        // A plus joins two keys when there is a key on each side of it.
        if ch == '+' && i > start && i + 1 < keys.len() {
            names.push(&keys[start..i]);
            start = i + 1;
        }
    }
    if start < keys.len() {
        names.push(&keys[start..]);
    }
    names
}

/// A shortcut as key caps, one small rounded chip per key: "Ctrl+K" is a Ctrl cap and a K
/// cap. The one way keys are shown, wherever they are.
pub fn keycap(ui: &mut egui::Ui, keys: &str) -> egui::Response {
    let (caps, width) = keycap_galleys(ui, keys);
    let (rect, response) = ui.allocate_exact_size(vec2(width, KEYCAP), egui::Sense::hover());
    paint_keycaps(ui, rect.left_top(), caps);
    response
}

/// [`keycap`], painted only (nothing to hover or click), ending at `right_center`: the keys
/// shown inside a control.
pub fn keycap_at(ui: &egui::Ui, right_center: egui::Pos2, keys: &str) {
    let (caps, width) = keycap_galleys(ui, keys);
    paint_keycaps(ui, right_center - vec2(width, KEYCAP / 2.0), caps);
}

/// How wide [`keycap_at`] paints a shortcut's caps: to line rows of them up.
pub fn keycaps_width(ui: &egui::Ui, keys: &str) -> f32 {
    keycap_galleys(ui, keys).1
}

/// A key cap's height, the padding either side of its key, and the gap between two caps.
const KEYCAP: f32 = 18.0;
const KEYCAP_PAD: f32 = 5.0;
const KEYCAP_GAP: f32 = 3.0;

/// A key's cap is as wide as its name, a single letter's is square.
fn keycap_width(galley: &egui::Galley) -> f32 {
    (galley.size().x + 2.0 * KEYCAP_PAD).max(KEYCAP)
}

/// The keys of a shortcut laid out, and the width of their caps in a row.
fn keycap_galleys(ui: &egui::Ui, keys: &str) -> (Vec<std::sync::Arc<egui::Galley>>, f32) {
    let n = neutrals(ui.visuals().dark_mode);
    let font = FontId::new(size::CAPTION, FontFamily::Proportional);
    let caps: Vec<_> = key_names(keys)
        .into_iter()
        .map(|key| {
            ui.painter()
                .layout_no_wrap(key.to_owned(), font.clone(), n.text)
        })
        .collect();
    let width = caps.iter().map(|g| keycap_width(g)).sum::<f32>()
        + KEYCAP_GAP * caps.len().saturating_sub(1) as f32;
    (caps, width)
}

fn paint_keycaps(ui: &egui::Ui, left_top: egui::Pos2, caps: Vec<std::sync::Arc<egui::Galley>>) {
    let dark = ui.visuals().dark_mode;
    let n = neutrals(dark);
    let mut x = left_top.x;
    for galley in caps {
        let r = egui::Rect::from_min_size(
            egui::pos2(x, left_top.y),
            vec2(keycap_width(&galley), KEYCAP),
        );
        x = r.right() + KEYCAP_GAP;
        ui.painter().rect(
            r,
            radius::CONTROL - 1,
            n.control,
            Stroke::new(1.0, n.control_border),
            egui::StrokeKind::Inside,
        );
        // A deeper bottom edge, like a key.
        ui.painter().hline(
            r.left() + 3.0..=r.right() - 3.0,
            r.bottom() - 0.5,
            Stroke::new(1.0, if dark { n.divider } else { n.tertiary }),
        );
        ui.painter().galley(
            egui::pos2(
                r.center().x - galley.size().x / 2.0,
                r.center().y - galley.size().y / 2.0 - 0.5,
            ),
            galley,
            n.text,
        );
    }
}

/// Keys as key caps ("Ctrl+Z / Ctrl+Y" is one shortcut or the other); a description in
/// their place ("Click inside a closed area") as text.
pub fn keycaps(ui: &mut egui::Ui, keys: &str) {
    ui.spacing_mut().item_spacing.x = 4.0;
    for (i, piece) in keys.split(" / ").enumerate() {
        if i > 0 {
            ui.label(egui::RichText::new("/").weak());
        }
        let is_key = !piece.contains(' ') || piece.len() <= 3;
        if is_key {
            keycap(ui, piece);
        } else {
            ui.label(egui::RichText::new(piece).weak());
        }
    }
}

/// The heights of the frame's bars: the top bar (menus, sheet tabs, search), the options bar
/// under it and the status bar.
pub mod bar {
    pub const TOP: f32 = 40.0;
    pub const OPTIONS: f32 = 34.0;
    pub const STATUS: f32 = 28.0;
}

/// A hairline between groups of controls on a bar.
pub fn bar_divider(ui: &mut egui::Ui) {
    let slot = bar_divider_slot(ui);
    paint_bar_divider(ui, slot);
}

/// Room for a [`bar_divider`], to be painted (with [`paint_bar_divider`]) once it is known
/// that something follows it.
pub fn bar_divider_slot(ui: &mut egui::Ui) -> egui::Rect {
    ui.allocate_exact_size(vec2(9.0, 16.0), egui::Sense::hover())
        .0
}

pub fn paint_bar_divider(ui: &egui::Ui, slot: egui::Rect) {
    ui.painter().vline(
        slot.center().x,
        slot.y_range(),
        Stroke::new(1.0, hairline(ui)),
    );
}

/// The hairline between two groups of options on the options bar: with the bar's 6 points
/// either side of it, the groups are 12 apart.
pub fn group_divider(ui: &mut egui::Ui) {
    let slot = group_divider_slot(ui);
    paint_bar_divider(ui, slot);
}

/// Room for a [`group_divider`], to be painted (with [`paint_bar_divider`]) once it is known
/// that something follows it.
pub fn group_divider_slot(ui: &mut egui::Ui) -> egui::Rect {
    ui.allocate_exact_size(vec2(1.0, 16.0), egui::Sense::hover())
        .0
}

/// A small caption before a control on a bar.
pub fn bar_caption(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(
        egui::RichText::new(text)
            .size(size::LABEL)
            .color(ui.visuals().weak_text_color()),
    )
}

/// The line between two panes, along one edge of `pane` (painted by the frame: egui's own
/// separator can't be told apart from a hairline inside a pane).
pub fn pane_edge(ui: &egui::Ui, pane: egui::Rect, edge: egui::Align2) {
    let stroke = Stroke::new(1.0, divider(ui.visuals().dark_mode));
    let p = ui.painter();
    match edge {
        egui::Align2::CENTER_TOP => p.hline(pane.x_range(), pane.top() + 0.5, stroke),
        egui::Align2::CENTER_BOTTOM => p.hline(pane.x_range(), pane.bottom() - 0.5, stroke),
        egui::Align2::LEFT_CENTER => p.vline(pane.left() + 0.5, pane.y_range(), stroke),
        _ => p.vline(pane.right() - 0.5, pane.y_range(), stroke),
    };
}

/// What a button on a bar does, as its tooltip: its name and keys, then a line about it.
pub fn command_tip(ui: &mut egui::Ui, name: &str, keys: &str, about: &str) {
    ui.set_max_width(280.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(name)
                .family(bold())
                .color(ui.visuals().strong_text_color()),
        );
        if !keys.is_empty() {
            keycap(ui, keys);
        }
    });
    if !about.is_empty() {
        ui.label(egui::RichText::new(about).color(ui.visuals().weak_text_color()));
    }
}

/// Space between a row's label column and its value.
pub const LABEL_WIDTH: f32 = 88.0;

/// Height of a label / value row: a control's, so rows of text, fields and menus keep one
/// rhythm down a panel.
pub const ROW_HEIGHT: f32 = 24.0;

/// Height of a row in a list (layers, sheets, blocks, relations).
pub const LIST_ROW: f32 = 26.0;

/// A label / value row in an inspector grid: the label in the secondary tone in a column of
/// its own, the value in what is left of the width (fields and menus fill it).
pub fn row(
    ui: &mut egui::Ui,
    label: &str,
    add: impl FnOnce(&mut egui::Ui),
) -> egui::InnerResponse<()> {
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_HEIGHT);
        ui.allocate_ui_with_layout(
            Vec2::new(LABEL_WIDTH, ROW_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(LABEL_WIDTH);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(label).color(ui.visuals().weak_text_color()),
                    )
                    .truncate(),
                );
            },
        );
        add(ui);
    })
}

/// A [`row`] whose value needs `least` points of width (a segmented control, two controls
/// side by side): beside its label where the pane has the room, under it where it hasn't,
/// so a narrow pane never has to grow for it.
pub fn wide_row(ui: &mut egui::Ui, label: &str, least: f32, add: impl FnOnce(&mut egui::Ui)) {
    let room = ui.available_width() - LABEL_WIDTH - ui.spacing().item_spacing.x;
    if room >= least {
        row(ui, label, add);
    } else {
        ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color()));
        ui.horizontal(|ui| {
            ui.set_min_height(ROW_HEIGHT);
            add(ui);
        });
    }
}

/// The width of a button with this text, as egui lays it out.
pub fn button_width(ui: &egui::Ui, text: &str) -> f32 {
    let font = TextStyle::Button.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .size()
        .x
        + 2.0 * ui.spacing().button_padding.x
}

/// A small button with a chevron that opens a menu of choices for the field beside it
/// (presets). `label` is what it is called.
pub fn menu_chevron<R>(
    ui: &mut egui::Ui,
    label: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(ROW_HEIGHT), egui::Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let n = neutrals(ui.visuals().dark_mode);
    let fill = if response.is_pointer_button_down_on() {
        n.pressed
    } else if response.hovered() {
        n.hover
    } else {
        n.control
    };
    ui.painter().rect(
        rect,
        radius::CONTROL,
        fill,
        Stroke::new(1.0, n.control_border),
        egui::StrokeKind::Inside,
    );
    chevron(ui.painter(), rect.center(), true, n.text);
    egui::Popup::menu(&response).show(add);
    response
}

/// A line of explanation under a title or a control: small, in the secondary tone, wrapped.
pub fn hint(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(
        egui::RichText::new(text)
            .size(size::CAPTION)
            .color(ui.visuals().weak_text_color()),
    )
}

/// A chevron, 8 points across: down when `open`, right when folded (a fold, a floating
/// panel's header, a layer's row, a menu's button).
pub fn chevron(painter: &egui::Painter, c: egui::Pos2, open: bool, color: Color32) {
    let pts = if open {
        vec![
            c + vec2(-4.0, -2.0),
            c + vec2(0.0, 2.0),
            c + vec2(4.0, -2.0),
        ]
    } else {
        vec![
            c + vec2(-2.0, -4.0),
            c + vec2(2.0, 0.0),
            c + vec2(-2.0, 4.0),
        ]
    };
    painter.add(egui::Shape::line(pts, Stroke::new(1.4, color)));
}

/// The fill behind a row of a list: the picked (or current) one on a faint accent tint, the
/// one under the pointer on a quiet neutral. The one look of a selected row, in every panel.
pub fn row_fill(dark: bool, selected: bool, hovered: bool) -> Option<Color32> {
    if selected {
        Some(accent_soft(dark))
    } else if hovered {
        Some(neutrals(dark).faint)
    } else {
        None
    }
}

/// Paint [`row_fill`] behind `rect`.
pub fn paint_row(ui: &egui::Ui, rect: egui::Rect, selected: bool, hovered: bool) {
    if let Some(fill) = row_fill(ui.visuals().dark_mode, selected, hovered) {
        ui.painter().rect_filled(rect, radius::CONTROL, fill);
    }
}

/// A small × that removes the row it ends (a relation, a variable, an option, a revision):
/// quiet until pointed at. `label` is what it is called, `tip` what it removes.
pub fn delete_button(ui: &mut egui::Ui, label: &str, tip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let dark = ui.visuals().dark_mode;
    let n = neutrals(dark);
    let color = if response.hovered() && enabled {
        let fill = if response.is_pointer_button_down_on() {
            n.pressed
        } else {
            n.hover
        };
        ui.painter().rect_filled(rect, radius::CONTROL, fill);
        error_color(dark)
    } else {
        n.tertiary
    };
    let c = rect.center();
    let d = 3.5;
    let stroke = Stroke::new(1.3, color);
    ui.painter()
        .line_segment([c + vec2(-d, -d), c + vec2(d, d)], stroke);
    ui.painter()
        .line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
    response.on_hover_text(tip)
}

/// A soft tile in the accent's tint with an icon painted in the accent: what an inspector's
/// title starts with. Returns the icon's rect.
pub fn icon_tile(ui: &mut egui::Ui, side: f32) -> egui::Rect {
    let (tile, _) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::hover());
    ui.painter()
        .rect_filled(tile, 6.0, accent_soft(ui.visuals().dark_mode));
    tile.shrink((side * 0.22).round())
}

/// A figure on a quiet tile: a number over what it counts.
pub fn stat(ui: &mut egui::Ui, value: usize, caption: &str, width: f32) {
    let n = neutrals(ui.visuals().dark_mode);
    let (r, _) = ui.allocate_exact_size(vec2(width, 48.0), egui::Sense::hover());
    ui.painter().rect_filled(r, radius::CONTROL + 1, n.faint);
    ui.painter().text(
        egui::pos2(r.left() + 10.0, r.top() + 17.0),
        egui::Align2::LEFT_CENTER,
        value.to_string(),
        FontId::new(16.0, bold()),
        n.text,
    );
    ui.painter().text(
        egui::pos2(r.left() + 10.0, r.bottom() - 13.0),
        egui::Align2::LEFT_CENTER,
        caption,
        FontId::new(size::CAPTION, FontFamily::Proportional),
        n.secondary,
    );
}

/// Buttons of one width across the pane, in one row (or in rows of `per_row` when the pane
/// is too narrow for their names). Returns the index of the one clicked, and each one's
/// response to `each` (for its tooltip).
pub fn button_row(
    ui: &mut egui::Ui,
    labels: &[&str],
    mut each: impl FnMut(usize, egui::Response) -> egui::Response,
) -> Option<usize> {
    let gap = 4.0;
    let font = FontId::new(size::LABEL, FontFamily::Proportional);
    let widest = labels
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap((*l).to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max);
    let room = ui.available_width();
    let fits =
        |across: usize| (room - gap * (across as f32 - 1.0)) / across as f32 >= widest + 10.0;
    // All in a row, or else in two rows (then one each).
    let across = if fits(labels.len()) {
        labels.len()
    } else if fits(labels.len().div_ceil(2)) {
        labels.len().div_ceil(2)
    } else {
        1
    };
    let width = ((room - gap * (across as f32 - 1.0)) / across as f32).floor();
    let mut clicked = None;
    for (k, line) in labels.chunks(across).enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.spacing_mut().button_padding.x = 2.0;
            for (j, label) in line.iter().enumerate() {
                let i = k * across + j;
                let button = egui::Button::new(egui::RichText::new(*label).size(size::LABEL))
                    .min_size(vec2(width, ROW_HEIGHT));
                let response = each(i, ui.add_sized([width, ROW_HEIGHT], button));
                if response.clicked() {
                    clicked = Some(i);
                }
            }
        });
    }
    clicked
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
    let height = 16.0_f32.min(below + 4.0);
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
        ui.allocate_exact_size(vec2(ui.available_width(), LIST_ROW), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });
    let dark = ui.visuals().dark_mode;
    let n = neutrals(dark);
    paint_row(ui, rect, selected, response.hovered());
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
    egui::Frame::window(style).inner_margin(style.spacing.window_margin)
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
    if response.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, radius::CONTROL, n.pressed);
    } else if response.hovered() {
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
    ui.add_space(8.0);
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
            ui.spacing_mut().item_spacing.x = 8.0;
            add(ui)
        },
    )
    .inner
}

/// The primary action: the one accent-filled button in a dialog or panel.
pub fn primary_button(label: &str, dark: bool) -> egui::Button<'static> {
    // White on the blue of Midnight and Light; dark on Obsidian's orange.
    let ink = if !dark || dark_look() == DarkLook::Midnight {
        Color32::WHITE
    } else {
        Color32::from_rgb(0x1c, 0x1c, 0x1c)
    };
    egui::Button::new(egui::RichText::new(label.to_owned()).color(ink))
        .fill(accent(dark))
        .stroke(Stroke::NONE)
}

/// The appearance (follow the system, Midnight, Obsidian or Light), as a segmented control.
pub fn appearance(ui: &mut egui::Ui) {
    let current = Look::current(ui.ctx());
    let options = Look::ALL.map(|l| (l, l.label()));
    if let Some(look) = segmented(ui, current, &options) {
        set_look(ui.ctx(), look);
    }
}

/// A label from the drawing's model (a line type, an alignment, a sheet's colours) in the
/// interface's US spelling: the model keeps its own names.
pub fn us(label: &str) -> &str {
    match label {
        "Centre" => "Center",
        "Colour" => "Color",
        "Greyscale" => "Grayscale",
        other => other,
    }
}

/// Space either side of a dock panel's contents: one margin, whatever the panel shows.
pub const PANE_MARGIN: f32 = 12.0;

/// A scrolled part of a dock panel. The area reaches into the pane's right margin, so its
/// bar floats there: the contents are as wide scrolled as not, and nothing is laid out
/// around the bar.
pub fn pane_scroll<R>(
    ui: &mut egui::Ui,
    area: egui::ScrollArea,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    pane_scroll_viewport(ui, area, |ui, _| add(ui))
}

/// [`pane_scroll`], told which part of the contents is in view.
pub fn pane_scroll_viewport<R>(
    ui: &mut egui::Ui,
    area: egui::ScrollArea,
    add: impl FnOnce(&mut egui::Ui, egui::Rect) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    let room = ui.available_rect_before_wrap();
    let wide = room.with_max_x(room.right() + PANE_MARGIN);
    let scroll = ui.spacing().scroll;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(wide)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.spacing_mut().scroll.floating_allocated_width = 0.0;
    let output = area.show_viewport(&mut child, |ui, viewport| {
        ui.spacing_mut().scroll = scroll;
        ui.set_max_width(room.width());
        add(ui, viewport)
    });
    // (The pane is as wide as it was: only what is inside the margin counts.)
    let used = child.min_rect().intersect(room);
    ui.allocate_rect(used, egui::Sense::hover());
    output
}

/// The text margin of a field: a control's own padding (`button_padding.x`), so its text
/// starts exactly where a menu's does and the two share one left edge down a column.
pub const FIELD_MARGIN: Margin = Margin {
    left: 9,
    right: 9,
    top: 2,
    bottom: 2,
};

/// A text field: its text as far in as a menu's is, its hint in the tertiary tone.
pub fn field(ui: &mut egui::Ui, edit: egui::TextEdit<'_>) -> egui::Response {
    let weak = ui.visuals().weak_text_color;
    ui.visuals_mut().weak_text_color = Some(neutrals(ui.visuals().dark_mode).tertiary);
    let response = ui.add(edit.margin(FIELD_MARGIN));
    ui.visuals_mut().weak_text_color = weak;
    response
}

/// A number field (dragged or typed) that fills the width it is given, drawn as a field:
/// its right edge is the column's.
pub fn drag(ui: &mut egui::Ui, value: egui::DragValue<'_>) -> egui::Response {
    drag_sized(ui, ui.available_width(), value)
}

/// [`drag`] at a width.
pub fn drag_sized(ui: &mut egui::Ui, width: f32, value: egui::DragValue<'_>) -> egui::Response {
    let n = neutrals(ui.visuals().dark_mode);
    let height = ui.spacing().interact_size.y;
    // (Laid out from the left, so the number starts where a text field's text does.)
    let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min);
    ui.allocate_ui_with_layout(vec2(width, height), layout, |ui| {
        let sp = ui.spacing_mut();
        sp.interact_size.x = width;
        sp.button_padding.x = f32::from(FIELD_MARGIN.left);
        let w = &mut ui.visuals_mut().widgets;
        for wv in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            wv.weak_bg_fill = n.field;
            wv.bg_fill = n.field;
        }
        ui.add(value)
    })
    .inner
}

/// The one chevron of every menu button (a `ComboBox`'s icon): painted, 8 points, in the
/// secondary tone.
pub fn combo_icon(ui: &egui::Ui, rect: egui::Rect, _: &egui::style::WidgetVisuals, _open: bool) {
    let n = neutrals(ui.visuals().dark_mode);
    let color = if ui.is_enabled() {
        n.secondary
    } else {
        n.tertiary
    };
    chevron(ui.painter(), rect.center(), true, color);
}

/// A search field: the search glyph at its left (as the top bar's has), then what is typed
/// or, empty, `hint` in the tertiary tone.
pub fn search_field(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    let n = neutrals(ui.visuals().dark_mode);
    let weak = ui.visuals().weak_text_color;
    ui.visuals_mut().weak_text_color = Some(n.tertiary);
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint.to_owned())
            .desired_width(width)
            .min_size(vec2(0.0, ROW_HEIGHT))
            .vertical_align(egui::Align::Center)
            .margin(Margin {
                left: 28,
                ..FIELD_MARGIN
            }),
    );
    ui.visuals_mut().weak_text_color = weak;
    crate::icons::glyph(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(response.rect.left() + 15.0, response.rect.center().y),
            Vec2::splat(13.0),
        ),
        crate::icons::Glyph::Search,
        n.secondary,
    );
    response
}

/// The presets of the field above them (an angle's 0 / 90 / 180 / 270), as a row of buttons
/// of one width under it, in the value column. Returns the one clicked.
pub fn preset_row(ui: &mut egui::Ui, labels: &[&str]) -> Option<usize> {
    let mut clicked = None;
    row(ui, "", |ui| {
        ui.vertical(|ui| clicked = button_row(ui, labels, |_, response| response));
    });
    clicked
}

/// What a checkbox or a radio button shows in its box.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    Tick,
    Dash,
    Dot,
}

/// A 14 point box (or ring, when `round`) with its mark, then the text: the body of
/// [`Checkbox`] and [`radio`]. The box is a field with the control border (it shows on a
/// pane and on what floats above one, in every look); the mark is in the accent.
fn mark_row(
    ui: &mut egui::Ui,
    text: egui::WidgetText,
    mark: Mark,
    round: bool,
    info: impl Fn(bool, &str) -> egui::WidgetInfo,
) -> egui::Response {
    let label = text.text().to_owned();
    let gap = ui.spacing().icon_spacing;
    let room = (ui.available_width() - 14.0 - gap).max(0.0);
    let galley = (!label.is_empty()).then(|| text.into_galley(ui, None, room, TextStyle::Body));
    let text_size = galley.as_ref().map_or(Vec2::ZERO, |g| g.size());
    let height = ui.spacing().interact_size.y.max(text_size.y);
    let width = if galley.is_some() {
        14.0 + gap + text_size.x
    } else {
        // (On its own it is a control's square, as egui's is.)
        ui.spacing().interact_size.y
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| info(enabled, &label));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let dark = ui.visuals().dark_mode;
    let n = neutrals(dark);
    let at = if galley.is_some() {
        egui::pos2(rect.left() + 7.0, rect.center().y)
    } else {
        rect.center()
    };
    let edge = if response.has_focus() {
        accent(dark)
    } else if response.hovered() && enabled {
        n.tertiary
    } else {
        n.control_border
    };
    let stroke = Stroke::new(1.0, edge);
    if round {
        ui.painter().circle(at, 6.5, n.field, stroke);
    } else {
        ui.painter().rect(
            egui::Rect::from_center_size(at, Vec2::splat(14.0)),
            4.0,
            n.field,
            stroke,
            egui::StrokeKind::Inside,
        );
    }
    let ink = if enabled { accent(dark) } else { n.tertiary };
    match mark {
        Mark::None => {}
        Mark::Tick => {
            ui.painter().add(egui::Shape::line(
                vec![
                    at + vec2(-3.5, 0.2),
                    at + vec2(-1.0, 2.8),
                    at + vec2(3.6, -2.8),
                ],
                Stroke::new(1.8, ink),
            ));
        }
        Mark::Dash => {
            ui.painter()
                .hline(at.x - 3.5..=at.x + 3.5, at.y, Stroke::new(1.8, ink));
        }
        Mark::Dot => {
            ui.painter().circle_filled(at, 3.5, ink);
        }
    }
    if let Some(galley) = galley {
        let color = if !enabled {
            n.tertiary
        } else if response.hovered() {
            n.text
        } else {
            ui.visuals().widgets.inactive.text_color()
        };
        ui.painter().galley(
            egui::pos2(
                rect.left() + 14.0 + gap,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
    response
}

/// A checkbox in the interface's own look (egui's can't be themed to it): a 14 point box
/// with the control border, ticked in the accent.
pub struct Checkbox<'a> {
    checked: &'a mut bool,
    text: egui::WidgetText,
    indeterminate: bool,
}

impl<'a> Checkbox<'a> {
    pub fn new(checked: &'a mut bool, text: impl Into<egui::WidgetText>) -> Self {
        Self {
            checked,
            text: text.into(),
            indeterminate: false,
        }
    }

    /// Neither on nor off (what it stands for is mixed): a dash.
    pub fn indeterminate(mut self, indeterminate: bool) -> Self {
        self.indeterminate = indeterminate;
        self
    }
}

impl egui::Widget for Checkbox<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let Checkbox {
            checked,
            text,
            indeterminate,
        } = self;
        let mark = if indeterminate {
            Mark::Dash
        } else if *checked {
            Mark::Tick
        } else {
            Mark::None
        };
        let on = *checked;
        let mut response = mark_row(ui, text, mark, false, |enabled, label| {
            if indeterminate {
                egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, enabled, label)
            } else {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, label)
            }
        });
        if response.clicked() {
            *checked = !*checked;
            response.mark_changed();
        }
        response
    }
}

/// [`Checkbox`], as `ui.checkbox` is to egui's.
pub fn checkbox(
    ui: &mut egui::Ui,
    checked: &mut bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.add(Checkbox::new(checked, text))
}

/// A radio button in the interface's own look: a 14 point ring with the control border,
/// its dot in the accent.
pub fn radio(ui: &mut egui::Ui, on: bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let mark = if on { Mark::Dot } else { Mark::None };
    mark_row(ui, text.into(), mark, true, |enabled, label| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, on, label)
    })
}

/// [`radio`] that sets `current` to `value` when clicked, as `ui.radio_value` does.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn radio_value<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let mut response = radio(ui, *current == value, text);
    if response.clicked() && *current != value {
        *current = value;
        response.mark_changed();
    }
    response
}

/// A slider in the interface's own look, in two steps. This one adds egui's slider (it
/// keeps the dragging, the keys and what assistive tech reads) with its own rail and handle
/// painted in nothing; [`paint_slider`] then draws ours over it, once the value is known.
pub fn slider_bare(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> egui::Response {
    ui.scope(|ui| {
        let clear = Color32::TRANSPARENT;
        let v = ui.visuals_mut();
        v.selection.bg_fill = clear;
        for wv in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.noninteractive,
        ] {
            wv.bg_fill = clear;
            wv.fg_stroke = Stroke::NONE;
            wv.expansion = 0.0;
        }
        ui.add(slider.show_value(false).trailing_fill(false))
    })
    .inner
}

/// The look of a slider added with [`slider_bare`]: a 3 point track in the field's tone,
/// filled with the accent up to a 12 point round handle. `at` is where the value sits along
/// it (0 to 1).
pub fn paint_slider(ui: &egui::Ui, response: &egui::Response, at: f32) {
    let rect = response.rect;
    if !ui.is_rect_visible(rect) {
        return;
    }
    let dark = ui.visuals().dark_mode;
    let n = neutrals(dark);
    // (The handle travels where egui's does: the rect less its own handle's radius.)
    let inset = rect.height() / 2.5;
    let at = if at.is_finite() {
        at.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x = egui::lerp(rect.left() + inset..=rect.right() - inset, at);
    let y = rect.center().y;
    let track = egui::Rect::from_min_max(
        egui::pos2(rect.left(), y - 1.5),
        egui::pos2(rect.right(), y + 1.5),
    );
    let p = ui.painter();
    p.rect(
        track,
        1.5,
        n.field,
        Stroke::new(1.0, n.control_border),
        egui::StrokeKind::Outside,
    );
    let ink = if ui.is_enabled() {
        accent(dark)
    } else {
        n.tertiary
    };
    p.rect_filled(track.with_max_x(x), 1.5, ink);
    let edge = if response.has_focus() || response.dragged() {
        accent(dark)
    } else {
        n.control_border
    };
    let fill = if response.dragged() {
        n.pressed
    } else if response.hovered() {
        n.hover
    } else {
        n.control
    };
    p.circle(egui::pos2(x, y), 6.0, fill, Stroke::new(1.0, edge));
}

/// A slider over a range with its value as a number field beside it (`suffix` after the
/// number), across the width it is given; `logarithmic` as egui's, for a range above zero.
pub fn value_slider<N: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut N,
    range: std::ops::RangeInclusive<N>,
    suffix: &str,
    logarithmic: bool,
) -> egui::Response {
    let (lo, hi) = (range.start().to_f64(), range.end().to_f64());
    // The number's field, at the row's end.
    let number_width = 64.0;
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let width = ui.spacing().slider_width;
        ui.spacing_mut().slider_width = (ui.available_width() - number_width - gap).max(60.0);
        let response = slider_bare(
            ui,
            egui::Slider::new(value, range.clone()).logarithmic(logarithmic),
        );
        ui.spacing_mut().slider_width = width;
        let now = value.to_f64();
        let at = if logarithmic && lo > 0.0 {
            (now / lo).ln() / (hi / lo).ln()
        } else {
            (now - lo) / (hi - lo)
        };
        paint_slider(ui, &response, at as f32);
        let span = hi - lo;
        let number = drag_sized(
            ui,
            number_width,
            egui::DragValue::new(value)
                .range(range)
                .speed(span / 200.0)
                .max_decimals(if span > 20.0 { 0 } else { 2 })
                .suffix(suffix.to_owned()),
        );
        response | number
    })
    .inner
}

/// A chip that is on or off (a property of what is picked): on, a faint accent tint with
/// the accent's edge and a filled dot; off, a quiet outline. `label` is what it shows and
/// what it is called.
pub fn toggle_chip(ui: &mut egui::Ui, on: bool, label: &str) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let n = neutrals(dark);
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::new(size::LABEL, FontFamily::Proportional),
        n.text,
    );
    let size = vec2(galley.size().x + 30.0, ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let enabled = ui.is_enabled();
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, label));
    let (fill, edge) = if on {
        (accent_soft(dark), accent(dark).gamma_multiply(0.7))
    } else if response.is_pointer_button_down_on() {
        (n.pressed, n.control_border)
    } else if response.hovered() {
        (n.hover, n.control_border)
    } else {
        (Color32::TRANSPARENT, n.control_border)
    };
    ui.painter().rect(
        rect,
        radius::CONTROL,
        fill,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    // A dot that fills when it is on: the state at a glance, in either theme.
    let dot = egui::pos2(rect.left() + 11.0, rect.center().y);
    if on {
        ui.painter().circle_filled(dot, 3.5, accent(dark));
    } else {
        ui.painter()
            .circle_stroke(dot, 3.0, Stroke::new(1.0, n.tertiary));
    }
    let color = if on || response.hovered() {
        n.text
    } else {
        n.secondary
    };
    ui.painter().galley(
        egui::pos2(rect.left() + 21.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative luminance of an opaque colour (WCAG 2).
    fn luminance(c: Color32) -> f64 {
        let lin = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Every tone of text reads (4.5:1, WCAG AA) on a pane and on what floats above one, in
    /// each look: captions and disabled text too, which once didn't.
    #[test]
    fn text_reads_on_panes_and_on_what_floats_in_every_look() {
        for skin in Skin::ALL {
            let n = skin.neutrals();
            for (surface, fill) in [("panel", n.panel), ("raised", n.raised)] {
                for (tone, color) in [
                    ("text", n.text),
                    ("secondary", n.secondary),
                    ("tertiary", n.tertiary),
                ] {
                    let ratio = contrast(color, fill);
                    assert!(
                        ratio >= 4.5,
                        "{skin:?}: {tone} on {surface} is {ratio:.2}:1, under 4.5:1"
                    );
                }
            }
        }
    }

    /// The surfaces step up from the canvas: the well is the darkest in the dark looks (and
    /// the lightest in Light), the panes are above it and what floats is above the panes.
    #[test]
    fn surfaces_step_up_from_the_canvas() {
        for skin in Skin::ALL {
            let n = skin.neutrals();
            let dark = skin != Skin::Light;
            let [r, g, b, _] = drift_render::Palette::themed(dark).background;
            let well = luminance(Color32::from_rgb(r, g, b));
            let (panel, raised) = (luminance(n.panel), luminance(n.raised));
            if dark {
                assert!(well < panel && panel < raised, "{skin:?}");
                assert!(luminance(n.divider) < well, "{skin:?}: the divider");
            } else {
                assert!(well > panel && raised > panel, "{skin:?}");
                assert!(luminance(n.divider) < panel, "{skin:?}: the divider");
            }
        }
    }

    /// A hairline shows on a pane and on what floats above one (a menu's separators, a
    /// dialog's sections), where a grey tuned to the panes alone sinks into the lighter fill.
    #[test]
    fn hairlines_show_on_panes_and_on_what_floats() {
        for skin in Skin::ALL {
            let n = skin.neutrals();
            for (surface, fill) in [("panel", n.panel), ("raised", n.raised)] {
                let ratio = contrast(n.border, fill);
                assert!(ratio >= 1.12, "{skin:?}: {ratio:.2}:1 on {surface}");
            }
        }
    }

    /// drift-edit fades underlays towards its own copy of the canvas colour (it can't read
    /// the palette): the copy is the canvas's.
    #[test]
    fn underlays_fade_towards_the_canvas() {
        for dark in [true, false] {
            let [r, g, b, _] = drift_render::Palette::themed(dark).background;
            assert_eq!(drift_edit::underlays::background(dark), [r, g, b]);
        }
    }

    #[test]
    fn a_shortcut_splits_into_its_keys() {
        assert_eq!(key_names("Ctrl+K"), ["Ctrl", "K"]);
        assert_eq!(key_names("Ctrl+Shift+Z"), ["Ctrl", "Shift", "Z"]);
        assert_eq!(key_names("L"), ["L"]);
        assert_eq!(key_names("+"), ["+"]);
        assert_eq!(key_names("Ctrl++"), ["Ctrl", "+"]);
        assert_eq!(key_names("Right-click"), ["Right-click"]);
        assert!(key_names("").is_empty());
    }
}
