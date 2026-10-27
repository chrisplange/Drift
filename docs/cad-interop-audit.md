# DXF and DWG interop: audit

*2026-10-25, against `main` at 05a2fc5 (file format 42), acadrust 0.6.5.* Phase 1 of the
"DXF and DWG fully robust" work: what Drift reads and writes today, measured against real
files and against AutoCAD itself, and the gaps ranked by how badly and how often they hurt
architects. Nothing here is fixed yet; §6 groups the fixes into PR-sized batches to choose
from.

Walls (W7 in [walls-design.md](walls-design.md) §8) need DXF export of walls later. The export
plan in §5 keeps a slot for them. Illustrator and PDF import are out of scope.

---

## 0. Summary

**Import is solid at the core and leaky at the edges.** Every one of the 354 public sample files
imported without a panic, a hang or a time-out (slowest under 2 s). DWG goes through acadrust
and matches what ODA's converter makes of the same file in 134 of 165 readable files, and every
difference traced back to acadrust, not Drift. So a DWG comes in as well as its DXF would, and
nearly all the work is in the DXF reader. What goes wrong is in five places:

1. **Blocks.** A mirrored block (a door swung the other way) comes in **turned 180° instead of
   mirrored**, and a block stretched on one axis comes in stretched on both. AutoCAD, ODA and
   acadrust leave the Y scale out when it is 1, and Drift then uses the X scale for it. Also,
   "by block" colours are lost on block copies, and a second import reuses blocks of the same
   name from the first.
2. **Hatches.** Patterns are sized by Drift's drawing scale instead of the file's units, so the
   density is wrong (25 times too sparse in one inch test). Patterns Drift doesn't have by name
   (AR-CONC, AR-BRSTD, DOLMIT, ANGLE and every user pattern) become ANSI31, even though the file
   carries their definition. Hatches inside blocks are dropped: about 300 of 426 in the private
   sample.
3. **Annotation that vanishes.** MULTILEADER (callouts and notes, with their text), ACAD_TABLE
   (schedules), ARC_DIMENSION, TOLERANCE, WIPEOUT and MLINE are skipped and reported. LEADER
   loses its arrowhead. SOLID is read as an outline, so solid arrowheads, poché and
   north-arrow fills come in hollow.
4. **Text.** Fonts, width factors, obliquing and mirrored text are ignored. MTEXT inline
   formatting (sizes, colours, bold) is flattened. Code pages other than Western are misread.
5. **Paper space.** Title blocks and notes drawn on layouts are dropped (2,337 objects in one
   sample). Viewports keep their size and scale, but lose their twist and frozen layers.

**Export is the real problem.** **AutoCAD rejects Drift's DXF whenever the drawing has an
ellipse or any curve:** "Invalid or incomplete DXF input -- drawing discarded" (checked with
AutoCAD's own engine, from DWG TrueView 2024). Drift writes R12 (AC1009), and puts ELLIPSE and
SPLINE in it, which R12 doesn't have. Without them the file opens, but the round trip loses
nearly everything that makes it a drawing:

- fills become hundreds of SOLID triangles;
- hatches become loose line segments;
- dimensions become lines;
- colours are cut to 14;
- line weights are dropped;
- non-ASCII text becomes `?`.

The fixture drawing (9 curves, 2 fills, 3 dimensions, 1 text) comes back from its own DXF as
**1,159 curves and no fills**.

**DWG export: recommended, through acadrust, written as R2018.** acadrust wrote all 240
combinations of its sample entity × 8 DWG versions without an error, and AutoCAD's engine loaded
every 2D one in R2018 and R2000 (only ACIS solids failed, which Drift never writes). One mapping
from Drift to acadrust's document model would give both DWG and a modern DXF (§5).

---

## 1. How this was measured

All of it is reproducible on this machine (§7).

| What | Source | Used for |
|---|---|---|
| ezdxf test and example files (63) | github.com/mozman/ezdxf, `examples_dxf/`, `integration_tests/data/` | DXF R12–R2018, binary DXF, encodings |
| ACadSharp samples (47) | github.com/DomCR/ACadSharp, `samples/` | the same drawing in every version, DXF and DWG, ASCII and binary; dynamic blocks |
| LibreDWG test data (230) | github.com/LibreDWG/libredwg, `test/test-data/` (GPL: used as input only; no code looked at or copied) | DWG R1.4 to 2018, one file per entity type |
| acadrust fixtures (14) | github.com/hakanaktt/acadrust, `tests/` | R12, viewports, dynamic blocks |
| A synthetic matrix (30) | written with ezdxf 1.4.5 for this audit: one file per feature (curves, polylines, splines, hatches, solids, text, MTEXT, every dimension type, leaders, blocks, layers, paper space, units, all versions, binary, code pages, other entities), each also saved as DWG by ODA | each feature in isolation |
| One private client DWG (2018 format) | the user's sample folder, used locally only | a real architectural file: ~3,000 curves, 426 hatches, 186 dimensions, 39 multileaders, 3 layouts |

Tools:
- **`cad_probe`** (drift-app example): imports through the agent's `import_file`, the same code
  as File → Import. It reports, renders a PNG, exports DXF and imports that again.
- **`cad_stats`** (drift-io example): one JSON line per file with what the reader made of it.
- **ODA File Converter 26.10**: DWG → DXF as a second opinion on every DWG, and audited
  DXF → DWG.
- **ezdxf's drawing add-on**: the reference picture for each synthetic file. QCAD's trial
  `dwg2bmp` was tried and dropped, because it leaves pattern hatches out.
- **DWG TrueView 2024's `accoreconsole.exe`**: AutoCAD's own engine, headless. It is the
  arbiter of "does it open in AutoCAD".

---

## 2. Import: entity by entity

Severity: **Critical** (wrong drawing, silently), **High** (visible loss in most architects'
files), **Medium** (visible loss, sometimes), **Low** (rare or cosmetic). "Common" is a guess
at how often architects' AutoCAD files have it.

### 2.1 Geometry

| Entity | Today | Gap | Sev. | Common |
|---|---|---|---|---|
| LINE, CIRCLE, ARC, POINT | Exact, including extrusion (0,0,−1). | — | — | — |
| ELLIPSE (whole, partial, flipped) | Exact. | — | — | — |
| LWPOLYLINE with bulges | Exact (lines and arcs, ends joined). | — | — | — |
| LWPOLYLINE / POLYLINE **widths** (40/41/43) | The centreline only. | Wide polylines (borders, thick lines, tapered arrows) come in as hairlines. Fix: a closed outline plus a solid fill (constant width), or a tapered outline. | Medium | Common in older files |
| POLYLINE 2D/3D, curve- and spline-fit | Vertices on the curve are kept; frame points are dropped. | — | — | — |
| POLYLINE mesh / polyface | Skipped and reported. | 3D. | — | — |
| SPLINE with control points (any degree, rational, closed) | Degrees 1–3 exact; weighted and higher degrees fitted closely. | — | — | — |
| SPLINE **with fit points only** | Catmull-Rom through the points (tangent = neighbour difference / 6). End tangents (12/13) are ignored. | Differs visibly from AutoCAD: a closed four-point fit spline came out square where ezdxf, which follows AutoCAD's method, draws it round (the known gap). Fix: AutoCAD's global cubic interpolation, chord-length parameters, honouring the end tangents. | Medium | Common (SPLINE drawn by clicking) |
| SOLID, TRACE | **Outlines only.** | Solid arrowheads (inside every dimension block), poché, filled triangles and north arrows come in hollow. Fix: the outline plus a solid fill. | High | Very common |
| 3DFACE | Outline. | Fine for 2D. | — | — |
| XLINE, RAY | Skipped. | Usually construction. Could become construction lines clipped to the extents. | Low | Occasional |
| REGION, 3DSOLID, BODY, MESH, SURFACE | Skipped. | ACIS 3D; out of scope. | — | — |
| HELIX, LIGHT, SHAPE | Skipped. | Rare. SHAPE needs the .shx font. | Low | Rare |

### 2.2 Hatches

| Aspect | Today | Gap | Sev. | Common |
|---|---|---|---|---|
| Boundary: polyline, line, arc, ellipse, spline edges | Read. Spline edges with fit points use the same Catmull-Rom. | — | — | — |
| Holes (islands) | The outer loop bounds the fill, and inner loops are added as curves. Region detection then leaves the hole out (the synthetic test was correct). | — | — | — |
| **Pattern scale** | `Hatch::scale = 41` with `model: false`. Drift sizes non-model patterns by **its own drawing scale**, AutoCAD by the file's units. | Density follows the Drift document's drawing scale, not the file: ANSI31 × 16 in an inch drawing came out ~25× too sparse, and in the private sample large hatched areas show one stray diagonal line each. Fix: `model: true`, scale = 41 × the file's mm per unit ÷ the pattern library's unit (acad.pat is inches, acadiso.pat mm; `$MEASUREMENT` says which). | **Critical** | Very common |
| **Pattern shape** | Looked up by name among Drift's 19 built-ins; anything else becomes ANSI31, with a note. | AR-CONC, AR-SAND, AR-BRSTD, AR-HBONE, ANGLE, DOLMIT, EARTH, GRAVEL, HONEY, NET, and every office or user pattern (LEGNO2 and LEGNO3 in the sample) come in as diagonal lines. **The HATCH entity carries its own pattern definition** (groups 53/43/44/45/46/79/49), already scaled and rotated. Fix: build a document custom pattern from it, named after the file's pattern. Exact for every pattern, with no .pat file needed. | High | Very common |
| **Hatches inside blocks** | Dropped ("boundaries are still there" note). | About 300 of the private sample's 426 hatches: all the hatched parts of furniture, fixture and detail blocks. `drift_core::Block` already has `fills`. Fix: carry them into the block. | High | Very common |
| Solid fill colour and opacity | Colour kept. Opacity is Drift's default, so solids look washed out next to AutoCAD's. | Fix: opaque unless the file says otherwise. | Medium | Very common |
| Transparency (440) | Ignored. | Tinted room fills come in opaque. Fix: read 440 (and the layer's transparency XDATA). | Medium | Common in newer files |
| Gradient (450) | A solid in the layer colour, with a note. | Drift has no gradients. Use the gradient's first colour rather than the layer's. | Low | Occasional |
| Pattern origin (43/44 of the first line) | Ignored. | Bricks and tiles shift. Fix: set `Hatch::origin`. | Low | Common, but rarely noticed |
| Background colour (MPolygon / 63 fill colour) | Ignored. | `Hatch::background` exists. | Low | Occasional |

### 2.3 Text

| Aspect | Today | Gap | Sev. | Common |
|---|---|---|---|---|
| TEXT position, height, rotation, alignment | Right, including middle and top. Aligned and fit use their first point. | **Aligned and fit** don't stretch to their two points. | Low | Occasional |
| **Text styles** (the STYLE table: font, width factor, oblique) | Not read: everything is in the built-in sans, at width 1. | Office fonts (Arial Narrow, RomanS, ArchiText, Stylus) and condensed styles change the look and the length of every note. `TextFont::System(name)` exists. Fix: map the style's TTF family to `System` and SHX fonts to a close match. Apply the width factor and oblique angle if Drift text can hold them (it can't yet: a small core addition). | Medium | Very common |
| Backwards / upside-down (71) | Read the right way round. | Rare on purpose; usually a mirrored block. | Low | Rare |
| `%%c %%d %%p %%nnn`, `\U+XXXX` | Decoded. `%%u`/`%%o` dropped (no underline). | — | — | — |
| MTEXT wrapping, attachment, line spacing, rotation, columns | Wrapping, attachment, spacing and rotation right. Columns read as one. | — | Low | — |
| MTEXT inline formatting (`\H \C \f \L \O \K \Q \W \A`, stacks) | Stripped to plain text; a stack reads `1/2`. | One size, one colour, no bold or underline per paragraph. Fix: at least a whole-paragraph override (`{\H2x;...}` around everything), which is the common case. Mixed runs need rich text in Drift (L, later). | Medium | Common |
| MTEXT `\t`, `\p...;` and unknown codes | A code it doesn't know swallows everything up to the next `;`. | A literal `\t` swallowed the rest of a paragraph in the test. Fix: only the codes that take an argument read to `;`. | Low | Rare |
| MTEXT background mask (90/63) | Ignored. | Notes over hatches become unreadable. Fix: a paper-coloured mask fill behind the text, as Drift already does for its own notes. | Medium | Common |
| **Code pages** | UTF-8, else Latin-1 with five Windows-1252 characters patched. `$DWGCODEPAGE` and `\M+` are ignored. | R2007+ and DWG are fine. Pre-2007 DXF from Central Europe (1250), Cyrillic (1251), Greek, Turkish, Baltic, Japanese, Chinese or Korean comes in garbled, and Windows-1252 characters 0x80–0x9F (`Ž`, `•`, `™`, `‰`, `…`, `ž`) are dropped. Fix: a decoder per `$DWGCODEPAGE`, and `\M+n` double-byte escapes. | Low (US) / Medium (global) | Occasional |
| Agent `import_file` | Reads DXF with `from_utf8_lossy`, not `decode`. | The same files differ between the app and the MCP tools. | Low | — |

### 2.4 Dimensions, leaders, tables

| Entity | Today | Gap | Sev. | Common |
|---|---|---|---|---|
| DIMENSION (linear, aligned, angular, radial, diameter, ordinate) | Expanded from its `*D` block: it looks right but no longer measures (noted). | Making them live Drift dimensions is possible for linear, aligned, radial and angular (the definition points are in the entity). It is a design question (they would constrain the import), so it is a separate item. The SOLID arrowheads inside are hollow (§2.1). | Medium | Very common |
| DIMENSION without a block | Skipped ("no drawn geometry"). | Some writers don't make the block. Fix: draw it from the definition points with Drift's own dimension renderer. | Low | Occasional |
| **ARC_DIMENSION**, LARGE_RADIAL_DIMENSION | Skipped. | They have a block just like DIMENSION. Fix: read it the same way. | Medium | Occasional |
| LEADER | The line, without an arrowhead (noted). | Fix: draw the arrowhead from the dimension style (size × DIMSCALE). | Medium | Common |
| **MULTILEADER** | **Skipped**, together with its text. | 39 in the private sample: every callout and keynote is lost. Fix: leader lines, arrowhead, landing and the MTEXT (or block) content, from the entity's own context data. | **High** | Very common (2008+) |
| **ACAD_TABLE** | **Skipped**, together with its text. | Door, window and finish schedules are lost. Like DIMENSION it points at an anonymous `*T` block of its graphics. Fix: read that block. | High | Common |
| TOLERANCE | Skipped. | Engineering; rare in architecture. | Low | Rare |

### 2.5 Blocks

| Aspect | Today | Gap | Sev. | Common |
|---|---|---|---|---|
| **INSERT Y and Z scale (42/43) left out** | `sy = 42 or else sx`. | **The DXF default is 1.** AutoCAD, ODA and acadrust (so every DWG) leave 42 out when it is 1. So an insert scaled (−1, 1), a mirrored door or fixture, comes in as (−1, −1): **turned 180° instead of mirrored**, with its swing on the wrong side. A (2, 1) stretch comes in as (2, 2). Fix: default to 1. **S, and the most valuable fix in this audit.** | **Critical** | Very common |
| Uniform, rotated, mirrored-in-X inserts | Kept as Drift instances of one block. | — | — | — |
| Non-uniform, mirrored-in-Y, arrayed (MINSERT), extruded | Expanded into curves (correct, after the fix above). | — | — | — |
| Nested blocks | Expanded into the outer block (Drift blocks are flat). | — | — | — |
| **"By block" colour** on kept copies | Lost: items drawn in the copy's colour come in black. | Common for furniture, symbols and title-block parts placed on coloured layers. Fix: resolve at import. Make one Drift block per (block, copy colour) that a ByBlock item needs, or expand those copies. ByBlock line types and weights likewise. | High | Common |
| Layer of items on layers other than 0 | Kept for expanded copies. Lost in kept blocks, whose items have no layer. | Freezing a layer doesn't hide those parts. Needs a layer per `BlockItem` (core, format addition). | Low | Common, rarely noticed |
| Text and attributes inside blocks | Added beside each copy as loose text (noted). | They don't move with the copy. Needs text in blocks (core, M). Invisible attributes are correctly left out. | Medium | Very common (tags, door numbers) |
| **Block name clashes** | `doc.block_named(name)` is reused if the drawing already has a block of that name. | Importing a second file whose DOOR differs from the first's places the first's DOOR. Anonymous dynamic-block names (`*U12`) clash between almost any two files. Fix: reuse only when the contents match, else name it "DOOR (2)". | High | Common (several files into one drawing) |
| Dynamic block names | Blocks are named `*U12` etc. | Unreadable in the Blocks panel. Fix: the dynamic block's own name, from the block record's `AcDbBlockRepresentation` data. | Medium | Very common |
| XREF (external reference) | The block is empty, so nothing comes in. **No note.** | Fix: a note naming each xref and its path ("bind it in AutoCAD, or import it separately"). Reading the referenced file is possible later. | Medium | Common |
| IMAGE (raster underlay) | Skipped (reported). | Its file is external. Could become a Drift picture underlay when the image sits beside the drawing. | Low | Occasional |
| WIPEOUT | Skipped (reported). | Masks behind tags and keynotes; without them lines run through text. Fix: a paper-coloured mask fill (`Prim::Fill { mask }` exists for output). | Medium | Common |
| MLINE | Skipped (reported). | Older wall drawings use MLINE. Fix: its element lines from the MLINESTYLE offsets (W1.5's "make walls from lines" could then build walls from them). | Medium | Occasional |
| OLE2FRAME, proxies | Skipped (reported). | Nothing to draw. Proxy graphics could be read later. | Low | Occasional |

### 2.6 Layers, colours, line types

| Aspect | Today | Gap | Sev. | Common |
|---|---|---|---|---|
| Layer colour (ACI and true colour), off, frozen, locked, no-plot | All read. | — | — | — |
| Entity colour (ACI, true colour 420), ByLayer | Read. | Colour books (430) fall back to 420, which is right. | — | — |
| Line weights (370, layer and entity) | Nearest AutoCAD weight (`LineWeight::nearest_cad`, as for PDF and SVG). | — | — | — |
| **Line type scale** (`$LTSCALE`, `$CELTSCALE`, entity 48) | Ignored. | Dashes come out at Drift's density, not the drawing's: dense or near-continuous in inch drawings with LTSCALE 48. Fix: carry `$LTSCALE` into the document's line type scale, and entity 48 where Drift can hold it. | Medium | Very common |
| Line types beyond five | Matched to the nearest of Drift's five, with a note. The note also counts the standard ones every new drawing has (DASHED2, DOT, BORDER, DIVIDE), so it shows on almost every file. | DASHDOT, DOT, BORDER, DIVIDE and complex line types (text or shapes in them) are approximated. Real custom line types need core support (L, later). Fix now: don't count types no entity or layer uses. | Low | Very common (the note) |
| Entity visibility (60) | Ignored: invisible entities are drawn. | Fix: skip them. | Low | Rare |
| Defpoints | Printable unless the file marks it no-plot (not checked against a real AutoCAD file). | AutoCAD never plots Defpoints. Fix: always mark it no-print. | Low | Common |

### 2.7 Drawing, units, layouts, files

| Aspect | Today | Gap | Sev. | Common |
|---|---|---|---|---|
| `$INSUNITS` 1, 2, 4, 5, 6, 8–10, 14 | Right. | — | — | — |
| Unitless (0 or none) | The document's display unit. | Reasonable. `$MEASUREMENT` (0 → inches) would be a better guess. | Low | Common in R12 |
| Extents, far-off strays | A stray far away doesn't spoil the join tolerance. | — | — | — |
| **Paper-space content** | Dropped (counted in a note). | Title blocks, sheet notes, keys and details drawn on the layout: 2,337 objects in one sample, 9 in the private one. A Drift sheet has its own title block, but users lose their notes. Fix options (design): put a layout's paper-space drawing in a block placed on the sheet, or an underlay on the sheet. | Medium | Very common |
| Viewports: size, place, scale | Right. | — | — | — |
| Viewport twist (51), per-viewport frozen layers (341), clipped (non-rectangular) viewports | Ignored. | A rotated plan shows unrotated. Frozen-in-viewport layers show in every view. Fix: twist → a turned view, if Drift views can turn. | Low | Occasional |
| Versions R12 → R2018 (DXF) | All read. | — | — | — |
| **Binary DXF** | Refused: "save it as ASCII". | acadrust reads binary DXF; ezdxf, ACadSharp and some CAM tools write it. Fix: binary → acadrust → ASCII → the reader, as DWG already goes. | Low | Rare |
| DWG R13 → R2018 | Through acadrust. Matches ODA in 134 of 165 files (§3). | — | — | — |
| DWG R12 (AC1009) | One of 7 test files fails ("SEQEND names owner…"). | Upstream. | Low | Rare |
| DWG before R12 (AC1003–AC1006) | Refused with a clear message. | Fine. | — | — |

---

## 3. DWG through acadrust

Every readable DWG was imported twice: through Drift's path (acadrust → DXF text → reader) and
as ODA's DXF of the same file. Per file, I compared the counts of each curve kind, texts,
hatches, blocks, block contents, inserts, dimensions, layers, layouts and skipped entities.

- **134 of 165 identical.** In most of the rest, ODA leaves out ACIS solids and proxy entities,
  which Drift would skip anyway.
- **Real acadrust differences** (0.6.5):
  - ACadSharp's sample, every version: one line short, so the known dropped polyline vertex is
    still there. Two extra layers.
  - R2013 and R2018 samples: one spline span short; one extra text.
  - LibreDWG `Leader` (2010): one leader segment short.
  - LibreDWG `example_r13` (R13): a hatch missing, an extra block, and 1,319 block items
    against ODA's 22. R13 is the weakest version.
  - The private sample (2018): one extra line, otherwise identical.
- **acadrust has moved on:** 0.6.7 is out. Recent fixes include legacy DXF code pages,
  clockwise hatch arc angles and binary DXF integer widths, and the repository merges fixes
  daily. Upgrading and re-running this comparison is part of batch H.
- acadrust's DXF output also leaves 42/43 out when they are 1, which is why the block mirror
  bug (§2.5) hits **every** DWG.

---

## 4. Export today (DXF R12)

Export writes the **printed model** (`output::model_drawing`), not the document: everything has
already been turned into print primitives. Results, checked against AutoCAD's engine (TrueView
2024 `accoreconsole`), ODA, LibreCAD/QCAD (lenient) and Drift's own importer:

| What | Written as | Result | Sev. |
|---|---|---|---|
| **Ellipses, Bézier curves** | ELLIPSE and SPLINE in an AC1009 file | **AutoCAD discards the whole file** ("Undefined group code 10 for object… Invalid or incomplete DXF input -- drawing discarded"). With both removed by hand it opens. ODA, QCAD and LibreCAD accept it. Any pen-tool curve, rounded-rectangle-with-Bézier or ellipse in the drawing breaks the export. | **Critical** |
| Lines, arcs, circles, polylines | LINE, ARC, CIRCLE, POLYLINE | Fine. | — |
| Solid fills | Hundreds of SOLID triangles | Uneditable; seams show in some viewers; they read back into Drift as triangle outlines (the 9 → 1,159 curve round trip). | High |
| Hatches | Loose LINE segments | Can't be edited, re-scaled or re-patterned in CAD. | High |
| Stacked fills, Arrange order | File order | R12 has no draw order. AutoCAD draws in file order, but other programs choose. | Medium |
| Dimensions | Lines and text | Not DIMENSION entities, so they don't update in CAD. | High |
| Text | TEXT, one per line; italic as oblique | Fonts and styles lost. **Anything outside ASCII becomes `?`** except Ø ° ±: é, ü, ñ, smart quotes, en dash, ½, ², CJK. | High (global) / Medium (US) |
| Colours | The nearest of **14** ACI colours | A brand or material colour becomes the nearest primary. R12 has no true colour. | High |
| Line weights | Dropped (R12) | Every weight becomes default. | High |
| Line types | Five, with patterns | Fine; `$LTSCALE` written. | — |
| Layers | Names upper-cased, spaces and punctuation → `_` | "Walls - Existing" becomes `WALLS___EXISTING`. Off, frozen, locked and no-print are not written. | Medium |
| Blocks | BLOCK + INSERT (mirror as −X scale) | Good. Block names are sanitised like layers and can collide. | Low |
| Groups | Not written | Lost. | Medium |
| Sheets | Not written (model only) | Layouts and viewports lost. | Medium |
| Units | `$INSUNITS`, `$LUNITS`, `$MEASUREMENT` | Right. | — |
| Underlays, pictures | Not written | Lost. | Low |

There is no DWG export.

---

## 5. Export: what to build

### 5.1 DWG export: yes, via acadrust, R2018 first

**For.** It's already a dependency (the `dwg` feature is on by default in the desktop and web
builds), so native size barely changes. The wasm build already carries its reader (~0.9 MB
gzipped); the writer adds code, estimated at a few hundred KB more, to be measured in the PR.
- **Maturity, measured:**
  - acadrust's own all-entities example wrote 30 entity types × 8 versions (R13 to R2018), 240
    files, with no failures.
  - AutoCAD 2024's engine loaded every 2D one in **R2018 and R2000**: lines, arcs, ellipses,
    splines, polylines, solid and pattern hatches, text, MTEXT, inserts, dimensions, leaders,
    multileaders, MLINE, viewports. Only ACIS solids failed.
  - acadrust's **DXF** writer is a little behind its DWG writer. AutoCAD refused its DXF of
    DIMENSION ("Invalid dimension block name", probably a blockless test dimension),
    POLYLINE3D and SHAPE.
- **Licence:** MPL-2.0 is per file. Using it unmodified, in a closed-source app, needs only the
  notices kept, which Drift already does for reading (THIRD-PARTY-NOTICES, ACadSharp's MIT
  notice). Any fix we make to acadrust's own files must be published: send it upstream as a PR.
  This is general information, not legal advice.

**Against / risks.** It's one maintainer's crate, at 0.x and moving fast. So pin the version,
and keep a regression suite that writes files and checks them in AutoCAD's engine locally (CI
can't run TrueView). DWG is a closed format, so every corner acadrust gets wrong is ours to
find. Anything Drift writes must be a subset that has been proven to open.

**Version.** Write **R2018 (AC1032)** by default: AutoCAD 2018 and later, BricsCAD, DraftSight,
LT. Offer **R2010** and **R2000** as "Save for older CAD" (acadrust writes both, and R2000
loaded in the test).

### 5.2 One mapping, two formats

Rather than grow the R12 writer, map **the document** (not the print primitives) into
acadrust's `CadDocument`, then let acadrust write DWG, or DXF of the same version. One mapping,
two outputs, one set of tests. What maps where:

| Drift | DXF/DWG |
|---|---|
| Lines, arcs, circles, ellipses | LINE, ARC, CIRCLE, ELLIPSE |
| Bézier chains, paths | SPLINE (exact: degree 3, clamped) or, closed, LWPOLYLINE with bulges where all arcs and lines |
| Fills (solid, stacked) | HATCH SOLID on the fill's boundary loops (holes as inner loops), true colour, transparency |
| Hatches | HATCH with the pattern name **and its definition lines** (so any reader draws it exactly), scale and angle in drawing units |
| Text | MTEXT (wrapped, multi-line) or TEXT, with a STYLE per Drift font |
| Dimensions | DIMENSION entities with a DIMSTYLE from Drift's annotation settings, **plus** the anonymous `*D` block of their graphics (readers that don't regenerate need it, and AutoCAD refuses a dimension without one) |
| Blocks, copies | BLOCK + INSERT, including the block's fills as hatches |
| Groups | GROUP objects |
| Layers | LAYER with true colour, line type, line weight, off/frozen/locked/plot, names unchanged |
| Arrange order | entity order plus SORTENTSTABLE |
| Sheets | LAYOUTs with paper size and VIEWPORTs at their scale |
| Units | `$INSUNITS`, `$LUNITS`, `$LUPREC`, `$MEASUREMENT`, `$LTSCALE`, `$DIMSCALE` |

**Room for walls (W7).** Write the mapper as one function per kind of Drift object (curves,
fills, texts, dimensions, instances, …), each adding entities to the `CadDocument`. Walls
then add their own (closed LWPOLYLINE outlines, a HATCH poché per cluster, INSERTs for
openings, as walls-design.md §8 plans) without touching the others.

**R12 stays** as "DXF R12 (laser, CNC, old software)". That is batch A's fix: it then only
contains R12 entities.

**Keep the export pure.** The mapping lives in drift-io behind the `dwg` feature (every library
crate stays wasm32-compatible). The demo build keeps leaving the writers out.

---

## 6. Ranked gaps and the batches to fix them

**Status (2026-10-26):** batches **A** (#174), **B** (#175) and **C** (#177, #178) are merged; **I1** (DWG and DXF R2018 export, `drift-io::cad`) is done; the rest are open.

Ranked by severity × how common it is in architects' files. Size: **S** ≤ 1 day, **M** 2–4
days, **L** a week or more.

| Rank | Gap | Sev. × common | Fix | Size | Batch |
|---|---|---|---|---|---|
| 1 | Exported DXF with ellipses or curves is **discarded by AutoCAD** | Critical × most drawings | R12 output with only R12 entities: ellipses and Béziers as fine POLYLINEs with bulges or segments (within 0.01 mm); text as `\U+` escapes instead of `?` | S | **A** |
| 2 | Mirrored / one-axis-stretched blocks come in **turned or stretched wrong** | Critical × every file with a mirrored door | 42/43 default to 1 | S | **B** |
| 3 | Hatch density follows Drift's drawing scale, not the file | Critical × every hatched file | Model-space size from the file's units and pattern library | S | **C** |
| 4 | Unknown patterns become ANSI31 | High × very common | A custom pattern from the HATCH's own definition lines | S–M | **C** |
| 5 | Hatches inside blocks dropped | High × very common | Into `Block.fills` | M | **C** |
| 6 | SOLID / TRACE hollow (arrowheads, poché) | High × very common | Outline + solid fill (also inside blocks) | S | **C** |
| 7 | Export loses fills, hatches, dimensions, colours, weights, layers, text | High × every export | §5: document → acadrust → DXF R2018 / DWG R2018 | L | **I1, I2** |
| 8 | MULTILEADER vanishes with its text | High × very common (2008+) | Lines, arrowhead, landing, MTEXT/block content | M | **D** |
| 9 | ACAD_TABLE and ARC_DIMENSION vanish | High/Med × common | Read their anonymous blocks like DIMENSION | S | **D** |
| 10 | Block name clashes across imports | High × multi-file drawings | Reuse only on equal content; else "NAME (2)" | S | **B** |
| 11 | ByBlock colour lost on block copies | High × common | One Drift block per (block, copy colour) where needed | S–M | **B** |
| 12 | Leader arrowheads; WIPEOUT masks; MTEXT background masks | Medium × common | Arrowheads from DIMSTYLE; masks as paper-coloured fills | M | **D** |
| 13 | Dynamic blocks named `*U12`; XREFs vanish silently | Medium × very common | Real names; a note per xref | S | **B** |
| 14 | `$LTSCALE` / entity line type scale ignored | Medium × very common | Into the document's line type scale | S | **F** |
| 15 | Solid fills washed out; transparency (440) ignored | Medium × very common | Opaque by default; read 440 | S | **C** |
| 16 | Text styles (font, width factor, oblique) ignored | Medium × very common | STYLE table → `TextFont::System`; width and oblique need a small core field | M | **E** |
| 17 | Fit-point splines differ from AutoCAD | Medium × common | Global cubic interpolation, chord-length, end tangents | M | **F** |
| 18 | Polyline widths ignored | Medium × common in older files | Outline + fill | S–M | **F** |
| 19 | MTEXT whole-paragraph formatting flattened | Medium × common | Apply uniform `\H`, `\C`, `\f`, `\L` to the text | S–M | **E** |
| 20 | Paper-space title blocks and notes dropped | Medium × very common | Design needed (a block on the sheet?) | L | **G** |
| 21 | Text and attributes in blocks don't move with copies | Medium × very common | Text in Drift blocks (core) | M–L | **G** (or its own) |
| 22 | Code pages (1250/1251/…, 0x80–0x9F, `\M+`) | Low (US) × occasional | Decoder per `$DWGCODEPAGE` | S–M | **E** |
| 23 | MLINE skipped | Medium × occasional | Element lines from MLINESTYLE | M | **D** |
| 24 | acadrust 0.6.5 gaps (vertex, spline span, R13) | Low–Med × occasional | Upgrade to 0.6.7, re-run §3, report what's left upstream | S | **H** |
| 25 | Binary DXF refused | Low × rare | Through acadrust | S | **H** |
| 26 | Defpoints prints; invisible entities drawn; line type note on every file; `\t` swallows text; agent reads DXF lossy | Low | Small fixes | S | **H** |
| 27 | Viewport twist, frozen-in-viewport layers | Low × occasional | Turned views; per-view layer visibility | M | **G** |

### The batches (one PR each)

- **A. Exported DXF opens in AutoCAD** (S). Make R12 output R12-only. Replace the `?` fallback
  with `\U+` escapes. Add a test that every entity in R12 output is an R12 type, and a local
  `scripts/cad-check.ps1` that opens exports in TrueView's `accoreconsole`. *Ship first: it
  fixes the export every user hits today.*
- **B. Blocks come in right** (S–M). The 42/43 default (rank 2); block name clashes (10);
  ByBlock colour, line type and weight (11); dynamic block names; xref notes (13).
- **C. Hatches come in right** (M). Model-space size (3); patterns from the entity's own
  definition (4); hatches in blocks (5); filled SOLID/TRACE (6); opacity and transparency
  (15); gradient colour and pattern origin.
- **D. Annotation that vanishes** (M). MULTILEADER (8); ACAD_TABLE and ARC_DIMENSION through
  their blocks (9); LEADER arrowheads, WIPEOUT and MTEXT masks (12); MLINE (23).
- **E. Text** (M). STYLE fonts, width and oblique (16); MTEXT paragraph formatting (19); code
  pages (22).
- **F. Line types, widths, splines** (M). `$LTSCALE` (14); polyline widths (18); AutoCAD-exact
  fit-point splines (17).
- **G. Paper space and block text** (L, needs a short design note first). Layout content
  (20); text in blocks (21); viewport twist and layers (27).
- **H. Formats and small fixes** (S). acadrust 0.6.7 and the re-run (24); binary DXF (25); the
  small fixes (26).
- **I1. Modern export: DXF R2018 and DWG R2018 via acadrust** (L). The §5.2 mapping for curves,
  fills as HATCH, hatches with definitions, text with styles, blocks, layers, true colour,
  weights and units; File → Export DWG…; R12 kept as an option; version choice. W7 builds on
  this.
- **I2. Export, the rest** (M). DIMENSION entities with dimstyles and blocks, groups, draw
  order, sheets as layouts.

**Suggested order:** A and B first (small, and they fix wrong drawings), then C, then I1 (it
makes Drift's own files usable in AutoCAD), then D, E, F, H, I2, and G last.

Every batch adds:
- regression tests in `crates/drift-io/tests/` (synthetic files only, written by the test or
  small hand-written DXF; no private or downloaded samples in the repo);
- cases in `tests/import_robustness.rs` where they fit;
- smoke tests for import and export in `crates/drift-app/src/ui_tests/smoke/`;
- an update to `docs/regression-checklist.md`, including the manual "open the export in
  AutoCAD / TrueView" check.

---

## 7. Notes for whoever picks this up

- `crates/drift-io/examples/cad_stats.rs` prints one JSON line per file with what the reader
  made of it (`cargo run --release -p drift-io --features dwg --example cad_stats -- files…`).
  The other probes used for this audit (an app-level import/render/export probe, a DWG → DXF
  dumper, acadrust's all-entities generator) were kept out of the repo.
- `scripts/cad-check.ps1 files…` opens DXF and DWG files in AutoCAD's engine (needs AutoCAD or
  the free DWG TrueView) and says which it refuses, and why.
- TrueView's `accoreconsole` loads DWG and DXF and prints why it refuses one. It has no AUDIT
  or SAVE (it's a viewer). Run it from PowerShell with `-RedirectStandardOutput`; in Git Bash
  its output is lost.
- ODA File Converter: `ODAFileConverter.exe <in dir> <out dir> ACAD2018 DXF 0 1 "*.DWG"`.
- The corpora were cloned under the session scratchpad, not into the repo. LibreDWG's test data
  is GPL: input only.
