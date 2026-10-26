# Reading Adobe Illustrator `.ai` files: research spike

Status: research only, 2026-10-25. No production code. The probes were throwaway scripts kept outside the repo.

**Question.** Should Drift read `.ai` natively, with full text and layer support, instead of (or as well as) the current path? Today an `.ai` goes through its PDF-compatible part and the PDF importer (`crates/drift-io/src/pdf_import.rs`, `pdf_text.rs`, `pdf_image.rs`, `icc.rs`, `vector.rs`).

**Short answer.**
1. Fix the PDF path first. It's cheap, it fixes real losses on every `.ai` and PDF, and it needs nothing private (size **S–M**).
2. Then build a separate `.ai` reader crate in stages. Its first stage reads only the *structure* Drift can't get from the PDF: the layer tree with lock, print and colour; text stories (point, area, paragraphs, font, size, tracking); artboard names; and CADtools' drawing scale (**M**).
   - Native geometry (sublayers, groups) is a second stage (**L**).
   - Turning CADtools dimensions into live Drift dimensions is a third (**M**), and is the strongest competitive hook.
3. Don't write `.ai`.

Details, evidence and costs follow.

---

## 1. What's in an `.ai` file

### 1.1 Modern (Illustrator 9 to 2026): a PDF with Illustrator's own data inside

I made test files with the installed Illustrator 2026 (30.8) by script and saved them as CC, CC uncompressed, CC with "Create PDF Compatible File" off, CS6, CS, 10, 8 and 3. I also inspected one real client file (a 2026 save, version 30.4). Every modern `.ai` has this structure:

```
%PDF-1.6                                  ← an ordinary PDF file
Catalog
  /OCProperties  /OCGs [...]  /D << /ON [...] /OFF [...] /Order [...] >>   ← top-level layers only
Page (one per artboard)
  /Contents  → the PDF-compatible drawing (what Acrobat, Inkscape, Affinity and Drift see)
  /PieceInfo << /Illustrator << /LastModified (...) /Private N 0 R >> >>   ← ISO 32000 §14.5 page-piece dictionary
Private dict
  /AIMetaData  M 0 R
  /AIPrivateData1 … /AIPrivateDataN       ← the native document, split into 64 KB streams
  /ContainerVersion 12  /CreatorVersion 30  /RoundtripVersion 24  /RoundtripStreamType 2  /NumBlock N
```

**The private payload is the native document.** Join the blocks in order, then decode them. The encoding depends on the version that wrote the file:

| Saved as | `ContainerVersion` / `RoundtripVersion` | Payload encoding |
|---|---|---|
| Illustrator 2020+ (CC "24") | 12 / 24 | `%AI24_ZStandard_Data` then one **zstd** frame |
| CS2–CS6 / CC ≤ 2019 | 11 / 16 | plain-text header and thumbnail, then `%AI12_CompressedData` and **zlib** |
| CS (11), 10, 9 | 9 / 10–11 | a plain-text first block, then each later block **zlib-compressed on its own** |
| "compressed" off | 12 / 24 | plain text |
| 8 and earlier | (no PDF) | the whole file *is* the native stream (`%!PS-Adobe-3.0`) |

**Decoded, every version is the same thing:** the PostScript-based Illustrator format of 1998. It starts with `%AI5_FileFormat 14.0` (2026), `12.0` (CS6), `7.0` (CS) or `6.0` (10), and it's been extended release by release ever since.
- **Layers:** `%AI5_BeginLayer` / `Lb` / `(name) Ln` … `LB` / `%AI5_EndLayer--`. Sublayers nest. `Lb` carries visible, preview, enabled (unlocked), printing, dimmed, colour index and an RGB layer colour.
- **Groups, compound paths, clips:** `u`/`U` are groups and `*u`/`*U` compound paths; clip groups are marked too.
- **Paths and paint:** paths use `m L c v y`; paint uses `k K x X Xa XA` (CMYK, spot or RGB); strokes use `w d j J M`.
- **Other objects:** rasters (`%AI5_BeginRaster` … `XI`), gradients (`%AI5_BeginGradient`, `Bb`/`Bh`/`BB`), symbols (`%AI14_BeginSymbol`), brushes, patterns, art styles (appearance), and plugin objects (`%AI8_BeginPluginObject`) for live effects and similar.
- **Per-object dictionaries:** lines starting `%_` hold `ArtDictionary` / `Dictionary` / `XMLUID` (object names, UUIDs, plugin data). Some keys are **ROT13-scrambled**, and third-party plugins store their own data here.
- **Artboards:** `ArtboardArray` in the document data, with names and UUIDs.

**Text since CS (Illustrator 11)** is not in PostScript form. It sits in one `%AI11_BeginTextDocument` block, `/AI11TextDocument : /ASCII85Decode`. Each text object is a `/AI11Text` with a `/StoryIndex` and `/FrameIndex` into that block.
- The decoded block is the Adobe Text Engine (ATE) dictionary: PostScript-like `<< >>`, `[ ]`, `/names` and UTF-16BE `( )` strings. It's the same grammar as Photoshop's text-layer EngineData.
- **In current files the keys are numbers** (`/0`, `/1`, `/99`…), not names like `/ParagraphSheet`.
- From my test file, a character run holds `/0` font index, `/1 14.0` size and `/8 200` tracking. Stories hold the plain text with `\r` between paragraphs.
- Each story also carries Illustrator's own *line layout* (glyph runs with positions), so lines can be placed exactly as Illustrator set them.
- **Text before CS (8, 10, 3)** is the documented legacy form: `To`/`TP`/`Tx`/`Tk`, one escaped string per run.

**With "Create PDF Compatible File" off,** the file is still a PDF wrapper with the same private data, but the page holds only a placeholder. Drift already detects this and says so.

**With CS or 10 compatibility,** the PDF part is PDF 1.4 with **no layers at all** (no OCGs), and in my test it held only one of the two artboards. The private data is complete.

### 1.2 Legacy (Illustrator 3–8)

These files are pure PostScript/EPS text: the same operators as above, a `%%Creator: Adobe Illustrator(R) 8.0` header and no PDF. Drift today rejects them ("not a readable PDF"). This dialect is the subject of Adobe's *Illustrator File Format Specification* (version 7, 1998), which is the risky document; see §5. Very few users have such files now. They would come almost free with a native reader, because it's the same dialect without the compression or ATE text.

### 1.3 The private sample (described abstractly)

The sample is a millwork plan and elevation for a client, saved by Illustrator 2026 (30.4). It is one artboard (about 13 × 19 in), roughly 100 paths and 20 text objects (titles, multi-line notes and dimension values) on **two top-level layers, one of which CADtools made for its dimensions**. It has no sublayers, and it was saved with PDF compatibility on.

Its native data also holds things the PDF part doesn't:
- **CADtools (Hot Door) data:** about 24 associative dimensions, each with its source object id, segment, offset and original numeric value. CADtools stores them as ROT13-keyed JSON in the art dictionaries.
- **The CADtools layer scale** ("1.5 in = 1 ft", i.e. 1:8).
- The layer colours.
- **Paragraph structure:** for example, one note with three lines is one story.

---

## 2. What Drift loses today

Method: I ran `drift_io::pdf_import::read_artwork` from a throwaway binary on the sample, its reference PDF and the test files, and compared the result with what is in the native data.

**For the private sample, the `.ai` and its reference PDF give identical results:** 99 paths, 23 texts, layers `CADtools Dimensions` / `Layer 1`. So Drift gains nothing today from the file being an `.ai`.

| Feature | Native data has | Drift gets today (PDF path) |
|---|---|---|
| **Top-level layers: names, visibility** | yes | ✅ yes (from the OCGs) |
| **Layer order** | yes | ❌ **alphabetical**: `layer_states` returns a `BTreeMap` and `/D /Order` is ignored. The test file came in `Base, Guides, Notes, Walls`; Illustrator's order is `Guides, Notes, Walls, Base`. This is a bug. **Fixed in the working tree after this report:** layers now come in the order the page draws them. |
| **Layer lock, non-printing, colour** | yes (`Lb`) | ❌ Illustrator writes none of these into the PDF part. A locked layer came in unlocked, and a hidden non-printing layer came in printable. |
| **Sublayers (any depth)** | yes | ❌ merged into their top-level layer (only top-level layers become OCGs; there is no nested marked content) |
| **Groups, named objects** | yes (`u`/`U`, names, UUIDs) | ❌ flat paths |
| **Compound paths** | yes | ✅ as subpaths of one painted path |
| **Clipping masks** | yes | ✅ applied, so paths come in trimmed; the mask itself is lost |
| **Dashed strokes** | yes | ✅ |
| **Gradients** | yes | ❌ skipped (Drift has no gradient fill) |
| **Spot colours** | name and tint | ⚠️ the alternate colour only; the spot name is lost |
| **Symbols** | definitions and instances | ⚠️ each instance comes in expanded as plain paths |
| **Appearance / stacked fills, live effects, brushes** | the source objects and effect parameters | ⚠️ the expanded result only (often fine to look at, but not editable) |
| **Embedded and linked images** | yes, with links | ✅ both come in as underlay pictures (the PDF part embeds linked files); the link is lost |
| **Artboards** | all, with names | ⚠️ one PDF page per artboard and one page per import; names lost. CS/10 saves: one artboard only. |
| **Point text: string, size, angle** | yes | ✅ |
| **Font family** | yes | ❌ not read (`Text` has no font, although `SketchText` has `TextFont::System`) |
| **Tracking** | yes | ❌ (in the PDF as `Tc`, but Drift's text has no tracking) |
| **Ligatures** | real text | ❌ **"fi" dropped**: "Area text first" came in as "Area text rst". This is a bug in mapping glyph codes to text. **Fixed in the working tree after this report:** ligature glyph names such as `f_i` and `ffl` now read as their letters. |
| **Paragraph or area text** | one story with `\r` paragraphs, the frame, wrap width and alignment | ❌ one Drift text per *line*: the sample's multi-line notes become 2–3 separate texts. Wrap width and alignment are lost. |
| **Text on a path** | one story with its path | ❌ one text per letter (23 one-letter texts in the test) |
| **CADtools dimensions** | measured values, sources, offsets | ❌ plain lines and number texts, and no drawing scale |
| **Illustrator 3–8 files** | yes | ❌ refused |
| **"PDF compatible" off** | yes | ❌ refused (Drift explains why) |

---

## 3. How much an improved PDF path recovers

Everything here uses only the PDF part, so it's standard ISO 32000 with no private data and no legal question.

| Fix | Size | Recovers |
|---|---|---|
| Layer order from `/OCProperties /D /Order` | **S** (hours) | the file's layer order (the bug above) |
| Glyph-name fallback (`/Differences` and the Adobe Glyph List; `fi` → "fi") when `ToUnicode` has no entry | **S** | dropped ligatures and other unmapped glyphs |
| Font family from `BaseFont` (strip the `ABCDEF+` subset tag; PostScript name → family, e.g. `ArialMT` → Arial), into `TextFont::System` | **S** | fonts, where installed |
| Join the lines of one `BT…ET` block that step by `Td`/`T*` at a steady leading into **one multi-line text** with `spacing` set; guess alignment from the line starts | **S–M** | most paragraph and area text, as one object per paragraph block; alignment approximately; no wrap width |
| Join one-glyph runs on a curve into one text (or a group of letters) | **S** | text on a path becomes readable (Drift has no text on a path, so it can't come in as that) |
| `/ActualText` and marked-content `/Span` | **S** | little for `.ai`: Illustrator didn't write either in any test file. Worth it for other PDFs. |
| Spot name from `Separation` | S | only once Drift has spot colours |

**Total: about S–M, roughly 1–1.5 weeks with tests.**

**What it covers.** For the user's "full text and layers" need, this probably gets **60–70% of what Drift's model can hold**:
- layer names, order and visibility;
- complete text with font, size and angle, grouped per paragraph block.

**What it can't cover**, because the PDF part doesn't contain it:
- sublayers;
- lock, print and colour layer states;
- point vs area vs path text, wrap width and true alignment;
- groups and object names;
- artboard names;
- CADtools semantics;
- legacy files and files saved without PDF compatibility.

**Drift's own model caps what any reader can deliver:**
- layers are flat (no nesting) and groups are one level deep;
- `SketchText` is one style per text (font, cap height, angle, alignment, line spacing, wrap width), with no per-run styles, no tracking and no text on a path.

So even a perfect native reader would flatten sublayers (for example to `Walls / Doors`) and would simplify rich text.

---

## 4. Native `AIPrivateData` parsing

### 4.1 Feasibility, by layer of the format

| Piece | Difficulty | Notes |
|---|---|---|
| Container (PieceInfo, blocks, zlib/zstd/per-block zlib) | easy | Verified on all eight test files and the sample. Needs a zstd **decoder**: `ruzstd` is pure Rust, keeps the wasm build and is MIT-licensed (check before adding). `zstd`/`zstd-sys` is C. |
| PostScript tokenizer and the layer tree (`Lb`/`Ln`, nesting, flags, colour) | easy | Seen working in every version from 8 to 2026 with the same `Lb` layout |
| Document data: artboards (names, rectangles), ruler units, `%AI24_LargeCanvasScale` | easy–medium | Coordinates are points with y up; the page origin comes from `%%PageOrigin` and the artboard. Large-canvas files carry a scale factor. |
| Paths, compound paths, groups, clip groups, colours and strokes | medium | The oldest and most stable part. **The PDF part of the same file is a built-in test oracle**: native geometry must match the PDF page's geometry. |
| Images, gradients, symbols | medium | Symbols need definition lookup and transforms |
| Text stories (ATE, numeric keys) | **hard** | The grammar is easy (like psd-tools' EngineData parser). The *meaning* of the numeric keys has to be learned by observation: make files that differ in one attribute, then diff. Illustrator's stored line layout makes placement exact. |
| Appearance, live effects, brushes, meshes, plugin objects | very hard, open-ended | Source parameters only, so you'd have to re-run Adobe's effects. **Use the PDF geometry for these.** |
| CADtools data | medium | Third-party JSON with ROT13-scrambled keys; it's tied to CADtools versions (8.0.0 in the sample) |

### 4.2 Prior art (from the research agent's survey; links in its notes)

- **Inkscape:** opens `.ai` through its PDF (Poppler) import, so it sees the PDF part only.
  - A newer native reader, **inkai / extension-ai** (GitLab `inkscape/extras/extension-ai`, **GPL-2.0+**, 2023, funded by the Software Freedom Conservancy and Inkscape), parses the private data. Its `docs/specification_amendments/` describe the compression chain (including AI24/zstd) and the ATE text document, including how to learn real key names by copying text as "Adobe Text Engine 2.0".
  - **GPL: read it for understanding, never copy code or prose.** Facts aren't copyrightable, but under clean-room practice even reading it is a choice to make deliberately (§5).
- **Scribus `importai`** (GPL): pulls `AIPrivateData` out of the PDF and inflates `%AI12_CompressedData`, then runs its legacy-AI parser. I saw no AI24/zstd support.
- **psd-tools** (MIT): an EngineData tokenizer for the same ATE grammar with named keys. **parse-engine-data** (npm, MIT) is similar.
- **LibreOffice:** no native AI reader; PDF-based `.ai` goes through PDF import (layers ignored), and old ones through EPS.
- **Affinity:** reads the PDF part (a blank document when there is none), according to forum statements; not verified in Serif's documentation.
- **CorelDRAW:** official AI import up to CS6; later files are read through the PDF part.
- **Ghostscript** (AGPL): the PDF part only.
- **Nobody ships a permissive-licence (MIT/Apache) `.ai` reader that I found.** A clean one would be a first.

### 4.3 Stability across versions

- **The PostScript dialect** has been stable since 1998 and is only ever extended. `Lb` and paths in a 2026 file read like the 1998 ones.
- **The container** changed twice (per-block zlib, then whole-stream zlib in CS2+, then zstd in 2020), and each change is marked by its header.
- **The text format** changed once in a big way (CS, 2003), and the keys became numeric at some later point. These are the parts most likely to move again.

**Maintenance risk: low–medium for structure and geometry; medium for text.** Expect a few days of work for each Illustrator major release that changes something. A test corpus made by script from Illustrator catches it.

### 4.4 Testability

Testability is good, because Illustrator 2026 is installed:
- An ExtendScript (`DoJavaScript` over COM) builds the same document with every feature and saves it in every compatibility mode. I did this for the spike; it took about a minute and touched none of the user's open documents.
- These generated files are ours and can be committed as fixtures. Client files never.
- Every PDF-compatible file carries its own oracle: the PDF page. Golden tests can compare native geometry with PDF geometry, and native text with PDF text runs.

### 4.5 Effort

| Stage | Size | Contents |
|---|---|---|
| **N0 container and tokenizer** | S (2–3 days) | the private data from any `.ai`, all versions; a PostScript and `%_` dictionary tokenizer; fuzzed (same rules as `tests/import_robustness.rs`) |
| **N1 structure sidecar** | M (1.5–2 weeks) | **N1 itself:** the layer tree with lock, print and colour; artboards with names; document units; CADtools layer scale (into "drawn at 1:n"); ATE stories (text, paragraphs, frame type, area wrap width, alignment, font, size, tracking) |
| | | **Merge with PDF geometry:** stories replace the PDF text runs they match by position; layer states apply to the top-level layers; sublayer names are lost until N2. |
| **N2 native geometry** | L (3–5 weeks) | paths, compounds, groups, clips, images, symbols, and sublayers mapped to Drift layers (`Parent / Child`), with groups and names. Falls back to PDF geometry per top-level layer for objects it can't read (effects, meshes, brushes). Reads Illustrator 3–8 and no-PDF files. |
| **N3 CADtools dimensions** | M (1–2 weeks) | CADtools linear, angular and radial dimensions become live Drift dimensions on the matching curves; the scale is set from the document. |
| Full parity with Illustrator | XL, never-ending | not a goal |

---

## 5. Legal position: general information, not legal advice

This is a general summary for orientation only, written by an AI, **not legal advice**. Drift is going closed-source and paid, so **have the user's lawyer friend review it** before a native reader ships. The most relevant law is US (the user is in the USA); the EU is noted because Drift will sell there.

- **File formats themselves are generally not protected by copyright.** In the EU, *SAS Institute v World Programming* (CJEU C-406/10, 2012) held that a program's functionality and its data file formats aren't protected expression. A lawful user may observe, study and test a program (Software Directive 2009/24/EC, Art. 5(3)), and contract terms that forbid this are void (Art. 8). Art. 6 allows even decompilation where it's indispensable for interoperability. In the US, *Sega v. Accolade* (1992) and *Sony v. Connectix* (2000) found reverse engineering for interoperability to be fair use; *Lotus v. Borland* and *Google v. Oracle* point the same way for functional interfaces.
- **DMCA §1201(f)** allows getting around access controls for the interoperability of an independently made program. Zlib, zstd, ASCII85 and ROT13 are encodings, and arguably not "technological measures" at all.
- **In the US the real risk is contract, not copyright.** *Bowers v. Baystate* (Fed. Cir. 2003) enforced a shrink-wrap ban on reverse engineering. So:
  - **Don't download or accept the Illustrator SDK**, and keep anyone who has accepted it out of the reader work. Related Adobe SDK licences ban reverse engineering and treat format documents as licensed material. I couldn't get the current Illustrator SDK wording.
  - **Avoid relying on the 1998 *Illustrator File Format Specification*.** Unofficial mirrors exist, but its own notice calls the content proprietary trade secrets "made available only to Adobe licensees". After 25+ years of publication that is weak, but avoiding it costs little, because everything needed can be observed in our own files.
  - The user's Creative Cloud terms forbid reverse engineering *the software*. Making files with Illustrator and reading those output files is ordinary use, not reverse engineering the program, but this is a point for the lawyer.
- **Clean-room practice:**
  - Work from observation of files we made ourselves.
  - Write down findings as facts in our own words.
  - Don't copy GPL code or prose (inkai, Scribus). Reading them is allowed, but it's cleaner to keep notes on facts only and record where each fact came from.
  - Keep a short provenance log in the crate.
- **Precedent:** I found no case of Adobe suing a third-party `.ai` or `.psd` reader. GIMP, Krita, Affinity, Photopea, Inkscape, Scribus and CorelDRAW all read them; a search can't prove a negative. The Autodesk–Open Design Alliance disputes over DWG were about **trademarks and branding** ("TrustedDWG", the DWG mark), not about reading the format. So:
  - Say "opens Illustrator (.ai) files".
  - Never imply that Adobe endorses Drift.
  - Use no Adobe logos.
- **CADtools data** belongs to Hot Door, a small vendor whose users are exactly Drift's target. The same interoperability reasoning applies. The business relationship risk (they may not like "CADtools plugin murderer" marketing that reads their data) is separate from the legal one.

---

## 6. Recommendation

### Options compared

| Option | Value | Cost | Risk |
|---|---|---|---|
| Do nothing | none; Drift drops ligatures and scrambles layer order today | 0 | annoys exactly the Illustrator converts Drift wants |
| **Improve the PDF path** | most text and layer value; helps every PDF too | **S–M** | none |
| Native parsing inside drift-io | everything, tied to Drift | M → L | more code in drift-io; closed, so no outside help |
| **Separate `.ai` reader crate** | the same, reusable and testable alone | M → L | as above, plus a choice about publishing |

### The recommendation

1. **Now: improve the PDF path (S–M, about 1–1.5 weeks).** Layer `/Order`, the glyph-name fallback for ligatures, font family, joining paragraph lines into one multi-line text, and joining path-text letters.
   - The two bugs (layer order and dropped ligatures) are worth fixing even alone (**S**, about a day with tests).
2. **Next: a separate reader crate, `ai-reader`, at stage N1 (M, about 2 weeks).**
   - Its own repository and API, with no Drift types. It produces a neutral document model (layer tree, artboards, stories, plugin dictionaries). drift-io does the mapping and the merge with PDF geometry.
   - Clean-room as in §5, with a corpus made by script from Illustrator.
   - **Why separate:** it's testable and fuzzable alone, it keeps Drift's closed codebase lean, and the boundary keeps provenance clear.
   - **Licence:** start it **private, under MIT/Apache, and decide on publishing after N1.**
     - *For publishing:* goodwill, outside bug reports from many Illustrator versions, and being the first permissive reader.
     - *Against:* it hands the same capability to Canvas X Draw, Affinity and others, which weakens a selling point. A public reader is also more visible to Adobe (low risk, per §5).
     - Since Drift is going closed for commercial reasons, my lean is to **keep it private until there's a reason to publish**.
3. **Then N3, CADtools dimensions into live Drift dimensions (M).** This is the single most persuasive demo for the "replaces CADTools" pitch: open the Illustrator drawing and the dimensions are *live*, so changing the dimension changes the drawing. Do it before N2.
4. **N2, native geometry and sublayers (L), only when users ask for it.** Mapping sublayers and groups needs the full geometry parse; the PDF path already gives the right geometry.

**Competitive value.** Architects and designers leaving Illustrator judge a tool by whether their old files open cleanly:
- Clean text (no "rst" for "first"), layers in the right order with the right lock and print states, and notes that stay paragraphs are table stakes.
- Live CADtools dimensions are something no competitor offers: Affinity, Inkscape, LibreOffice and CorelDRAW all read only the PDF part.

### Writing `.ai`: not worth it

- Illustrator already opens Drift's PDF and SVG exports well (PDF with layers).
- A native `.ai` writer would have to emit the PostScript dialect *and* an ATE text document with numeric keys that Illustrator accepts. That is unforgiving, untestable without Illustrator in the loop, and breaks when Adobe changes anything.
- The only cheap version is a PDF saved with an `.ai` extension. Illustrator opens it as a PDF, so it's cosmetic and slightly misleading.
- If users ask for "export for Illustrator", label the PDF export that way.

---

## Appendix: how the spike was done (reproducible)

All of this was outside the repo, in the session scratchpad:
- A Python reader for PDF objects and the private blocks, with a ROT13 decode and an ATE string dump.
- A Rust binary with `zstd` to decode AI24 payloads.
- A Rust binary that calls `drift_io::pdf_import::read_artwork` on each file.
- An ExtendScript run through Illustrator's COM `DoJavaScript`. It builds one document containing:
  - a sublayer and a sub-sublayer;
  - locked, hidden and non-printing layers;
  - a compound path, a clip group, a dashed line in a spot colour and a gradient;
  - point text (Arial 14, tracking 200), two-paragraph centred area text, text on a path and rotated text;
  - a symbol with two instances, a linked image and an embedded image;
  - two named artboards.

  It saves the document as CC, CC uncompressed, CC without PDF content, CS6, CS, 10, 8 and 3, plus a reference PDF.

The real client sample was used only locally. It must not be committed or described identifiably in issues or PRs.
