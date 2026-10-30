//! The tool preferences an editor works with: not about the drawing, so they carry over to
//! the next one.

use super::{DimMode, PickFilter};

/// How the tools behave and what they make, as the user last left them. The app copies
/// these into the editor of each drawing it opens.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditPrefs {
    /// Whether drawing tools add relations they infer (horizontal / vertical lines, rectangles).
    pub auto_relations: bool,
    /// What the select tool picks: everything, or only edges (curves) or only points.
    pub pick_filter: PickFilter,
    /// Drawing tools draw construction geometry (Alt+C with a drawing tool in hand).
    pub draw_construction: bool,
    /// A click inside an empty closed area picks it whatever the drawing's rules say (the
    /// assistant measures areas; it isn't saved with the drawing).
    pub pick_any_area: bool,
    /// How the dimension tool goes on after a horizontal or vertical dimension.
    pub dim_mode: DimMode,
    /// Chain dimensions end with an overall dimension (a reference) outside them.
    pub dim_overall: bool,
    /// How the Fill tool fills.
    pub fill_style: crate::fills::FillStyle,
    /// Closed shapes the drawing tools make (rectangles, polygons, circles, ellipses, slots,
    /// closed pen paths) are filled as they are drawn, in [`fill_style`](Self::fill_style).
    pub fill_new_shapes: bool,
    /// How many times a shape's fill has been picked on its own (by a double-click,
    /// Alt-click or the menu's Edit fill): the status line says how until it has been a few
    /// times (kept with the view preferences).
    pub fill_alone_picks: u32,
    /// Sides of the next polygon (0: the default), and whether it's drawn around its circle.
    pub polygon_sides: usize,
    pub polygon_circumscribed: bool,
    /// Which slot the Slot tool draws (U cycles).
    pub slot_mode: crate::tools::SlotMode,
    /// Show every end and centre point, or (Illustrator-like) only those of hovered or picked
    /// curves.
    pub always_endpoints: bool,
    /// Cap height for new text (0: the default).
    pub text_height: f64,
    /// The Wall tool's thickness (0: by the drawing's units) and the expression it came
    /// from, which face the centreline is, whether the next segment is an arc, and the
    /// poché: kept across drawings, as the Fill switch is.
    pub wall_thickness: f64,
    pub wall_thickness_expr: Option<String>,
    pub wall_justify: drift_core::Justify,
    pub wall_arc: bool,
    pub wall_poche: crate::walls::PocheSetting,
    /// What Walls from lines does with the lines it uses.
    pub wall_lines: crate::walls_from_lines::UsedLines,
    /// The Opening tool bar's family, type, sizes and swing.
    pub opening: crate::openings::OpeningSettings,
}
