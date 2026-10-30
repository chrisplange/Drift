//! Rectangle from two opposite corners.

use super::*;

#[derive(Default)]
pub struct RectangleTool {
    pub(super) first: Option<Anchor>,
    pub(super) typed: [Option<f64>; 2],
}

impl RectangleTool {
    pub(super) fn second_corner(&self, p: &Pointer) -> DVec2 {
        let Some(first) = self.first else {
            return typed_point(self.typed, p);
        };
        let d = p.pos() - first.pos;
        let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
        DVec2::new(
            self.typed[0].map_or(d.x, |w| sign(d.x) * w.abs()),
            self.typed[1].map_or(d.y, |h| sign(d.y) * h.abs()),
        ) + first.pos
    }

    pub(super) fn create(&mut self, second: Anchor, dims: [Option<f64>; 2], ed: &mut Editor) {
        let Some(first) = self.first else { return };
        let (a, c) = (first.pos, second.pos);
        if a.x == c.x || a.y == c.y {
            return;
        }
        ed.doc.begin();
        let p0 = first.own(&mut ed.doc);
        let p1 = ed.doc.add_point(DVec2::new(c.x, a.y));
        let p2 = second.own(&mut ed.doc);
        let p3 = ed.doc.add_point(DVec2::new(a.x, c.y));
        let corners = [p0, p1, p2, p3];
        let mut lines = Vec::new();
        for i in 0..4 {
            match ed.doc.add_line(corners[i], corners[(i + 1) % 4]) {
                Ok(l) => lines.push(l),
                Err(_) => {
                    ed.doc.rollback();
                    ed.sync();
                    self.first = None;
                    return;
                }
            }
        }
        for (i, line) in lines.iter().enumerate() {
            let relation = if i % 2 == 0 {
                GuideKind::Horizontal
            } else {
                GuideKind::Vertical
            };
            auto_relation(ed, Some(relation), *line);
        }
        if let Some(w) = dims[0] {
            ed.add_typed_dimension(
                0,
                Constraint::Distance {
                    a: p0,
                    b: p1,
                    value: w.abs(),
                },
            );
        }
        if let Some(h) = dims[1] {
            ed.add_typed_dimension(
                1,
                Constraint::Distance {
                    a: p1,
                    b: p2,
                    value: h.abs(),
                },
            );
        }
        ed.add_shape(&lines);
        ed.commit("Rectangle");
        self.first = None;
    }
}

impl Tool for RectangleTool {
    fn is_busy(&self) -> bool {
        self.first.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Rectangle
    }

    fn hint(&self) -> String {
        match self.first {
            None => "Click the first corner, or type X / Y".into(),
            Some(_) => "Click the opposite corner, or type a width (Tab: height)".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.first.is_none() => p,
            _ => return,
        };
        if self.first.is_none() {
            self.first = Some(p.anchor());
        } else {
            self.create(p.anchor(), [None; 2], ed);
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.first.take().is_some()
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.first.is_some() {
            &["Width", "Height"]
        } else {
            XY
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        let corner = self.second_corner(p);
        if self.first.is_none() {
            self.first = Some(exact(p, corner).anchor());
        } else {
            let dims = self.typed.map(|v| v.filter(|x| *x != 0.0));
            self.create(exact(p, corner).anchor(), dims, ed);
        }
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(first), Some(p)) = (self.first, pointer) else {
            return Vec::new();
        };
        let (a, c) = (first.pos, self.second_corner(p));
        let corners = [a, DVec2::new(c.x, a.y), c, DVec2::new(a.x, c.y)];
        (0..4)
            .map(|i| {
                preview(Shape::Line {
                    a: corners[i],
                    b: corners[(i + 1) % 4],
                })
            })
            .collect()
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let d = self.second_corner(p) - self.first?.pos;
        Some(format!(
            "{} × {}",
            p.units.format_readout(d.x.abs()),
            p.units.format_readout(d.y.abs())
        ))
    }
}
