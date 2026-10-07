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

mod arc;
mod circle;
mod dimension;
mod line;
mod point;
mod rectangle;
mod select;
mod tangent_arc;
#[cfg(test)]
mod tests;
mod three_point_arc;

pub use self::{
    arc::ArcTool, circle::CircleTool, dimension::DimensionTool, line::LineTool, point::PointTool,
    rectangle::RectangleTool, select::SelectTool, tangent_arc::TangentArcTool,
    three_point_arc::ThreePointArcTool,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Add to the selection.
    pub shift: bool,
    /// Toggle in the selection (Ctrl, or Cmd on macOS).
    pub command: bool,
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
}

/// Keys that tools can claim (Blender-style modal keys). Unclaimed keys keep their global
/// meaning (e.g. X toggles construction).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKey {
    /// Lock to the X axis.
    X,
    /// Lock to the Y axis.
    Y,
    /// Grab: move the selection.
    G,
    /// Pick the base point of a move.
    B,
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
    Rectangle,
    Circle,
    /// Centre-point arc.
    Arc,
    ThreePointArc,
    TangentArc,
    Point,
    Dimension,
}

impl ToolKind {
    pub const ALL: [ToolKind; 9] = [
        ToolKind::Select,
        ToolKind::Line,
        ToolKind::Rectangle,
        ToolKind::Circle,
        ToolKind::Arc,
        ToolKind::ThreePointArc,
        ToolKind::TangentArc,
        ToolKind::Point,
        ToolKind::Dimension,
    ];

    /// The arc tools, in the order pressing A cycles through them.
    pub const ARCS: [ToolKind; 3] = [ToolKind::Arc, ToolKind::ThreePointArc, ToolKind::TangentArc];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Select => "Select",
            ToolKind::Line => "Line",
            ToolKind::Rectangle => "Rectangle",
            ToolKind::Circle => "Circle",
            ToolKind::Arc => "Arc",
            ToolKind::ThreePointArc => "3-pt arc",
            ToolKind::TangentArc => "Tangent arc",
            ToolKind::Point => "Point",
            ToolKind::Dimension => "Dimension",
        }
    }

    pub fn shortcut(self) -> char {
        match self {
            ToolKind::Select => 'S',
            ToolKind::Line => 'L',
            ToolKind::Rectangle => 'R',
            ToolKind::Circle => 'C',
            // A selects the centre arc; pressed again it cycles through the arc tools.
            ToolKind::Arc | ToolKind::ThreePointArc | ToolKind::TangentArc => 'A',
            ToolKind::Point => 'P',
            ToolKind::Dimension => 'D',
        }
    }

    pub fn create(self) -> Box<dyn Tool> {
        match self {
            ToolKind::Select => Box::<SelectTool>::default(),
            ToolKind::Line => Box::<LineTool>::default(),
            ToolKind::Rectangle => Box::<RectangleTool>::default(),
            ToolKind::Circle => Box::<CircleTool>::default(),
            ToolKind::Arc => Box::<ArcTool>::default(),
            ToolKind::ThreePointArc => Box::<ThreePointArcTool>::default(),
            ToolKind::TangentArc => Box::<TangentArcTool>::default(),
            ToolKind::Point => Box::<PointTool>::default(),
            ToolKind::Dimension => Box::<DimensionTool>::default(),
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
