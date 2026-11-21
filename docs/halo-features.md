# Halo features

Features meant to make Drift stand out, kept apart from the usability and bug backlog.
Nothing here is scheduled. Each idea becomes a GitHub issue with a Done when before any work starts, and the freeze on new feature areas still applies.

Every idea builds on something Drift already has, so none needs a new subsystem. None of them was in `FEATURES.md` or the code on main when this list was written (2026-11-21).

**Starting point (owner, 2026-11-21): 2 and 4.** Both make the solver easier to read and work with rather than adding a feature area, so they fit the usability focus.

## 1. Suggest dimensions to finish the sketch
One command proposes the smallest clean set of dimensions and relations that would fully define the sketch. They appear as ghosts you can accept, drop one by one, or swap for another.
- **Why it stands out:** SolidWorks has "Fully Define Sketch", but its choices are messy. Drift's version would prefer baseline-style schemes that respect symmetry and could go straight onto a sheet.
- **Builds on:** `drift_solver::free_directions`, `crates/drift-app/src/agent/analyze.rs`.
- **Best demo:** a half-defined sketch goes black in one click.

## 2. "Why won't this move?" (starting point)
Click or try to drag a point that is held and Drift lights up the relations and dimensions holding it. It also names the one constraint whose removal would let it move the way you tried to drag it.
- **Why it stands out:** the red "N conflicting" pill already explains conflicts. This explains the more common case: a sketch that is fully defined, just not the way you wanted.
- **Builds on:** `drift_solver::free_directions` and the solver's conflict and redundancy reporting (`crates/drift-solver/src/lib.rs`). Shows on the canvas the same way as the conflict pill (`crates/drift-app/src/app/panels.rs`).
- **Watch:** a change inside `crates/drift-solver/**` is solver tier (`.github/tiers.txt`). If possible, keep the solver change to a read-only query and put the rest in the app.
- **Open questions for the Done when:** what starts it (a failed drag, a click with a modifier, a context-menu item); how long a chain it shows before it summarizes; whether "remove this one" is a button or only a hint.

## 3. Drag a dimension's value on the canvas
Hold a modifier and drag a dimension label, and its value scrubs the way the Parameters sliders do. A faint ghost of the starting shape stays until you let go.
- **Why it stands out:** the most direct way to show that dimensions drive the drawing.
- **Builds on:** the variable scrub transaction (`editor.scrub_variable`, `crates/drift-app/src/parameters.rs`).

## 4. Animate solves instead of snapping (starting point)
When a typed value, a new relation or an undo re-solves the sketch, the geometry eases into place over about 120 ms instead of jumping.
- **Why it stands out:** it makes the solver's moves easy to follow and Drift more fun to draw in, at little risk.
- **Shape:** display only. The model updates at once, and only drawing interpolates. Picking, snapping and readouts always use the real geometry.
- **Watch:** never during a drag or a slider scrub, which already move every frame. Skip it when the change is large, or start it from the current on-screen position when a new solve lands mid-animation. Respect the operating system's reduced-motion setting. Keep the canvas idle (no CPU use) once the animation ends.
- **Open questions for the Done when:** duration and easing curve; whether a preference can turn it off; whether fills, hatches and dimensions animate with the geometry or appear at the end.

## 5. A sheet of variants (a design table)
Pick a variable range, or load a CSV with one row per variant. Drift lays every version out on one sheet, or makes one sheet per row, with labelled views.
- **Why it stands out:** for millwork, signage and dielines it replaces copy-and-edit. Canvas X Draw and Illustrator with CADTools can't do it.
- **Builds on:** Options (`crates/drift-core/src/params.rs`) and sheet views.

## 6. Automatic revision clouds
Compare the drawing with the last issued revision and Drift clouds what changed on each sheet, then adds a row to the revision table.
- **Why it stands out:** architects do this by hand at every issue. AutoCAD LT doesn't do it.
- **Builds on:** stable entity ids in the line-per-entity `.drift` file, and the title block's revision table.

## 7. Straighten a photo for tracing
Drop in a photo of an elevation, click the four corners of something rectangular, give one known dimension, and get a straightened underlay at true scale.
- **Why it stands out:** surveys of existing buildings, and site photos for signage and millwork.
- **Builds on:** underlay calibration and snapping (`crates/drift-io/src/underlays.rs`). The straightening is a single homography.

## 8. Replay how a drawing was built
Play the undo journal back as a timed animation and export it as video. Later, a scrubber over the same journal could let you branch from any point in the history.
- **Why it stands out:** demo videos, guide tutorials and marketing clips without screen recording.
- **Builds on:** the journal (`crates/drift-core/src/document/journal.rs`).

## Left out
Cut lists, CNC kerf-compensated export and fold previews for dielines. Each would open a new feature area rather than build on what's there.
