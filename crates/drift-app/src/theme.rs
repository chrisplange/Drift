//! Look and feel: the platform's UI font, a restrained palette with one accent colour, soft
//! corners and comfortable spacing, plus small helpers for consistent panels.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme,
    Vec2, vec2,
};

/// The accent colour (Apple's system blue).
pub fn accent(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x0a, 0x84, 0xff)
    } else {
        Color32::from_rgb(0x00, 0x7a, 0xff)
    }
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
    ctx.set_fonts(fonts);
}

/// Apply the light and dark styles.
pub fn apply(ctx: &egui::Context) {
    ctx.style_mut_of(Theme::Dark, |s| style(s, true));
    ctx.style_mut_of(Theme::Light, |s| style(s, false));
}

fn style(s: &mut egui::Style, dark: bool) {
    // Typography.
    s.text_styles = [
        (
            TextStyle::Small,
            FontId::new(11.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.5, FontFamily::Proportional),
        ),
        (TextStyle::Heading, FontId::new(17.0, bold())),
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
    ]
    .into();

    // Spacing: roomier than egui's defaults, like a native inspector.
    let sp = &mut s.spacing;
    sp.item_spacing = vec2(8.0, 6.0);
    sp.button_padding = vec2(10.0, 4.0);
    sp.interact_size = vec2(24.0, 24.0);
    sp.menu_margin = Margin::same(6);
    sp.window_margin = Margin::same(14);
    sp.combo_width = 120.0;
    sp.icon_spacing = 6.0;

    let v = &mut s.visuals;
    let (panel, canvasish, field, faint, fill, hover, press, line, text, weak) = if dark {
        (
            Color32::from_rgb(0x23, 0x24, 0x28),
            Color32::from_rgb(0x1b, 0x1d, 0x21),
            Color32::from_rgb(0x18, 0x19, 0x1c),
            Color32::from_rgb(0x2a, 0x2c, 0x31),
            Color32::from_rgb(0x30, 0x32, 0x38),
            Color32::from_rgb(0x3a, 0x3c, 0x43),
            Color32::from_rgb(0x46, 0x49, 0x51),
            Color32::from_rgb(0x34, 0x36, 0x3c),
            Color32::from_rgb(0xe8, 0xe8, 0xea),
            Color32::from_rgb(0x98, 0x9a, 0xa1),
        )
    } else {
        (
            Color32::from_rgb(0xf3, 0xf3, 0xf5),
            Color32::from_rgb(0xfb, 0xfb, 0xfa),
            Color32::WHITE,
            Color32::from_rgb(0xea, 0xea, 0xee),
            Color32::from_rgb(0xe3, 0xe3, 0xe8),
            Color32::from_rgb(0xd8, 0xd8, 0xde),
            Color32::from_rgb(0xcb, 0xcb, 0xd2),
            Color32::from_rgb(0xdc, 0xdc, 0xe1),
            Color32::from_rgb(0x1d, 0x1d, 0x1f),
            Color32::from_rgb(0x6e, 0x6e, 0x73),
        )
    };
    let accent = accent(dark);
    let _ = canvasish;
    v.panel_fill = panel;
    v.window_fill = panel;
    v.extreme_bg_color = field;
    v.text_edit_bg_color = Some(field);
    v.faint_bg_color = faint;
    v.code_bg_color = faint;
    v.weak_text_color = Some(weak);
    v.override_text_color = None;
    v.hyperlink_color = accent;
    v.selection.bg_fill = accent.gamma_multiply(if dark { 0.55 } else { 0.35 });
    v.selection.stroke = Stroke::new(1.0, if dark { Color32::WHITE } else { accent });
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, line);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 40 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 90 } else { 30 }),
    };
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;

    let radius = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = panel;
    w.noninteractive.weak_bg_fill = panel;
    // Separators and panel edges: a hairline in the panel's own tone, never white.
    w.noninteractive.bg_stroke = Stroke::new(1.0, line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, text);
    w.noninteractive.corner_radius = radius;
    for (wv, bg) in [
        (&mut w.inactive, fill),
        (&mut w.hovered, hover),
        (&mut w.active, press),
        (&mut w.open, hover),
    ] {
        wv.bg_fill = bg;
        wv.weak_bg_fill = bg;
        wv.bg_stroke = Stroke::NONE;
        wv.fg_stroke = Stroke::new(1.0, text);
        wv.corner_radius = radius;
        wv.expansion = 0.0;
    }
    w.hovered.bg_stroke = Stroke::new(1.0, line);
    w.inactive.fg_stroke = Stroke::new(1.0, text.gamma_multiply(0.92));
}

/// A small, quiet section header (like a macOS inspector's), with space above it.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .small()
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(2.0);
}

/// A small, letter-spaced all-caps header (as in macOS sidebars).
pub fn caps_header(ui: &mut egui::Ui, title: &str) {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &title.to_uppercase(),
        6.0,
        egui::TextFormat {
            font_id: egui::FontId::new(9.5, bold()),
            color: ui.visuals().weak_text_color().gamma_multiply(0.8),
            extra_letter_spacing: 1.6,
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
    let track = ui.visuals().widgets.inactive.bg_fill;
    let raised = if dark {
        Color32::from_rgb(0x5a, 0x5d, 0x66)
    } else {
        Color32::WHITE
    };
    let width = ui.available_width();
    let height = 26.0;
    let (rect, track_response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, 7.0, track);
    // Segments share the width in proportion to their labels (so long ones fit).
    let font = FontId::new(12.5, bold());
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
            ui.painter().rect_filled(r, 5.0, raised);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(r, 5.0, ui.visuals().widgets.hovered.bg_fill);
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
                12.5,
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
pub const LABEL_WIDTH: f32 = 78.0;

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
