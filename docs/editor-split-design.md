# R3: the Editor split — design

*2026-10-28, against `main` at 29df65c (R1 merged; format 43). Written by the Fable review
session for the Opus sessions that build it, one slice per PR. Companion to
[open-drawings-decision.md](open-drawings-decision.md) (tabs step 2, which this does) and
issue #186. Updated 2026-10-30 as it landed with S1 (main at 347fee6, format 44): the rulings
on the measure-first survey (§8) and what S1 decided (marked "S1:") are folded in, and what
S2 decided (marked "S2:"). Offsets as
objects (#247) had not merged when S1 landed, so there is no `offsets.rs` yet; its `impl
Editor` module goes into drift-edit when it lands.*

R3 changes nothing a user can see. The smoke suite (`cargo test -p drift-app smoke_`), the
format fixtures, the agent's render snapshots and the export tests are the acceptance test of
every slice.

---

## 0. Decisions at a glance

| Question | Decision |
|---|---|
| New crate | `drift-edit`: everything that edits a `Document` without a window — `Editor` and the modules that hang off it, the tools, the assistant (tools, script, link). Depends on core/solver/geom/draft/io/platform; from S5 on `egui` too (text layout only; §2.3). Never on eframe, drift-render, wgpu or winit. S1 landed it with no egui at all. |
| `Editor` | **Stays one type.** It is already toolkit-free; it is "one editing session on one document". Two groups of fields leave it: the app-wide tool preferences (`EditPrefs`) and the one-shot messages to the UI (`Outbox`). No document/session split. S2 landed them as `editor.prefs` and `editor.out` (`drift_edit::editor::{EditPrefs, Outbox}`); the open drawing's `editor.prefs` is the app's copy until tabs (§3). |
| Undo | One `History` per `Editor`, i.e. per drawing, as now. Views have none. Tabs change nothing here. |
| Script / MCP | `Session` moves to `drift-edit` unchanged in shape: it owns an `Editor` (the server's own drawing) or borrows the app's (`LiveDoc`). `drift-mcp` depends on `drift-edit` only. |
| Generated geometry | Record in `drift-core`, generator in `drift-geom`, `follow_*` hook in `Editor::commit`, ownership through `Owners`. Walls do this today; offsets (#247) do the same. #247 had not landed by S1, so its `impl Editor` hooks are written in drift-edit. |
| `DriftApp` | `Drawing { editor, file, caches, edits-in-progress, view }` and `CanvasView { camera, pointer, sheet shown, view drags }`; the app keeps the tool, the GPU painter, preferences, layout, files, sessions, dialogs. `self.editor` becomes `self.drawing.editor` by a rename, **not** `Deref`. S3 landed them as private structs in `lib.rs`: 53 fields into `Drawing`, 18 into `CanvasView` (one, `drawing.view`, until drift-releases#16), 80 left on the app; `Drawing::new(doc, file, &prefs)` replaces `forget_drawing_state` (§7). |
| Slices | S1 crate move with the tools (S4 folded in) → S2 `Editor` fields → S3 `Drawing`/`View` → S5 assistant. S1–S3 are what tabs need; S5 is what a GUI-free MCP needs. 4 PRs, sized in §7 by files moved, `pub` items added and knots. |
| Tool keys | No `ToolKeys` table: no tool reads its key. The key helpers (`shortcut`, `cycle_hint`) live in drift-app's command registry as `commands::{tool_char, cycle_hint}(kind)`; `ToolKind::cycle_head` stays with the tools (S1, §2.2). |
| Test-only helpers other crates call | A `test-support` feature on drift-edit un-gates exactly those helpers (`#[cfg(any(test, feature = "test-support"))]`); drift-app turns it on for its tests. Test conveniences don't become API (§8, ruling 4). |
| Not in R3 | Splitting `Document` (4.9k lines); `#187`; `#183`; tabs themselves (step 3); a toolkit-free text layout (§2.3, option B). |

---

## 1. What exists today (the finding that shapes the plan)

Issue #186 was written from line counts. Reading the code gives a better picture: **the
layering #186 asks for is already in the code; it is not in the crate graph.**

- `Editor` (`editor/mod.rs`, 398 lines, ~95 fields) and every `impl Editor` module — `modify.rs`
  3.9k, `fills.rs` 3.0k, `walls.rs` 1.6k, `blocks.rs`, `openings.rs`, `transform.rs`,
  `clipboard.rs`, `texts.rs`, `curves.rs`, `stack.rs`, `underlays.rs`, `checker.rs`,
  `temp_dims.rs`, `walls_from_lines.rs`, `frame_items.rs` — contain **no egui at all**
  (grep `egui::|Ui\b|Painter`: 0 in each). So do `tools/*` (one `egui` hit, in a test) and 19
  of the 21 files under `agent/`.
- History is `drift_core::History` (`document/journal.rs`): `Document::begin/commit/rollback`
  record a `Transaction`; `Editor::commit` (`editor/solve.rs:111`) solves, runs the follow
  hooks and pushes it. Already per document, already toolkit-free.
- What ties these to the GUI crate is thin and countable:
  1. **Text layout.** Dimension and note labels are measured with `egui::Galley` through an
     `egui::Painter` (`annotations.rs`, `notes.rs`, `output.rs`). The assistant needs it for
     `render` and the exports (`Session::painter()`, `agent/mod.rs:426`, makes a CPU-only
     `egui::Context` for exactly this). Painting is already abstracted (`annotations::Pen`,
     implemented by `egui::Painter` for the screen and by `Recorder` for PDF/SVG/DXF); only
     *measuring* still needs egui.
  2. **Eight pure helpers** in UI files that edit code calls: `annotations::{dimension_anchor,
     centred_offset, foot, corner_order}` (walls, openings, temp_dims, tools/dimension,
     agent/constrain) and `notes::{label_world, with_label_at, anchor_at, next_mark_label}`
     (picks, clipboard, tools/annotate). All take a `&Document`, none touches egui.
  3. `picks.rs`: `dimension_in_box`/`note_in_box` test the app's laid-out labels
     (`DimLayout`, `NoteLayout`, made with egui) against a selection box (21 egui hits).
     `BoxPick` itself is already in world `Bounds`.
  4. `tools/mod.rs` → `commands::tool_key` (the registry, a UI table).
  5. `agent/mod.rs` → `app::document::saved_new_rules` (reads the preferences file through
     `eframe::storage_dir`) and `theme::install_fonts`.
  6. `frame_items.rs` tests → `perf_tests::torture_sketch`.
  7. (Found by S1's trial move.) Two `impl Editor` blocks in UI files: the drawing's fonts
     (`fonts.rs`: `ensure_font`, `embed_font`, …) and layer edits (`layers_ui.rs`:
     `add_layer`, `set_layers_visible`, …, with `LayerFlag`); and two plain-data canvas types
     from drift-render that edit code builds (`FillMesh` in `fills.rs`, `CanvasImage` in
     `underlays.rs`).
- `DriftApp` (`lib.rs:249`, ~150 fields) mixes the drawing's state with the app's;
  `forget_drawing_state` (`app/document.rs:188`) is already the inventory of the per-drawing
  part. `self.editor` is used ~1,090 times across 30 files (`app/view.rs` 116,
  `app/sheets_ui.rs` 100).
- Visibility: about 96 `pub(crate)` items in the files that move (28 in `editor/mod.rs`, 19 in
  `fills.rs`, 14 in `editor/regions.rs`, 11 in `walls.rs`); the UI calls 164 distinct `Editor`
  methods.
- wasm: the areas thread in `editor/regions.rs` is already `cfg(not(wasm32))` with a
  synchronous path for wasm (lines 280–371). The `demo` feature leaves out `agent` and the
  file writers.
- The Blender add-on (`integrations/blender`) is Python only; it needs core/solver/geom free of
  UI dependencies, which R3 doesn't touch.

So R3 is mostly **moving files across a crate line and renaming a field**, plus one small
abstraction (text measurement) that can be taken in a cheap form. The risk is in volume and
visibility, not in design.

---

## 2. The crate boundary

### 2.1 `drift-edit`

```
drift-core ─┬─ drift-solver ─┬─ drift-geom ─┬─ drift-draft ─┬─ drift-io
            │                │              │               │
            └────────────────┴──────────────┴───────────────┴── drift-edit ── drift-app ── apps/native, apps/web
                                                                     │
                                                                     └── apps/mcp
```

`drift-edit` = "edit a `Document` without a window". It holds:

| Module | From | Notes |
|---|---|---|
| `editor/` (all) | `editor/` | `Editor`, drag, solve, selection, regions, items, options, variables, pointer, filter |
| `modify`, `fills`, `walls`, `walls_from_lines`, `openings`, `offsets` (#247), `blocks`, `transform`, `clipboard`, `texts`, `curves`, `stack`, `underlays`, `checker`, `temp_dims`, `frame_items` | same names | file moves |
| `library/` | same | sizes JSON, categories (data) |
| `picks` | `picks.rs` | the Editor side: `BoxPick` (world `Bounds`), `Riders`, the picked notes and riders. `dimension_in_box`, `note_in_box` and `Piece` stay in drift-app's `picks.rs` and rejoin in S5 |
| `labels` (new, small) | `annotations.rs`, `notes.rs` | the eight pure helpers of §1.2, with the six private helpers they call (`center_of`, `line_points`, `angle_vertex`, `ray_from`, `centre`, `line_ends`), which the layout code also calls until S5; `annotations.rs`/`notes.rs` keep layout and painting |
| `fonts` | `fonts.rs` (its `impl Editor`) | the drawing's fonts: keeping them in it, its own font, making a font known for outlining. drift-app's `fonts.rs` keeps the screen part. `theme::font_definitions` joins it in S5 (it returns an egui type, and nothing in drift-edit needs it before then) |
| `editor/layers.rs` | `layers_ui.rs` (its `impl Editor`) | layer edits and `LayerFlag`, after the move-to-layer code already there |
| `prefs` (new, small) | `app/document.rs`, `lib.rs` | `saved_new_rules`, `rules_from_prefs`, `rules_pref` and `KEY_VIEW`: reading the preferences file, with the data dir from `drift_platform::storage_dir` (the folder `eframe::storage_dir` gives, checked by a test on every CI system) |
| `test_sketches` (test-only) | `perf_tests.rs`, `perf_big.rs` | the 10,000-curve torture sketch and the big made-up CAD drawing, which tests in both crates use; behind `test-support` |
| `tools/` | `tools/` | S1 (S4 folded in); see §2.2 |
| `agent/` (tools, script, link, render, raster, files) | `agent/` | S5; see §2.3 |
| tests: `drag_props`, `areas_props`, `fill_tests`, `walls_props`, `openings_props`, `perf_tests`, `perf_big`, `editor/tests`, `tools/tests` and the four `tools/*_tests`; in S5 `walls_gallery`, `walls_from_lines_gallery`, `agent/tests`, `agent/script/tests` | same | move with their subjects. `ui_tests/` (egui_kittest) and `mem_tests` stay. `perf_tests` and `perf_big` split: what drives `DriftApp` or builds GPU frames stays (`perf_app_frames`, `perf_zoom_frame_cost`, `perf_editor_pipeline_frames`; `perf_big`, `perf_big_drag`, `perf_big_save`). The two galleries render through the assistant and stay until S5, as do the tests in drift-app's `edit_session_tests.rs` (scripts and import/export through `Session`, `output::model_drawing`) |

Dependencies: `drift-core`, `drift-solver`, `drift-geom`, `drift-draft`, `drift-io`,
`drift-platform` (for `crash::catch`, `FilePath`, the system fonts and the data dir only — it
pulls `rfd`, which `drift-mcp` already links today; acceptable), `glam`, `log`, `serde`,
`serde_json`; with S5 `egui` (not `eframe`) and `tiny-skia`. Features: `dwg` and `demo` pass
through from `drift-app` (`demo` leaves out `agent`, as now), and `test-support` (§0).
S1: `FillMesh` and `CanvasImage` moved from drift-render to `drift_core::shape` beside
`SketchItem` (plain data), so drift-edit needs no drift-render.

`drift-app` keeps: `lib.rs` (`DriftApp`), `app/*`, `canvas.rs`, `commands.rs`, `dock.rs`,
`theme.rs`, `fonts.rs` (screen fonts), `icons.rs`, `layers_ui.rs`, `overlay.rs`, `parameters.rs`,
`picks.rs` (the screen half), `annotations.rs`, `notes.rs`, `output.rs` and `agent/` (until
S5), `update.rs`, `soak.rs`, `links.rs`, `ui_tests/`.

### 2.2 Tools

`tools/` is toolkit-free (`Input`, `Pointer`, `Modifiers`, `Overlay` are plain data; the `Tool`
trait takes `&mut Editor`). It belongs in `drift-edit`: it is the mapping from gestures to
edits, it has a 5.9k-line headless test file, and keeping it in `drift-app` makes every tool
test build eframe. The one knot is `tools/mod.rs` → `commands::tool_key(kind)` (the key a
tool's hint names). Invert it: `drift-edit` defines `ToolKeys` (a small table `ToolKind →
Option<Shortcut>`), the tool reads it, and `drift-app`'s registry fills it (the registry stays
the one source; its duplicate-key test keeps working). `Shortcut` moves with `ToolKeys` if it
is plain data; if it is an egui type, the tool hint takes a `&str`.

S1: no table was needed, because no tool reads its key. The only users were
`ToolKind::cycle_hint` (through `shortcut()`), called only by the registry's shortcuts text and
the tool rail, and `ToolKind::shortcut_label` (with the macOS key text), which had no caller
and was deleted. `shortcut` and `cycle_hint` became `commands::{tool_char, cycle_hint}(kind)`
in drift-app; `cycle_head` stays on `ToolKind`. (`Shortcut` holds an `egui::Key`.)

### 2.3 Text layout — the one real decision

The assistant's `render` and the exports lay out dimension labels through egui. Two ways:

- **A (recommended for R3): `drift-edit` depends on `egui` for text layout.** `egui` on its
  own is pure Rust (epaint, text, no window, no GPU, builds for wasm). `eframe::egui::` in the
  moved files becomes `egui::`; `Session::painter()` stays as it is; `annotations.rs`,
  `notes.rs` and `output.rs` move into `drift-edit` unchanged. `drift-mcp` drops eframe,
  winit and wgpu. A small part of S5. The crate is headless, not toolkit-free; that is what
  #186 and the MCP server actually need.
- **B (later, if ever): a `TextMetrics` trait** (`measure(text, font) → width/height`) with
  an egui implementation in `drift-app` and a `drift-draft` implementation (`font::Face::width`
  over `epaint_default_fonts`, the same files egui uses) for the server; `DimLayout` in
  `DVec2`/`Bounds`; the `Pen` trait takes a laid-out label instead of a `Galley`. A slice of
  its own, and the metrics must be proved identical (render snapshots, export byte-compare). Nothing
  planned needs it: do it only if a build target can't take egui.

Decision: **A.** S5 is written for A; the doc records B so nobody re-derives it.

---

## 3. `Editor`: what splits and what doesn't

**Keep `Editor` as one type.** #186 proposes "document + history" versus "session/UI state".
Every edit path touches both halves in one call (`commit` solves, follows walls and fills,
pushes history, syncs caches, prunes the selection), every headless client needs the
"session" half too (the assistant sets `selection` after a live call, uses `pick_any_area`,
reads `notice`), and the drag/regions/areas machinery is keyed to the history depth. Two
structs would mean two parameters on ~160 methods for no caller that wants only one.

What does leave `Editor` (S2) are the fields that aren't *about this drawing*:

1. **`EditPrefs`** — the set `keep_prefs_from` copied (15 fields) plus the
   same-natured fields: `auto_relations`, `always_endpoints`, `pick_any_area`, `pick_filter`,
   `dim_mode`, `dim_overall`, `draw_construction`, `fill_new_shapes`, `fill_alone_picks`,
   `fill_style`, `polygon_sides`, `polygon_circumscribed`, `slot_mode`, `text_height`,
   `wall_thickness`, `wall_thickness_expr`, `wall_justify`, `wall_arc`, `wall_poche`,
   `wall_lines`, `opening`. Grouped as `editor.prefs: EditPrefs` (a `Clone` struct the app
   owns one of and copies into each drawing's editor; with tabs, one per app, written back
   from the front drawing when it changes). `keep_prefs_from` disappears. Call sites are a
   rename (`editor.fill_style` → `editor.prefs.fill_style`), sed-able.
2. **`Outbox`** — one-shot messages to the UI: `notice`, `far_off`, `pending_edit`,
   `pending_driven`, `pending_note_edit`, `new_view`, `box_pick`, `base_point_done`,
   `fill_changes`, `from_lines`. Grouped as `editor.out: Outbox`. This makes the contract
   visible: the UI drains it; a headless client reads `notice` and ignores the rest.

S2: both structs live in `editor/edit_prefs.rs` and `editor/outbox.rs`, re-exported from
`drift_edit::editor` only; fields `pub`, no `Deref`, no helpers (the UI drains `out` with
`std::mem::take` where it did before). With one drawing the app has no `EditPrefs` field of
its own: the open drawing's `editor.prefs` is the app's copy, and `replace_document` clones it
into the next drawing's editor (the assistant's live open does the same; the server's own
drawings start from the defaults with `pick_any_area`). S3's `Drawing::new(doc, file, &prefs)`
takes that copy; the app field appears with tabs (step 3), written back from the front drawing.
Copying the whole struct widens what carries over by six fields: `auto_relations` and
`always_endpoints` (no visible change: the app pushes its own fields of those names into
`editor.prefs` every frame) and the Polygon, Slot and Text bars' settings (`polygon_sides`,
`polygon_circumscribed`, `slot_mode`, `text_height`), which now survive New and Open as every
other tool bar's do; `text_height` carries as a fixed size in mm, as `wall_thickness` does.
`EditPrefs::default()` is derived (`auto_relations` false, as `Editor::default()` gave) and
`Editor::new` sets `auto_relations`. S3 left the default as it is: `DriftApp::new` states the
app's starting prefs (`EditPrefs { auto_relations: true, ..Default::default() }`, the literal
`Editor::new` uses) and hands them to the first `Drawing::new`; the tabs step turns that
literal into the app's own `EditPrefs` field.

Everything else stays where it is and is per drawing: `doc`, `history`, selection and hover
(every kind), the revision-keyed caches (`index`, analysis, evaluation, regions, areas,
fill shapes, stack, instance bounds, text, font, handles, wall owners, lowering), the drag
session, the modes a tool sets for this drawing (`drawing_curves`, `show_centrelines`,
`block_edit`, `open_group`, `place_block`, `editing_text`, `repick_fill`, …).

Hover and selection live in `Editor`, not in the view: a second view on the same drawing
(drift-releases#16) should show the same selection, and the pick happens through the view's
camera but lands on the drawing. `snapped_point` and `chain_preview` are the same.

---

## 4. Undo with several drawings and views

One `History` per `Editor`, so one per drawing, exactly as now. Nothing in R3 changes it;
tabs (step 3) get it for free:

- A view holds no history (camera, fit, pan, the sheet shown: none of it is undoable today
  either).
- Switching tabs cancels the tool (`tool.cancel(&mut editor)`), which closes any open
  transaction; the areas thread's pending follow (`AreasState`, keyed by that editor's
  `undo_depth`) stays with its drawing and settles on the drawing's next call.
- The app's `undo()`/`redo()` wrappers (`app/document.rs:1135`) act on the front drawing.
- Across drawings there are no shared ids: paste carries a `Document` fragment through the
  system clipboard already (`clipboard.rs`), and lands as one step in the target.
- Two views of one drawing (#16) share its `Editor` and so its history; a second window on
  the same drawing is an immediate viewport inside the same `update`, so no second owner.
- Memory: `History::MAX_STEPS` is 500 per drawing, but history memory is set by big steps, not
  by the count. Measured (survey, e681d49): a parked 70.7k-curve drawing is 341 MB, 152 MB of
  it the undo history, nearly all of that the import step (the journal holds every imported
  record before and after). That is over the ~300 MB threshold of the open-drawings doc. A
  finding for the tabs step, not R3: parked tabs are budgeted; the first lever is the history
  (the import step), the second the regenerable caches (`sketch_items` 28 MB, `fill_meshes`
  5.5 MB, the Editor's revision-keyed caches); "drop caches on park" is measured before
  "unload the tab".

---

## 5. Script and MCP without the GUI

Today's shape is right and stays: `Session` (`agent/mod.rs`) owns an `Editor` for the
server's own drawing, or borrows the app's through `LiveDoc`/`call_live` for one call, one
undo step, in the app's own history. The script runner lowers statements to the same tool
calls. After S5 all of it is in `drift-edit`; `apps/mcp` depends on `drift-edit` and
`drift-platform` only.

Changes, all small:

- `Session.ctx: Option<egui::Context>` stays (option A). `LiveDoc.ctx` stays: the app lends
  its context so labels lay out with the on-screen fonts.
- `saved_new_rules` moves to `drift_edit::prefs` and reads the data dir from
  `drift-platform`, not `eframe::storage_dir` (same path; the builder checks the two agree
  on every platform, or `drift-platform` grows a `storage_dir("Drift")` that both use).
- `theme::install_fonts` → `drift_edit::fonts::install(ctx)`; `drift-app`'s theme calls it.
- With tabs, `call_live` borrows the drawing in front of the focused window. The open
  decision 4 in the open-drawings doc (follow the front tab vs. stay bound) resolves itself:
  a link call runs synchronously inside `update`, so a tab can't change mid-call, and a job
  that spans calls already re-reads ids each call. **Follow the front tab**; touch the link
  file on tab switch so `drift-mcp` reconnects to it.
- Nothing in `agent/` holds `DriftApp` state (`run_link_call`, `app/link_ui.rs:73`, is the
  only bridge; it stays in the app).

---

## 6. Generated geometry: walls now, offsets next

Walls define the protocol; R3 moves it, #247 (built before R3) follows it. The contract, by
example from walls:

| Part | Walls | Offsets (#247 must do the same) |
|---|---|---|
| Record | `drift_core::Wall` (`walls.rs`), `Made` (what it generated), `Owners`/`Owner` | `drift_core::Offset { source, side, distance, look, made }`, `Owner::Offset(OffsetId)` |
| Generator | `drift_geom::walls::regen::{regenerate, regenerate_moved}` from centrelines | `drift_geom::offsets::regenerate[_moved]` using `drift_geom::offset_curve` plus the corner-rejoin half of `modify::offset_shapes` (`modify.rs:2075–2255`), which moves to drift-geom as the generator |
| Follow hook | `Editor::follow_walls` in `commit` after the solve | `Editor::follow_offsets`, run **after** `follow_walls` (a source may be a wall face) and before `follow_paths`/`follow_fills` |
| Load | `repair_walls` in `Editor::new` | `repair_offsets`: regenerate and compare, as walls do |
| Ownership guards | `guard_owned`/`refuse_owned(ids, Op)` | the same table gains the offset rows: generated curves can't be moved, trimmed or constrained; Detach first |
| Whole-thing picking | `wall_pick_set`, `walls_for_picks` | `offset_pick_set` |
| Copy / mirror / paste | `copy_walls_onto(curves)` | `copy_offsets_onto` |
| Explode | `explode_walls` | `detach_offsets` |
| Solver | `Lowering` (face relations onto centrelines) | none: offset curves take no relations (Detach to constrain). If #247 wants dimensions *to* an offset curve, that is a lowering question — stop and ask |

Where it lives after R3: records in `drift-core`, generators in `drift-geom`, the `impl
Editor` hooks in `drift-edit` (`walls.rs`, `offsets.rs`), and only the tool, the Inspector
field and the drag in `drift-app`. A `Generated` trait isn't worth it for two kinds; the
table above is the checklist, and the S1 builder adds it to `docs/walls-design.md` §0 as a
pointer.

---

## 7. Slices

Each is one PR, mergeable on its own, green on fmt, clippy (native + wasm + demo), the demo
test, the smoke suite, the fixtures and the GPU-gated workspace run. Pure moves go in their
own commit (`git diff -M --stat` shows renames), edits in another, so the diff is reviewable.
Sizes are files moved, `pub` items added and knots, not time.

**S1 — `drift-edit` crate: `Editor`, its modules and the tools (S4 folded in).**
New crate with the workspace lints; move `editor/`, the `impl Editor` modules of §2.1,
`library/`, the Editor side of `picks`, the eight helpers as `labels`, the drawing-font and
layer methods, `prefs`, `tools/` with its five test modules, and the headless tests.
`frame_items`' tests take their sketches from `test_sketches`. `pub(crate)` → `pub` only for
what the UI calls (most of the ~164 methods were `pub` already); the rest stays `pub(crate)`
in `drift-edit`. `drift-app` re-exports nothing: it `use drift_edit::…`. The duplicates S1
can cut go (two tools tests, and ui_tests' hatch test, whose undo check folds into
`fill_tests`); the A1 ui tests become headless tests (6: one of the 7 the survey listed
clicks Inspector buttons and stays). Add `cargo test -p drift-edit` and the wasm clippy of
`drift-edit` to CI; the smoke line is unchanged. Measure after: `drift-app` incremental build
time. Knots: tool keys (§2.2), `labels`, the picks split, the two canvas types, the two `impl
Editor` blocks in UI files, the cross-crate test helpers (§8, ruling 4).

**S2 — `Editor` fields: `prefs` and `out`.** About 20 fields regrouped; call sites by sed.
§3. The app owns an `EditPrefs`; `replace_document` copies it in; `keep_prefs_from` goes.
Call sites by sed; the view-prefs string (`view_prefs`/`apply_view_prefs`) reads from
`EditPrefs`. No test changes except names.
S2: 31 fields (21 into `EditPrefs`, 10 into `Outbox`); call sites renamed where the compiler
found an access on an `Editor`, not by sed, so same-named fields of other types stayed. The
view-prefs string is byte-identical (a test reads a string saved before S2 and writes it back
unchanged). Three tests added: prefs carry over on open (app and live link), the server's own
drawings don't take them, and the saved string.

**S3 — `DriftApp` → `Drawing` + `View`.** The open-drawings doc's step 2.
`Drawing { editor, file: FileState, saving, lossy_save, fill_meshes, sketch_items,
preview_cache, layer state (names, expanded, counts, drag, renaming, isolated, solos), every
edit-in-progress field `forget_drawing_state` resets, views: Vec<View>, front: usize }`;
`View { camera, fit_target, pan_anchor, wheel, pointer, last_pointer, canvas_rect,
active_sheet, sheet_preview, preview, view_drag, view_guides, align_with, sheet_view,
sheet_pick, view_set, view_box, crop_drag }`. `DriftApp` keeps `tool`, `painter`, `files`,
`pending`, `status`, `hatch_patterns`, `new_rules`, `clipboard`, the palette/rail/dock/
layout/fonts/dialog/session/update fields, and the view switches (`show_grid`,
`show_dimensions`, `thin_lines`, …) which are preferences. `self.editor` →
`self.drawing.editor` (one `Drawing` for now, `self.drawing`; the `Vec` comes with tabs).
`forget_drawing_state` and `replace_document` become `Drawing::new(doc, file, &prefs)`.
S3 also folds `DriftApp.auto_relations` and `always_endpoints` into the app's `EditPrefs`:
today they are app fields pushed into `editor.prefs` every frame (§3, S2).
**No `Deref`** (§9). Borrow splits are the work: a method that mutates both app and drawing
fields through `&mut self` compiles as is; closures that borrow `self` whole need
`let d = &mut self.drawing;` first. The compiler lists them. The smoke tests' `view()` helper
sets `fit_target` through the view. Measure after: memory of a parked 73k-curve drawing
(`Editor` caches + `fill_meshes` + `sketch_items`) with `mem_tests`.
S3: landed as `Drawing` (53 fields + `view`) and `CanvasView` (18), private structs in
`lib.rs` beside `DriftApp`, no `pub` items added; 80 fields stay on the app. Not `View`:
`drift_core::View` (a model view placed on a sheet) is imported by that bare name in
modules that also glob `crate::*` (`mark_views.rs`, `views_ui.rs`), so a crate-root `View`
would shadow it. One `drawing.view` for now, no `Vec`, no `front` and no accessor: an
accessor borrowing the whole struct is what would create borrow splits, and `front` has one
legal value until #16; the target shape there is `views: Vec<CanvasView>, front: usize`,
reached by the same compiler-found rename. `wheel` stays on the app, against the list above:
its `mode` is the saved `wheel=` preference, and a fresh view would reset it on New and Open.
`keys_held` joins the view (the canvas's input state, read only by `make_pointer`), and
`replace_document` carries `canvas_rect` over (the docks and the start panel read it before
the canvas draws in the frame a file opens). Fields the rule left open, as Fable ruled:
`live_profiles`, `import_report` (it holds `EntityId`s; the report now closes on New and
Open, a fixed leak), `text_blink` and the whole `LayerList` (its filter now clears on New and
Open) went to `Drawing`; the file lifecycle in flight (`opened`, `opening_revision`,
`after_save`, `import_as`, `pending_import`, `dropped`, `open_path`, `import_new`, `picking`)
and the panel fields that name nothing in the document stayed. The compiler found 3,284
field-path sites to rename (1,871 in the app, 1,413 in tests; `editor` 2,235 of them) and
no borrow split. The fold: 28 sites, the two per-frame push lines gone, the `auto`/`ends`
keys unchanged; one test guards that a switch reaches the editor in the frame it is made.
The parked-drawing number is in the S3 PR.

**S4 — folded into S1** (§8, ruling 7): tools/ is toolkit-free and its two knots were small.
What S4 also planned, converting the 27 A4 ui tests (they drive a tool by canvas clicks) into
Input-driven tool tests, is a follow-up PR after S1 or part of S5; S1 is a move.

**S5 — the assistant into `drift-edit`.** §2.3 option A and §5. `annotations.rs`,
`notes.rs`, `output.rs`, `agent/` and `theme::font_definitions` (into `drift_edit::fonts`)
move; `eframe::egui` → `egui`; the picks split and the label helpers' private dependencies
rejoin; `walls_gallery`, `walls_from_lines_gallery` and `edit_session_tests` move; the 9 A5 ui
tests become headless output tests; `drift-mcp` depends on `drift-edit`. `ui_tests/link.rs`
stays in the app (it drives the window). Measure before and after: `drift-mcp` binary size and
cold build time; render snapshots and export outputs byte-identical to the baseline set (§8).

Order: S1 → S2 → S3 → S5. S1–S3 give tabs their base; if the series stops after S3, the MCP
server is no worse than today. S5 touches files S3 doesn't, so a second session could take S5
while another takes S3, if the coordinator wants (two writers, disjoint files).

Total: 4 PRs. Compare: #186 guessed "large"; S3 is smaller than the open-drawings doc's
step 2 because `forget_drawing_state` already did the inventory.

---

## 8. Risks, and what to measure first

Measure before S1 starts (read-only; numbers go in the S1 PR). Done by the measure-first
survey on e681d49 (2026-10-29), with the rulings below:

1. **`drift-mcp` today:** binary size and cold build (`cargo build -p drift-mcp --release`,
   target on E:). It isn't in the main target dir yet. This is the number S5 is judged by.
2. **A parked drawing's memory:** `mem_tests` on the 73k-curve PDF after a fit and an edit:
   the `Editor` plus `fill_meshes` and `sketch_items`. Over ~300 MB the tabs step must unload
   parked tabs (the open-drawings doc's threshold); R3 only needs the number.
3. **The cfg audit:** every `#[cfg(feature = "demo")]`, `#[cfg(target_arch = "wasm32")]` and
   `#[cfg(test)]` in the files of §2.1 (a list, so S1 carries each over knowingly).
4. **Baselines to compare against after every slice:** the perf tests' numbers (`perf_big`,
   `perf_tests`), the agent's render PNGs and the export fixtures. There were no committed
   render or export baselines; the survey captured a set on e681d49 (the agent's render
   PNGs, and DWG/DXF/PDF/SVG exports of five format fixtures; deterministic across two runs),
   kept outside the repo. It is the reference for every slice: not committed; a changed byte
   is explained in the PR.

Measured on e681d49: drift-mcp 29,888,000 bytes, 103.7 s cold release build (no wrapper);
big-PDF import 1,443 ms, first frame 225 ms, nudge next frame 136.6 + fills 301.4 ms; an
editor-file touch 7.2 s `cargo build -p drift` / 9.1 s `cargo test -p drift-app --no-run`.
Perf rule for every S PR: alternate main and branch, three rounds; flag a line more than 10 %
slower **and** outside main's spread; five rounds for lines with more than 15 % spread.

Rulings on the survey (Fable, 2026-10-29), which this document now follows:
1. Parked-tab memory is a finding for the tabs step (§4), not R3.
2. `picks.rs` splits in S1 (§2.1).
3. `labels` takes eight helpers (§1, §2.1).
4. `#[cfg(test)]` helpers that tests in another crate call get
   `#[cfg(any(test, feature = "test-support"))]`; drift-app turns `test-support` on in its
   dev-dependencies. No smoke test is edited. Test conveniences don't become plain API. S1
   gated `make_block`, `save_variant`, `text_items`, `fill_new_shape` and `test_sketches`.
5. `mem_tests` and the three `Harness` probes of `perf_big` stay in drift-app; `perf_tests`
   splits; drift-edit takes no drift-render dev-dependency (wgpu). Test names unchanged, except
   `perf_editor_pipeline_frames`, the GPU frame-build lines of `perf_editor_pipeline`.
6. The e681d49 baseline set is the reference (item 4).
7. S4 folds into S1 (§7).
8. `KEY_VIEW` moves to `drift_edit::prefs` as `pub const`; the two readers keep their
   `all(not(wasm32), not(demo))` gates.
9. No day estimates in this document.

Risks and their answers:

- **Visibility creep.** ~96 `pub(crate)` items; the temptation is `pub` everywhere. Rule: `pub`
  what `drift-app` calls (the 164 methods, listed by the grep in §1), `pub(crate)` the rest.
  The reviewer greps the diff for `pub fn` additions. Items that were already `pub` stay so:
  a move is not a cull.
- **Borrow splits in S3.** Known cost, compiler-guided; not a design risk. If a method needs
  the drawing and the tool at once (most canvas code does), it takes both as parameters or
  stays on `DriftApp`: that's fine, R3 doesn't require `impl Drawing` methods.
- **wasm and demo.** `drift-edit` must build for `wasm32` with and without `demo` (the areas
  thread is already cfg'd; `agent` is `not(demo)`). CI's two wasm clippies cover it once the
  crate is in the workspace.
- **Test discovery.** Tests that move out of `drift-app` leave the `-p drift-app` runs. CI
  and the handoff memory get a `cargo test -p drift-edit` line; `DRIFT_REQUIRE_GPU` doesn't
  apply to it.
- **`saved_new_rules` path.** `eframe::storage_dir` vs the `drift-platform` data dir must be
  the same folder on Windows, macOS and Linux, or users lose their new-drawing rules. eframe
  0.36 works it out itself (roaming AppData `\Drift\data` on Windows, `~/Library/Application
  Support/Drift` on macOS, `$XDG_DATA_HOME` or `~/.local/share` + `drift` on Linux);
  `drift_platform::storage_dir` mirrors it (reading `%APPDATA%`, as the workspace forbids
  `unsafe`), and the test `the_server_reads_settings_where_the_app_saves_them` checks that
  the two agree on each CI system.
- **Text metrics (S5).** None with option A: same crate, same fonts, same layout code.
- **Merge conflicts with #247 and the wall-bug batch.** The wall-bug batch landed before S1;
  #247 had not. It now builds on drift-edit: its `impl Editor` modules go there (§6).
- **The big files stay big.** `modify.rs` and `fills.rs` move, they don't shrink. That is
  honest: R3 is a boundary, not a rewrite. Splitting `Document` (#186's last paragraph) and
  thinning `modify.rs` are separate issues.

---

## 9. Where this disagrees with #186 and the open-drawings doc

1. **#186: "split `Editor` into document+history and session/UI state".** No (§3). The
   split that pays is at the crate line (a move) and in ownership (prefs out, outbox named),
   not inside `Editor`. The document-vs-session split would put two parameters on every edit
   path for no client that wants only one.
2. **#186: "scripting and the MCP server depend on the whole GUI crate".** True today, but
   the code under them is toolkit-free; the fix is a crate move plus `egui`-not-`eframe`, not
   a rewrite. #186 reads as if domain logic were tangled with egui; it isn't, except for text
   measuring.
3. **#186: "natural first slices are the derived data and picking".** Picking was done
   (`target_at`, #196); the derived data is already inside `Editor` keyed by revision and
   moves with it. No extraction slice is needed.
4. **Open-drawings doc, step 2: "`self.editor` keeps working through `Deref` or a
   method".** Rename, don't `Deref`. `Deref` from `DriftApp` to `Drawing` hides which drawing
   a line touches, which is the one thing tabs need to be explicit, and it would have to be
   undone at step 3. 1,090 sites is one sed and one compile.
5. **Open-drawings doc: "the tool and gestures with the view that owns the pointer".** The
   tool stays one per app (`Box<dyn Tool>`), cancelled on tab switch; its editing state is in
   the `Editor` already. A per-view tool is a #16 question, and only if two views ever take
   input at once.
6. **Open-drawings doc, decision 4 (AI link on tab switch).** Resolved: follow the front tab
   (§5); the "stay bound" option guards against a race that can't happen.
7. **#186's "when": after the outside-user test.** Void (user, 2026-10-28); the series runs
   now, in the order the coordinator recorded.

---

## 10. Rules for the sessions that build a slice

- One slice per PR, from a main that has the previous slice. Moves in one commit, edits in
  another. No behaviour change: if a smoke test needs more than a path or field rename to
  pass, stop and report.
- Widen visibility only as §8 says. Don't add `Deref`s, blanket re-exports or a `prelude`.
- A design question this doc doesn't answer (a field that fits neither `Drawing` nor `View`;
  a `pub(crate)` the UI needs that looks like it shouldn't; a cfg that can't move): stop and
  report with the file and line. Don't improvise.
- Each PR body lists: the files moved, the `pub` items added, the numbers of §8 before and
  after, and the check list run (fmt, clippy native + 2 wasm, demo test, smoke, fixtures,
  `cargo test -p drift-edit`, workspace with GPU).
- This document landed in `docs/editor-split-design.md` with S1; §0 is updated as decisions
  are taken.
