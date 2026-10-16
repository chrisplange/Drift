//! The drawing crosshair: drawn by Drift (the system cursor's size and weight can't be set)
//! while a drawing or editing tool is over the canvas, with its size (or full screen) and line
//! weight set in Preferences.

use crate::*;

/// Shortest and longest arms (points, from the centre).
pub(crate) const SIZE_RANGE: std::ops::RangeInclusive<f32> = 6.0..=200.0;
/// Thinnest and thickest lines (points).
pub(crate) const WEIGHT_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;

/// How the crosshair looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Crosshair {
    /// Each arm's length from the centre (points), unless `full`.
    pub(crate) size: f32,
    /// Lines across the whole canvas, as in AutoCAD.
    pub(crate) full: bool,
    pub(crate) weight: f32,
}

impl Default for Crosshair {
    fn default() -> Self {
        Self {
            size: 16.0,
            full: false,
            weight: 1.0,
        }
    }
}

impl Crosshair {
    /// Its lines, centred on `at`, within `canvas`.
    pub(crate) fn lines(&self, at: egui::Pos2, canvas: egui::Rect) -> [[egui::Pos2; 2]; 2] {
        let (left, right, top, bottom) = if self.full {
            (canvas.left(), canvas.right(), canvas.top(), canvas.bottom())
        } else {
            (
                (at.x - self.size).max(canvas.left()),
                (at.x + self.size).min(canvas.right()),
                (at.y - self.size).max(canvas.top()),
                (at.y + self.size).min(canvas.bottom()),
            )
        };
        [
            [egui::pos2(left, at.y), egui::pos2(right, at.y)],
            [egui::pos2(at.x, top), egui::pos2(at.x, bottom)],
        ]
    }

    /// Draw it in `color` (the theme's strongest text colour suits both themes).
    pub(crate) fn paint(
        &self,
        painter: &egui::Painter,
        at: egui::Pos2,
        canvas: egui::Rect,
        color: egui::Color32,
    ) {
        let stroke = egui::Stroke::new(self.weight, color);
        for line in self.lines(at, canvas) {
            painter.line_segment(line, stroke);
        }
    }

    /// Preference pairs (`key=value`).
    pub(crate) fn prefs(&self) -> [String; 3] {
        [
            format!("xsize={}", self.size),
            format!("xfull={}", u8::from(self.full)),
            format!("xweight={}", self.weight),
        ]
    }

    /// Read a preference; false if `key` isn't one of the crosshair's.
    pub(crate) fn apply(&mut self, key: &str, value: &str) -> bool {
        let number = |range: &std::ops::RangeInclusive<f32>| {
            value
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(*range.start(), *range.end()))
        };
        match key {
            "xsize" => self.size = number(&SIZE_RANGE).unwrap_or(self.size),
            "xweight" => self.weight = number(&WEIGHT_RANGE).unwrap_or(self.weight),
            "xfull" => self.full = value == "1",
            _ => return false,
        }
        true
    }
}

impl DriftApp {
    /// Preferences → Cursor.
    pub(crate) fn crosshair_prefs(&mut self, ui: &mut egui::Ui) {
        let x = &mut self.crosshair;
        theme::row(ui, "Size", |ui| {
            ui.add_enabled(
                !x.full,
                egui::Slider::new(&mut x.size, SIZE_RANGE)
                    .suffix(" pt")
                    .logarithmic(true),
            )
            .on_hover_text("How far each arm reaches from the centre");
        });
        ui.checkbox(&mut x.full, "Full screen")
            .on_hover_text("Lines across the whole canvas, as in AutoCAD");
        theme::row(ui, "Weight", |ui| {
            ui.add(egui::Slider::new(&mut x.weight, WEIGHT_RANGE).suffix(" pt"));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arms_reach_their_size_and_stay_on_the_canvas() {
        let canvas = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0));
        let x = Crosshair {
            size: 20.0,
            ..Default::default()
        };
        let [h, v] = x.lines(egui::pos2(100.0, 300.0), canvas);
        assert_eq!(h, [egui::pos2(80.0, 300.0), egui::pos2(120.0, 300.0)]);
        assert_eq!(v, [egui::pos2(100.0, 280.0), egui::pos2(100.0, 320.0)]);
        // Near an edge, clipped to the canvas.
        let [h, _] = x.lines(egui::pos2(10.0, 300.0), canvas);
        assert_eq!(h[0].x, 0.0);
        // Full screen: edge to edge.
        let full = Crosshair { full: true, ..x };
        let [h, v] = full.lines(egui::pos2(100.0, 300.0), canvas);
        assert_eq!((h[0].x, h[1].x, v[0].y, v[1].y), (0.0, 800.0, 0.0, 600.0));
    }

    #[test]
    fn preferences_round_trip_and_clamp() {
        let x = Crosshair {
            size: 42.0,
            full: true,
            weight: 1.5,
        };
        let mut back = Crosshair::default();
        for pair in x.prefs() {
            let (k, v) = pair.split_once('=').unwrap();
            assert!(back.apply(k, v));
        }
        assert_eq!(back, x);
        assert!(back.apply("xweight", "99"));
        assert_eq!(back.weight, *WEIGHT_RANGE.end());
        assert!(!back.apply("grid", "1"));
    }
}
