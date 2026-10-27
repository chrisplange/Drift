# The `.drift` file format: versions and stability

*2026-10-24, against `main` at 6a625ec (format 42, reader 42); format 43, reader 43 since 2026-10-27.*

A `.drift` file is UTF-8 JSON with one entity, constraint or record per line, so drawings
diff and merge cleanly. The reader and writer are in `crates/drift-io/src/lib.rs`; the
current version is `drift_io::FORMAT_VERSION`, and the oldest version that reads what this
build writes is `drift_io::READER_VERSION`. Coordinates are always millimetres
(`meta.units` is `"mm"`), whatever the display units.

Drift is heading for a paid release, and people will keep drawings for years. The format went
from 38 to 41 in two days in October 2026; this document sets the rules that keep that safe.

```json
{
  "format": "drift",
  "version": 43,
  "reader": 43,
  "meta": {"title":"Bracket","units":"mm","generator":"Drift 0.1.0"},
  ...
}
```

- **`version`** is the format of the build that wrote the file.
- **`reader`** (from 42) is the oldest format that reads the file correctly: the last breaking
  change. A build refuses a file only when its `reader` is above the build's own
  `FORMAT_VERSION`. A file without `reader` (everything before 42) counts its `version`, so
  those behave as before.

## 1. The policy

1. **Changes are additive by default.** A new feature adds an optional table (`"paths": [...]`)
   or an optional field on a record (`"order": -1`). Every new field is
   `#[serde(default, skip_serializing_if = ...)]`, so a drawing that doesn't use the feature
   writes exactly what it wrote before, and a file without the field reads as before.
2. **An addition doesn't bump anything.** Builds from 42 on pass over fields and tables they
   don't know, and keep them for saving (section 4). So an addition needs neither a
   `FORMAT_VERSION` nor a `reader` bump, only:
   - a fixture (`vNN.M.drift`, section 2) and the test of what it holds,
   - a line in the additions table in section 3.

   (`version` *may* still be bumped with an addition, to mark a release; `reader` must not
   be.)
3. **An addition must survive an older build's edits.** An older build keeps a newer field on
   a record it changes, and a newer table when it changes the drawing around it. So the newer
   reader must check what it added against the drawing, and drop or repair what no longer
   fits (a reference to a curve that was deleted, a count that no longer matches). If it
   can't, the change is breaking.
4. **Breaking changes bump `FORMAT_VERSION` and `READER_VERSION` together.** A change is
   breaking when an older build would read a newer file *wrongly*, rather than just without
   the new thing:
   - a field whose absence now means something different, or a value whose meaning changed;
   - a new kind of record in an existing table (a new entity `type`, a new relation, a new
     annotation kind) or a new value of an existing field: older builds refuse these (they
     can't place a record they don't know) and would rather say "please update Drift";
   - a renamed, retyped or removed table or field (as `variants` became `options` in 41);
   - a migration that must run;
   - an addition that can't meet rule 3.
5. **Nothing is renamed, retyped or removed in place.** If a table's meaning has to change, the
   new name is new, and `migrate` turns the old one into it (a breaking change, rule 4).
6. **Every breaking change comes with**
   - a line in the histories on `FORMAT_VERSION` and `READER_VERSION` (and in section 3),
   - the migration in `drift_io::migrate` (or a note there that none is needed),
   - a migration test: an old file opens, says what it should, and saves as the new version,
   - a fixture: `crates/drift-io/tests/fixtures/vNN.drift` (see section 2).
7. **Old fixtures are never edited.** They are what real builds wrote. If one stops opening,
   the reader is wrong, not the fixture.
8. **Readers check, writers don't guess.** Ids, references, numbers (finite, in range) and
   names are validated when a file is read; a bad file is refused with a message, never
   half-loaded. A file that Drift wrote must always open again.
9. **The layout is part of the format.** Top-level entries one per line, lists with one record
   per line, records written compactly by `serde_json` in a fixed field order. Carrying data
   through (section 4) relies on it: a newer writer must keep it, or what it wrote is
   rewritten (still correctly, but no longer byte for byte) by an older build that saves it.

## 2. Fixtures and the tests that guard them

`crates/drift-io/tests/fixtures/` holds one small drawing (a room: walls drawn as one filled
shape, a hatched column, a door swing, an ellipse, a Bézier, dimensions driven by variables,
a measurement, a chair block placed twice with a filled seat, a group, a note, a centre mark,
a text, an embedded font, a sheet with a view, a project, a PDF underlay with a picture, the
drawing rules, an option of values and a whole-sketch option) as each format version wrote it.

- `vNN.drift` is the drawing as format NN wrote it.
- `vNN.M.drift` (from 42 on) is the drawing after the M-th addition made at format NN, which
  didn't bump the version.

| Fixture | Written by | Shipped in |
|---|---|---|
| `v43.drift` | `drift-io` with `fix_x` and `fix_y` (#203) | — |
| `v42.2.drift` | `drift-io` with openings (W3) | — |
| `v42.1.drift` | `drift-io` with walls (W1) | — |
| `v42.drift` | `drift-io` with `reader` (tolerant reading) | — |
| `v41.drift` | `main` (b0a24e4, batch 3) | — |
| `v40.drift` – `v34.drift` | the last commit at each version | — |
| `v33.drift` | 55d9ee0 | Beta 1.4 and 1.5 |
| `v32.drift` | 9bdde61 | Beta 1.3 |
| `v31.drift` | 3365138 | Beta 1.2 |
| `v30.drift` | 7d333c9 | — |
| `v29.drift` | 77f492a | Beta 1.1 |

The newest fixture is made by `write_current_fixture` in
`crates/drift-io/tests/format_fixtures.rs` from `fixture_drawing()`. Each fixture up to 41 was
written by the Drift build of its day: that build loaded the next newer fixture, with that
version's additions taken out, and saved it. (Its strict reader refused anything it didn't
know, which checked the downgrade.) Versions before 29 never shipped; unit tests in `lib.rs`
still cover the migrations from 1, 17, 23–26.

`format_fixtures.rs` checks, for every fixture:

- it opens, with no notes, and with nothing carried through (this build knows all of it);
- the counts and key properties survive: entities, constraints, layers, variables and their
  expressions, fills (the hatch pattern, the solid colour, which fills follow a path), paths,
  rules, stack order, options and whole-sketch options, blocks and their fills, instances,
  groups, texts, annotations, sheets, views, underlays and their pictures, the font, the
  project, measurements, each from the version that added it;
- saving writes the current version, reads back equal, and saves again byte for byte; the
  current version's fixture saves exactly as it was read.

It also checks that:

- a fixture exists for the current `FORMAT_VERSION` (a breaking change without one fails CI
  with what to do);
- the newest fixture's `reader` is `READER_VERSION` (so raising `reader` needs a fixture);
- every fixture's name matches the `version` inside it, files from 42 on have a `reader` no
  higher than their version, and older ones have none;
- the newest fixture is exactly what the writer produces for `fixture_drawing()`. An addition
  that `fixture_drawing()` uses changes that, so it fails until the addition has its own
  fixture. An addition the drawing doesn't use passes, so **add every addition to
  `fixture_drawing()`**.

**When you add to the format** (rule 2): extend `fixture_drawing()` with the addition, run

```bash
cargo test -p drift-io --test format_fixtures write_current_fixture -- --ignored
```

which writes `vNN.M.drift` (the next M) beside the others, commit it, and add the addition's
check (gated on the fixture) to `every_fixture_opens_keeps_its_contents_and_saves_as_current`.

**When you make a breaking change** (rule 4): bump `FORMAT_VERSION` and `READER_VERSION`, run
the same command (which then writes `vNN.drift`), and add the migration test.

`tests/newer_files.rs` checks the other direction: a file from a newer Drift (the current
fixture with unknown tables and fields added on many kinds of record, nested ones too) opens,
saves unchanged byte for byte, keeps its unknown fields on an edited record, warns when a
record that had some is deleted, and is refused when its `reader` is newer.

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
| 42 | `reader` (the oldest version that reads the file). Reading passes over what it doesn't know and keeps it for saving (section 4). No migration. Older builds refuse 42 files with "please update Drift" (they were strict, so they couldn't read a `reader` field anyway). |
| 43 | `fix_x` and `fix_y` relations: a point's x or y alone held where it is (what Fix pins of a point something else partly holds, #203). Breaking (a new relation type), so `reader` is 43 too. No migration. |

### Additions without a bump (from 42)

Additions made since 42 (rule 2), each with its `vNN.M.drift` fixture. Older builds from 42 on
read these files, pass over the addition, and keep it when they save.

| Fixture | Added |
|---|---|
| `v42.1.drift` | `walls` (runs of wall: centreline path, thickness and expression, justification, poché, layer, ends with their joins, and what each made, by key) and `property_dims` (a wall's thickness dimension). The faces, corners, outline paths and pochés walls make are ordinary `entities`, `paths` and `fills`, so an older 42 build shows them as plain geometry and keeps the tables when it saves; opening the file again repairs what it edited (`docs/walls-design.md` §2.6). |
| `v42.2.drift` | `openings` (doors, windows and cased openings in walls: the wall and segment, `from` and `offset` (or centred), width and its expression, leaf, height, the symbol's library family and type, hand, facing, layer, and the symbol's `curves` and `points` by key) and `rules.size_set` (the drawing's door and window size set; unset: by its units). The cut is the wall's own faces (keyed by the new station spelling `jamb:o2.0` and part `J`), and the symbol is ordinary entities, so an older 42 build shows both as plain geometry. A 42 build from before openings that has walls refuses a `walls` table naming a jamb key (no released build reads walls). |

## 4. What Drift does with a file it doesn't fully know

*Checked in the code on 2026-10-24.*

**A newer `reader`.** `migrate` refuses a file whose `reader` (or, without one, its
`version`) is above `FORMAT_VERSION`, and `version` 0 or anything that isn't a number, before
reading anything else, with `LoadError::UnsupportedVersion`: *"file version 45 is newer than
this version of Drift supports (43); please update Drift"* (the message names the file's
`version`). When the user opens such a file (File → Open, a drop, a recent file), the app
shows *"Could not open NAME: …"* in the status bar and keeps the drawing it had. The file isn't
touched, so nothing is lost. The same goes for the AI link (`agent/files.rs`) and for pasting
(`clipboard.rs`), which both report the error. Builds before 42 refuse every 42 file this way.

**A newer `version` this build can read, or unknown fields and tables at its own version.**
The reader passes over them: no record is `deny_unknown_fields` any more. What it passed over
is kept (`crates/drift-io/src/carry.rs`):

- When a file at this build's version or newer is opened, it is written again as this build
  would write it (the *baseline*). If the file has anything the baseline doesn't (a field or
  table anywhere), the file's text and the baseline are kept in `Metadata::carried`. Files
  from older versions carry nothing: the strict builds that wrote them wrote only what this
  build knows. (The check costs one extra write of the drawing when a current file opens; a
  file Drift itself wrote is found equal at once.)
- Saving (`save`, `save_checked`) writes the drawing, then compares it with the baseline,
  record by record (matched by `id`, or `of` for a dimension's or annotation's settings, or
  by position in a list without ids whose length didn't change):
  - **unchanged**: written as the file had it, byte for byte, newer fields and all. A file
    opened and saved unchanged is the same file, `version` and `reader` included;
  - **changed**: written as now, with the fields this build doesn't know put back after the
    field they followed. A field this build writes now wins over the same field in the file;
  - **unknown tables**: written back where they were;
  - **gone**: what it held that this build doesn't know is lost. That is a record deleted, a
    field cleared, a record replaced by one of another `type` under the same id, or a list
    without ids that changed length.
- The `version` and `reader` the file had are kept (they are unchanged records too), so the
  newer build still knows its additions when it opens the file again, and checks them (rule 3).

**The warning.** Only the "gone" case loses anything, so only it warns (`Saved::loses_newer_data`).
Changing a record never warns: by rule 3 the newer build copes with an older build's edits.
In the app, Save and Save As (including Save in the unsaved-changes prompt) then ask first:

> **Save NAME?** This drawing has things from a newer Drift that this version doesn't know;
> saving here may lose them. To keep the file as it is, save to a new file with Save As.
> *Cancel* / *Save anyway*

Cancel saves nothing, and whatever was to follow the save (closing, opening another drawing)
doesn't happen. After a save, the drawing compares with the file just written, so the next
save doesn't ask again about what is already gone. To see it: open a file from a newer Drift
(or one with a field added by hand to, say, a text record), delete that text, and save.

The other ways a drawing is written:

- **Autosave** (the restored session) writes with the carried data, without asking: nothing is
  overwritten that the user can't still save from.
- **The AI link** (`save_drawing`) refuses with the same sentence and saves nothing, unless it
  is called with `"anyway": true`; its description tells the assistant to ask the user first.
- **Copy and paste** (`clipboard.rs`): copied records are written by this build, and pasted
  ones are new records, so a paste from a newer Drift reads tolerantly and leaves out what this
  build doesn't know (nothing is lost from any file).
- **The browser demo** reads tolerantly too; it never saves, so it carries nothing.

**Known limits.** A record deleted and a new one made with the same id and type takes the old
one's unknown fields. A newer field that refers to something an older build deleted is
dangling: rule 3 makes the newer reader drop it. Edits to the drawing around an unknown
table never warn, for the same reason.

**The restored session (fixed 2026-10-24).** At start the app restores the last session
from its autosave (`restore_session` in `app/document.rs`). Before this fix, if that autosave
was written by a newer build (the user tried a newer Drift, then went back), the app only
logged the failure, and the next autosave (`KEY_DOCUMENT` in `lib.rs`) overwrote the stored
text, so the unsaved work from the newer build was lost. Saved files were always safe.
From 42, an autosave whose `reader` this build knows simply opens, carrying what it doesn't
know; the rest of this applies to one it can't read.

Now (`app/recovery.rs`):

- the stored text is written, byte for byte, to `recovered-<YYYY-MM-DD-HHMMSS>.drift` (UTC)
  in Drift's data folder before anything else happens. An existing file is never replaced
  (a second one the same second gets `-2`);
- a window stays up until it is closed: *"The last session was saved by a newer Drift
  (format 45) and can't be opened here. It was kept as recovered-….drift in FOLDER. Open it
  with the Drift that saved it."* Other failures say *"The last session couldn't be opened:
  REASON."* A **Show the file** button opens Explorer or the Finder at it;
- if the file can't be written, the window says so, and Drift doesn't autosave over the
  stored session for the rest of that run;
- in the browser there is no file: the window says the session couldn't be opened and a new
  drawing was started. (The browser demo never restores a session.)

Tests: `a_session_from_a_newer_drift_is_kept_as_a_file_and_said` and
`a_session_that_cant_be_kept_is_not_autosaved_over` in `drift-app/src/ui_tests.rs`.

## 5. Related safeguards

- Importers (PDF, SVG, DXF, DWG) never panic on bad input, and anything they add to a drawing
  saves and opens again: `crates/drift-io/tests/import_robustness.rs` (section 1 of the
  hardening work, 2026-10-23).
- `load` validates ids, references and numbers; `save` is deterministic and bit-exact
  (`round_trip_is_exact` in `lib.rs`).
