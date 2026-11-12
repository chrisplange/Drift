# Papercuts

Small usability annoyances in Drift that aren't real bugs and aren't worth their own issue yet: an odd label, a misaligned control, a missing hover hint, an extra click.

GitHub issues stay the backlog. This file is a fast inbox. Add a row in one plain sentence, newest first. The user can report one in a few words to the lead session, and it lands here. Small ones are cleared in a sweep PR; bigger ones become their own issue with a Done when line, and their rows point to it. Rows that already have an issue just link it. No client names or drawings.

Areas: canvas, panel, tool, menu, file, other. Status: `open`, `batched #NNN`, `fixed #NNN`.

| Date | Area | Papercut | Status |
|---|---|---|---|
| 2026-11-12 | menu | After Esc or a click outside, the custom scale box keeps the typed text and error and shows them again next time | open |
| 2026-11-12 | tool | Door openings don't include a jamb | open |
| 2026-11-12 | tool | Pressing Space to change a wall's orientation while drawing skips the "center" option | fixed #416 |
| 2026-11-11 | other | A sheet view can't be duplicated together with its drawing entities and annotations in one operation | open |
| 2026-11-11 | panel | Layer 0 is a DWG/DXF requirement that shows in every drawing. New files must never show a layer 0, and a file without one must still open; layer 0 appears only on an imported document or a drawing that already has one. Every DWG/DXF export writes an empty layer 0 | open |
| 2026-11-11 | menu | Hatches are slow to reach and change; a right-click or fly-out menu to switch a fill's hatch quickly would help | fixed #416 |
| 2026-11-11 | tool | There's no way to pick one entity out of several overlapping ones (e.g. click again to cycle, or a pick list) | open |
| 2026-11-11 | panel | The right-side panels need a whole UX/UI review | open |
| 2026-11-11 | canvas | How annotations work with the canvas and drawing scale is unintuitive (details to collect when batched; see #369) | open |
| 2026-11-11 | canvas | An SVG logo shows broken strokes because of how antialiasing is done | open |
| 2026-11-11 | canvas | Geometry on one layer still attaches to geometry on other layers, so vertices and edges merge across layers when you don't expect it (#267 fixed this for Tab chain selection only) | open |
| 2026-11-11 | menu | The custom scale input box closes when you click in its field to type | fixed #416 |
| 2026-11-11 | file | An uploaded company logo doesn't stay in Drift; it should persist so it doesn't have to be loaded again | open |
| 2026-11-11 | tool | Mirror needs a temporary axis (two picked points, no line drawn first); the user has asked for this before and needs it badly | open |
| 2026-11-11 | canvas | Text notes are edited in a separate text box, which makes multi-line formatting a nightmare; editing should happen live on the canvas | open |
| 2026-11-11 | canvas | In the sheet view you can't add or edit annotations in sheet space; the user needs this badly (see #369 for note positions) | open |
| 2026-11-10 | tool | Shift+D with no pointer on the canvas, or on a locked layer, isn't handled well (follow-up to #264) | open |
| 2026-11-06 | panel | The print-size hint doesn't offer the scale the drawing's sheet views share | fixed #416 |
| 2026-11-03 | canvas | Fill edges look jagged when the fill has no outline | batched #326 |
| 2026-10-31 | tool | Automatic relations keep redundant ones for snaps other than onto-point | batched #306 |
| 2026-10-28 | file | PDF export defaults to fit-to-A3 instead of true scale | batched #245 |
