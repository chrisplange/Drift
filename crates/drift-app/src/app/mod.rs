//! `DriftApp` behaviour, split by area.

mod blocks_ui;
pub(crate) mod checker_ui;
mod context_ui;
pub(crate) mod crosshair;
#[cfg(feature = "demo")]
pub(crate) mod demo_ui;
pub(crate) mod dialogs;
pub(crate) mod dim_align;
pub(crate) mod dock_ui;
mod document;
mod drafting_ui;
mod edit;
mod fill_ui;
mod groups_ui;
pub(crate) mod hatch_ui;
pub(crate) mod import_ui;
mod keyboard;
#[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
pub(crate) mod link_ui;
pub(crate) mod mark_views;
pub(crate) mod note_ui;
pub(crate) mod palette;
pub(crate) mod panels;
pub(crate) mod patterns;
pub(crate) mod pie;
pub(crate) mod rename_ui;
pub(crate) mod sheet_editor;
pub(crate) mod sheets_ui;
pub(crate) mod start_ui;
pub(crate) mod temp_dims_ui;
pub(crate) mod text_ui;
pub(crate) mod underlays_ui;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod update_ui;
pub(crate) mod view;
pub(crate) mod view_align;
pub(crate) mod views_ui;
pub(crate) mod visibility_ui;
