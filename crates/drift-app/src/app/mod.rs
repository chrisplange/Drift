//! `DriftApp` behaviour, split by area.

mod blocks_ui;
pub(crate) mod checker_ui;
mod context_ui;
pub(crate) mod dialogs;
pub(crate) mod dock_ui;
mod document;
mod drafting_ui;
mod edit;
mod fill_ui;
mod groups_ui;
pub(crate) mod hatch_ui;
mod keyboard;
pub(crate) mod note_ui;
pub(crate) mod palette;
pub(crate) mod panels;
pub(crate) mod patterns;
pub(crate) mod pie;
pub(crate) mod sheets_ui;
pub(crate) mod start_ui;
pub(crate) mod temp_dims_ui;
pub(crate) mod text_ui;
pub(crate) mod underlays_ui;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod update_ui;
pub(crate) mod view;
pub(crate) mod view_align;
pub(crate) mod visibility_ui;
