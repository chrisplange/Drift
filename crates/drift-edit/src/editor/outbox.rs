//! One-shot messages from an editor to the UI.

use drift_core::{Constraint, ConstraintId, DVec2, EntityId};

/// What an edit or a tool left for the UI to act on once: the UI takes each when it handles
/// it; a client without a UI reads `notice` and ignores the rest.
#[derive(Default)]
pub struct Outbox {
    /// A message for the UI explaining a refused edit, or something that went wrong: shown as
    /// an error.
    pub notice: Option<String>,
    /// A message for the UI that only says what a done edit did (relations it took off, what
    /// went with it): shown plainly, not as an error (see [`Outbox::inform`]).
    pub info: Option<String>,
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
    /// A sheet note just placed by a tool (#432), whose text the UI should let the user type.
    pub pending_sheet_note_edit: Option<drift_core::SheetAnnotationId>,
    /// A sheet annotation just made by a tool, for the UI to pick.
    pub new_sheet_annotation: Option<drift_core::SheetAnnotationId>,
    /// A view just drawn with the View tool, for the UI to pick.
    pub new_view: Option<drift_core::ViewId>,
    /// The Base point tool has done its one pick (the app goes back to the select tool).
    pub base_point_done: bool,
    /// What edits did to fills beyond following their outlines, until taken (by the
    /// assistant's results; the app clears it).
    pub fill_changes: Vec<crate::fills::FillChange>,
    /// A click or box of the select tool that picked nothing while the Pick filter was
    /// narrowed, with the kinds it would have picked without the filter (the app adds the
    /// annotations, which it lays out, and says so).
    pub filtered_out: Option<FilteredOut>,
    /// What Walls from lines did last.
    pub from_lines: Option<crate::walls_from_lines::FromLinesReport>,
}

/// What a select click or box would have picked but for the Pick filter (see
/// [`Outbox::filtered_out`]).
#[derive(Clone, Debug, PartialEq)]
pub struct FilteredOut {
    /// The kinds it hit that the filter leaves out (empty when it hit no geometry, text,
    /// copy, fill or area: the app may still find annotations there).
    pub kinds: Vec<crate::editor::PickKind>,
    /// Where: the box, or for a click the point (a box of no size).
    pub rect: drift_core::Bounds,
    /// A box (dragged right to left: crossing), not a click.
    pub boxed: bool,
    pub crossing: bool,
}

impl Outbox {
    /// Say what a done edit did, plainly (`notice` stays for refusals and failures).
    pub fn inform(&mut self, text: impl Into<String>) {
        self.info = Some(text.into());
    }

    /// Whether a message is waiting already, of either kind.
    pub fn has_message(&self) -> bool {
        self.notice.is_some() || self.info.is_some()
    }
}
