//! Section and elevation marks linked to model views: Make view from a mark, linking one in
//! its Inspector, and the marks a view is referenced by.

use drift_core::{Annotation, AnnotationId, Bounds, View, ViewId};
use glam::DVec2;

use crate::*;

/// The paper (mm) a view made from a mark is scaled to fit.
const ROOM: DVec2 = DVec2::new(180.0, 120.0);

/// The crop (world mm) of a view made from a mark, or `None` for other annotations.
///
/// A section's runs along its cutting line and as deep as the model goes the way it looks
/// (between a fifth of the cut's length and its whole length; half of it with no model
/// there). An elevation's is the model in front of the mark, or a square of `fallback` with
/// nothing there. A crop that isn't square to the axes takes the box round it.
pub(crate) fn mark_crop(a: &Annotation, model: Bounds, fallback: f64) -> Option<Bounds> {
    let corners = (!model.is_empty()).then(|| {
        [
            model.min,
            DVec2::new(model.max.x, model.min.y),
            model.max,
            DVec2::new(model.min.x, model.max.y),
        ]
    });
    // How far the model reaches from `from` along `dir`, at most and at least.
    let reach = |from: DVec2, dir: DVec2| {
        corners.map(|c| {
            let d = c.map(|p| (p - from).dot(dir));
            (
                d.iter().copied().fold(f64::INFINITY, f64::min),
                d.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            )
        })
    };
    let (origin, along, look, across, depth) = match a {
        Annotation::Section { a, b, flip, .. } => {
            let len = a.distance(*b);
            if len < 1e-9 {
                return None;
            }
            let along = (*b - *a) / len;
            let look = along.perp() * if *flip { -1.0 } else { 1.0 };
            let depth = reach(*a, look).map_or(len * 0.5, |(_, far)| far.clamp(len * 0.2, len));
            (*a, along, look, (0.0, len), depth)
        }
        Annotation::Elevation { at, angle, .. } => {
            let look = DVec2::from_angle(angle.to_radians());
            let along = -look.perp();
            let (across, depth) = match (reach(*at, along), reach(*at, look)) {
                (Some(across), Some((_, far))) if far > 1e-9 && across.1 > across.0 => {
                    (across, far)
                }
                _ => ((-fallback * 0.5, fallback * 0.5), fallback),
            };
            (*at, along, look, across, depth)
        }
        _ => return None,
    };
    let mut crop = Bounds::from_point(origin + along * across.0);
    for p in [
        origin + along * across.1,
        origin + along * across.0 + look * depth,
        origin + along * across.1 + look * depth,
    ] {
        crop.include(p);
    }
    Some(crop)
}

/// The name of a view made from a mark: `Section A`, `Elevation 1`.
fn mark_view_name(a: &Annotation) -> String {
    let (kind, label) = match a {
        Annotation::Section { label, .. } => ("Section", label),
        Annotation::Elevation { label, .. } => ("Elevation", label),
        _ => ("View", &String::new()),
    };
    match label.trim() {
        "" => kind.to_owned(),
        l => format!("{kind} {l}"),
    }
}

impl DriftApp {
    /// Make a model view from a section or elevation mark and link the mark to it, in one
    /// undo step. The view is picked in the Views list (not placed on a sheet).
    pub(crate) fn make_view_from_mark(&mut self, id: AnnotationId) -> Option<ViewId> {
        let a = self.drawing.editor.doc.annotation(id)?.clone();
        let fallback = self.fit_bounds().size().max_element().max(1.0);
        let crop = mark_crop(&a, self.drawing.editor.bounds(), fallback)?;
        let scale = drift_core::sheets::fit_scale(crop.size(), ROOM, self.imperial());
        let mut linked = a.clone();
        self.drawing.editor.doc.begin();
        let view = self
            .drawing
            .editor
            .doc
            .add_view(View::new(mark_view_name(&a), crop, scale));
        linked.set_view(Some(view));
        let _ = self.drawing.editor.doc.set_annotation(id, linked);
        self.drawing.editor.commit("Make view");
        self.pick_view_alone(view);
        self.show_panel(crate::dock::Panel::Sheets);
        Some(view)
    }

    /// Pick a mark in the model (from a view's Referenced by list).
    pub(crate) fn pick_mark(&mut self, id: AnnotationId) {
        self.show_sheet(None);
        self.drawing.editor.selection.clear();
        self.drawing.editor.selected_fills.clear();
        self.drawing.editor.selected_instances.clear();
        self.drawing.editor.annotation_picks.clear();
        self.drawing.selected_constraint = None;
        self.drawing.picked_view = None;
        self.drawing.editor.selected_annotation = Some(id);
        self.show_panel(crate::dock::Panel::Inspector);
    }

    /// The marks that point at a view, as rows that pick them in the model.
    pub(crate) fn referenced_by(&mut self, ui: &mut egui::Ui, view: ViewId) {
        let doc = &self.drawing.editor.doc;
        let marks: Vec<(AnnotationId, String)> = doc
            .marks_of_view(view)
            .into_iter()
            .filter_map(|id| {
                let a = doc.annotation(id)?;
                Some((id, format!("{} mark", mark_view_name(a))))
            })
            .collect();
        theme::row(ui, "Referenced by", |ui| {
            ui.vertical(|ui| {
                if marks.is_empty() {
                    ui.label(egui::RichText::new("No marks").weak())
                        .on_hover_text(
                            "Link a section or elevation mark to this view in the mark's \
                             Inspector, or right-click a mark: Make view",
                        );
                }
                for (id, text) in marks {
                    if ui
                        .link(text)
                        .on_hover_text("Pick the mark in the model")
                        .clicked()
                    {
                        self.pick_mark(id);
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(min: [f64; 2], max: [f64; 2]) -> Bounds {
        Bounds {
            min: DVec2::from_array(min),
            max: DVec2::from_array(max),
        }
    }

    fn section(a: [f64; 2], b: [f64; 2], flip: bool) -> Annotation {
        Annotation::Section {
            a: DVec2::from_array(a),
            b: DVec2::from_array(b),
            label: "A".into(),
            sheet: String::new(),
            flip,
            view: None,
        }
    }

    #[test]
    fn a_section_crops_along_its_cut_as_deep_as_the_model() {
        let model = bounds([0.0, 0.0], [1000.0, 600.0]);
        // Cut across at y = 200, looking up (left of a → b): the model goes 400 further.
        let up = mark_crop(
            &section([-100.0, 200.0], [1100.0, 200.0], false),
            model,
            1.0,
        );
        assert_eq!(up, Some(bounds([-100.0, 200.0], [1100.0, 600.0])));
        // Flipped, it looks down: 200 deep, still at least a fifth of the cut.
        let down = mark_crop(&section([-100.0, 200.0], [1100.0, 200.0], true), model, 1.0);
        assert_eq!(down, Some(bounds([-100.0, -40.0], [1100.0, 200.0])));
        // With no model, half the cut's length.
        let bare = mark_crop(
            &section([0.0, 0.0], [0.0, 100.0], false),
            Bounds::EMPTY,
            1.0,
        );
        assert_eq!(bare, Some(bounds([-50.0, 0.0], [0.0, 100.0])));
    }

    #[test]
    fn an_elevation_crops_the_model_in_front_of_it() {
        let model = bounds([0.0, 0.0], [1000.0, 600.0]);
        let mark = |at: [f64; 2], angle: f64| Annotation::Elevation {
            at: DVec2::from_array(at),
            angle,
            label: "1".into(),
            sheet: String::new(),
            view: None,
        };
        let close = |got: Option<Bounds>, want: Bounds| {
            let got = got.expect("a crop");
            assert!(
                got.min.distance(want.min) < 1e-9 && got.max.distance(want.max) < 1e-9,
                "{got:?}"
            );
        };
        // Looking up from below the model: all of its width, up to its far side.
        close(
            mark_crop(&mark([500.0, -200.0], 90.0), model, 1.0),
            bounds([0.0, -200.0], [1000.0, 600.0]),
        );
        // Looking away from it: a square of the fallback.
        close(
            mark_crop(&mark([500.0, -200.0], 270.0), model, 100.0),
            bounds([450.0, -300.0], [550.0, -200.0]),
        );
        assert_eq!(mark_view_name(&mark([0.0, 0.0], 0.0)), "Elevation 1");
    }
}
