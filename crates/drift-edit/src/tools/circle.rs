//! Circle from its centre and a point on it.

use super::*;

#[derive(Default)]
pub struct CircleTool {
    pub(super) center: Option<Anchor>,
    pub(super) typed: [Option<f64>; 2],
}

impl CircleTool {
    pub(super) fn radius(&self, p: &Pointer) -> f64 {
        match (self.center, self.typed[0]) {
            (Some(_), Some(diameter)) => diameter.abs() / 2.0,
            (Some(c), None) => p.pos().distance(c.pos),
            (None, _) => 0.0,
        }
    }

    pub(super) fn create(&mut self, radius: f64, dimension: bool, ed: &mut Editor) {
        let Some(center) = self.center else { return };
        if radius == 0.0 {
            return;
        }
        ed.doc.begin();
        let c = center.own(&mut ed.doc);
        match ed.doc.add_circle(c, radius) {
            Ok(circle) => {
                if dimension {
                    ed.add_typed_dimension(
                        0,
                        Constraint::Diameter {
                            curve: circle,
                            value: 2.0 * radius,
                        },
                    );
                }
                ed.add_shape(&[circle]);
                ed.commit("Circle");
            }
            Err(_) => {
                ed.doc.rollback();
                ed.sync();
            }
        }
        self.center = None;
    }
}

impl Tool for CircleTool {
    fn is_busy(&self) -> bool {
        self.center.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Circle
    }

    fn hint(&self) -> String {
        match self.center {
            None => "Click the centre, or type X / Y".into(),
            Some(_) => "Click a point on the circle, or type the diameter".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.center.is_none() => p,
            _ => return,
        };
        match self.center {
            None => self.center = Some(p.anchor()),
            Some(center) => {
                let radius = p.pos().distance(center.pos);
                self.create(radius, false, ed);
            }
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.center.take().is_some()
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.center.is_some() {
            &["Diameter"]
        } else {
            XY
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        match self.center {
            None => self.center = Some(exact(p, typed_point(self.typed, p)).anchor()),
            Some(_) => {
                let radius = self.radius(p);
                self.create(radius, self.typed[0].is_some(), ed);
            }
        }
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(center), Some(p)) = (self.center, pointer) else {
            return Vec::new();
        };
        vec![
            preview(Shape::Circle {
                center: center.pos,
                radius: self.radius(p),
            }),
            preview_vertex(center.pos),
        ]
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        self.center?;
        let r = self.radius(p);
        Some(format!(
            "R {}  Ø {}",
            p.units.format_readout(r),
            p.units.format_readout(2.0 * r)
        ))
    }
}
