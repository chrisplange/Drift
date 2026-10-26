# Regression checklist

Big batches have shipped regressions that the tests missed and that were found by hand: an SVG
chosen for the title-block logo went to Import instead (#151), a picked hatched shape lost its
pattern handles (#158), and a click in a grouped shape's fill picked the shape, not the group
(#159). The smoke suite is the guard against that. It drives the real app headlessly, as a user
would: clicks, keys, files dropped or handed over as the system's file dialog would.

## Run it

```bash
cargo test -p drift-app smoke_
```

Run it before reporting any change, together with the usual checks (format, clippy for native,
wasm32 and the demo, `cargo test --workspace`). With the test build already compiled, the 27
tests take about 10 seconds run in parallel on an idle machine (82 seconds one at a time, and up
to a minute and a half when other builds share the GPU and CPU). Compiling the test build after a
change to `drift-app` takes several minutes more.

CI runs it on every pull request, as part of `cargo test --workspace` on Linux, Windows and
macOS. Like every UI test, a smoke test is **skipped (and reported as passing) when the
computer has no graphics adapter**: the log then says `no GPU adapter available; skipping UI
test`. Linux CI installs a software Vulkan driver (lavapipe) so the tests really run there.

The tests live in [`crates/drift-app/src/ui_tests/smoke/`](../crates/drift-app/src/ui_tests/smoke/),
one file per area. Run one area by its file name, for example
`cargo test -p drift-app smoke::fills`. Screenshots of what they saw go to `target/tmp/`
(`drift_smoke_*.png`).

## The rule

**Every regression the user finds becomes a smoke test in the matching file before its fix
merges.** The test drives the app the way the user did when they found it, fails on the code
before the fix (check that by turning the fix off locally), and passes with it. Name it
`smoke_<area>_<what>` and add a line for it below.

## Areas

| Area | File | What it covers | Run |
| --- | --- | --- | --- |
| Sheets | `sheets.rs` | A sheet added from the Sheets panel, with a view of the model drawn on it; the title block changed from its menu and undone; the PDF (what Print sends) and SVG of the sheet; Print with two sheets asks which, and Cancel prints nothing. A logo chosen with Logo → Choose SVG… in the sheet's settings is the logo, with no import dialog and nothing drawn (#151). | `cargo test -p drift-app smoke::sheets` |
| Fills and hatches | `fills.rs` | Solid and hatched rectangles from the Rectangle tool's Fill box. A click in a hatched shape picks the shape and shows its pattern's handles and Pattern alignment (#158); Align to edge… turns the pattern along a side, in one undo step. Two overlapping squares: Ctrl+[ sends the top one back, on screen and in the PDF and SVG alike (print matches the screen); Stroke: none from the Inspector. Two overlapping filled squares, both picked, T: Trim cuts only against them (the hint says so), each drag across pieces is one undo step, and the joined outline keeps one fill, the top one's. | `cargo test -p drift-app smoke::fills` |
| Groups and blocks | `groups.rs` | Two filled shapes picked (click, Shift-click) and grouped with Ctrl+G. A click in a fill picks the whole group with both fills (#159); G, X, 40, Enter moves it all, fills included; a double-click enters the group with that shape picked. Make block from the Inspector keeps the fills; Explode to group brings both shapes back. | `cargo test -p drift-app smoke::groups` |
| Selection and move | `selection.rs` | Each selection filter alone (Alt-click), badge on; Fills alone picks a fill without its outline; Fills plus Edges together; All. G with a line picked by its midpoint, X, a typed 12 (#154). X pressed during a mouse drag locks it to the axis (#155). An area whose outline is on a hidden layer can't be picked (#153). Alt-click on a layer's lock and snap (drift-releases#10). | `cargo test -p drift-app smoke::selection` |
| Import | `import.rs` | File → Import… with an SVG, a PDF and a DXF (synthetic files) through the import dialog. A dropped DXF offers Open as new drawing, Import into this drawing… and Cancel; Cancel brings nothing in, Import does. A dropped `.drift` and a `.pdf` that can't be read leave the drawing, its name and the recent files as they were. A DXF with a door block placed as drawn and mirrored as AutoCAD writes it (X scale −1, no Y scale) comes in as two copies, the second mirrored, not turned 180°; imported again, it reuses the same block. An inch DXF's ANSI31 at 16 comes in at real size (2" apart), a SOLID comes in filled and opaque, and a block's hatch is the block's own fill, shown by each copy. | `cargo test -p drift-app smoke::import` |
| Files | `files.rs` | Ctrl+S to the drawing's file, Ctrl+N, then File → Open recent: the same drawing, not marked unsaved. An autosaved session from a newer Drift is kept byte for byte as a file of its own, said in a window, and not autosaved over. A file with a field and a table this Drift doesn't know opens by drop and, after an edit, Ctrl+S writes them back without asking (#165). A drawing with a solid-filled rectangle, an ellipse, a pen curve and text with accents exports a DXF with only R12 entities (AutoCAD discarded the file at an ELLIPSE or SPLINE) and plain-ASCII text escapes, and File → Import… brings it back with the text unchanged. | `cargo test -p drift-app smoke::files` |
| Dimensions and solver | `dimensions.rs` | D on a line, a place, a typed 30; a double-click on the text and 45; one undo step. A mouse drag of one line in a chain moves its ends only: the next line's far end and a line apart stay put (#143). The same drag, undone and done again, ends in exactly the same place. | `cargo test -p drift-app smoke::dimensions` |
| Commands | `commands.rs` | The palette's best match for move, rotate and scale; Move typed in the palette grabs the picked line. The Draw and Modify menus open with their commands. Enter repeats the last tool. Shift+Alt+W toggles thin lines. Preferences (Ctrl+,) keeps Delete and Ctrl+Z off the drawing, and Esc closes only the dialog. | `cargo test -p drift-app smoke::commands` |
| Options | `options.rs` | Save current in the Parameters panel as Values only, then as Whole sketch; a click on each applies it (the values; the whole sketch, taking away a line drawn since) and Ctrl+Z undoes it. | `cargo test -p drift-app smoke::options` |
| Walls | `walls.rs` | W, a room clicked round and closed on its start: one ring, one poché. A wall started on the room's side tees into it (one poché for both). A click in the poché picks the wall; a new thickness re-thickens it and Ctrl+Z puts it back. Saved, closed and opened again: the walls as they were, nothing repaired; Edit → Explode on a picked wall leaves plain lines and a fill. | `cargo test -p drift-app smoke::walls` |

Deeper tests of the same features stay where they were (`ui_tests.rs`, `ui_tests/*.rs`,
`fill_tests.rs`, `tools/tests.rs`, `drag_props.rs`). The smoke suite is the quick pass over
everything.

## By hand

Headless tests can't see these. Check them by hand before a release, and after a change that
touches them:

- **File dialogs.** File → Open, Save as…, Import…, Export, and Logo → Choose SVG… open the
  system's dialog, and what is picked there arrives (the tests hand files over directly).
- **Printing.** Print opens the PDF in the system viewer, and it prints on a real printer at the
  right size and scale, in colour, greys or black and white as the sheet says.
- **PDF and SVG viewers.** An exported sheet opens in Acrobat or Preview, and the SVG in a
  browser and Inkscape, looking like the screen (fills over and under as drawn, line weights).
- **macOS.** Trackpad pinch to zoom and two-finger pan, a Retina display (sharp lines and text at
  2×), Cmd in place of Ctrl in menus and shortcuts.
- **Windows.** High-DPI displays at 125 %, 150 % and 200 %, and a window moved between monitors
  of different scales.
- **Drag and drop from the desktop.** A file dragged from Explorer or Finder onto the window.
- **Walls by eye.** Joins at odd angles, curved walls meeting straight ones, hatched pochés
  across a cluster, and a wall plan printed to PDF and exported to DXF (faces as lines and
  arcs, pochés as fills, butt edges and centrelines left out).
- **AutoCAD.** An exported DXF opens in AutoCAD or the free DWG TrueView. Headless, with
  either installed: `scripts\cad-check.ps1 export.dxf` opens each file in AutoCAD's own engine
  and prints AutoCAD's reason if it refuses one.
- **The installed build.** The installer, the update notice, and opening a `.drift` file by
  double-clicking it.
