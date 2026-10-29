//! Editing state independent of the UI toolkit: document, history, selection, picking and
//! constraint solving.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use drift_core::params::{self, Evaluation};
use drift_core::{
    Bounds, Constraint, ConstraintId, DVec2, Document, DofState, Emphasis, EntityId, EntityKind,
    History, Kind, LineKind, MeasurementId, Shape, SketchItem, Units, Variable, VariableId,
    Variant, VariantId, dimension_kind,
};
use drift_geom::{PointRole, SceneIndex, Snap};
use drift_solver::{Definition, Param, SolveOptions, SolveReport};

mod drag;
use drag::DragSession;
mod constraints;
mod filter;
mod items;
mod options;
mod pointer;
mod regions;
mod selection;
mod solve;
#[cfg(test)]
mod tests;
mod variables;

use drift_core::blocks::exact_rotor;
pub use drift_core::blocks::is_quarter_turn;
pub use filter::{DimMode, PickFilter, PickKind};
pub(crate) use items::ItemLook;
pub use items::TaggedItem;
pub(crate) use items::contrast;
pub use options::ConstraintOption;
pub use pointer::{Anchor, Guide, GuideKind, Pointer};
pub(crate) use regions::RegionsKey;
pub(crate) use regions::interior_point;
pub use regions::{RegionMotion, Regions};

/// Where some geometry is, to put it back after a drag step that went wrong
/// ([`Editor::drag`]), and what the drag keeps.
#[derive(Clone, Default)]
struct Geometry {
    points: Vec<(EntityId, DVec2)>,
    circles: Vec<(EntityId, f64)>,
    /// Arcs' radii.
    radii: std::collections::HashMap<EntityId, f64>,
    /// Lines' lengths to keep where they can (when a move of curves carries them along).
    lengths: std::collections::HashMap<EntityId, f64>,
    /// Lines a move of curves keeps from turning round (all but those with a dragged end).
    lines: Vec<EntityId>,
}

/// Why an edit was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// The constraint can't be satisfied together with the existing ones.
    Conflict,
    /// The constraint is already implied by existing ones.
    Redundant,
    /// Satisfying it would collapse geometry (e.g. a line to zero length).
    Degenerate,
    /// The selection doesn't fit the constraint, or a value is invalid.
    Invalid(String),
    /// Fix found nothing left to pin: what was picked ("this line is", …) can't move already.
    FullyDefined(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::Conflict => write!(f, "conflicts with existing constraints"),
            EditError::Redundant => write!(f, "already implied by existing constraints"),
            EditError::Degenerate => write!(f, "would collapse geometry to zero size"),
            EditError::Invalid(why) => write!(f, "{why}"),
            EditError::FullyDefined(what) => write!(f, "{what} already fully defined"),
        }
    }
}

/// Each copy of a block's extent in the world.
pub(crate) type InstanceBounds =
    std::rc::Rc<std::collections::HashMap<drift_core::InstanceId, Bounds>>;

/// A drag's cached scope: the dragged ids, the document structure it was worked out for, the
/// connected entities, and the same split into parts nothing joins to one another.
type DragScope = (
    Vec<EntityId>,
    u64,
    std::rc::Rc<std::collections::HashSet<EntityId>>,
    DragParts,
);

/// The parts of a drag's scope ([`drift_solver::connected_parts_with`]), each solved on its
/// own ([`Editor::drag`]).
pub(crate) type DragParts = std::rc::Rc<Vec<std::rc::Rc<std::collections::HashSet<EntityId>>>>;

#[derive(Default)]
pub struct Editor {
    pub doc: Document,
    /// The drag in progress, if any ([`Editor::drag`]).
    drag_session: Option<DragSession>,
    /// How many solves the last drag call ran (for the perf tests).
    pub(crate) drag_solves: u32,
    /// How many entities those solves took in, all told (for the tests: what a frame of a
    /// drag costs).
    pub(crate) drag_solved: usize,
    pub history: History,
    pub selection: BTreeSet<EntityId>,
    pub hovered: Option<EntityId>,
    /// Lines whose midpoints are selected (midpoints aren't entities, but can be picked).
    pub selected_midpoints: BTreeSet<EntityId>,
    /// The line whose midpoint is under the pointer.
    pub hovered_midpoint: Option<EntityId>,
    pub index: SceneIndex,
    /// Degrees-of-freedom analysis of the document at `analyzed`, kept cluster by cluster
    /// so an edit analyses again only what it changed.
    analysis_cache: drift_solver::AnalysisCache,
    analyzed: Option<u64>,
    /// A message for the UI explaining a refused edit.
    pub notice: Option<String>,
    /// Geometry the last commit put far off (see [`drift_core::FAR_MM`]), for the UI to
    /// warn about: an import lists it in its report, anything else says so in the status bar.
    pub far_off: Vec<EntityId>,
    /// Entities to emphasise because a related constraint is hovered or selected.
    pub highlight: BTreeSet<EntityId>,
    /// Entities the tool's preview stands in for this frame (a wall's pieces the Wall
    /// tool's next segment would change): not drawn.
    pub preview_hidden: BTreeSet<EntityId>,
    /// A dimension just created by a tool, whose value the UI should let the user type.
    pub pending_edit: Option<ConstraintId>,
    /// A dimension refused because it would over-define the sketch, with its label placement:
    /// the UI offers to add it as a driven (reference) dimension instead.
    pub pending_driven: Option<(Constraint, Option<DVec2>)>,
    /// Whether drawing tools add relations they infer (horizontal / vertical lines, rectangles).
    pub auto_relations: bool,
    /// Variable and linked-dimension values as of the last [`sync`](Self::sync).
    evaluation: Evaluation,
    evaluated: Option<u64>,
    /// Expressions typed into a tool's fields (index = field) that use variables, so the
    /// dimensions the tool creates can stay linked to them.
    pub typed_exprs: [Option<String>; 2],
    /// The option the drawing is in, as of a document revision (comparing whole sketches
    /// isn't free, so not every frame).
    active_option: std::cell::Cell<Option<(u64, Option<VariantId>)>>,
    /// A variable being scrubbed with a slider: its expression before the scrub started.
    scrubbing: Option<(VariableId, String)>,
    /// Closed regions of the solid curves, and what they were found from.
    pub(crate) regions: Option<(RegionsKey, Regions)>,
    /// Areas found lately, and the thread finding an edit's.
    pub(crate) areas: regions::AreasState,
    /// A point inside the selected region (regions are recomputed, so they're picked by a
    /// point rather than an id).
    pub selected_region: Option<DVec2>,
    /// A move or turn in progress, which the selected region's point follows until it is
    /// confirmed (or cancelled).
    pub region_motion: Option<RegionMotion>,
    /// What the select tool picks: everything, or only edges (curves) or only points.
    pub pick_filter: PickFilter,
    /// Tab over a curve: its chain, highlighted to be picked by the next click (the curve
    /// the pointer was on, and the chain).
    pub chain_preview: Option<(EntityId, BTreeSet<EntityId>)>,
    /// Drawing tools draw construction geometry (Alt+C with a drawing tool in hand).
    pub draw_construction: bool,
    /// The tool in hand draws curves (kept up to date by the app): with
    /// [`draw_construction`](Self::draw_construction), what it adds is construction.
    pub drawing_curves: bool,
    /// A click inside an empty closed area picks it whatever the drawing's rules say (the
    /// assistant measures areas; it isn't saved with the drawing).
    pub pick_any_area: bool,
    /// How the dimension tool goes on after a horizontal or vertical dimension.
    pub dim_mode: DimMode,
    /// Chain dimensions end with an overall dimension (a reference) outside them.
    pub dim_overall: bool,
    /// The curves bounding the selected region when it was last found, so it can be found
    /// again when a move takes it somewhere its point doesn't follow (a dimension held part
    /// of it back).
    region_outline: Option<BTreeSet<EntityId>>,
    /// The entity each curve passed to the region finder came from (by index).
    region_ids: std::rc::Rc<Vec<EntityId>>,
    pub selected_fills: BTreeSet<drift_core::FillId>,
    pub hovered_fill: Option<drift_core::FillId>,
    /// How the Fill tool fills.
    pub fill_style: crate::fills::FillStyle,
    /// A fill whose boundary the Fill tool's next click re-picks.
    pub repick_fill: Option<drift_core::FillId>,
    /// The Fill tool's next click picks up a fill's style instead of filling.
    pub fill_eyedropper: bool,
    /// Closed shapes the drawing tools make (rectangles, polygons, circles, ellipses, slots,
    /// closed pen paths) are filled as they are drawn, in [`fill_style`](Self::fill_style).
    pub fill_new_shapes: bool,
    /// How many times a shape's fill has been picked on its own (by a double-click,
    /// Alt-click or the menu's Edit fill): the status line says how until it has been a few
    /// times (kept with the view preferences).
    pub fill_alone_picks: u32,
    pub selected_annotation: Option<drift_core::AnnotationId>,
    /// Dimensions and annotations picked besides the primary ones (the app's picked
    /// dimension and `selected_annotation`): added with Shift, or by a box.
    pub annotation_picks: Vec<drift_core::AnnotationRef>,
    /// A box selection just made, for the app to pick the dimensions and annotations in it
    /// (they are laid out on screen).
    pub box_pick: Option<crate::picks::BoxPick>,
    pub hovered_annotation: Option<drift_core::AnnotationId>,
    /// A note just placed by a tool, whose text the UI should let the user type.
    pub pending_note_edit: Option<drift_core::AnnotationId>,
    /// A view just drawn with the View tool, for the UI to pick.
    pub new_view: Option<drift_core::ViewId>,
    /// The group being edited (its members pick one by one).
    pub open_group: Option<drift_core::GroupId>,
    /// Picked and hovered copies of blocks.
    pub selected_instances: BTreeSet<drift_core::InstanceId>,
    pub hovered_instance: Option<drift_core::InstanceId>,
    /// A block being edited in place.
    pub block_edit: Option<crate::blocks::BlockEdit>,
    /// What the Base point tool's click is for: a block's new base point, or the base point
    /// of a block being made.
    pub base_point_pick: Option<crate::blocks::BasePointPick>,
    /// The Base point tool has done its one pick (the app goes back to the select tool).
    pub base_point_done: bool,
    /// The block the Place block tool places.
    pub place_block: Option<drift_core::BlockId>,
    pub(crate) fill_shapes: Option<crate::fills::FillShapesCache>,
    /// The fills' shapes as the open transaction found them (its serial, the shapes): what
    /// a drag moving a fill and all round it as one moves along (see `moved_fill_shapes`).
    pub(crate) fill_shapes_at_begin: Option<(
        u64,
        std::rc::Rc<std::collections::BTreeMap<drift_core::FillId, crate::fills::FillShape>>,
    )>,
    /// The sketch's solid curves, by [`Document::geometry`](drift_core::Document::geometry).
    pub(crate) solid_cache:
        std::cell::RefCell<Option<(u64, std::rc::Rc<crate::fills::SolidCurves>)>>,
    /// Each copy of a block's extent in the world, by document structure.
    pub(crate) instance_bounds_cache: std::cell::RefCell<Option<(u64, InstanceBounds)>>,
    /// The fills of copies of blocks, in the world (by document revision).
    pub(crate) copy_fills: Option<(u64, std::rc::Rc<Vec<crate::fills::CopyFill>>)>,
    /// The areas among each block's curves, with the curves they were found among (kept
    /// across revisions: a block's curves rarely change).
    pub(crate) block_areas: std::collections::HashMap<
        drift_core::BlockId,
        (
            Vec<drift_core::BlockItem>,
            std::rc::Rc<Vec<drift_geom::regions::Region>>,
        ),
    >,
    /// What draws over what (by document revision).
    pub(crate) stack: Option<(u64, std::rc::Rc<crate::stack::Stack>)>,
    /// Each fill's last region, kept for when its boundary is lost.
    pub(crate) last_fill_shapes:
        std::collections::HashMap<drift_core::FillId, crate::fills::FillShape>,
    /// Whether each fill was last found kept whole across curves that cross it, so an edit
    /// that newly divides its area can be told from one that leaves it as it was.
    pub(crate) fill_crossed: std::collections::HashMap<drift_core::FillId, bool>,
    /// The curves round the holes of each shape's fill, as last found (sorted): what tells
    /// an edit that cut a hole's outline open from one that moved a hole (#201).
    pub(crate) shape_holes: std::collections::HashMap<drift_core::FillId, Vec<EntityId>>,
    /// An import is being committed: its area fills don't become shapes (#202).
    pub(crate) importing: bool,
    /// The last [`fill_kind`](Editor::fill_kind) found: (revision, fill, kind).
    pub(crate) fill_kind_cache: std::cell::RefCell<Option<crate::fills::FillKindFound>>,
    /// What edits did to fills beyond following their outlines, until taken (by the
    /// assistant's results; the app clears it).
    pub fill_changes: Vec<crate::fills::FillChange>,
    /// Sides of the next polygon (0: the default), and whether it's drawn around its circle.
    pub polygon_sides: usize,
    pub polygon_circumscribed: bool,
    /// Which slot the Slot tool draws (U cycles).
    pub slot_mode: crate::tools::SlotMode,
    /// The underlay picked in the Layers tab (calibrate and move act on it).
    pub selected_underlay: Option<drift_core::UnderlayId>,
    /// What the last SVG or PDF import brought in, to calibrate its scale.
    pub last_import: Option<crate::underlays::Imported>,
    pub(crate) underlay_cache: crate::underlays::UnderlayCache,
    /// Show every end and centre point, or (Illustrator-like) only those of hovered or picked
    /// curves.
    pub always_endpoints: bool,
    /// The point the pointer has snapped to while drawing (lit up like a hovered one).
    pub snapped_point: Option<EntityId>,
    /// What a drag of these entities can move (connected through points and relations), kept
    /// while the sketch's structure stays the same: (dragged ids, entity and constraint
    /// counts, the connected set).
    drag_scope: std::cell::RefCell<Option<DragScope>>,
    /// The sketch texts picked, and the one under the pointer.
    pub selected_texts: BTreeSet<drift_core::TextId>,
    pub hovered_text: Option<drift_core::TextId>,
    pub(crate) text_cache: std::cell::RefCell<crate::texts::TextCache>,
    /// The drawing's font as loaded, by its family and file.
    #[allow(clippy::type_complexity)]
    pub(crate) font_cache: std::cell::RefCell<
        Option<(
            (String, usize),
            Option<std::rc::Rc<drift_draft::font::DrawingFont>>,
        )>,
    >,
    /// Overlapping curves, as of a document revision.
    pub(crate) overlap_cache: std::cell::RefCell<Option<selection::Overlaps>>,
    /// The text being typed in place on the canvas (just placed, or double-clicked).
    pub editing_text: Option<drift_core::TextId>,
    /// Where the caret was asked to go in the text being typed (a click that began it).
    pub text_caret_at: Option<DVec2>,
    /// Cap height for new text (0: the default).
    pub text_height: f64,
    /// Bézier handles and their owners, by document structure.
    pub(crate) handle_cache: std::cell::RefCell<Option<crate::curves::HandleCache>>,
    /// The Wall tool's thickness (0: by the drawing's units) and the expression it came
    /// from, which face the centreline is, whether the next segment is an arc, and the
    /// poché: kept across drawings, as the Fill switch is.
    pub wall_thickness: f64,
    pub wall_thickness_expr: Option<String>,
    pub wall_justify: drift_core::Justify,
    pub wall_arc: bool,
    pub wall_poche: crate::walls::PocheSetting,
    /// What Walls from lines does with the lines it uses, and what it did last.
    pub wall_lines: crate::walls_from_lines::UsedLines,
    pub from_lines: Option<crate::walls_from_lines::FromLinesReport>,
    /// The Opening tool bar's family, type, sizes and swing.
    pub opening: crate::openings::OpeningSettings,
    /// Walls' centrelines show (the Wall or Dimension tool is in hand, or View → Wall
    /// centrelines is on); otherwise only a picked or hovered wall's do.
    pub show_centrelines: bool,
    /// Who owns which entity among the walls, by document revision.
    pub(crate) wall_owner_cache: std::cell::RefCell<Option<(u64, std::rc::Rc<drift_core::Owners>)>>,
    /// Relations on walls' faces as the solver sees them, by document revision.
    pub(crate) lowering_cache: std::cell::RefCell<LoweringCache>,
}

/// Walls' face relations lowered onto their centrelines (None: there are none), and the
/// revision they were lowered at.
pub(crate) type LoweringCache = Option<(u64, Option<std::rc::Rc<drift_solver::Lowering>>)>;

impl Editor {
    pub fn new(doc: Document) -> Self {
        let mut editor = Self {
            doc,
            auto_relations: true,
            ..Default::default()
        };
        // Bring linked dimensions up to date with their expressions (not an undo step), and
        // check the walls: a file stores what they made, so nothing should change unless an
        // older build edited it.
        editor.doc.begin();
        if editor.apply_parameters().is_ok() {
            editor.solve(&[]);
        }
        let repaired = editor.repair_walls();
        editor.doc.commit("Load");
        if repaired > 0 {
            editor.notice = Some(format!(
                "Walls were repaired: {repaired} faces edited outside the wall were rebuilt"
            ));
        }
        editor.sync();
        editor
    }

    /// Take the preferences kept on an editor (how new shapes are filled, what is picked,
    /// how the dimension tool goes on) from the one this replaces, for a newly opened drawing.
    pub fn keep_prefs_from(&mut self, was: &Editor) {
        self.opening = was.opening.clone();
        self.pick_any_area = was.pick_any_area;
        self.fill_new_shapes = was.fill_new_shapes;
        self.fill_alone_picks = was.fill_alone_picks;
        self.fill_style = was.fill_style.clone();
        self.pick_filter = was.pick_filter;
        self.dim_mode = was.dim_mode;
        self.dim_overall = was.dim_overall;
        self.draw_construction = was.draw_construction;
        self.wall_thickness = was.wall_thickness;
        self.wall_thickness_expr = was.wall_thickness_expr.clone();
        self.wall_justify = was.wall_justify;
        self.wall_arc = was.wall_arc;
        self.wall_poche = was.wall_poche.clone();
        self.wall_lines = was.wall_lines;
    }

    /// The document revision [`analysis`](Self::analysis) is of.
    pub(crate) fn analyzed(&self) -> Option<u64> {
        self.analyzed
    }

    /// Refresh caches after document changes. Cheap when nothing changed.
    pub fn sync(&mut self) {
        self.index.update(&self.doc);
        if self.analyzed != Some(self.doc.revision()) && !self.doc.in_transaction() {
            let lowering = self.lowering();
            drift_solver::analyze_cached(&self.doc, lowering.as_deref(), &mut self.analysis_cache);
            self.analyzed = Some(self.doc.revision());
        }
        if self.evaluated != Some(self.doc.revision()) {
            self.evaluation = params::evaluate(&self.doc);
            self.evaluated = Some(self.doc.revision());
        }
        let doc = &self.doc;
        // Hidden or locked geometry can't stay selected.
        self.selection
            .retain(|id| doc.contains(*id) && doc.is_visible(*id) && !doc.layer_of(*id).locked);
        let is_line =
            |id: &EntityId| matches!(doc.get(*id).map(|e| e.kind), Some(EntityKind::Line { .. }));
        self.selected_midpoints.retain(is_line);
        if self.hovered_midpoint.is_some_and(|id| !is_line(&id)) {
            self.hovered_midpoint = None;
        }
        if self.hovered.is_some_and(|id| !doc.contains(id)) {
            self.hovered = None;
        }
    }

    /// The degrees-of-freedom analysis of the document as of the last [`sync`](Self::sync).
    pub fn analysis(&self) -> &SolveReport {
        self.analysis_cache.report()
    }
}
