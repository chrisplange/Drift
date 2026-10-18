//! Drafting settings (the Drafting section of File → Drawing settings), the inspector for a
//! picked dimension's prefix, suffix and tolerance, and the Style section where picked
//! dimensions and annotations set their own style over the drawing's.

use drift_core::{
    AnnotationRef, ArrowStyle, DraftStyle, LengthFormat, Standard, StyleOverride, Tolerance,
};

use crate::*;

/// A millimetre size field (paper sizes are always mm, whatever the display units).
fn mm_field(ui: &mut egui::Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .speed(0.05)
            .range(range)
            .max_decimals(3)
            .suffix(" mm"),
    )
    .changed()
}

impl DriftApp {
    /// Preferences' Drafting section: the drawing's dimension standard and its settings.
    pub(crate) fn drafting_section(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Drafting");
        let mut style = self.editor.doc.draft_style();
        let before = style;
        if let Some(standard) = theme::segmented(
            ui,
            style.standard,
            &[(Standard::Iso, "ISO 129"), (Standard::Asme, "ASME Y14.5")],
        ) && standard != style.standard
        {
            // A new preset, keeping the drawing's scale and alternate units.
            style = DraftStyle {
                scale: style.scale,
                alt_units: style.alt_units,
                color: style.color,
                ..DraftStyle::preset(standard)
            };
        }
        ui.add_space(4.0);
        theme::row(ui, "Scale", |ui| {
            ui.label("1 :");
            ui.add(
                egui::DragValue::new(&mut style.scale)
                    .speed(0.5)
                    .range(0.001..=100_000.0)
                    .max_decimals(3),
            )
            .on_hover_text("The drawing scale: drafting hatch patterns are sized by it");
        });
        theme::row(ui, "Text height", |ui| {
            mm_field(ui, &mut style.text_height, 1.0..=20.0);
        });
        self.drawing_font_row(ui);
        self.kept_fonts_note(ui);
        theme::row(ui, "Arrows", |ui| {
            egui::ComboBox::from_id_salt("arrow_style")
                .selected_text(style.arrow.label())
                .width(ui.available_width() - 6.0)
                .show_ui(ui, |ui| {
                    for a in ArrowStyle::ALL {
                        ui.selectable_value(&mut style.arrow, a, a.label());
                    }
                });
        });
        theme::row(ui, "Arrow size", |ui| {
            mm_field(ui, &mut style.arrow_size, 0.5..=20.0);
        });
        theme::row(ui, "Ext. gap", |ui| {
            mm_field(ui, &mut style.extension_gap, 0.0..=10.0);
        })
        .response
        .on_hover_text("Gap between the geometry and an extension line");
        theme::row(ui, "Ext. past", |ui| {
            mm_field(ui, &mut style.extension_overshoot, 0.0..=10.0);
        })
        .response
        .on_hover_text("How far extension lines run past the dimension line");
        theme::row(ui, "Text", |ui| {
            ui.checkbox(&mut style.aligned_text, "Aligned")
                .on_hover_text("ISO: above the dimension line and parallel to it");
        });
        theme::row(ui, "Zeros", |ui| {
            ui.checkbox(&mut style.trailing_zeros, "Trailing")
                .on_hover_text("Pad decimals to the display precision: 1.500");
            ui.checkbox(&mut style.leading_zero, "Leading")
                .on_hover_text("Write 0.5 rather than .5");
        });
        theme::row(ui, "Dual units", |ui| {
            let text = style.alt_units.map_or("None", LengthFormat::label);
            egui::ComboBox::from_id_salt("alt_units")
                .selected_text(text)
                .width(ui.available_width() - 6.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut style.alt_units, None, "None");
                    for f in LengthFormat::METRIC
                        .into_iter()
                        .chain(LengthFormat::IMPERIAL)
                    {
                        ui.selectable_value(&mut style.alt_units, Some(f), f.label());
                    }
                });
        });
        let dark = ui.visuals().dark_mode;
        theme::row(ui, "Colour", |ui| {
            if let Some(color) = crate::layers_ui::color_choice(ui, style.color, "Default", dark) {
                style.color = color;
            }
        })
        .response
        .on_hover_text(
            "What dimensions and notes are drawn in on the canvas. Default: white on the \
             dark canvas, charcoal on the light one",
        );
        if style != before {
            self.editor.doc.begin();
            self.editor.doc.set_draft_style(style);
            self.editor.commit("Drafting style");
        }
    }

    /// The inspector for a picked dimension: prefix, suffix and tolerance. Returns true if a
    /// dimension is picked.
    pub(crate) fn dimension_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(id) = self.selected_constraint else {
            return false;
        };
        let Some(c) = self.editor.doc.constraint(id).copied() else {
            return false;
        };
        if !c.is_dimension() {
            return false;
        }
        let angle = matches!(c, drift_core::Constraint::Angle { .. });
        let label = annotations::dimension_label(&self.editor.doc, Some(id), &c);
        let shown = match &label.stack {
            Some((u, l)) => format!("{} {u} / {l}{}", label.before, label.after),
            None => format!("{}{}", label.before, label.after),
        };
        crate::app::panels::title(
            ui,
            Some(ToolKind::Dimension),
            crate::constraint_label(&self.editor.doc, &c),
            &shown,
        );
        let now = ui.input(|i| i.time);
        // Driven: a reference that reports the geometry's size rather than setting it.
        let driven = self.editor.doc.is_driven(id);
        let mut want = driven;
        if ui
            .checkbox(&mut want, "Driven (reference)")
            .on_hover_text(
                "A driven dimension reports the geometry's size, in parentheses, and doesn't \
                 hold it. Untick to make it drive the geometry again",
            )
            .changed()
            && let Err(e) = self.editor.set_driven(id, want)
        {
            self.set_status(format!("Can't make it driving: it {e}"), true, now);
        }
        let mut note = self.editor.doc.dim_note(id).cloned().unwrap_or_default();
        let before = note.clone();
        theme::section(ui, "Text");
        theme::row(ui, "Prefix", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut note.prefix)
                    .hint_text("e.g. 4×")
                    .desired_width(ui.available_width() - 8.0),
            );
        });
        theme::row(ui, "Suffix", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut note.suffix)
                    .hint_text("e.g. THRU")
                    .desired_width(ui.available_width() - 8.0),
            );
        });
        dimension_chips(ui, &mut note);
        theme::row(ui, "Under line", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut note.below)
                    .hint_text("e.g. TO FACE OF STUD")
                    .desired_width(ui.available_width() - 8.0),
            )
            .on_hover_text("A line of text under the dimension line");
        });
        theme::row(ui, "Show instead", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut note.text)
                    .hint_text("e.g. EQ (the value still holds)")
                    .desired_width(ui.available_width() - 8.0),
            )
            .on_hover_text(
                "Text shown in place of the value, such as EQ or VARIES. The dimension still \
                 holds its value",
            );
        });
        if !note.text.is_empty() {
            let value = format!("{}{}", label.before, label.after);
            ui.label(
                egui::RichText::new(format!(
                    "Holds {}",
                    self.editor.dimension_edit_text(id).unwrap_or(value)
                ))
                .small()
                .weak(),
            );
        }
        theme::section(ui, "Tolerance");
        let kinds = [
            Tolerance::None,
            Tolerance::Symmetric(0.1),
            Tolerance::Deviation {
                upper: 0.1,
                lower: 0.0,
            },
            Tolerance::Limits {
                upper: 0.1,
                lower: 0.1,
            },
            Tolerance::Fit("H7".into()),
        ];
        egui::ComboBox::from_id_salt("tolerance_kind")
            .selected_text(note.tolerance.label())
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for k in kinds {
                    let same =
                        std::mem::discriminant(&k) == std::mem::discriminant(&note.tolerance);
                    if ui.selectable_label(same, k.label()).clicked() && !same {
                        note.tolerance = k;
                    }
                }
            });
        let units = self.editor.doc.units();
        // Tolerances are edited in the display unit (degrees for angles).
        let scale = if angle {
            1.0
        } else {
            units.length.mm_per_unit()
        };
        let unit = if angle { "°" } else { units.length.symbol() };
        let field = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
            theme::row(ui, label, |ui| {
                let mut shown = *v / scale;
                if ui
                    .add(
                        egui::DragValue::new(&mut shown)
                            .speed(0.001)
                            .max_decimals(4)
                            .suffix(format!(" {unit}")),
                    )
                    .changed()
                {
                    *v = shown * scale;
                }
            });
        };
        match &mut note.tolerance {
            Tolerance::None => {}
            Tolerance::Symmetric(t) => field(ui, "±", t),
            Tolerance::Deviation { upper, lower } | Tolerance::Limits { upper, lower } => {
                field(ui, "Upper +", upper);
                field(ui, "Lower −", lower);
            }
            Tolerance::Fit(f) => {
                theme::row(ui, "Fit", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(f)
                            .hint_text("H7, g6…")
                            .desired_width(80.0),
                    );
                });
            }
        }
        if note != before {
            self.editor.doc.begin();
            self.editor.doc.set_dim_note(id, Some(note));
            self.editor.commit("Dimension text");
        }
        self.visibility_section(ui);
        self.style_section(ui);
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Double-click the dimension on the canvas to change its value.")
                .small()
                .weak(),
        );
        true
    }
}

/// The quick texts for a dimension: before the value (×, Ø, ±) and after it (TYP, REF,
/// MIN, MAX, VIF, THRU), and EQ in place of the value. A chip already there comes off.
pub(crate) fn dimension_chips(ui: &mut egui::Ui, note: &mut drift_core::DimNote) {
    // (text, where it goes: -1 before, 1 after, 0 instead of the value, what it means)
    const CHIPS: [(&str, i8, &str); 10] = [
        ("×", -1, "Before the value: a count, such as 4×"),
        ("Ø", -1, "Before the value: a diameter"),
        ("±", -1, "Before the value"),
        ("TYP", 1, "Typical: the same elsewhere"),
        ("REF", 1, "Reference only"),
        ("MIN", 1, "Minimum"),
        ("MAX", 1, "Maximum"),
        ("VIF", 1, "Verify in field"),
        ("THRU", 1, "Through"),
        (
            "EQ",
            0,
            "Equal: shown instead of the value, which still holds",
        ),
    ];
    ui.horizontal_wrapped(|ui| {
        for (text, place, hint) in CHIPS {
            let on = match place {
                -1 => note.prefix.contains(text),
                1 => note.suffix.split_whitespace().any(|w| w == text),
                _ => note.text == text,
            };
            if ui.selectable_label(on, text).on_hover_text(hint).clicked() {
                match place {
                    0 => note.text = if on { String::new() } else { text.to_owned() },
                    -1 if on => note.prefix = note.prefix.replace(text, "").trim().to_owned(),
                    -1 => note.prefix.push_str(text),
                    _ if on => {
                        note.suffix = note
                            .suffix
                            .split_whitespace()
                            .filter(|w| *w != text)
                            .collect::<Vec<_>>()
                            .join(" ");
                    }
                    _ => {
                        if !note.suffix.is_empty() && !note.suffix.ends_with(' ') {
                            note.suffix.push(' ');
                        }
                        note.suffix.push_str(text);
                    }
                }
            }
        }
    });
}

/// One change to the picked dimensions' and annotations' own settings.
type StyleEdit = Box<dyn Fn(&mut StyleOverride)>;

/// A setting's row in the Drafting style section: the label (in the accent when the picked ones set it
/// themselves), the control, and a reset arrow when overridden. Returns whether reset was
/// clicked.
fn style_row(
    ui: &mut egui::Ui,
    label: &str,
    overridden: bool,
    add: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut reset = false;
    ui.horizontal(|ui| {
        let dark = ui.visuals().dark_mode;
        ui.allocate_ui_with_layout(
            egui::vec2(theme::LABEL_WIDTH, 20.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(theme::LABEL_WIDTH);
                let color = if overridden {
                    theme::accent(dark)
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.label(egui::RichText::new(label).color(color));
            },
        );
        add(ui);
        if overridden {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                reset = icons::button(ui, 22.0, false, "Reset", |p, r, c| {
                    icons::glyph(p, r, icons::Glyph::Reset, c)
                })
                .on_hover_text("Back to the drawing's setting")
                .clicked();
            });
        }
    });
    reset
}

impl DriftApp {
    /// The Drafting style section for the picked dimensions and annotations: each setting shows what
    /// they are drawn with (the drawing's style unless they set it themselves). Changing one
    /// makes it theirs, marked in the accent with a reset arrow beside it.
    pub(crate) fn style_section(&mut self, ui: &mut egui::Ui) {
        let refs = self.picked_annotations();
        let Some(&first) = refs.first() else {
            return;
        };
        let doc = &self.editor.doc;
        let drawing = doc.draft_style();
        let shown = doc.draft_style_for(first);
        let own: Vec<StyleOverride> = refs.iter().map(|r| doc.style_override(*r)).collect();
        let set = |f: fn(&StyleOverride) -> bool| own.iter().any(f);
        let overridden = own.iter().map(StyleOverride::count).max().unwrap_or(0);
        // What the picked ones are: dimensions (and ordinates) show numbers; notes have their
        // own text height in their Text section.
        let numbers = refs.iter().all(|r| match r {
            AnnotationRef::Dimension(_) => true,
            AnnotationRef::Annotation(id) => {
                matches!(
                    doc.annotation(*id),
                    Some(drift_core::Annotation::Ordinate { .. })
                )
            }
        });
        let notes = refs.iter().any(|r| match r {
            AnnotationRef::Annotation(id) => {
                matches!(
                    doc.annotation(*id),
                    Some(drift_core::Annotation::Note { .. })
                )
            }
            AnnotationRef::Dimension(_) => false,
        });
        let standard = match drawing.standard {
            Standard::Iso => "ISO 129",
            Standard::Asme => "ASME Y14.5",
        };
        let mut edit: Option<StyleEdit> = None;
        let mut open_settings = false;
        theme::section(ui, "Drafting style");
        ui.horizontal(|ui| {
            let text = if overridden == 0 {
                format!("The drawing's style ({standard})")
            } else {
                format!(
                    "The drawing's style ({standard}), {overridden} setting{} changed here",
                    if overridden == 1 { "" } else { "s" }
                )
            };
            ui.label(egui::RichText::new(text).small().weak());
        });
        ui.add_space(2.0);
        if !notes {
            let mut v = shown.text_height;
            if style_row(ui, "Text height", set(|o| o.text_height.is_some()), |ui| {
                if mm_field(ui, &mut v, 1.0..=20.0) {
                    edit = Some(Box::new(move |o| o.text_height = Some(v)));
                }
            }) {
                edit = Some(Box::new(|o| o.text_height = None));
            }
        }
        let mut arrow = shown.arrow;
        if style_row(ui, "Arrows", set(|o| o.arrow.is_some()), |ui| {
            egui::ComboBox::from_id_salt("own_arrow")
                .selected_text(arrow.label())
                .width(110.0)
                .show_ui(ui, |ui| {
                    for a in ArrowStyle::ALL {
                        if ui.selectable_value(&mut arrow, a, a.label()).clicked() {
                            edit = Some(Box::new(move |o| o.arrow = Some(a)));
                        }
                    }
                });
        }) {
            edit = Some(Box::new(|o| o.arrow = None));
        }
        let mut v = shown.arrow_size;
        if style_row(ui, "Arrow size", set(|o| o.arrow_size.is_some()), |ui| {
            if mm_field(ui, &mut v, 0.5..=20.0) {
                edit = Some(Box::new(move |o| o.arrow_size = Some(v)));
            }
        }) {
            edit = Some(Box::new(|o| o.arrow_size = None));
        }
        if numbers {
            let mut aligned = shown.aligned_text;
            if style_row(ui, "Text", set(|o| o.aligned_text.is_some()), |ui| {
                if ui
                    .checkbox(&mut aligned, "Aligned")
                    .on_hover_text("Above the dimension line and parallel to it (ISO)")
                    .changed()
                {
                    edit = Some(Box::new(move |o| o.aligned_text = Some(aligned)));
                }
            }) {
                edit = Some(Box::new(|o| o.aligned_text = None));
            }
            let (mut trailing, mut leading) = (shown.trailing_zeros, shown.leading_zero);
            let zeros = set(|o| o.trailing_zeros.is_some() || o.leading_zero.is_some());
            if style_row(ui, "Zeros", zeros, |ui| {
                if ui.checkbox(&mut trailing, "Trailing").changed() {
                    edit = Some(Box::new(move |o| o.trailing_zeros = Some(trailing)));
                }
                if ui.checkbox(&mut leading, "Leading").changed() {
                    edit = Some(Box::new(move |o| o.leading_zero = Some(leading)));
                }
            }) {
                edit = Some(Box::new(|o| {
                    o.trailing_zeros = None;
                    o.leading_zero = None;
                }));
            }
            let mut alt = shown.alt_units;
            if style_row(ui, "Dual units", set(|o| o.alt_units.is_some()), |ui| {
                egui::ComboBox::from_id_salt("own_alt_units")
                    .selected_text(alt.map_or("None", LengthFormat::label))
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        let options = std::iter::once(None).chain(
                            LengthFormat::METRIC
                                .into_iter()
                                .chain(LengthFormat::IMPERIAL)
                                .map(Some),
                        );
                        for f in options {
                            let label = f.map_or("None", LengthFormat::label);
                            if ui.selectable_value(&mut alt, f, label).clicked() {
                                edit = Some(Box::new(move |o| o.alt_units = Some(f)));
                            }
                        }
                    });
            }) {
                edit = Some(Box::new(|o| o.alt_units = None));
            }
        }
        // Colour: the drawing's, or one of its own.
        let own_color = own[0].color.flatten();
        let dark = ui.visuals().dark_mode;
        if style_row(ui, "Colour", set(|o| o.color.is_some()), |ui| {
            let custom = own_color.is_some();
            egui::ComboBox::from_id_salt("own_colour")
                .selected_text(if custom { "Custom" } else { "Drawing's" })
                .width(90.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(!custom, "Drawing's").clicked() && custom {
                        edit = Some(Box::new(|o| o.color = None));
                    }
                    if ui.selectable_label(custom, "Custom").clicked() && !custom {
                        let red = drift_core::Rgb([0xd0, 0x40, 0x40]);
                        edit = Some(Box::new(move |o| o.color = Some(Some(red))));
                    }
                });
            if let Some(c) = own_color {
                let c = crate::layers_ui::to_color32(Some(c), dark);
                let mut rgb = [c.r(), c.g(), c.b()];
                if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                    let c = drift_core::Rgb(rgb);
                    edit = Some(Box::new(move |o| o.color = Some(Some(c))));
                }
            }
        }) {
            edit = Some(Box::new(|o| o.color = None));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if overridden > 0
                && ui
                    .button("Reset all")
                    .on_hover_text("Back to the drawing's style for everything")
                    .clicked()
            {
                edit = Some(Box::new(|o| *o = StyleOverride::default()));
            }
            if ui
                .button("Drawing style…")
                .on_hover_text("Change the style for the whole drawing (File → Drawing settings)")
                .clicked()
            {
                open_settings = true;
            }
        });
        if let Some(edit) = edit {
            self.editor.doc.begin();
            for r in &refs {
                let mut o = self.editor.doc.style_override(*r);
                edit(&mut o);
                self.editor.doc.set_style_override(*r, o);
            }
            self.editor.commit("Annotation style");
        }
        if open_settings {
            self.show_drawing_settings = true;
        }
    }
}
