//! Ellipses and partial ellipses: the centre, the end of one axis, then a point setting the
//! other axis's size; a partial ellipse then takes its start and end (counter-clockwise).

use drift_core::shape::{ellipse_angle, ellipse_point};

use super::*;

#[derive(Default)]
pub struct EllipseTool {
    /// Draw part of an ellipse (start and end picked after the axes).
    pub(super) partial: bool,
    pub(super) center: Option<Anchor>,
    pub(super) major: Option<Anchor>,
    /// The other semi-axis's length, once set.
    pub(super) minor: Option<f64>,
    /// A partial ellipse's start, once picked.
    pub(super) start: Option<Anchor>,
    pub(super) typed: [Option<f64>; 2],
    /// Sizes typed earlier, dimensioned when the ellipse is made.
    pub(super) typed_major: Option<f64>,
    pub(super) typed_minor: Option<f64>,
}

impl EllipseTool {
    pub fn partial() -> Self {
        Self {
            partial: true,
            ..Default::default()
        }
    }

    /// The major semi-axis vector (to the pointer until it's picked).
    fn major_vec(&self, p: &Pointer) -> Option<DVec2> {
        let c = self.center?.pos;
        let m = match self.major {
            Some(m) => m.pos,
            None => {
                let d = p.pos() - c;
                match self.typed[0] {
                    Some(len) => c + d.normalize_or(DVec2::X) * len.abs(),
                    None => p.pos(),
                }
            }
        };
        let v = m - c;
        (v.length() > 1e-9).then_some(v)
    }

    /// The minor semi-axis length: the pointer's distance from the major axis until set.
    fn minor_len(&self, p: &Pointer) -> Option<f64> {
        if let Some(b) = self.minor {
            return Some(b);
        }
        let c = self.center?.pos;
        let major = self.major_vec(p)?;
        self.major?;
        let b = self.typed[0].map_or_else(
            || (p.pos() - c).dot(major.perp().normalize()).abs(),
            f64::abs,
        );
        (b > 1e-9).then_some(b)
    }

    fn reset(&mut self) {
        let partial = self.partial;
        *self = Self {
            partial,
            ..Default::default()
        };
    }

    /// Make the ellipse (or, given its ends, part of one).
    fn create(&mut self, b: f64, ends: Option<(Anchor, Anchor)>, ed: &mut Editor) {
        let (Some(center), Some(major)) = (self.center, self.major) else {
            return;
        };
        let (c, m) = (center.pos, major.pos);
        ed.doc.begin();
        let made = (|| -> Result<(), drift_core::DocumentError> {
            let pc = center.own(&mut ed.doc);
            let pm = major.own(&mut ed.doc);
            let pn = ed.doc.add_point(c + (m - c).perp().normalize() * b);
            let ellipse = match ends {
                Some((s, e)) => {
                    let (ps, pe) = (s.resolve(&mut ed.doc), e.resolve(&mut ed.doc));
                    ed.doc.add_ellipse_arc(pc, pm, pn, ps, pe)?
                }
                None => ed.doc.add_ellipse(pc, pm, pn)?,
            };
            if ends.is_none() {
                ed.add_shape(&[ellipse]);
            }
            // A level or plumb axis keeps it.
            let scale = c.distance(m).max(1.0) * 1e-9;
            if ed.auto_relations {
                if (c.y - m.y).abs() < scale {
                    let _ = ed
                        .doc
                        .add_constraint(Constraint::HorizontalPoints { a: pc, b: pm });
                } else if (c.x - m.x).abs() < scale {
                    let _ = ed
                        .doc
                        .add_constraint(Constraint::VerticalPoints { a: pc, b: pm });
                }
            }
            if let Some(a) = self.typed_major {
                ed.add_typed_dimension(
                    0,
                    Constraint::Distance {
                        a: pc,
                        b: pm,
                        value: a,
                    },
                );
            }
            if let Some(b) = self.typed_minor {
                ed.add_typed_dimension(
                    0,
                    Constraint::Distance {
                        a: pc,
                        b: pn,
                        value: b,
                    },
                );
            }
            Ok(())
        })();
        match made {
            Ok(()) => {
                ed.commit(if ends.is_some() {
                    "Partial ellipse"
                } else {
                    "Ellipse"
                });
            }
            Err(e) => {
                ed.doc.rollback();
                ed.sync();
                ed.notice = Some(format!("Ellipse: {e}"));
            }
        }
        self.reset();
    }

    /// The ellipse as it would be now (for previews and picking its ends).
    fn shape(&self, p: &Pointer) -> Option<(DVec2, DVec2, f64)> {
        Some((self.center?.pos, self.major_vec(p)?, self.minor_len(p)?))
    }

    /// The pointer moved onto the ellipse (radially from its centre), as an anchor.
    fn on_ellipse(&self, p: &Pointer) -> Option<Anchor> {
        let (c, m, b) = self.shape(p)?;
        let t = ellipse_angle(c, m, b, p.pos());
        let at = ellipse_point(c, m, b, t);
        // Keep an existing point only if it's already on the ellipse.
        let anchor = p.anchor();
        Some(
            if anchor.pos.distance(at) <= 1e-9 * at.abs().max_element().max(1.0) {
                anchor
            } else {
                exact(p, at).anchor()
            },
        )
    }
}

impl Tool for EllipseTool {
    fn is_busy(&self) -> bool {
        self.center.is_some()
    }

    fn kind(&self) -> ToolKind {
        if self.partial {
            ToolKind::EllipseArc
        } else {
            ToolKind::Ellipse
        }
    }

    fn hint(&self) -> String {
        match (self.center, self.major, self.minor, self.start) {
            (None, ..) => "Click the centre, or type X / Y".into(),
            (Some(_), None, ..) => "Click the end of one axis, or type its half-length".into(),
            (Some(_), Some(_), None, _) => {
                "Click to set the other axis, or type its half-length".into()
            }
            (_, _, Some(_), None) => "Click where it starts (counter-clockwise from there)".into(),
            _ => "Click where it ends".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.center.is_none() => p,
            _ => return,
        };
        if self.center.is_none() {
            self.center = Some(p.anchor());
        } else if self.major.is_none() {
            if self.major_vec(&p).is_some() {
                self.major = Some(p.anchor());
            }
        } else if self.minor.is_none() {
            let Some(b) = self.minor_len(&p) else { return };
            if self.partial {
                self.minor = Some(b);
            } else {
                self.create(b, None, ed);
            }
        } else if self.start.is_none() {
            self.start = self.on_ellipse(&p);
        } else if let (Some(b), Some(s), Some(e)) = (self.minor, self.start, self.on_ellipse(&p))
            && s.pos.distance(e.pos) > 1e-9
        {
            self.create(b, Some((s, e)), ed);
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let busy = self.center.is_some();
        self.reset();
        busy
    }

    fn fields(&self) -> &'static [&'static str] {
        match (self.center, self.major, self.minor) {
            (None, ..) => XY,
            (Some(_), None, _) => &["Semi-major"],
            (Some(_), Some(_), None) => &["Semi-minor"],
            _ => &[],
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        if self.center.is_none() {
            self.center = Some(exact(p, typed_point(self.typed, p)).anchor());
        } else if self.major.is_none() {
            if let (Some(c), Some(v)) = (self.center, self.major_vec(p)) {
                self.typed_major = self.typed[0].map(f64::abs);
                self.major = Some(exact(p, c.pos + v).anchor());
            }
        } else if self.minor.is_none()
            && let Some(b) = self.minor_len(p)
        {
            self.typed_minor = self.typed[0].map(f64::abs);
            if self.partial {
                self.minor = Some(b);
            } else {
                self.typed = [None; 2];
                self.create(b, None, ed);
                return;
            }
        }
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(c), Some(p)) = (self.center, pointer) else {
            return Vec::new();
        };
        let mut items = vec![preview_vertex(c.pos)];
        let Some(major) = self.major_vec(p) else {
            return items;
        };
        // The axis being placed, then the ellipse (a circle until the second axis is set).
        items.push(
            preview(Shape::Line {
                a: c.pos - major,
                b: c.pos + major,
            })
            .with_kind(drift_core::LineKind::Construction),
        );
        let b = self.minor_len(p).unwrap_or(major.length());
        let (start, sweep) = match (self.minor, self.start) {
            (Some(_), Some(s)) => {
                let t0 = ellipse_angle(c.pos, major, b, s.pos);
                let t1 = ellipse_angle(c.pos, major, b, p.pos());
                items.push(preview_vertex(s.pos));
                (t0, (t1 - t0).rem_euclid(TAU).max(1e-6))
            }
            _ => (0.0, TAU),
        };
        if self.major.is_some() || self.minor.is_some() {
            items.push(preview(Shape::Ellipse {
                center: c.pos,
                major,
                minor: b,
                start,
                sweep,
            }));
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let major = self.major_vec(p)?;
        Some(match self.minor_len(p) {
            Some(b) if self.minor.is_none() => format!(
                "{} × {}",
                p.units.format_readout(2.0 * major.length()),
                p.units.format_readout(2.0 * b)
            ),
            _ => p.units.format_readout(2.0 * major.length()),
        })
    }
}
