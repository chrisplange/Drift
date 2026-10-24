# The `.drift` file format: versions and stability

*2026-10-23, against `main` at 74c5b2f (format 41).*

A `.drift` file is UTF-8 JSON with one entity, constraint or record per line, so drawings
diff and merge cleanly. The reader and writer are in `crates/drift-io/src/lib.rs`; the
current version is `drift_io::FORMAT_VERSION`. Coordinates are always millimetres
(`meta.units` is `"mm"`), whatever the display units.

Drift is heading for a paid release, and people will keep drawings for years. The format went
from 38 to 41 in two days in October 2026; this document sets the rules that keep that safe.

## 1. The policy

1. **Changes are additive by default.** A new feature adds an optional table (`"paths": [...]`)
   or an optional field on a record (`"order": -1`). Every new field is
   `#[serde(default, skip_serializing_if = ...)]`, so a drawing that doesn't use the feature
   writes exactly what it wrote before, and a file without the field reads as before.
2. **Nothing is renamed, retyped or removed in place.** If a table's meaning has to change
   (as `variants` became `options` in 41), the new name is new, and `migrate` turns the old
   one into it.
3. **A version bump is for changes older readers would get wrong**, not for every addition.
   Bump `FORMAT_VERSION` when a file written by the new build would be misread by the previous
   one: a field whose absence now means something different, a value whose meaning changed,
   a migration that must run. (See section 4 for why, today, every addition still bumps.)
4. **Every bump comes with**
   - a line in the history on `FORMAT_VERSION` (and in section 3 below),
   - the migration in `drift_io::migrate` (or a note there that none is needed),
   - a migration test: an old file opens, says what it should, and saves as the new version,
   - a fixture: `crates/drift-io/tests/fixtures/vNN.drift` (see section 2).
5. **Old fixtures are never edited.** They are what real builds wrote. If one stops opening,
   the reader is wrong, not the fixture.
6. **Readers check, writers don't guess.** Ids, references, numbers (finite, in range) and
   names are validated when a file is read; a bad file is refused with a message, never
   half-loaded. A file that Drift wrote must always open again.

## 2. Fixtures and the tests that guard them

`crates/drift-io/tests/fixtures/` holds one small drawing (a room: walls drawn as one filled
shape, a hatched column, a door swing, an ellipse, a Bézier, dimensions driven by variables,
a measurement, a chair block placed twice with a filled seat, a group, a note, a centre mark,
a text, an embedded font, a sheet with a view, a project, a PDF underlay with a picture, the
drawing rules, an option of values and a whole-sketch option) as each format version wrote it.

| Fixture | Written by | Shipped in |
|---|---|---|
| `v41.drift` | `main` (b0a24e4, batch 3) | — |
| `v40.drift` – `v34.drift` | the last commit at each version | — |
| `v33.drift` | 55d9ee0 | Beta 1.4 and 1.5 |
| `v32.drift` | 9bdde61 | Beta 1.3 |
| `v31.drift` | 3365138 | Beta 1.2 |
| `v30.drift` | 7d333c9 | — |
| `v29.drift` | 77f492a | Beta 1.1 |

The newest fixture is made by `write_current_fixture` in
`crates/drift-io/tests/format_fixtures.rs` from `fixture_drawing()`. Each older one was written
by the Drift build of its day: that build loaded the next newer fixture, with that version's
additions taken out, and saved it. (Its strict reader refused anything it didn't know, which
checked the downgrade.) Versions before 29 never shipped; unit tests in `lib.rs` still cover
the migrations from 1, 17, 23–26.

`format_fixtures.rs` checks, for every fixture:

- it opens, with no notes;
- the counts and key properties survive: entities, constraints, layers, variables and their
  expressions, fills (the hatch pattern, the solid colour, which fills follow a path), paths,
  rules, stack order, options and whole-sketch options, blocks and their fills, instances,
  groups, texts, annotations, sheets, views, underlays and their pictures, the font, the
  project, measurements, each from the version that added it;
- saving writes the current version, reads back equal, and saves again byte for byte.

It also checks that a fixture exists for the current `FORMAT_VERSION` (a bump without one
fails CI with what to do), that every fixture's name matches the version inside it, and that
the current fixture is still exactly what the writer produces for the same drawing.

**When you bump the format:** extend `fixture_drawing()` with what the new version adds,
run

```bash
cargo test -p drift-io --test format_fixtures write_current_fixture -- --ignored
```

commit the new `vNN.drift` beside the old ones, and add the new feature's check (gated on
`version >= NN`) to `every_fixture_opens_keeps_its_contents_and_saves_as_current`.

## 3. What each version added

From the history on `FORMAT_VERSION` and `migrate` in `crates/drift-io/src/lib.rs`. Unless a
migration is named, the change was additive and older files read unchanged.

| Version | Added |
|---|---|
| 1 | Entities only (M1). |
| 2 | `constraints` (M2). Migration: a v1 file gets an empty `constraints`. |
| 3 | Diameter, horizontal/vertical and point–line distances; a dimension's `label` offset (M3). |
| 4 | Tangent, concentric, coradial, collinear, point-on-curve and symmetric relations; `driven` dimensions (M4). |
| 5 | `display` units, `variables`, a dimension's `expr`, `measurements` and `variants` (M5). |
| 6 | `layers`, `current_layer`, and an entity's `layer`, `color`, `weight` (mm) and `linetype` (M6). |
| 7 | `drafting` (the dimension style), `fills` (solid and hatch), `annotations` (notes, centre marks, callouts, ordinates), a dimension's `prefix`, `suffix` and `tol` (M7). |
| 8 | `sheets` (paper, scale, view, title block, revisions) (M8). |
| 9 | `groups`. |
| 10 | `blocks` and `instances`. |
| 11 | A layer's `fill` colour. |
| 12 | `underlays` (M8.5). |
| 13 | `ellipse` entities. |
| 14 | `bezier` curves and `smooth` relations. |
| 15 | `texts` (M9). |
| 16 | The `modern` title block. |
| 17 | Sheet flags. |
| 18 | A sheet's `views` (before, a sheet had one view in its own fields; read as such). |
| 19 | `fonts`, and a text's `align`, `spacing` and `wrap`. |
| 20 | The drafting style's `color`. |
| 21 | `shown` (where dimensions and annotations show), a dimension's `below` and `text`. |
| 22 | A block's `fills`. |
| 23 | A layer's `snap`; layers are listed in draw order. |
| 24 | Notes have any number of `leaders` (with a `bend` or `bulge` and a shoulder) and a text style. |
| 25 | A sheet's `colours` (greyscale, black & white). |
| 26 | `styles` (a dimension's or annotation's own settings over the drafting style). |
| 27 | `views` are the model's (a named crop at a scale), placed on sheets by `viewports`. Migration on read: a sheet's own views become model views. |
| 28 | A view's `hidden_layers`. |
| 29 | A section or elevation mark's `view`. *(Beta 1.1)* |
| 30 | The `project` the sheets share, a sheet's `status`. Migration on read: a title or company every sheet shares becomes the project's. |
| 31 | A `shown` entry's `views` (drawn in those views only). *(Beta 1.2)* |
| 32 | A view's `construction`. *(Beta 1.3)* |
| 33 | An underlay's `images` (pictures from a PDF page). *(Beta 1.4, 1.5)* |
| 34 | A dimension's `ext` (extension line starts set by hand). |
| 35 | A sheet's `title_block` can be `blank`; `none` stays the border without a title block. |
| 36 | A block's `profile` flag (for the Blender profiles file). |
| 37 | A fill's `divided` (an edit left it covering more than one area). |
| 38 | `paths` (curves drawn as one shape) and a fill's `path`. |
| 39 | `rules` (how closed areas and fills behave, the gap tolerance). Migration: an older file reads with the defaults, and `Metadata::older_rules` tells the app to apply its own preferences, which is where the rules lived before. |
| 40 | `stack` (where curves stack among their layer's fills) and an underlay's `layer` and `order`. |
| 41 | `variants` become `options`, holding values or a whole `sketch` (a Drift file of its own, leaving out the sections it has the `same` as the drawing); a block fill's `"path": "shape"`. Migration: `variants` are renamed `options`. |

## 4. What Drift does with a file it doesn't fully know

*Checked in the code on 2026-10-23.*

**A newer version.** `migrate` refuses any `version` above `FORMAT_VERSION` (and 0, and
anything that isn't a number) before reading anything else, with
`LoadError::UnsupportedVersion`: *"file version 42 is newer than this version of Drift
supports (41); please update Drift"*. When the user opens such a file (File → Open, a drop, a
recent file), the app shows *"Could not open NAME: …"* in the status bar and keeps the drawing
it had. The file isn't touched, so nothing is lost. The same goes for the AI link
(`agent/files.rs`) and for pasting (`clipboard.rs`), which both report the error.

**An unknown field or table at a known version.** Every record is
`#[serde(deny_unknown_fields)]`, including the top level, so the file is refused (*"invalid
JSON: unknown field `x`, expected one of …"*). Again nothing is lost, but this is why today
**every addition needs a bump**: a build at 41 can't open a 41 file that has one field more.
Policy rule 3 (bump only when old readers would misread) only becomes true once readers
tolerate additions; see the proposal below.

**The restored session (fixed 2026-10-24).** At start the app restores the last session
from its autosave (`restore_session` in `app/document.rs`). Before this fix, if that autosave
was written by a newer build (the user tried a newer Drift, then went back), the app only
logged the failure, and the next autosave (`KEY_DOCUMENT` in `lib.rs`) overwrote the stored
text, so the unsaved work from the newer build was lost. Saved files were always safe.

Now (`app/recovery.rs`):

- the stored text is written, byte for byte, to `recovered-<YYYY-MM-DD-HHMMSS>.drift` (UTC)
  in Drift's data folder before anything else happens. An existing file is never replaced
  (a second one the same second gets `-2`);
- a window stays up until it is closed: *"The last session was saved by a newer Drift
  (format 42) and can't be opened here. It was kept as recovered-….drift in FOLDER. Open it
  with the Drift that saved it."* Other failures say *"The last session couldn't be opened:
  REASON."* A **Show the file** button opens Explorer or the Finder at it;
- if the file can't be written, the window says so, and Drift doesn't autosave over the
  stored session for the rest of that run;
- in the browser there is no file: the window says the session couldn't be opened and a new
  drawing was started. (The browser demo never restores a session.)

Tests: `a_session_from_a_newer_drift_is_kept_as_a_file_and_said` and
`a_session_that_cant_be_kept_is_not_autosaved_over` in `drift-app/src/ui_tests.rs`.

### Proposed (not made yet)

**Let additions through without a bump, safely** (tracked separately). To make rule 3 real:

- drop `deny_unknown_fields` on the top level and on records, so a reader passes over
  what it doesn't know;
- keep what it passed over: collect unknown top-level tables and per-record fields as
  raw JSON (`#[serde(flatten)] extra: serde_json::Map`) and write them back on save, so
  an older build that edits a newer file doesn't strip the newer build's data from it;
- where carrying it through isn't possible (an unknown field on a record the user then
  deletes or changes), warn before saving: *"This drawing has things from a newer Drift
  that this version doesn't know; saving here may lose them."*
- add `"reader": NN` next to `"version"`: the oldest format that reads the file correctly.
  A writer sets it to the last breaking version, so files of an additive release still open
  in the release before; a reader refuses only files whose `reader` is above its own
  `FORMAT_VERSION`.

Until that lands, every format change must bump `FORMAT_VERSION` (as now), and the fixture
guard makes sure each bump is covered.

## 5. Related safeguards

- Importers (PDF, SVG, DXF, DWG) never panic on bad input, and anything they add to a drawing
  saves and opens again: `crates/drift-io/tests/import_robustness.rs` (section 1 of the
  hardening work, 2026-10-23).
- `load` validates ids, references and numbers; `save` is deterministic and bit-exact
  (`round_trip_is_exact` in `lib.rs`).
