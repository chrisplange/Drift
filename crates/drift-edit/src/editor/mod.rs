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
#[cfg(test)]
mod active_tests;
mod constraints;
#[cfg(test)]
mod drag_speed_tests;
mod edit_prefs;
mod filter;
mod items;
mod layers;
mod options;
mod outbox;
mod pointer;
#[cfg(any(test, feature = "test-support"))]
mod probes;
mod regions;
mod scope;
mod selection;
mod solve;
#[cfg(test)]
mod tests;
mod variables;

use drift_core::blocks::exact_rotor;
pub use drift_core::blocks::is_quarter_turn;
pub use edit_prefs::EditPrefs;
pub use filter::{DimMode, PickFilter, PickKind};
pub(crate) use items::ItemLook;
pub use items::TaggedItem;
pub use items::contrast;
pub use layers::PickedShapes;
pub use layers::{LayerFlag, LayerMove};
pub use options::ConstraintOption;
pub use outbox::Outbox;
pub use pointer::{Anchor, Guide, GuideKind, Pointer};
pub(crate) use regions::RegionsKey;
pub(crate) use regions::interior_point;
pub use regions::{RegionMotion, Regions};
pub use scope::EditScope;

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

/// The maps the drawing's regions are looked up by, and the regions and their curves they
/// were made from (see `Editor::areas_of`).
type KeptAreaMaps = (
    Regions,
    std::rc::Rc<Vec<EntityId>>,
    std::rc::Rc<crate::fills::AreaMaps>,
);

/// A drag's cached scope: the dragged ids, the drawing's make-up it was worked out for, the
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
    /// The tool preferences this drawing is edited with (the app copies them in when it
    /// opens a drawing).
    pub prefs: EditPrefs,
    /// Messages for the UI, each taken once.
    pub out: Outbox,
    /// The drag in progress, if any ([`Editor::drag`]).
    drag_session: Option<DragSession>,
    /// How many solves the last drag call ran (for the perf tests).
    pub(crate) drag_solves: u32,
    /// Of those, how many didn't finish (see [`Editor::unfinished`]; for the tests).
    pub(crate) drag_unfinished: u32,
    /// Whether a drag leaves the curves holding its dragged points where they are rather than
    /// carry them (see `DragFrame::holding`; for the tests: a drag as it was without).
    pub(crate) drag_no_carry: bool,
    /// How many times the last drag call tried a frame again with the solver's fold steps
    /// (see `Editor::drag_frame`; for the tests).
    pub(crate) drag_folds: u32,
    /// Whether a drag never tries a frame with the fold steps (for the tests: a drag as it
    /// was without).
    pub(crate) drag_no_fold: bool,
    /// Whether the modal move's confirm never keeps the open move (for the tests: the move
    /// rolled back and made again from the start, as it was; see `Editor::land_whole_move`).
    pub(crate) land_whole_off: bool,
    /// How many modal moves were kept open on release (for the tests; see
    /// `Editor::land_whole_move`).
    pub(crate) landed_whole: u32,
    /// Whether a drag always looks at its points to tell it goes on (for the tests: as it
    /// was, without telling from the drawing's geometry counter; see `Editor::drag`).
    pub(crate) drag_always_look: bool,
    /// How many entities those solves took in, all told (for the tests: what a frame of a
    /// drag costs).
    pub(crate) drag_solved: usize,
    /// The last constraints added: whether they kept the active element where it was, and how
    /// many iterations the solve they were checked with took (for the tests: none, where it
    /// was kept).
    pub(crate) relation_kept: Option<(bool, usize)>,
    /// How many times the drawing's areas were found here, and crossed fills' and walls'
    /// pochés' areas (for the tests: what an edit or a frame looked for again).
    pub(crate) regions_found: usize,
    /// How many times the open group's own areas were found (see
    /// [`scoped_areas`](Editor::scoped_areas)).
    pub(crate) scoped_regions_found: usize,
    pub(crate) uncrossed_found: std::cell::Cell<usize>,
    /// How many times a fill's last shape was looked inside, for an area to follow it (see
    /// [`fill_shapes`](Editor::fill_shapes)).
    pub(crate) fill_near_points: std::cell::Cell<usize>,
    pub(crate) wall_fills_found: std::cell::Cell<usize>,
    pub history: History,
    pub selection: BTreeSet<EntityId>,
    /// The active element: the one picked last, by a click, which a relation added between
    /// picked elements keeps (#333). Editor state, not saved; see [`active`](Self::active).
    active: Option<EntityId>,
    pub hovered: Option<EntityId>,
    /// A curve hovered last whose centre stays shown and pickable while the pointer stays
    /// within its circle and over nothing else, on its way to the centre (#259).
    pub reach: Option<EntityId>,
    /// Lines whose midpoints are selected (midpoints aren't entities, but can be picked).
    pub selected_midpoints: BTreeSet<EntityId>,
    /// The line whose midpoint is under the pointer.
    pub hovered_midpoint: Option<EntityId>,
    pub index: SceneIndex,
    /// Degrees-of-freedom analysis of the document at `analyzed`, kept cluster by cluster
    /// so an edit analyses again only what it changed.
    analysis_cache: drift_solver::AnalysisCache,
    analyzed: Option<u64>,
    /// Entities to emphasise because a related constraint is hovered or selected.
    pub highlight: BTreeSet<EntityId>,
    /// Entities the tool's preview stands in for this frame (a wall's pieces the Wall
    /// tool's next segment would change): not drawn.
    pub preview_hidden: BTreeSet<EntityId>,
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
    /// The picked region as a move found it (see [`region`](Editor::region)).
    pub(crate) region_moving: Option<regions::RegionMoving>,
    /// The areas among the open group's curves alone, while one is open (see
    /// [`scoped_areas`](Editor::scoped_areas)): (group, its scope, key, areas, the entity
    /// of each curve index).
    #[allow(clippy::type_complexity)]
    pub(crate) scoped_regions: Option<(
        drift_core::GroupId,
        EditScope,
        RegionsKey,
        Regions,
        std::rc::Rc<Vec<EntityId>>,
    )>,
    /// Areas found lately, and the thread finding an edit's.
    pub(crate) areas: regions::AreasState,
    /// A point inside the selected region (regions are recomputed, so they're picked by a
    /// point rather than an id).
    pub selected_region: Option<DVec2>,
    /// A move or turn in progress, which the selected region's point follows until it is
    /// confirmed (or cancelled).
    pub region_motion: Option<RegionMotion>,
    /// Tab over a curve: its chain, highlighted to be picked by the next click (the curve
    /// the pointer was on, and the chain).
    pub chain_preview: Option<(EntityId, BTreeSet<EntityId>)>,
    /// The tool in hand draws curves (kept up to date by the app): with
    /// [`draw_construction`](EditPrefs::draw_construction), what it adds is construction.
    pub drawing_curves: bool,
    /// The curves bounding the selected region when it was last found, so it can be found
    /// again when a move takes it somewhere its point doesn't follow (a dimension held part
    /// of it back).
    region_outline: Option<BTreeSet<EntityId>>,
    /// The entity each curve passed to the region finder came from (by index).
    region_ids: std::rc::Rc<Vec<EntityId>>,
    pub selected_fills: BTreeSet<drift_core::FillId>,
    pub hovered_fill: Option<drift_core::FillId>,
    /// A fill whose boundary the Fill tool's next click re-picks.
    pub repick_fill: Option<drift_core::FillId>,
    /// The Fill tool's next click picks up a fill's style instead of filling.
    pub fill_eyedropper: bool,
    pub selected_annotation: Option<drift_core::AnnotationId>,
    /// Dimensions and annotations picked besides the primary ones (the app's picked
    /// dimension and `selected_annotation`): added with Shift, or by a box.
    pub annotation_picks: Vec<drift_core::AnnotationRef>,
    pub hovered_annotation: Option<drift_core::AnnotationId>,
    /// The group being edited (its members pick one by one).
    pub open_group: Option<drift_core::GroupId>,
    /// Picked and hovered copies of blocks.
    pub selected_instances: BTreeSet<drift_core::InstanceId>,
    pub hovered_instance: Option<drift_core::InstanceId>,
    /// A block being edited in place.
    pub block_edit: Option<crate::blocks::BlockEdit>,
    /// Every block edit begun in this drawing, by its group: an undo or redo that brings
    /// the group back is back in that edit (#319). (Group ids aren't used twice.)
    pub(crate) block_edits: HashMap<drift_core::GroupId, crate::blocks::BlockEdit>,
    /// The copies the last commit turned into a block's curves, inside a block edit (a
    /// block holds no copies, #318), and the curves each became.
    pub(crate) exploded: HashMap<drift_core::InstanceId, Vec<EntityId>>,
    /// What the Base point tool's click is for: a block's new base point, or the base point
    /// of a block being made.
    pub base_point_pick: Option<crate::blocks::BasePointPick>,
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
    /// The maps the drawing's regions are looked up by, for the regions and curves they were
    /// made from (see `Editor::areas_of`).
    pub(crate) area_maps: std::cell::RefCell<Option<KeptAreaMaps>>,
    /// Each crossed fill's area as last found among the curves near it, with what it was
    /// found from: found again only when those change (an edit elsewhere leaves it).
    pub(crate) uncrossed_memo: std::cell::RefCell<
        std::collections::HashMap<drift_core::FillId, crate::fills::UncrossedFound>,
    >,
    /// Each wall poché's area as last found, with what it was found from: found again only
    /// when its outline's curves change (dragging what isn't a wall leaves it).
    pub(crate) wall_fill_memo: std::cell::RefCell<
        std::collections::HashMap<drift_core::FillId, crate::walls::WallFillFound>,
    >,
    /// The curves round the holes of each shape's fill, as last found (sorted): what tells
    /// an edit that cut a hole's outline open from one that moved a hole (#201).
    pub(crate) shape_holes: std::collections::HashMap<drift_core::FillId, Vec<EntityId>>,
    /// An import is being committed: its area fills don't become shapes (#202).
    pub(crate) importing: bool,
    /// The last [`fill_kind`](Editor::fill_kind) found: (revision, fill, kind).
    pub(crate) fill_kind_cache: std::cell::RefCell<Option<crate::fills::FillKindFound>>,
    /// The underlay picked in the Layers tab (calibrate and move act on it).
    pub selected_underlay: Option<drift_core::UnderlayId>,
    /// What the last SVG or PDF import brought in, to calibrate its scale.
    pub last_import: Option<crate::underlays::Imported>,
    pub(crate) underlay_cache: crate::underlays::UnderlayCache,
    /// The point the pointer has snapped to while drawing (lit up like a hovered one).
    pub snapped_point: Option<EntityId>,
    /// What a drag of these entities can move (connected through points and relations), kept
    /// while the sketch's make-up apart from fills stays the same: (dragged ids, the
    /// make-up, the connected set and its parts).
    drag_scope: std::cell::RefCell<Option<DragScope>>,
    /// What tools see while a group or block is open for editing (see
    /// [`edit_scope`](Editor::edit_scope)), by group and document structure.
    edit_scope_cache: std::cell::RefCell<Option<(drift_core::GroupId, u64, EditScope)>>,
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
    /// Block copies stacked on others, found once per change (see
    /// [`Editor::overlapping_instances`]).
    pub(crate) copy_overlap_cache: std::cell::RefCell<Option<selection::CopyOverlaps>>,
    /// The text being typed in place on the canvas (just placed, or double-clicked).
    pub editing_text: Option<drift_core::TextId>,
    /// Where the caret was asked to go in the text being typed (a click that began it).
    pub text_caret_at: Option<DVec2>,
    /// Bézier handles and their owners, by document structure.
    pub(crate) handle_cache: std::cell::RefCell<Option<crate::curves::HandleCache>>,
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
    /// An editor over `doc` with `prefs` and nothing worked out yet (no snap index, no
    /// analysis): what only reads the drawing, such as output, needs no more.
    pub(crate) fn bare(doc: Document, prefs: EditPrefs) -> Self {
        Self {
            doc,
            prefs,
            ..Default::default()
        }
    }

    pub fn new(doc: Document) -> Self {
        let mut editor = Self {
            doc,
            prefs: EditPrefs {
                auto_relations: true,
                ..Default::default()
            },
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
        let offsets_repaired = editor.repair_offsets();
        editor.doc.commit("Load");
        let mut notices = Vec::new();
        if repaired > 0 {
            notices.push(format!(
                "Walls were repaired: {repaired} faces edited outside the wall were rebuilt"
            ));
        }
        if offsets_repaired > 0 {
            notices.push(format!(
                "Offsets were repaired: {offsets_repaired} curves edited outside the offset \
                 were rebuilt"
            ));
        }
        if !notices.is_empty() {
            editor.out.inform(notices.join(" · "));
        }
        editor.sync();
        editor
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
        if self.active.is_some_and(|id| !self.selection.contains(&id)) {
            self.active = None;
        }
        let is_line =
            |id: &EntityId| matches!(doc.get(*id).map(|e| e.kind), Some(EntityKind::Line { .. }));
        self.selected_midpoints.retain(is_line);
        if self.hovered_midpoint.is_some_and(|id| !is_line(&id)) {
            self.hovered_midpoint = None;
        }
        if self.hovered.is_some_and(|id| !doc.contains(id)) {
            self.hovered = None;
        }
        if self.reach.is_some_and(|id| !doc.contains(id)) {
            self.reach = None;
        }
    }

    /// The degrees-of-freedom analysis of the document as of the last [`sync`](Self::sync).
    pub fn analysis(&self) -> &SolveReport {
        self.analysis_cache.report()
    }
}
