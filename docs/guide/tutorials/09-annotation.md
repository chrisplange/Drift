# Annotation

**What you'll make:** the example plate, annotated for the workshop: a note with a leader, a
centre mark, a hole callout, section and elevation marks, and a dimension marked TYP. Then
you'll hide some annotations on the canvas or leave them off the sheets.

![The plate with a note (DEBURR ALL EDGES) and a hole callout (4× Ø6.4 THRU)](../images/annotate-notes.png)

Time: about 15 minutes.

Open the example (**File → Open example**). The annotation tools are in the **Annotate**
group of the tool palette.

## 1. A note with a leader

1. Press N for the **Note** tool.
2. Click the plate's top edge. That's where the leader's arrow goes.
3. Click above the plate, where the text goes.
4. Type `DEBURR ALL EDGES` and press Enter. (Shift+Enter starts a new line; Esc cancels.)

For a note without a leader, click empty canvas in step 2 and go straight to typing.

Leaders attach to points or to spots on curves, so they follow the geometry when it moves.
Pick a note and the **Inspector** has its text, the leader's **End** (**Arrow**, **Dot** or
**None**) and **Remove leader**. Double-click a note to edit its text.

## 2. A centre mark

1. Click **Centre mark** in the tool palette.
2. Click the bore.

Click another circle, arc or point next, and you get a centre line through both centres.
Click two lines to get a centre line midway between them. Esc stops.

To mark several circles at once, pick them with **Select** and click **Add centre marks**
in the **Inspector**.

## 3. A hole callout

1. Click **Callout** in the tool palette.
2. Click one of the bolt holes.
3. Click where the text goes.

Drift counts the holes of the same size: the callout reads **4× Ø6.4 THRU**. Pick it to
change the suffix to **TAP**, **CBORE** or **CSK**. Click a chamfer's line instead to get
`2 × 45°` (or `2 × 3` for unequal legs).

## 4. Ordinate dimensions

1. Click **Ordinate** in the tool palette.
2. Click the point to measure from. It reads 0.
3. Click more points. Each gets its X distance from the origin point, lined up in a row.
4. Press Y to measure Y distances instead (X to go back). Esc finishes.

## 5. Section and elevation marks

1. Click **Section mark**, then click where the cutting line starts and where it ends.
   Press X or Y before the second click to keep it along an axis. Each end gets a flag
   with the next free letter (A, then B…).
2. Click **Elevation mark**, click where the mark goes, then click towards what it looks
   at. X and Y keep it to an axis; Alt allows any angle. It gets the next free number.

![A section mark (A) across the plate and an elevation mark (1), with the elevation's Inspector](../images/annotate-marks.png)

Pick a mark to edit it in the **Inspector**: its **Label**, **On sheet** (the sheet it's
drawn on, shown in the lower half of the flag, such as `A-201`), and which way it **Looks**
(a section has **Flip**, to look the other way, and **Swap ends**; an elevation has an
angle and →, ↑, ←, ↓ buttons). Drag a mark to move it whole.

## 6. Dimension text

Double-click a dimension's value. Under the value field are:

- **before** and **after** fields, for text either side of the value.
- **Chips** that toggle common text on and off: **×**, **Ø**, **±**, **TYP**, **REF**,
  **MIN**, **MAX**, **VIF**, **THRU** and **EQ**.

![A dimension's value editor with TYP chosen; the Inspector shows the same settings](../images/dimension-chips.png)

Pick a dimension (click its value once) for more in the **Inspector**:

- **Prefix** and **Suffix**, and the same chips.
- **Under line:** a second line of text under the dimension line, such as
  `TO FACE OF STUD`.
- **Show instead:** text shown in place of the value, such as `EQ`. The dimension still
  holds its value; only the text changes. (Tolerances and dual units aren't shown with it.)
- **Tolerance:** **None**, **Symmetric** (±), **Deviation** (stacked + and −), **Limits**
  (stacked) or **Fit** (such as `H7`).
- **Driven (reference)**, as in [Constraints and dimensions](02-constraints-and-dimensions.md).

## 7. Hide annotations, on the canvas or on sheets

Every dimension and annotation has two switches, in the **Inspector**'s **Show** section:

- **On the canvas.** Turned off, it isn't drawn and can't be picked. A hidden dimension
  still holds its value.
- **On sheets, in print and export.** Turned off, it stays on the canvas (marked with a
  struck-through sheet badge) but is left out of sheets, PDF, SVG, DXF and printing.

Shift+click picks several dimensions and notes to change at once. The right-click menu has
**Hide on canvas** and **Leave off sheets** too.

To bring hidden ones back, press Alt+H (or **View → Hidden annotations**, or click the
**N hidden** pill in the status bar). Hidden annotations then show faded, so you can pick
them and switch them back on. Alt+H again hides them.

Alt+D hides or shows every dimension and note at once (**View → Dimensions and notes**).
That's a view setting; it doesn't change the drawing.

## The drafting style

The look of dimensions and notes is set for the whole drawing in **Parameters → Drafting**:

- **ISO 129** or **ASME Y14.5**. ISO writes text aligned above the dimension line; ASME
  writes it horizontal, in a break in the line.
- Text height, **Arrows** (closed, open, tick, dot) and **Arrow size**, extension line gap
  and overshoot (**Ext. gap**, **Ext. past**), leading and trailing **Zeros**, and
  **Dual units**.
- **Colour** on the canvas (paper output isn't affected), and a **Font** for all
  annotation and title-block text. A font you pick is kept inside the drawing, where its
  licence allows, so it looks the same on another computer and in PDFs.

Sizes are paper sizes: text set 3.5 mm high prints 3.5 mm high, whatever the sheet's
scale.

## What you learned

- N places notes (with a leader if you start on geometry); **Callout**, **Centre mark**,
  **Ordinate**, **Section mark** and **Elevation mark** are in the **Annotate** group.
- Double-click a dimension for before / after text and chips like TYP and EQ.
- Each annotation can be off the canvas or off the sheets; Alt+H shows hidden ones.

## Next

[Sheets and output](10-sheets-and-output.md).
