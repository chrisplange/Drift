//! Drafting content that annotates the sketch without constraining it: fills and hatches,
//! notes and leaders, centre marks, callouts, ordinate dimensions, dimension tolerances, and
//! the document's drafting standard.

use glam::DVec2;

use crate::document::EntityId;
use crate::layers::{LayerId, LineWeight, Rgb};
use crate::units::{LengthFormat, Units};

/// Stable identifier of a fill or hatch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FillId(pub u32);

impl std::fmt::Display for FillId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "f{}", self.0)
    }
}

/// A hatch pattern and how it is laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct Hatch {
    /// Pattern name: a built-in pattern or one imported from a `.pat` file.
    pub pattern: String,
    /// Multiplies the pattern's spacing.
    pub scale: f64,
    /// Rotation of the whole pattern, in degrees counter-clockwise.
    pub angle: f64,
    /// Where the pattern's origin sits, in world units.
    pub origin: DVec2,
    /// Model patterns have a real-world size; drafting patterns keep their size on paper
    /// (scaled by the drawing scale).
    pub model: bool,
    /// A solid colour behind the pattern lines.
    pub background: Option<Rgb>,
}

impl Hatch {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            scale: 1.0,
            angle: 0.0,
            origin: DVec2::ZERO,
            model: false,
            background: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FillKind {
    Solid,
    Hatch(Hatch),
}

/// A solid fill or hatch of a closed region. It is associative: it stores the curves of the
/// region's outer boundary and is re-found among the sketch's regions when geometry changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    /// The curves of the region's outer boundary, sorted (holes are whatever lies inside).
    pub boundary: Vec<EntityId>,
    /// A point inside the region when it was picked, to choose between regions bounded by
    /// the same curves.
    pub seed: DVec2,
    pub kind: FillKind,
    pub layer: LayerId,
    /// `None` uses the layer's colour.
    pub color: Option<Rgb>,
    /// Opacity of a solid fill, 0–1.
    pub opacity: f64,
    /// Weight of hatch lines; `None` uses the thinnest line.
    pub weight: Option<LineWeight>,
    /// Draw order within the layer: higher draws on top.
    pub order: i32,
}

impl Fill {
    pub fn new(mut boundary: Vec<EntityId>, seed: DVec2, kind: FillKind) -> Self {
        boundary.sort_unstable();
        boundary.dedup();
        Self {
            boundary,
            seed,
            kind,
            layer: LayerId::DEFAULT,
            color: None,
            opacity: 0.35,
            weight: None,
            order: 0,
        }
    }
}

/// Stable identifier of an annotation (note, centre mark, callout, ordinate dimension).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnnotationId(pub u32);

impl std::fmt::Display for AnnotationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "n{}", self.0)
    }
}

/// Where a leader points: a free spot, a sketch point, or a spot on a curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    Free(DVec2),
    Point(EntityId),
    /// On a curve: for a line, the fraction from its start to its end; for an arc or circle,
    /// the angle in radians.
    Curve {
        curve: EntityId,
        t: f64,
    },
}

impl Anchor {
    pub fn entity(&self) -> Option<EntityId> {
        match *self {
            Anchor::Free(_) => None,
            Anchor::Point(id) | Anchor::Curve { curve: id, .. } => Some(id),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeaderEnd {
    #[default]
    Arrow,
    Dot,
    None,
}

impl LeaderEnd {
    pub const ALL: [LeaderEnd; 3] = [LeaderEnd::Arrow, LeaderEnd::Dot, LeaderEnd::None];

    pub fn label(self) -> &'static str {
        match self {
            LeaderEnd::Arrow => "Arrow",
            LeaderEnd::Dot => "Dot",
            LeaderEnd::None => "None",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            LeaderEnd::Arrow => "arrow",
            LeaderEnd::Dot => "dot",
            LeaderEnd::None => "none",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.key() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leader {
    pub to: Anchor,
    pub end: LeaderEnd,
}

/// Which coordinate an ordinate dimension reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinateAxis {
    X,
    Y,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Annotation {
    /// Text, with an optional leader. With a leader, `at` is the text's offset from the
    /// leader's target (so it follows the geometry); without, it is a world position.
    Note {
        text: String,
        at: DVec2,
        leader: Option<Leader>,
    },
    /// A centre mark on a circle or arc, with centre lines just past its edge.
    CentreMark { curve: EntityId },
    /// A centre line midway between two lines, or through the centres of two circles, arcs
    /// or points.
    Centreline { a: EntityId, b: EntityId },
    /// `4× Ø6 THRU`: counts the holes of the same size; `at` is the text's offset from the
    /// hole's centre.
    HoleCallout {
        curve: EntityId,
        at: DVec2,
        suffix: String,
    },
    /// `2 × 45°` for a chamfer line; `at` is the text's offset from the line's midpoint.
    ChamferCallout { line: EntityId, at: DVec2 },
    /// The X or Y distance of `point` from `origin`, written at the end of a leader. `at` is
    /// where the label sits across the measured direction, relative to the origin (the label's
    /// y for an X ordinate, its x for a Y ordinate).
    Ordinate {
        origin: EntityId,
        point: EntityId,
        axis: OrdinateAxis,
        at: f64,
    },
}

impl Annotation {
    /// Sketch entities the annotation depends on.
    pub fn entities(&self) -> Vec<EntityId> {
        match self {
            Annotation::Note { leader, .. } => {
                leader.and_then(|l| l.to.entity()).into_iter().collect()
            }
            Annotation::CentreMark { curve } | Annotation::HoleCallout { curve, .. } => {
                vec![*curve]
            }
            Annotation::Centreline { a, b } => vec![*a, *b],
            Annotation::ChamferCallout { line, .. } => vec![*line],
            Annotation::Ordinate { origin, point, .. } => vec![*origin, *point],
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Annotation::Note { leader: None, .. } => "Note",
            Annotation::Note { .. } => "Leader note",
            Annotation::CentreMark { .. } => "Centre mark",
            Annotation::Centreline { .. } => "Centre line",
            Annotation::HoleCallout { .. } => "Hole callout",
            Annotation::ChamferCallout { .. } => "Chamfer callout",
            Annotation::Ordinate { .. } => "Ordinate dimension",
        }
    }

    pub(crate) fn replace_entity(&mut self, from: EntityId, into: EntityId) {
        let swap = |e: &mut EntityId| {
            if *e == from {
                *e = into;
            }
        };
        match self {
            Annotation::Note {
                leader: Some(Leader { to, .. }),
                ..
            } => match to {
                Anchor::Point(e) | Anchor::Curve { curve: e, .. } => swap(e),
                Anchor::Free(_) => {}
            },
            Annotation::Note { .. } => {}
            Annotation::CentreMark { curve } | Annotation::HoleCallout { curve, .. } => swap(curve),
            Annotation::Centreline { a, b } => {
                swap(a);
                swap(b);
            }
            Annotation::ChamferCallout { line, .. } => swap(line),
            Annotation::Ordinate { origin, point, .. } => {
                swap(origin);
                swap(point);
            }
        }
    }
}

/// A dimension's tolerance. Deviations are added to (`upper`) and subtracted from (`lower`)
/// the nominal value.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Tolerance {
    #[default]
    None,
    /// `25 ±0.1`
    Symmetric(f64),
    /// `25 +0.1 / −0.05`, stacked.
    Deviation { upper: f64, lower: f64 },
    /// The limits `25.1 / 24.95`, stacked.
    Limits { upper: f64, lower: f64 },
    /// An ISO fit such as `H7` or `g6`.
    Fit(String),
}

impl Tolerance {
    pub fn label(&self) -> &'static str {
        match self {
            Tolerance::None => "None",
            Tolerance::Symmetric(_) => "Symmetric",
            Tolerance::Deviation { .. } => "Deviation",
            Tolerance::Limits { .. } => "Limits",
            Tolerance::Fit(_) => "Fit",
        }
    }
}

/// Text around a dimension's value: a prefix (`4×`), a suffix (`THRU`) and a tolerance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimNote {
    pub prefix: String,
    pub suffix: String,
    pub tolerance: Tolerance,
}

impl DimNote {
    pub fn is_empty(&self) -> bool {
        self.prefix.is_empty() && self.suffix.is_empty() && self.tolerance == Tolerance::None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Standard {
    #[default]
    Iso,
    Asme,
}

impl Standard {
    pub fn label(self) -> &'static str {
        match self {
            Standard::Iso => "ISO 129",
            Standard::Asme => "ASME Y14.5",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrowStyle {
    /// A filled, closed arrowhead.
    #[default]
    Closed,
    Open,
    /// An architectural tick (oblique stroke).
    Tick,
    Dot,
}

impl ArrowStyle {
    pub const ALL: [ArrowStyle; 4] = [
        ArrowStyle::Closed,
        ArrowStyle::Open,
        ArrowStyle::Tick,
        ArrowStyle::Dot,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ArrowStyle::Closed => "Closed arrow",
            ArrowStyle::Open => "Open arrow",
            ArrowStyle::Tick => "Tick",
            ArrowStyle::Dot => "Dot",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ArrowStyle::Closed => "closed",
            ArrowStyle::Open => "open",
            ArrowStyle::Tick => "tick",
            ArrowStyle::Dot => "dot",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == s)
    }
}

/// How dimensions and annotations are drawn: an ISO or ASME preset, adjustable. Sizes are
/// millimetres on paper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DraftStyle {
    pub standard: Standard,
    pub text_height: f64,
    pub arrow: ArrowStyle,
    pub arrow_size: f64,
    /// Gap between the geometry and an extension line.
    pub extension_gap: f64,
    /// How far extension lines run past the dimension line.
    pub extension_overshoot: f64,
    /// ISO: text above the dimension line and parallel to it. ASME: horizontal text in a
    /// break in the line.
    pub aligned_text: bool,
    /// Pad decimals to the display precision (`1.500`).
    pub trailing_zeros: bool,
    /// Write `0.5` rather than `.5`.
    pub leading_zero: bool,
    /// Also show the value in these units, in brackets.
    pub alt_units: Option<LengthFormat>,
    /// The drawing scale as 1 : `scale` (drafting hatch patterns are scaled by it).
    pub scale: f64,
}

impl DraftStyle {
    pub fn iso() -> Self {
        Self {
            standard: Standard::Iso,
            text_height: 3.5,
            arrow: ArrowStyle::Closed,
            arrow_size: 2.5,
            extension_gap: 0.8,
            extension_overshoot: 2.0,
            aligned_text: true,
            trailing_zeros: false,
            leading_zero: true,
            alt_units: None,
            scale: 1.0,
        }
    }

    pub fn asme() -> Self {
        Self {
            standard: Standard::Asme,
            text_height: 3.175,
            arrow: ArrowStyle::Closed,
            arrow_size: 3.175,
            extension_gap: 1.5,
            extension_overshoot: 3.175,
            aligned_text: false,
            trailing_zeros: false,
            leading_zero: true,
            alt_units: None,
            scale: 1.0,
        }
    }

    pub fn preset(standard: Standard) -> Self {
        match standard {
            Standard::Iso => Self::iso(),
            Standard::Asme => Self::asme(),
        }
    }
}

impl DraftStyle {
    /// A length as dimensions show it under this style: the document's units, with trailing
    /// zeros padded to the display precision and the leading zero dropped if the style says
    /// so (decimal formats only).
    pub fn length(&self, units: &Units, mm: f64) -> String {
        let text = units.format_length(mm);
        if units.length == LengthFormat::Architectural {
            return text;
        }
        let split = text
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .unwrap_or(text.len());
        let (number, unit) = text.split_at(split);
        let mut number = number.to_owned();
        if self.trailing_zeros && units.decimals > 0 {
            let decimals = number.split_once('.').map_or(0, |(_, d)| d.len());
            if decimals == 0 {
                number.push('.');
            }
            for _ in decimals..usize::from(units.decimals) {
                number.push('0');
            }
        }
        if !self.leading_zero {
            if let Some(rest) = number.strip_prefix("0.") {
                number = format!(".{rest}");
            } else if let Some(rest) = number.strip_prefix("-0.") {
                number = format!("-.{rest}");
            }
        }
        format!("{number}{unit}")
    }

    /// The value in the alternate units, if the style shows them (`[0.984"]`).
    pub fn alternate(&self, units: &Units, mm: f64) -> Option<String> {
        let length = self.alt_units?;
        let alt = Units { length, ..*units };
        Some(format!("[{}]", self.length(&alt, mm)))
    }
}

impl Default for DraftStyle {
    fn default() -> Self {
        Self::iso()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Constraint, Document, History};

    fn square(doc: &mut Document) -> Vec<EntityId> {
        let p = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
        (0..4)
            .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect()
    }

    #[test]
    fn fills_and_annotations_undo() {
        let mut doc = Document::new();
        let mut history = History::default();
        doc.begin();
        let lines = square(&mut doc);
        history.push(doc.commit("Square"));
        doc.begin();
        let fill = doc.add_fill(Fill::new(lines.clone(), DVec2::splat(5.0), FillKind::Solid));
        let note = doc
            .add_annotation(Annotation::Note {
                text: "Plate".into(),
                at: DVec2::new(3.0, 4.0),
                leader: Some(Leader {
                    to: Anchor::Curve {
                        curve: lines[0],
                        t: 0.5,
                    },
                    end: LeaderEnd::Arrow,
                }),
            })
            .unwrap();
        history.push(doc.commit("Annotate"));
        assert!(doc.fill(fill).is_some());
        assert_eq!(
            doc.anchor_pos(&Anchor::Curve {
                curve: lines[0],
                t: 0.5
            }),
            Some(DVec2::new(5.0, 0.0))
        );
        history.undo(&mut doc);
        assert!(doc.fill(fill).is_none() && doc.annotation(note).is_none());
        history.redo(&mut doc);
        assert!(doc.fill(fill).is_some() && doc.annotation(note).is_some());
    }

    #[test]
    fn deleting_geometry_frees_notes_and_drops_marks() {
        let mut doc = Document::new();
        let lines = square(&mut doc);
        let c = doc.add_point(DVec2::new(5.0, 5.0));
        let circle = doc.add_circle(c, 2.0).unwrap();
        let mark = doc
            .add_annotation(Annotation::CentreMark { curve: circle })
            .unwrap();
        let note = doc
            .add_annotation(Annotation::Note {
                text: "Edge".into(),
                at: DVec2::new(0.0, -3.0),
                leader: Some(Leader {
                    to: Anchor::Curve {
                        curve: lines[0],
                        t: 0.5,
                    },
                    end: LeaderEnd::Dot,
                }),
            })
            .unwrap();
        doc.delete([circle, lines[0]]);
        assert!(doc.annotation(mark).is_none());
        assert_eq!(
            doc.annotation(note),
            Some(&Annotation::Note {
                text: "Edge".into(),
                at: DVec2::new(5.0, -3.0),
                leader: None,
            })
        );
        // Fills survive (they report a lost boundary instead).
        let fill = doc.add_fill(Fill::new(lines, DVec2::splat(5.0), FillKind::Solid));
        doc.delete([lines_first(&doc)]);
        assert!(doc.fill(fill).is_some());
    }

    fn lines_first(doc: &Document) -> EntityId {
        doc.iter()
            .find(|(_, e)| !e.kind.is_point())
            .map(|(id, _)| id)
            .unwrap()
    }

    #[test]
    fn dimension_notes_go_with_their_dimension() {
        let mut doc = Document::new();
        let a = doc.add_point(DVec2::ZERO);
        let b = doc.add_point(DVec2::new(25.0, 0.0));
        let dim = doc
            .add_constraint(Constraint::Distance { a, b, value: 25.0 })
            .unwrap();
        doc.set_dim_note(
            dim,
            Some(DimNote {
                tolerance: Tolerance::Symmetric(0.1),
                ..DimNote::default()
            }),
        );
        assert!(doc.dim_note(dim).is_some());
        doc.set_dim_note(dim, Some(DimNote::default()));
        assert!(doc.dim_note(dim).is_none(), "an empty note is no note");
        doc.set_dim_note(
            dim,
            Some(DimNote {
                prefix: "2×".into(),
                ..DimNote::default()
            }),
        );
        doc.remove_constraint(dim);
        assert!(doc.dim_note(dim).is_none());
    }

    #[test]
    fn style_number_formats() {
        let mm = Units::default();
        let iso = DraftStyle::iso();
        assert_eq!(iso.length(&mm, 12.5), "12.5");
        let mut asme = DraftStyle::asme();
        asme.trailing_zeros = true;
        asme.leading_zero = false;
        let inch = Units {
            length: LengthFormat::Inch,
            ..Units::default()
        };
        assert_eq!(asme.length(&inch, 12.7), ".500\"");
        assert_eq!(asme.length(&inch, 25.4), "1.000\"");
        assert_eq!(asme.length(&mm, -0.5), "-.500");
        let arch = Units::architectural();
        assert_eq!(asme.length(&arch, 18.5 * 25.4), "1'-6 1/2\"");
        let mut dual = DraftStyle::iso();
        dual.alt_units = Some(LengthFormat::Inch);
        assert_eq!(dual.alternate(&mm, 25.4).as_deref(), Some("[1\"]"));
        assert_eq!(iso.alternate(&mm, 25.4), None);
    }

    #[test]
    fn merged_points_carry_annotations() {
        let mut doc = Document::new();
        let o = doc.add_point(DVec2::ZERO);
        let p = doc.add_point(DVec2::new(5.0, 2.0));
        let q = doc.add_point(DVec2::new(5.0, 2.0));
        let ord = doc
            .add_annotation(Annotation::Ordinate {
                origin: o,
                point: q,
                axis: OrdinateAxis::X,
                at: -5.0,
            })
            .unwrap();
        doc.merge_points(q, p).unwrap();
        assert!(matches!(
            doc.annotation(ord),
            Some(Annotation::Ordinate { point, .. }) if *point == p
        ));
    }
}
