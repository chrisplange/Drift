//! One-shot messages from an editor to the UI.

use drift_core::{Constraint, ConstraintId, DVec2, EntityId};

/// What an edit or a tool left for the UI to act on once: the UI takes each when it handles
/// it; a client without a UI reads `notice` and ignores the rest.
#[derive(Default)]
pub struct Outbox {
    /// A message for the UI explaining a refused edit.
    pub notice: Option<String>,
    /// Geometry the last commit put far off (see [`drift_core::FAR_MM`]), for the UI to
    /// warn about: an import lists it in its report, anything else says so in the status bar.
    pub far_off: Vec<EntityId>,
    /// A dimension just created by a tool, whose value the UI should let the user type.
    pub pending_edit: Option<ConstraintId>,
    /// A dimension refused because it would over-define the sketch, with its label placement:
    /// the UI offers to add it as a driven (reference) dimension instead.
    pub pending_driven: Option<(Constraint, Option<DVec2>)>,
    /// A box selection just made, for the app to pick the dimensions and annotations in it
    /// (they are laid out on screen).
    pub box_pick: Option<crate::picks::BoxPick>,
    /// A note just placed by a tool, whose text the UI should let the user type.
    pub pending_note_edit: Option<drift_core::AnnotationId>,
    /// A view just drawn with the View tool, for the UI to pick.
    pub new_view: Option<drift_core::ViewId>,
    /// The Base point tool has done its one pick (the app goes back to the select tool).
    pub base_point_done: bool,
    /// What edits did to fills beyond following their outlines, until taken (by the
    /// assistant's results; the app clears it).
    pub fill_changes: Vec<crate::fills::FillChange>,
    /// What Walls from lines did last.
    pub from_lines: Option<crate::walls_from_lines::FromLinesReport>,
}
