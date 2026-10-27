//! Tools for AI assistants: create, inspect, constrain, edit, render and analyse a drawing.
//!
//! A [`Session`] holds one drawing in an [`Editor`] and runs tools on it by name with JSON
//! arguments ([`Session::call`]); [`tools`] lists them with their JSON schemas. Nothing here
//! knows about a transport: the `drift-mcp` server speaks MCP over stdio with it, and an
//! in-app assistant can call the same tools.
//!
//! The tools follow how Drift is meant to be used: place approximate geometry, then state
//! intent with relations and dimensions and let the solver make it exact. Every tool that
//! changes the drawing is one undo step, and all or nothing: if any part of it fails (a
//! relation that conflicts, say), the drawing is left as it was.

mod analyze;

#[cfg(not(target_arch = "wasm32"))]
pub use crate::app::document::saved_new_rules;
mod args;
mod constrain;
mod edit;
mod files;
mod geometry;
mod inspect;
#[cfg(not(target_arch = "wasm32"))]
pub mod link;
mod raster;
mod render;
mod schema;
mod script;

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::editor::Editor;
use crate::fills::PatternLibrary;

pub use schema::{INSTRUCTIONS, ToolSpec, tools};

/// What a tool returns: a JSON result (or error), and a picture for `render`.
#[derive(Clone, Debug)]
pub struct ToolOutput {
    pub json: Value,
    /// A PNG image.
    pub image: Option<Vec<u8>>,
    pub is_error: bool,
}

type Res = Result<Value, String>;

/// One drawing being worked on by an assistant.
pub struct Session {
    pub editor: Editor,
    /// Where the drawing was opened from or last saved to.
    path: Option<PathBuf>,
    meta: drift_io::Metadata,
    /// Files may only be read and written inside this folder (when set).
    root: Option<PathBuf>,
    patterns: PatternLibrary,
    /// A CPU-only egui context, for laying out dimension and note text.
    ctx: Option<eframe::egui::Context>,
    /// Working on the drawing open in Drift (the live link), rather than on its own.
    live: Option<Live>,
    /// The rules a new drawing (and an older file) is given, as Drift gives them.
    new_rules: drift_core::DrawingRules,
}

/// A drawing open in Drift, lent to the tools for one call through the live link.
pub struct LiveDoc<'a> {
    pub editor: &'a mut Editor,
    pub path: Option<PathBuf>,
    pub meta: drift_io::Metadata,
    /// It has changes that aren't saved (the assistant mustn't replace it then).
    pub dirty: bool,
    /// The app's own context, whose fonts lay out annotations as on screen.
    pub ctx: eframe::egui::Context,
    /// The rules Drift starts new drawings with.
    pub new_rules: drift_core::DrawingRules,
}

/// What Drift must do after a live call: the tools replaced or saved its drawing.
#[derive(Clone, Debug)]
pub enum LiveChange {
    /// A new or opened drawing is now in the editor.
    Replaced {
        path: Option<PathBuf>,
        meta: drift_io::Metadata,
    },
    /// The drawing was written to `path`.
    Saved {
        path: PathBuf,
        meta: drift_io::Metadata,
    },
}

struct Live {
    dirty: bool,
    change: Option<LiveChange>,
}

/// Run tool `name` on the drawing open in Drift, as one undo step in its own history.
/// Files are kept inside `root` when it is given. Returns the result and anything Drift
/// must do about its file (a new, opened or saved drawing).
pub fn call_live(
    doc: LiveDoc<'_>,
    name: &str,
    args: &Value,
    root: Option<PathBuf>,
) -> (ToolOutput, Option<LiveChange>) {
    let mut s = Session {
        editor: std::mem::take(doc.editor),
        path: doc.path,
        meta: doc.meta,
        root: None,
        patterns: PatternLibrary::default(),
        ctx: Some(doc.ctx),
        live: Some(Live {
            dirty: doc.dirty,
            change: None,
        }),
        new_rules: doc.new_rules,
    };
    if let Some(root) = root {
        s = s.with_root(root);
    }
    let out = s.call(name, args);
    *doc.editor = s.editor;
    (out, s.live.and_then(|l| l.change))
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// A session on a new, empty drawing.
    pub fn new() -> Self {
        Self {
            editor: new_editor(drift_core::Document::new()),
            path: None,
            meta: drift_io::Metadata::default(),
            root: None,
            patterns: PatternLibrary::default(),
            ctx: None,
            live: None,
            new_rules: drift_core::DrawingRules::default(),
        }
    }

    /// Start new drawings (and give older files) these rules, as Drift does.
    pub fn with_new_rules(mut self, rules: drift_core::DrawingRules) -> Self {
        self.new_rules = rules;
        self
    }

    /// Put a new or opened drawing in place of the current one.
    fn replace(&mut self, doc: drift_core::Document) {
        let mut editor = Editor::new(doc);
        if self.live.is_some() {
            // Drift's own preferences carry over to its next drawing.
            editor.keep_prefs_from(&self.editor);
        } else {
            editor.pick_any_area = true;
        }
        self.editor = editor;
    }

    /// Nothing has been done here yet: an empty drawing that came from no file.
    pub fn is_untouched(&self) -> bool {
        self.path.is_none() && self.editor.doc.is_empty() && self.editor.history.recorded() == 0
    }

    /// Only let tools read and write files inside `root`.
    pub fn with_root(mut self, root: PathBuf) -> Self {
        self.root = Some(root.canonicalize().unwrap_or(root));
        self
    }

    /// Run tool `name` with `args` (a JSON object).
    pub fn call(&mut self, name: &str, args: &Value) -> ToolOutput {
        let mut image = None;
        let empty = json!({});
        let args = if args.is_null() { &empty } else { args };
        if name == "run_script" {
            return script::call(self, args);
        }
        match self.dispatch(name, args, &mut image) {
            Ok(json) => ToolOutput {
                json,
                image,
                is_error: false,
            },
            Err(e) => ToolOutput {
                json: json!({ "error": e }),
                image: None,
                is_error: true,
            },
        }
    }

    fn dispatch(&mut self, name: &str, a: &Value, image: &mut Option<Vec<u8>>) -> Res {
        if !a.is_object() {
            return Err("arguments must be a JSON object".into());
        }
        let step = |label: &str| format!("Assistant: {label}");
        match name {
            // Files and the session.
            "new_drawing" => files::new_drawing(self, a),
            "open_drawing" => files::open(self, a),
            "save_drawing" => files::save(self, a),
            "drawing_info" => inspect::info(self),
            "undo" => self.undo_redo(a, true),
            "redo" => self.undo_redo(a, false),
            // Looking.
            "list_entities" => inspect::list_entities(self, a),
            "list_constraints" => inspect::list_constraints(self, a),
            "measure" => inspect::measure(self, a),
            "render" => render::render(self, a, image),
            "analyze" => analyze::analyze(self),
            "export" => files::export(self, a),
            // Changes, each one undo step.
            "add_geometry" => self.step(&step("add geometry"), |s| geometry::add(s, a)),
            "add_relations" => self.step(&step("add relations"), |s| constrain::relations(s, a)),
            "add_dimensions" => self.step(&step("add dimensions"), |s| constrain::dimensions(s, a)),
            "set_dimension" => {
                self.step(&step("set dimension"), |s| constrain::set_dimension(s, a))
            }
            "remove_constraints" => {
                self.step(&step("remove constraints"), |s| constrain::remove(s, a))
            }
            "set_variables" => self.step(&step("set variables"), |s| constrain::variables(s, a)),
            "move_point" => self.step(&step("move point"), |s| edit::move_point(s, a)),
            "transform" => self.step(&step("transform"), |s| edit::transform(s, a)),
            "offset" => self.step(&step("offset"), |s| edit::offset(s, a)),
            "trim_extend_split" => self.step(&step("trim"), |s| edit::trim(s, a)),
            "fillet_chamfer" => self.step(&step("fillet"), |s| edit::fillet(s, a)),
            "delete" => self.step(&step("delete"), |s| edit::delete(s, a)),
            "set_construction" => {
                self.step(&step("construction"), |s| edit::set_construction(s, a))
            }
            "layers" => self.step(&step("layers"), |s| edit::layers(s, a)),
            "add_fill" => self.step(&step("fill"), |s| edit::add_fill(s, a)),
            "add_text" => self.step(&step("text"), |s| edit::add_text(s, a)),
            "import_file" => self.step(&step("import"), |s| files::import(s, a)),
            "batch" => self.step(&step("batch"), |s| s.batch(a, image)),
            "run_script" => Err("`run_script` can't be used inside a batch".into()),
            _ => Err(format!("unknown tool `{name}`")),
        }
    }

    /// Run `f` as one undo step labelled `label`: the steps it records are folded into one,
    /// or, if it fails, undone and forgotten. The result gains the sketch's status.
    fn step(&mut self, label: &str, f: impl FnOnce(&mut Self) -> Res) -> Res {
        let before = self.editor.history.recorded();
        self.editor.fill_changes.clear();
        let result = f(self);
        if self.editor.doc.in_transaction() {
            self.editor.doc.rollback();
        }
        let n = (self.editor.history.recorded() - before) as usize;
        match result {
            Ok(mut v) => {
                self.editor.history.squash_last(n, label);
                self.editor.sync();
                let fills = self.fill_changes();
                if let Some(o) = v.as_object_mut() {
                    if !fills.is_empty() {
                        o.insert("fills".into(), Value::Array(fills));
                    }
                    o.insert("sketch".into(), self.status());
                }
                Ok(v)
            }
            Err(e) => {
                let ed = &mut self.editor;
                ed.history.discard_last(n, &mut ed.doc);
                ed.notice = None;
                ed.fill_changes.clear();
                ed.sync();
                Err(format!("{e} (nothing was changed)"))
            }
        }
    }

    /// What the step's edits did to fills beyond following their outlines (taken).
    fn fill_changes(&mut self) -> Vec<Value> {
        use crate::fills::FillChange;
        std::mem::take(&mut self.editor.fill_changes)
            .into_iter()
            .map(|c| match c {
                FillChange::Divided { id, parts } => json!({
                    "id": id.to_string(),
                    "change": "divided",
                    "areas": parts,
                    "note": "an edit divided its area; it was kept whole and flagged",
                }),
                FillChange::Refound { id, before, after } => json!({
                    "id": id.to_string(),
                    "change": "refound",
                    "area_before_mm2": before.map(args::num),
                    "area_mm2": args::num(after),
                    "note": "it now fills the area around its seed",
                }),
                FillChange::Dropped { id } => json!({
                    "id": id.to_string(),
                    "change": "removed",
                    "note": "curves of its outline were deleted and no area is left around it",
                }),
                FillChange::Merged { id, into } => json!({
                    "id": id.to_string(),
                    "change": "removed",
                    "note": format!("it came to cover the same area as {into}, which kept it"),
                }),
                FillChange::Opened { id } => json!({
                    "id": id.to_string(),
                    "change": "kept",
                    "note": "its shape's outline was cut open and bounds no area: it keeps its last shape until the outline closes",
                }),
            })
            .collect()
    }

    /// Several tools in order, as one undo step.
    fn batch(&mut self, a: &Value, image: &mut Option<Vec<u8>>) -> Res {
        let calls = args::array(a, "calls")?;
        let mut results = Vec::with_capacity(calls.len());
        for (i, call) in calls.iter().enumerate() {
            let tool = call
                .get("tool")
                .and_then(Value::as_str)
                .ok_or(format!("calls[{i}] needs a `tool`"))?;
            let empty = json!({});
            let args = call.get("args").unwrap_or(&empty);
            match self.run_call(tool, args, image) {
                Ok(v) => results.push(json!({ "tool": tool, "result": v })),
                Err(e) => return Err(format!("calls[{i}] ({tool}): {e}")),
            }
        }
        Ok(json!({ "results": results }))
    }

    /// One call within a batch or a script (which can't nest, or start another drawing).
    fn run_call(&mut self, tool: &str, args: &Value, image: &mut Option<Vec<u8>>) -> Res {
        if matches!(
            tool,
            "batch" | "run_script" | "undo" | "redo" | "new_drawing" | "open_drawing"
        ) {
            return Err(format!("`{tool}` can't be used inside a batch or a script"));
        }
        let mut v = self.dispatch(tool, args, image)?;
        if let Some(o) = v.as_object_mut() {
            o.remove("sketch");
        }
        Ok(v)
    }

    fn undo_redo(&mut self, a: &Value, undo: bool) -> Res {
        let steps = args::opt_f64(a, "steps")?.unwrap_or(1.0).max(1.0) as usize;
        let mut done = Vec::new();
        for _ in 0..steps {
            let ed = &mut self.editor;
            let label = if undo {
                ed.history.undo_label()
            } else {
                ed.history.redo_label()
            }
            .map(str::to_owned);
            let ok = if undo { ed.undo() } else { ed.redo() };
            if !ok {
                break;
            }
            done.extend(label);
        }
        self.editor.sync();
        let key = if undo { "undone" } else { "redone" };
        Ok(json!({ key: done, "sketch": self.status() }))
    }

    /// A short summary of the sketch's state, added to every change's result.
    fn status(&self) -> Value {
        let a = self.editor.analysis();
        json!({
            "dof": a.dof,
            "fully_defined": a.dof == Some(0) && a.conflicts.is_empty(),
            "conflicts": a.conflicts.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
            "undo_steps": self.editor.history.undo_depth(),
        })
    }

    /// A painter for laying out annotation text, on a context that never draws.
    fn painter(&mut self) -> eframe::egui::Painter {
        use eframe::egui;
        let ctx = self.ctx.get_or_insert_with(|| {
            let ctx = egui::Context::default();
            crate::theme::install_fonts(&ctx);
            // A first frame loads the fonts. Nothing is ever drawn, so the font atlas it
            // makes isn't uploaded anywhere.
            let mut frame = ctx.run_ui(egui::RawInput::default(), |_| {});
            frame.textures_delta.clear();
            ctx
        });
        egui::Painter::new(
            ctx.clone(),
            egui::LayerId::background(),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1000.0)),
        )
    }

    /// The drawing's name, for titles: its file's stem, or "Drawing".
    fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or("Drawing".into(), |s| s.to_string_lossy().into_owned())
    }
}

/// An editor set up as the assistant works: no automatic relations from drawing tools (the
/// tools add their own), and areas picked whether filled or not.
fn new_editor(doc: drift_core::Document) -> Editor {
    let mut ed = Editor::new(doc);
    ed.pick_any_area = true;
    ed
}

#[cfg(test)]
mod tests;
