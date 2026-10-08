//! Regular polygons: a centre, then a corner (inscribed) or the middle of a side
//! (circumscribed). They're equal sides with every corner on a construction circle, so they
//! stay regular. A typed size is the circle's diameter (inscribed) or the distance from the
//! centre to a side (circumscribed: half the size across flats).

use super::*;
use drift_core::LineKind;

/// Sides when none have been chosen yet.
pub const DEFAULT_SIDES: usize = 6;

#[derive(Default)]
pub struct PolygonTool {
    pub(super) center: Option<Anchor>,
    pub(super) typed: [Option<f64>; 2],
    /// The last pointer, for the preview (which needs the editor's settings too).
    pub(super) pointer: Option<Pointer>,
}

/// A polygon's corners, its construction circle's radius (through the corners) and the
/// distance from its centre to a side.
pub(super) struct PolygonShape {
    pub corners: Vec<DVec2>,
    pub circle: f64,
    pub apothem: f64,
}

/// Snap an angle (radians) to multiples of 15° when within 2°.
fn sticky_angle(a: f64) -> f64 {
    let step = 15f64.to_radians();
    let snapped = (a / step).round() * step;
    if (a - snapped).abs() <= 2f64.to_radians() {
        snapped
    } else {
        a
    }
}

pub(super) fn polygon_shape(
    center: DVec2,
    toward: DVec2,
    sides: usize,
    circumscribed: bool,
    radius: Option<f64>,
) -> Option<PolygonShape> {
    let d = toward - center;
    let r = radius.unwrap_or(d.length());
    if sides < 3 || r <= 1e-9 || !r.is_finite() {
        return None;
    }
    let angle = sticky_angle(if d.length() > 1e-12 {
        d.y.atan2(d.x)
    } else {
        -std::f64::consts::FRAC_PI_2
    });
    let n = sides as f64;
    let half = PI / n;
    // Inscribed: the pointer is a corner. Circumscribed: the middle of a side, so the corners
    // are half a step either side of it, further out.
    let (start, corner_r) = if circumscribed {
        (angle - half, r / half.cos())
    } else {
        (angle, r)
    };
    let corners = (0..sides)
        .map(|k| center + corner_r * DVec2::from_angle(start + f64::from(k as u32) * 2.0 * half))
        .collect();
    Some(PolygonShape {
        corners,
        circle: corner_r,
        apothem: corner_r * half.cos(),
    })
}

impl PolygonTool {
    fn sides(&self, ed: &Editor) -> usize {
        self.typed[1]
            .map(|s| s.round().clamp(3.0, 64.0) as usize)
            .unwrap_or(if ed.polygon_sides == 0 {
                DEFAULT_SIDES
            } else {
                ed.polygon_sides
            })
            .max(3)
    }

    fn shape(&self, p: &Pointer, ed: &Editor) -> Option<PolygonShape> {
        let c = self.center?.pos;
        let toward = if self.typed[0].is_some() && p.pos() == c {
            c + DVec2::Y
        } else {
            p.pos()
        };
        polygon_shape(
            c,
            toward,
            self.sides(ed),
            ed.polygon_circumscribed,
            self.typed[0].map(f64::abs),
        )
    }

    fn create(&mut self, p: &Pointer, ed: &mut Editor) {
        let (Some(center), Some(shape)) = (self.center, self.shape(p, ed)) else {
            return;
        };
        let sides = self.sides(ed);
        ed.polygon_sides = sides;
        let circumscribed = ed.polygon_circumscribed;
        ed.doc.begin();
        let c = center.resolve(&mut ed.doc);
        let made = (|| -> Result<(), drift_core::DocumentError> {
            let circle = ed.doc.add_circle(c, shape.circle)?;
            ed.doc.set_style(circle, LineKind::Construction);
            let points: Vec<EntityId> =
                shape.corners.iter().map(|q| ed.doc.add_point(*q)).collect();
            let mut lines = Vec::with_capacity(sides);
            for i in 0..sides {
                lines.push(ed.doc.add_line(points[i], points[(i + 1) % sides])?);
            }
            for &point in &points {
                ed.doc.add_constraint(Constraint::PointOnCurve {
                    point,
                    curve: circle,
                })?;
            }
            for &line in &lines[1..] {
                ed.doc.add_constraint(Constraint::Equal {
                    a: lines[0],
                    b: line,
                })?;
            }
            // A side that came out level or plumb keeps it.
            if let Some((i, kind)) = shape.corners.iter().enumerate().find_map(|(i, a)| {
                let b = shape.corners[(i + 1) % sides];
                let scale = shape.circle.max(1.0) * 1e-9;
                if (a.y - b.y).abs() < scale {
                    Some((i, GuideKind::Horizontal))
                } else if (a.x - b.x).abs() < scale {
                    Some((i, GuideKind::Vertical))
                } else {
                    None
                }
            }) {
                auto_relation(ed, Some(kind), lines[i]);
            }
            if self.typed[0].is_some() {
                let size = if circumscribed {
                    Constraint::PointLineDistance {
                        point: c,
                        line: lines[0],
                        value: shape.apothem,
                    }
                } else {
                    Constraint::Diameter {
                        curve: circle,
                        value: 2.0 * shape.circle,
                    }
                };
                ed.add_typed_dimension(0, size);
            }
            Ok(())
        })();
        match made {
            Ok(()) => {
                ed.commit("Polygon");
            }
            Err(e) => {
                ed.doc.rollback();
                ed.sync();
                ed.notice = Some(format!("Polygon: {e}"));
            }
        }
        self.center = None;
    }
}

impl Tool for PolygonTool {
    fn is_busy(&self) -> bool {
        self.center.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Polygon
    }

    fn hint(&self) -> String {
        match self.center {
            None => "Polygon: click the centre. Type sides with Tab; X: inscribed / circumscribed"
                .into(),
            Some(_) => "Click a corner (or the middle of a side when circumscribed), or type \
                        the radius (Tab: sides)"
                .into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        if let Input::Hover(p) | Input::Drag(p) = input {
            self.pointer = Some(p);
        }
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.center.is_none() => p,
            _ => return,
        };
        match self.center {
            None => self.center = Some(p.anchor()),
            Some(_) => self.create(&p, ed),
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.center.take().is_some()
    }

    fn key(&mut self, key: ToolKey, _pointer: Option<&Pointer>, ed: &mut Editor) -> bool {
        if key == ToolKey::X {
            ed.polygon_circumscribed = !ed.polygon_circumscribed;
            return true;
        }
        false
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.center.is_some() {
            &["Radius", "Sides"]
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
                if self.typed[0].is_some() {
                    self.create(p, ed);
                } else if let Some(s) = self.typed[1] {
                    // Only the sides: keep drawing with them.
                    ed.polygon_sides = s.round().clamp(3.0, 64.0) as usize;
                }
            }
        }
        self.typed = [None; 2];
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        let (Some(center), Some(p)) = (self.center, self.pointer.as_ref()) else {
            return Vec::new();
        };
        let Some(shape) = self.shape(p, ed) else {
            return vec![preview_vertex(center.pos)];
        };
        let n = shape.corners.len();
        let mut items: Vec<SketchItem> = (0..n)
            .map(|i| {
                preview(Shape::Line {
                    a: shape.corners[i],
                    b: shape.corners[(i + 1) % n],
                })
            })
            .collect();
        items.push(
            preview(Shape::Circle {
                center: center.pos,
                radius: shape.circle,
            })
            .with_kind(LineKind::Construction),
        );
        items.push(preview_vertex(center.pos));
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let c = self.center?.pos;
        let r = self.typed[0].map_or_else(|| p.pos().distance(c), f64::abs);
        Some(format!("R {}", p.units.format_readout(r)))
    }
}
