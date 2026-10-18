//! Interactive tools as small state machines over [`Editor`].
//!
//! Tools receive pointer input already converted to world space and snapped, so they are
//! independent of the UI toolkit and can be tested by feeding them events.

use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};

use drift_core::{
    Bounds, Constraint, DVec2, Emphasis, EntityId, EntityKind, Shape, SketchItem, arc_shape,
};
use drift_solver::Param;

use crate::editor::{Anchor, Editor, Guide, GuideKind, Pointer};

mod annotate;
mod arc;
mod circle;
mod corners;
mod curve_ops;
mod dimension;
mod ellipse;
mod fill;
mod line;
mod offset_mirror;
mod pen;
mod place_block;
mod point;
mod polygon;
mod rectangle;
mod rotate;
mod select;
mod slot;
mod tangent_arc;
#[cfg(test)]
mod tests;
mod text;
mod three_point_arc;
mod underlay;
pub(crate) mod view;

pub use self::{
    annotate::{CalloutTool, CentreMarkTool, ElevationTool, NoteTool, OrdinateTool, SectionTool},
    corners::{ChamferTool, FilletTool},
    curve_ops::{ExtendTool, SplitTool, TrimTool},
    fill::FillTool,
    offset_mirror::{MirrorTool, OffsetTool},
    pen::PenTool,
    place_block::BlockTool,
    polygon::PolygonTool,
    slot::{SlotMode, SlotTool},
    text::TextTool,
    underlay::{UnderlayMode, UnderlayTool},
    view::ViewTool,
};
pub use self::{
    arc::ArcTool, circle::CircleTool, dimension::DimensionTool, ellipse::EllipseTool,
    line::LineTool, point::PointTool, rectangle::RectangleTool, select::SelectTool,
    tangent_arc::TangentArcTool, three_point_arc::ThreePointArcTool,
};

/// The geometry under the pointer that a temporary dimension's far end can measure to: a
/// point, or a line (picked at its middle, the line).
pub(crate) fn reference_at(ed: &Editor, p: &Pointer) -> Option<EntityId> {
    select::pick_target_with(ed, p, crate::editor::PickFilter::All).map(|t| match t {
        select::Target::Entity(id) | select::Target::Midpoint(id) => id,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Add to the selection.
    pub shift: bool,
    /// Toggle in the selection (Ctrl, or Cmd on macOS).
    pub command: bool,
    /// Alt (Option on macOS): place freely, or the Fill tool's eyedropper.
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    /// The pointer moved (or the frame was redrawn) with no button action.
    Hover(Pointer),
    Click(Pointer, Modifiers),
    DoubleClick(Pointer),
    /// A primary-button drag began. The pointer is at the press position.
    DragStart(Pointer, Modifiers),
    Drag(Pointer),
    DragEnd(Pointer, Modifiers),
    /// Finish the current operation (right click or Enter).
    Finish,
}

/// What the overlay should draw for the tool, beyond its preview geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overlay {
    SelectionBox {
        rect: Bounds,
        crossing: bool,
    },
    /// A modal move: the base point and where it has been moved to (`None` while the user is
    /// picking a new base point).
    Move {
        base: DVec2,
        to: Option<DVec2>,
    },
    /// A modal rotate: the pivot, the reference direction's point, and the angle so far
    /// (`None` while a new pivot is being picked).
    Rotate {
        pivot: DVec2,
        from: DVec2,
        degrees: Option<f64>,
    },
    /// Highlight the closed area around a point (the Fill tool's preview).
    Region {
        at: DVec2,
    },
}

/// Keys that tools can claim (Blender-style modal keys). Unclaimed keys keep their global
/// meaning (e.g. R picks the rectangle tool).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKey {
    /// Lock to the X axis.
    X,
    /// Lock to the Y axis.
    Y,
    /// Grab: move the selection.
    G,
    /// Pick the base point of a move (or the pivot of a rotate).
    B,
    /// Rotate the selection.
    R,
}

/// A world axis a drawing tool can lock the segment being drawn to (Blender-style X / Y keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    pub fn dir(self) -> DVec2 {
        match self {
            Axis::X => DVec2::X,
            Axis::Y => DVec2::Y,
        }
    }

    /// The relation a segment locked to this axis gets.
    fn relation(self) -> GuideKind {
        match self {
            Axis::X => GuideKind::Horizontal,
            Axis::Y => GuideKind::Vertical,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Select,
    Line,
    Pen,
    Rectangle,
    Polygon,
    Circle,
    Ellipse,
    EllipseArc,
    /// Centre-point arc.
    Arc,
    ThreePointArc,
    TangentArc,
    Slot,
    Point,
    Text,
    Block,
    Dimension,
    Ordinate,
    Note,
    Callout,
    CentreMark,
    /// A section's cutting line, with a flag at each end.
    Section,
    /// An elevation's flag.
    Elevation,
    /// A view of the model: its crop.
    View,
    Fill,
    Trim,
    Extend,
    Split,
    Fillet,
    Chamfer,
    Offset,
    Mirror,
    /// Set an underlay's scale from two points and a known distance (from the Layers tab).
    Calibrate,
    /// Move an underlay by two points (from the Layers tab).
    MoveUnderlay,
}

impl ToolKind {
    pub const ALL: [ToolKind; 31] = [
        ToolKind::Select,
        ToolKind::Line,
        ToolKind::Pen,
        ToolKind::Rectangle,
        ToolKind::Polygon,
        ToolKind::Circle,
        ToolKind::Ellipse,
        ToolKind::EllipseArc,
        ToolKind::Arc,
        ToolKind::ThreePointArc,
        ToolKind::TangentArc,
        ToolKind::Slot,
        ToolKind::Point,
        ToolKind::Text,
        ToolKind::Block,
        ToolKind::Dimension,
        ToolKind::Ordinate,
        ToolKind::Note,
        ToolKind::Callout,
        ToolKind::CentreMark,
        ToolKind::Section,
        ToolKind::Elevation,
        ToolKind::View,
        ToolKind::Fill,
        ToolKind::Trim,
        ToolKind::Extend,
        ToolKind::Split,
        ToolKind::Fillet,
        ToolKind::Chamfer,
        ToolKind::Offset,
        ToolKind::Mirror,
    ];

    /// The toolbar's sections, with their headers (Select stands alone at the top).
    pub const SECTIONS: [(&'static str, &'static [ToolKind]); 4] = [
        ("", &[ToolKind::Select]),
        (
            "Draw",
            &[
                ToolKind::Line,
                ToolKind::Pen,
                ToolKind::Rectangle,
                ToolKind::Polygon,
                ToolKind::Circle,
                ToolKind::Ellipse,
                ToolKind::EllipseArc,
                ToolKind::Arc,
                ToolKind::ThreePointArc,
                ToolKind::TangentArc,
                ToolKind::Slot,
                ToolKind::Point,
                ToolKind::Text,
                ToolKind::Block,
            ],
        ),
        (
            "Annotate",
            &[
                ToolKind::Dimension,
                ToolKind::Ordinate,
                ToolKind::Note,
                ToolKind::Callout,
                ToolKind::CentreMark,
                ToolKind::Section,
                ToolKind::Elevation,
                ToolKind::View,
                ToolKind::Fill,
            ],
        ),
        (
            "Modify",
            &[
                ToolKind::Trim,
                ToolKind::Extend,
                ToolKind::Split,
                ToolKind::Fillet,
                ToolKind::Chamfer,
                ToolKind::Offset,
                ToolKind::Mirror,
            ],
        ),
    ];

    /// The circle tools, in the order pressing C cycles through them.
    pub const CIRCLES: [ToolKind; 3] = [ToolKind::Circle, ToolKind::Ellipse, ToolKind::EllipseArc];

    /// Whether the tool draws curves (which can be drawn as construction geometry).
    pub fn draws_curves(self) -> bool {
        matches!(
            self,
            ToolKind::Line
                | ToolKind::Pen
                | ToolKind::Rectangle
                | ToolKind::Polygon
                | ToolKind::Circle
                | ToolKind::Ellipse
                | ToolKind::EllipseArc
                | ToolKind::Arc
                | ToolKind::ThreePointArc
                | ToolKind::TangentArc
                | ToolKind::Slot
        )
    }

    /// The arc tools, in the order pressing A cycles through them.
    pub const ARCS: [ToolKind; 3] = [ToolKind::Arc, ToolKind::ThreePointArc, ToolKind::TangentArc];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Select => "Select",
            ToolKind::Line => "Line",
            ToolKind::Pen => "Pen",
            ToolKind::Rectangle => "Rectangle",
            ToolKind::Polygon => "Polygon",
            ToolKind::Slot => "Slot",
            ToolKind::Circle => "Circle",
            ToolKind::Ellipse => "Ellipse",
            ToolKind::EllipseArc => "Partial ellipse",
            ToolKind::Arc => "Arc",
            ToolKind::ThreePointArc => "3-pt arc",
            ToolKind::TangentArc => "Tangent arc",
            ToolKind::Point => "Point",
            ToolKind::Text => "Text",
            ToolKind::Block => "Place block",
            ToolKind::Dimension => "Dimension",
            ToolKind::Fill => "Fill / hatch",
            ToolKind::Ordinate => "Ordinate",
            ToolKind::Note => "Note",
            ToolKind::Callout => "Callout",
            ToolKind::CentreMark => "Centre mark",
            ToolKind::Section => "Section mark",
            ToolKind::Elevation => "Elevation mark",
            ToolKind::View => "View",
            ToolKind::Trim => "Trim",
            ToolKind::Extend => "Extend",
            ToolKind::Split => "Split",
            ToolKind::Fillet => "Fillet",
            ToolKind::Chamfer => "Chamfer",
            ToolKind::Offset => "Offset",
            ToolKind::Mirror => "Mirror",
            ToolKind::Calibrate => "Calibrate underlay",
            ToolKind::MoveUnderlay => "Move underlay",
        }
    }

    /// One line on what the tool does, for tooltips.
    pub fn description(self) -> &'static str {
        match self {
            ToolKind::Select => "Pick, drag and box-select geometry; G moves the selection",
            ToolKind::Line => "Draw connected lines; type a length, X / Y lock an axis",
            ToolKind::Pen => {
                "Curves: click for corners, drag for smooth points (Alt breaks the handle)"
            }
            ToolKind::Rectangle => "Two opposite corners, or type the width and height",
            ToolKind::Polygon => {
                "Regular polygon: centre, then a corner. Tab types sides; X: around the circle"
            }
            ToolKind::Slot => "Straight, centre or arc slot (press U again to switch), then width",
            ToolKind::Circle => "Centre, then a point on the circle (or type the diameter)",
            ToolKind::Ellipse => "Centre, one axis end, then the other axis (C again: partial)",
            ToolKind::EllipseArc => "An ellipse's centre and axes, then where it starts and ends",
            ToolKind::Arc => "Centre, start, end. Press A again for other arc types",
            ToolKind::ThreePointArc => "Start, end, then a point on the arc",
            ToolKind::TangentArc => "Continues tangent from the end of a line or arc",
            ToolKind::Point => "A sketch point, for reference or construction",
            ToolKind::Text => "A line of text drawn as outlines, convertible to curves",
            ToolKind::Block => "Place copies of a block from the Blocks tab; R turns, X mirrors",
            ToolKind::Dimension => "Click geometry, then place the label: the type follows",
            ToolKind::Fill => "Fill or hatch a closed area: click inside it",
            ToolKind::Ordinate => "X or Y distances from an origin point, in rows",
            ToolKind::Note => "Text, with a leader if you start on geometry",
            ToolKind::Callout => "Hole (4× Ø6 THRU) and chamfer (2 × 45°) callouts",
            ToolKind::CentreMark => "Centre marks on circles; centre lines between features",
            ToolKind::Section => "A section's cutting line, with a lettered flag at each end",
            ToolKind::Elevation => "A numbered flag pointing the way an elevation looks",
            ToolKind::View => "Drag a crop of the model: a named view at a scale, for sheets",
            ToolKind::Trim => "Remove a piece up to the curves that cut it; drag to trim several",
            ToolKind::Extend => "Lengthen a line or arc to the next curve",
            ToolKind::Split => "Cut a curve in two at a point (a circle or ellipse into halves)",
            ToolKind::Fillet => "Round the corner where two lines meet",
            ToolKind::Chamfer => "Cut the corner where two lines meet",
            ToolKind::Offset => "A parallel copy of a curve and the curves connected to it",
            ToolKind::Mirror => "Mirror the selection about a line, kept symmetric",
            ToolKind::Calibrate => {
                "Two points on the underlay, then the real distance between them"
            }
            ToolKind::MoveUnderlay => "A point on the underlay, then where it goes",
        }
    }

    /// For a tool in a cycle (C: circle, ellipse, partial ellipse), how its key steps
    /// through the cycle.
    pub fn cycle_hint(self) -> Option<String> {
        let key = self.shortcut()?;
        let cycle: Vec<&str> = Self::SECTIONS
            .iter()
            .flat_map(|(_, group)| group.iter())
            .filter(|k| k.shortcut() == Some(key))
            .map(|k| k.label())
            .collect();
        (cycle.len() > 1).then(|| {
            format!(
                "{key} again switches through: {}",
                cycle.join(", ").to_lowercase()
            )
        })
    }

    /// The shortcut as shown to the user: the key, or "Shift+T" for the text tool.
    pub fn shortcut_label(self) -> Option<String> {
        match self {
            ToolKind::Text => Some("Shift+T".to_owned()),
            _ => self.shortcut().map(String::from),
        }
    }

    pub fn shortcut(self) -> Option<char> {
        Some(match self {
            ToolKind::Select => 'S',
            ToolKind::Line => 'L',
            ToolKind::Pen => 'P',
            ToolKind::Rectangle => 'R',
            ToolKind::Polygon => 'K',
            ToolKind::Slot => 'U',
            // C selects the circle; pressed again it cycles through the ellipse tools.
            ToolKind::Circle | ToolKind::Ellipse | ToolKind::EllipseArc => 'C',
            // A selects the centre arc; pressed again it cycles through the arc tools.
            ToolKind::Arc | ToolKind::ThreePointArc | ToolKind::TangentArc => 'A',
            // V for vertex (P is the pen, as in Illustrator).
            ToolKind::Point => 'V',
            ToolKind::Block => 'I',
            ToolKind::Dimension => 'D',
            ToolKind::Fill => 'H',
            ToolKind::Note => 'N',
            ToolKind::Trim => 'T',
            ToolKind::Extend => 'E',
            ToolKind::Offset => 'O',
            ToolKind::Mirror => 'M',
            // F selects the fillet; pressed again it switches to the chamfer.
            ToolKind::Fillet | ToolKind::Chamfer => 'F',
            ToolKind::Split
            | ToolKind::Section
            | ToolKind::Elevation
            | ToolKind::View
            | ToolKind::Ordinate
            | ToolKind::Callout
            | ToolKind::CentreMark
            | ToolKind::Text
            | ToolKind::Calibrate
            | ToolKind::MoveUnderlay => return None,
        })
    }

    pub fn create(self) -> Box<dyn Tool> {
        match self {
            ToolKind::Select => Box::<SelectTool>::default(),
            ToolKind::Line => Box::<LineTool>::default(),
            ToolKind::Pen => Box::<PenTool>::default(),
            ToolKind::Rectangle => Box::<RectangleTool>::default(),
            ToolKind::Polygon => Box::<PolygonTool>::default(),
            ToolKind::Slot => Box::<SlotTool>::default(),
            ToolKind::Circle => Box::<CircleTool>::default(),
            ToolKind::Ellipse => Box::<EllipseTool>::default(),
            ToolKind::EllipseArc => Box::new(EllipseTool::partial()),
            ToolKind::Arc => Box::<ArcTool>::default(),
            ToolKind::ThreePointArc => Box::<ThreePointArcTool>::default(),
            ToolKind::TangentArc => Box::<TangentArcTool>::default(),
            ToolKind::Point => Box::<PointTool>::default(),
            ToolKind::Text => Box::<TextTool>::default(),
            ToolKind::Block => Box::<BlockTool>::default(),
            ToolKind::Dimension => Box::<DimensionTool>::default(),
            ToolKind::Fill => Box::<FillTool>::default(),
            ToolKind::Ordinate => Box::<OrdinateTool>::default(),
            ToolKind::Note => Box::<NoteTool>::default(),
            ToolKind::Callout => Box::<CalloutTool>::default(),
            ToolKind::CentreMark => Box::<CentreMarkTool>::default(),
            ToolKind::Section => Box::<SectionTool>::default(),
            ToolKind::Elevation => Box::<ElevationTool>::default(),
            ToolKind::View => Box::<ViewTool>::default(),
            ToolKind::Trim => Box::<TrimTool>::default(),
            ToolKind::Extend => Box::<ExtendTool>::default(),
            ToolKind::Split => Box::<SplitTool>::default(),
            ToolKind::Fillet => Box::<FilletTool>::default(),
            ToolKind::Chamfer => Box::<ChamferTool>::default(),
            ToolKind::Offset => Box::<OffsetTool>::default(),
            ToolKind::Mirror => Box::<MirrorTool>::default(),
            ToolKind::Calibrate => Box::new(UnderlayTool::new(UnderlayMode::Calibrate)),
            ToolKind::MoveUnderlay => Box::new(UnderlayTool::new(UnderlayMode::Move)),
        }
    }
}

pub trait Tool {
    fn kind(&self) -> ToolKind;

    /// One-line instruction for the status bar.
    fn hint(&self) -> String;

    fn handle(&mut self, input: Input, ed: &mut Editor);

    /// Abandon the operation in progress (for the select tool with nothing in progress: clear
    /// the selection). Returns false if there was nothing to cancel.
    fn cancel(&mut self, ed: &mut Editor) -> bool;

    /// True while an operation is in progress (a drag, or a shape with points already placed).
    fn is_busy(&self) -> bool;

    /// Geometry to draw for the operation in progress.
    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        Vec::new()
    }

    /// Previews that need the document (e.g. the piece a trim would remove).
    fn editor_preview(&self, _ed: &Editor) -> Vec<SketchItem> {
        Vec::new()
    }

    /// A live measurement to show next to the cursor, e.g. a length.
    fn measurement(&self, _pointer: &Pointer) -> Option<String> {
        None
    }

    fn overlay(&self) -> Option<Overlay> {
        None
    }

    /// Whether the pointer should snap right now.
    fn wants_snap(&self) -> bool {
        true
    }

    /// Entities that must not be snapped to (e.g. the geometry being dragged).
    fn snap_excludes(&self, _ed: &Editor, _id: EntityId) -> bool {
        false
    }

    /// Adjust the pointer after snapping (e.g. lock a line horizontal).
    fn refine_pointer(&self, _pointer: &mut Pointer) {}

    /// A modal key was pressed with the pointer at `pointer` (if over the canvas). Returns
    /// false if the tool doesn't use the key, so it can do something else.
    fn key(&mut self, _key: ToolKey, _pointer: Option<&Pointer>, _ed: &mut Editor) -> bool {
        false
    }

    /// True during a modal operation (a grab): other shortcuts are suspended and right-click
    /// cancels it.
    fn is_modal(&self) -> bool {
        false
    }

    /// The existing point a chain of curves (polyline, tangent arcs) currently ends at, so
    /// switching between line and tangent arc can continue from it.
    fn chain_end(&self) -> Option<EntityId> {
        None
    }

    /// Continue a chain from an existing point (after switching tools mid-chain).
    fn continue_from(&mut self, _point: EntityId, _ed: &mut Editor) {}

    /// The active axis lock and the point the locked segment runs from, for the overlay.
    fn axis_lock(&self) -> Option<(DVec2, Axis)> {
        None
    }

    /// Labels of the values the user can type right now (at most two), e.g. length and angle.
    fn fields(&self) -> &'static [&'static str] {
        &[]
    }

    /// The values typed so far (`None` = not typed), for previews and measurements.
    fn set_typed(&mut self, _values: [Option<f64>; 2]) {}

    /// Enter was pressed while typing: apply the typed values.
    fn commit_typed(&mut self, _pointer: &Pointer, _ed: &mut Editor) {}

    /// A dimension being placed (and its label offset), for the overlay to preview.
    fn pending_dimension(
        &self,
        _ed: &Editor,
        _pointer: Option<&Pointer>,
    ) -> Option<(Constraint, DVec2)> {
        None
    }
}

fn preview(shape: Shape) -> SketchItem {
    SketchItem::new(shape).with_emphasis(Emphasis::Preview)
}

fn preview_vertex(at: DVec2) -> SketchItem {
    preview(Shape::Point(at)).as_vertex()
}

// ---------------------------------------------------------------------------------------------
// Shared helpers for drawing tools

/// Field labels for typing the first point of a shape.
const XY: &[&str] = &["X", "Y"];

/// The typed coordinates, falling back to the pointer for any not typed.
fn typed_point(typed: [Option<f64>; 2], p: &Pointer) -> DVec2 {
    let at = p.pos();
    DVec2::new(typed[0].unwrap_or(at.x), typed[1].unwrap_or(at.y))
}

fn any_typed(typed: [Option<f64>; 2]) -> bool {
    typed.iter().any(Option::is_some)
}

/// A pointer at an exact location, without snaps (typed input creates new points).
fn exact(p: &Pointer, at: DVec2) -> Pointer {
    Pointer::new(at, p.tolerance)
}

/// Add a relation if automatic relations are on; failures (e.g. redundancy) are ignored and
/// the commit's solve decides whether the whole step is acceptable.
fn auto_relation(ed: &mut Editor, relation: Option<GuideKind>, line: EntityId) {
    if !ed.auto_relations {
        return;
    }
    let constraint = match relation {
        Some(GuideKind::Horizontal) => Constraint::Horizontal { line },
        Some(GuideKind::Vertical) => Constraint::Vertical { line },
        _ => return,
    };
    let _ = ed.doc.add_constraint(constraint);
}

/// Unit vector at `degrees`, exact for multiples of 90° (where `cos`/`sin` would leave ~1e-16
/// residue that the solver would then spread onto already-placed points).
fn exact_direction(degrees: f64) -> DVec2 {
    let quarter = degrees / 90.0;
    if (quarter - quarter.round()).abs() <= 1e-12 {
        match (quarter.round() as i64).rem_euclid(4) {
            0 => DVec2::X,
            1 => DVec2::Y,
            2 => -DVec2::X,
            _ => -DVec2::Y,
        }
    } else {
        DVec2::from_angle(degrees.to_radians())
    }
}

/// Horizontal/vertical relation implied by a typed angle that is a multiple of 90°.
fn relation_for_angle(degrees: f64) -> Option<GuideKind> {
    let quarter = degrees / 90.0;
    if (quarter - quarter.round()).abs() > 1e-12 {
        return None;
    }
    Some(if (quarter.round() as i64).rem_euclid(2) == 0 {
        GuideKind::Horizontal
    } else {
        GuideKind::Vertical
    })
}
