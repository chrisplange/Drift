# Features

The single list of what Drift actually does today. Every line points to code. If it isn't here, don't claim it: not in PRs, docs, the website or marketing copy. A PR that adds, removes or changes a user-visible feature updates this file in the same PR (the product checker fails it otherwise).

Seeded 2026-11-09 from the command registry, importers/exporters, agent schema and solver constraint enum. Lines marked (unverified) need a check against `docs/guide/reference/tools.md`.

## Files
- New / Open / Save / Save as for `.drift` drawings. `crates/drift-app/src/commands.rs`
- Versioned `.drift` format with test fixtures. `crates/drift-core/src/document/mod.rs`
- Crash recovery of unsaved work. `crates/drift-app/src/app/recovery.rs`
- Start panel with an example drawing. `crates/drift-app/src/app/start_ui.rs`
- Import DXF. `crates/drift-io/src/dxf_import.rs`
- Import DWG (read via acadrust). `crates/drift-io/src/dwg_import.rs`
- Import vector PDF pages. `crates/drift-io/src/pdf_import.rs`
- Import SVG. `crates/drift-io/src/svg_import.rs`
- Import Illustrator `.ai`, PDF-compatible files only (through the PDF importer). `crates/drift-app/src/app/import_ui.rs`
- Export DXF. `crates/drift-io/src/dxf.rs`
- Export DWG with fills, hatches and blocks. `crates/drift-io/src/cad.rs`
- Export PDF of sheets or the model. `crates/drift-io/src/pdf.rs`
- Export SVG of the model or a sheet. `crates/drift-io/src/svg.rs`
- Print sheets (through PDF). `crates/drift-app/src/commands.rs`
- Export profiles for the Blender add-on. `crates/drift-io/src/profiles.rs`
- Underlays: reference drawings placed under the sketch. `crates/drift-io/src/underlays.rs`

## Drawing
- Line and polyline. `crates/drift-edit/src/tools/line.rs`
- Rectangle. `crates/drift-edit/src/tools/rectangle.rs`
- Regular polygon. `crates/drift-edit/src/tools/polygon.rs`
- Circle. `crates/drift-edit/src/tools/circle.rs`
- Arcs: centre, three-point and tangent. `crates/drift-edit/src/tools/arc.rs`
- An arc continuing a line or arc: Space cycles tangent, reverse tangent and free. `crates/drift-edit/src/tools/tangent_arc.rs`
- Ellipse. `crates/drift-edit/src/tools/ellipse.rs`
- Pen (smooth curves, Illustrator-style). `crates/drift-edit/src/tools/pen.rs`
- Slot. `crates/drift-edit/src/tools/slot.rs`
- Point. `crates/drift-edit/src/tools/point.rs`
- Construction geometry toggle. `crates/drift-app/src/commands.rs`
- Snaps: grid, end, midpoint, centre, quadrant, on curve, origin, perpendicular, tangent. `crates/drift-geom/src/snap.rs`
- Snap pie (Shift+S): each kind of snap on or off, by flick, click or its letter; saved with the drawing. Perpendicular and Tangent are the two halves of the status bar's Perpendicular & tangent switch. `crates/drift-app/src/app/snap_pie.rs`
- Typed lengths and angles while drawing. `crates/drift-app/src/app/temp_dims_ui.rs`

## Modify
- Move, rotate, scale, copy, mirror. `crates/drift-edit/src/modify.rs`
- Rotate tool around a base point. `crates/drift-edit/src/tools/rotate.rs`
- Trim (click a piece, or drag across pieces as a power trim; every curve cuts, whatever is picked), extend (to the next curve, likewise whatever is picked), break at point. `crates/drift-edit/src/tools/curve_ops.rs`
- Fillet and chamfer, editable by drag. `crates/drift-edit/src/tools/corners.rs`
- A corner kept for a dimension, and a fillet's centre, drawn as a muted cross until the fillet or its lines are lit. `crates/drift-edit/src/editor/muted_points.rs`
- Join corners. `crates/drift-app/src/commands.rs`
- Linear and circular arrays, previewed on the canvas until applied (one undo step). `crates/drift-app/src/app/arrays.rs`
- Find overlapping curves. `crates/drift-app/src/commands.rs`
- Check sketch: gaps and short ends. `crates/drift-app/src/app/checker_ui.rs`
- Undo / redo. `crates/drift-core/src/document/journal.rs`
- Cut, copy, paste, paste in place. `crates/drift-app/src/commands.rs`

## Offsets
- Offset curves. `crates/drift-edit/src/tools/offset_mirror.rs`
- Live offsets that follow their source. `crates/drift-geom/src/offsets/regen.rs`
- Detach an offset to plain geometry. `crates/drift-edit/src/offsets.rs`

## Solver: relations and dimensions
- Relations: coincident, horizontal, vertical, parallel, perpendicular, equal, tangent, concentric, coradial, collinear, point on curve, symmetric, smooth, midpoint, equal distance. `crates/drift-core/src/constraint.rs`
- Fix: point, X only, Y only, radius. `crates/drift-core/src/constraint.rs`
- Driving dimensions: distance, horizontal/vertical, point-to-line, radius, diameter, the gap between two concentric circles or arcs (pick one, then the other), angle. `crates/drift-core/src/constraint.rs`
- Sparse incremental solver. `crates/drift-solver/src/lib.rs`
- Automatic relations while drawing. `crates/drift-core/src/rules.rs`
- Status colours: free, fully defined, conflicting. `crates/drift-app/src/commands.rs`
- The red "N conflicting" pill picks the conflicting relations; redundant ones are marked in the Relations list. `crates/drift-app/src/app/panels.rs`
- Clear relations / clear dimensions on the selection. `crates/drift-app/src/commands.rs`
- Variables and unit-aware expressions; Parameters panel. `crates/drift-core/src/expr.rs`
- Metric and imperial units. `crates/drift-core/src/units.rs`
- Options: saved variants (values only or whole sketch). `crates/drift-core/src/params.rs`
- Curvature combs. `crates/drift-app/src/commands.rs`
- Solves ease into place (typed values, new relations, undo, redo) over 120 ms; display only, never while dragging; Preferences → General → Animate solves (on by default), and reduced motion in the browser turns it off. `crates/drift-app/src/motion.rs`

## Walls and openings
- Wall tool with thickness. `crates/drift-edit/src/tools/wall.rs`
- Wall joins: join / unjoin with corner cleanup. `crates/drift-geom/src/walls/mod.rs`
- Walls from lines or curves. `crates/drift-edit/src/walls_from_lines.rs`
- Explode walls. `crates/drift-edit/src/walls.rs`
- Doors, windows and openings in walls. `crates/drift-edit/src/tools/opening.rs`
- Opening properties in the Inspector. `crates/drift-edit/src/openings_props.rs`
- Wall centrelines display. `crates/drift-app/src/app/wall_ui.rs`

## Fills and hatches
- Solid fills of closed areas. `crates/drift-edit/src/tools/fill.rs`
- Hatch patterns. `crates/drift-draft/src/hatch.rs`
- H with the Fill tool on switches between the last hatch and Solid. `crates/drift-app/src/app/fill_ui.rs`
- Edit fill. `crates/drift-app/src/app/fill_ui.rs`
- Closed-area detection. `crates/drift-geom/src/regions.rs`

## Annotation
- Dimensions: aligned, linear, angular, radial, ordinate. `crates/drift-edit/src/tools/dimension.rs`
- Align and space dimensions. `crates/drift-app/src/app/dim_align.rs`
- Single and multi-line text, edited in place. `crates/drift-edit/src/tools/text.rs`
- Leaders and notes, typed straight onto the canvas or sheet (Ctrl+B / Ctrl+I: whole-note bold, italic). `crates/drift-app/src/app/note_ui.rs`
- Centre marks. `crates/drift-edit/src/tools/annotate.rs`
- Annotation scale with auto-scale. `crates/drift-app/src/app/scale_menu.rs`
- Fonts. `crates/drift-core/src/fonts.rs`

## Sheets
- Sheets (page layouts). `crates/drift-core/src/sheets.rs`
- Views on sheets: add by region, crop, align, delete. `crates/drift-app/src/app/views_ui.rs`
- Sheet editor and preview. `crates/drift-app/src/app/sheet_editor.rs`
- Leave items off sheets. `crates/drift-app/src/app/visibility_ui.rs`
- Notes, leaders and reference dimensions on a sheet (paper space), tied to the model through a view. `crates/drift-core/src/sheet_annotations.rs`
- Move or copy the model's notes and dimensions onto a sheet. `crates/drift-edit/src/sheet_annotations.rs`
- Page frame. `crates/drift-draft/src/page.rs`

## Blocks, groups, layers
- Make block from a group. `crates/drift-edit/src/blocks.rs`
- Place block copies. `crates/drift-edit/src/tools/place_block.rs`
- Explode block copies to groups. `crates/drift-app/src/app/blocks_ui.rs`
- Group, ungroup, rename. `crates/drift-app/src/app/groups_ui.rs`
- Layers panel. `crates/drift-core/src/layers.rs`
- Draw order (front / back). `crates/drift-app/src/app/arrange_ui.rs`

## Selection and view
- Selection filters: points, edges, areas, fills, text, annotations, blocks/groups. `crates/drift-app/src/commands.rs`
- Box select and Shift/Ctrl modifiers. `crates/drift-edit/src/tools/select.rs`
- Pan, zoom, zoom to fit, zoom to selection. `crates/drift-edit/src/tools/view.rs`
- Grid and origin axes; grid spacing Auto or fixed (typed length, divisions per major square, View → Grid spacing). `crates/drift-app/src/commands.rs`, `crates/drift-render/src/frame.rs`
- Right-click context menus. `crates/drift-app/src/app/context_ui.rs`
- Pies, one look and one flick-or-click model (`crates/drift-app/src/app/pie_style.rs`): relations pie (Q, `pie.rs`), Selection filter pie (Shift+Q, `select_pie.rs`), snap pie (Shift+S, `snap_pie.rs`). Hold the key and flick, or tap it and click; each pill's letter chooses it while the pie is open.

## UI
- Command palette with CAD aliases (LINE, TRIM, HATCH…). `crates/drift-app/src/app/palette.rs`
- Assignable keyboard shortcuts with presets (Drift default, AutoCAD-style, your own): Preferences → Keyboard. `crates/drift-app/src/keymap.rs`
- Dockable panels: Inspector, Layers, Blocks, Sheets, Parameters; reset layout. `crates/drift-app/src/app/dock_ui.rs`
- Themes: light, dark, Midnight, Obsidian, follow system. `crates/drift-app/src/commands.rs`
- Preferences and drawing settings. `crates/drift-app/src/app/start_ui.rs`
- Update check. `crates/drift-app/src/app/update_ui.rs`
- Report a problem; show logs folder. `crates/drift-app/src/commands.rs`

## Automation and AI
- Standalone stdio MCP server. `apps/mcp/src/router.rs`
- Agent tools: files, geometry, relations, dimensions, fills, text, offset, fillet/chamfer, trim/extend/split, transform, measure, analyze, render, batch, undo/redo, import, export, layers, blocks (make, place, explode, block ↔ group). `crates/drift-app/src/agent/schema.rs`
- Render to PNG with ids and status colours. `crates/drift-app/src/agent/render.rs`
- Analyze: DOF, conflicts, open ends, closed areas. `crates/drift-app/src/agent/analyze.rs`
- Drift script (rooms, walls, openings, grids, arrays, relations, dimensions; one undo step). `crates/drift-app/src/agent/script/run.rs`
- AI link (Labs): an AI client drives the open window. `crates/drift-app/src/app/link_ui.rs`
- Blender add-on imports Drift profiles. `integrations/blender/drift_profiles` (unverified: behaviour)

## Not features (don't claim)
- Blocks in Drift script: the `blocks` tool makes, places and explodes them, the script has no words for them (#456).
- Script `room … walls` making real walls; it makes offset lines today (#314).
- Non-PDF-compatible `.ai` files.
- Web build (`apps/web`) is a build target, not a shipped product. (unverified)
- XREF palette alias target. (unverified)
