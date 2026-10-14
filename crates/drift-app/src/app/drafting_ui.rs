//! Drafting settings (the Parameters tab's Drafting section) and the inspector for a picked
//! dimension's prefix, suffix and tolerance.

use drift_core::{ArrowStyle, DraftStyle, LengthFormat, Standard, Tolerance};

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
    /// The Parameters tab's Drafting section: the dimension standard and its settings.
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
        theme::row(ui, "Text", |ui| {
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
        ui.horizontal_wrapped(|ui| {
            for (text, prefix) in [("×", true), ("Ø", true), ("THRU", false), ("TYP", false)] {
                if ui
                    .small_button(text)
                    .on_hover_text(if prefix {
                        "Add to the prefix"
                    } else {
                        "Add to the suffix"
                    })
                    .clicked()
                {
                    let target = if prefix {
                        &mut note.prefix
                    } else {
                        &mut note.suffix
                    };
                    if !target.is_empty() && !target.ends_with(' ') && !prefix {
                        target.push(' ');
                    }
                    target.push_str(text);
                }
            }
        });
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
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Double-click the dimension on the canvas to change its value.")
                .small()
                .weak(),
        );
        true
    }
}
