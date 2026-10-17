# Sheets and output

**What you'll make:** an A3 sheet of the example plate with a title block, a second view
showing a 2:1 detail, and a PDF of it.

![An A3 sheet of the plate with the modern title block, and the Sheets panel](../images/sheet.png)

Time: about 15 minutes.

## The idea

You draw in **model space**, at full size. A **sheet** is a piece of paper with a title
block. It holds one or more **views**, each a window onto the model at its own scale. What
you see in the sheet preview is exactly what prints.

## 1. Make a sheet

1. Open the example (**File → Open example**). Hatch the plate if you like (see
   [Fills and hatches](06-fills-and-hatches.md)).
2. Open the **Sheets** panel (the page icon among the panel tabs).
3. Click **New sheet**.

Drift makes an A3 landscape sheet with the **Modern** title block and one view that fills
the drawing area. It picks the largest standard scale at which the drawing, dimensions
included, clears the title block, and switches the canvas to the sheet.

The canvas is now the sheet editor. Scroll to zoom, drag with the middle or right button to
pan, and press Shift+F to fit the sheet. Esc, **Back to model**, or **View → Sheet**
returns to the model. **Show sheet** in the panel, or **View → Sheet**, brings the sheet
back.

## 2. Set up the paper and title block

In the **Sheets** panel:

- **Paper:** **Name**, **Size** (A4 to A0, ANSI A to E) and **Landscape** or **Portrait**.
- **Colours:** how the sheet prints (see [Colour, greyscale or black & white](#colour-greyscale-or-black--white)).
- **Title block:** **Modern**, **ISO 7200**, **ASME Y14.1** or **None**. Then the fields:
  title, number, drawn by, checked, date (**Today** fills it in), company and material.
  The scale, the sheet number ("1 / 2") and the revision fill themselves in.
- **Logo:** **Choose SVG…** puts a logo in the title block. It's kept in the drawing.
- **Revisions:** **Add revision** adds a row with the next letter, today's date and the
  author. The latest revision shows in the title block.

## 3. Views and scales

Pick a view by clicking it on the sheet, or in the **Views** list. For the picked view:

- **Title:** its name. Tick **Title** under **Show** to print the name, with the scale
  beneath it, under the view.
- **Scale:** choose from the menu, or click **Fit** for the largest scale that fits.
  Metric scales run from 10:1 to 1:10000. Architectural scales (3" = 1'-0" down to
  1/32" = 1'-0") and engineering scales (1" = 10' to 1" = 200') are offered too, with your
  drawing's units first.
- **Frame:** **Fills sheet** (the whole drawing area) or **Framed** (a rectangle of its
  own on the paper).
- **Show:** **Dimensions**, **Notes**, **Title** and **Border**, for this view only.

If the views on a sheet have different scales, the title block's scale reads **As shown**.

## 4. Add a detail view

1. Click **Add view**. Drift adds a framed view beside the others: of what's picked in the
   model, or of the whole drawing if nothing is.
2. Give it the title `Detail A` and the scale **2:1**.
3. On the sheet:
   - **Drag the view** to move it on the paper.
   - **Drag its handles** to crop it. The drawing stays where it is on the paper.
   - **Alt-drag** inside it to slide the drawing within the frame.

![A sheet with the main view and a framed 2:1 detail of the bore](../images/sheet-views.png)

Views snap as you drag them: their edges and middles line up with other frames and the
drawing area, and drawings at the same scale line up level or in line with each other.
Dashed guides show what lined up. Hold Ctrl to place freely.

**Align with** lines the picked view up with another one: **Left**, **Centre**, **Right**,
**Top**, **Middle**, **Bottom**, **Drawings level** or **Drawings in line**.

Delete removes the picked view. A sheet always keeps at least one.

## 5. Output

From the **Sheets** panel's **Output** section, or **File → Export**:

| Output | What you get |
|---|---|
| **PDF…** | Every sheet, one page each, at true scale and true line weights. Without any sheets, the drawing fitted to A3. Text stays selectable. |
| **SVG…** | The current sheet, at true scale, in millimetres. |
| **DXF…** | The model at full size (not the sheets), with its layers and line types, in the drawing's units. |
| **Print…** (Ctrl+P) | Writes the PDF and opens it in your system's PDF viewer (a new tab in the browser), to print from there. |

Exporting never replaces your `.drift` file.

### Colour, greyscale or black & white

Under **Paper**, each sheet has its own colour choice. The sheet preview shows it, and the
PDF, SVG and print come out the same:

| Choice | On paper |
|---|---|
| **Colour** | Layer and item colours, as drawn. |
| **Greyscale** | Every colour becomes its grey (by how light it is). Light fills stay light and dark ones dark. |
| **Black & white** | Like a CAD monochrome plot style. Lines, text and hatch lines print black. Solid fills print as their grey, so a dark fill doesn't hide the lines on it. White stays white: the masks behind dimension text, and lettering knocked out of a dark box (the Modern title block's sheet number). |

A new sheet starts in **Colour**. The choice is saved with the sheet. It doesn't change DXF,
which is the model, not a sheet: DXF keeps the layer and item colours.

What goes on paper:

- Construction geometry, points, hidden layers and layers whose print flag is off are left
  out.
- Dimensions and notes are left out where a view's **Show** says so, or where they're set
  off sheets (see [Annotation](09-annotation.md)).
- Drafting hatch patterns are sized for the sheet's scale.

About DXF:

- It's written as DXF R12, which every CAD program reads. Ellipses and pen curves go out
  exactly, as ELLIPSE and SPLINE entities. Those are newer than R12: most programs read
  them, but a strict R12 reader may skip them.
- Blocks go out as blocks (BLOCK and INSERT). Dimensions and notes go out as plain lines and
  text on a layer called **ANNOTATIONS**, so they no longer measure. Fills go out as solid
  triangles.
- To bring DXF in, see [Underlays and DXF import](11-underlays-and-dxf.md).

## What you learned

- **New sheet** lays the drawing out on paper with a title block, at a fitting scale.
- A sheet holds several views, each with its own scale and frame.
- PDF prints every sheet; SVG the current one; DXF the model at full size.
- Each sheet prints in colour, greyscale or black & white, as its preview shows.

## Next

[Underlays and DXF import](11-underlays-and-dxf.md).
