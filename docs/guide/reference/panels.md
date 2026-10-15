# Panels

Drift has five panels. They start docked as tabs on the right of the window.

- Click a tab's icon to bring that panel to the front.
- Drag a tab onto the canvas to float the panel; drag it (or a floating panel's header) onto
  either side to dock it there. Blue drop zones show where it will go.
- Right-click a tab, or use the **⋯** menu, to dock, float or close it.
- **»** collapses the dock to a strip of icons. A narrow window collapses both docks.
- **View → Panels** shows or hides each panel, **Hide all panels** (Ctrl+\) clears them all
  away, and **Reset panel layout** puts them back.

The layout is remembered between sessions.

## Inspector

What's picked, and what you can do with it. Its contents depend on the selection.

| Picked | The Inspector shows |
|---|---|
| Nothing | A summary: the numbers of curves, relations and layers. |
| One curve or point | Its **Geometry** as fields: a line's **Length**, **Angle**, **Start** and **End**; a circle's or arc's **Radius**, **Diameter** and **Center**; a point's **Position**. Typing a size sets the dimension that drives it, or adds one (shown as **dim**). Typing a position or angle moves the geometry as far as its relations allow. Fields apply when you press Enter or Tab, or click elsewhere; Esc puts the old value back. |
| Curves | **Appearance** (**Geometry** / **Construction** / **Centre line**, **Layer**, **Colour**, **Weight**, **Line type**, each "By layer" unless set), **Add relation** and **Add dimension** buttons for what fits, **Pin measurement**, **Pattern**, the **Relations & dimensions** on the selection (each with **×**; **Clear relations** and **Clear dimensions**), and **Delete selection**. |
| Circles | **Add centre mark** (or **Add N centre marks**). |
| A dimension | **Driven (reference)**, its **Text** (**Prefix**, **Suffix**, chips, **Under line**, **Show instead**), **Tolerance**, and **Show** (**On the canvas**, **On sheets, in print and export**). |
| A note, callout or mark | Its text or label, leader, sheet and direction, and **Show**. |
| A fill or hatch | Its **Style**, **Layer**, draw order and **Area**. See [Fills and hatches](../tutorials/06-fills-and-hatches.md). |
| A text | Its words, **Height**, **Angle**, **Font**, **Align**, **Spacing** and **Wrap**, and **Convert to curves**. |
| A block copy | **Block**, **Placement**, **Layer**, **Edit block**, **Explode** and **Select all copies**. |
| A group | Its **Name**, **Members**, **Edit group** and **Ungroup**. |
| A closed area (with area picking on) | **Area**, **Perimeter**, **Centroid** and **Holes**, and **Fill** / **Hatch** buttons. |

With the **Fill / hatch** tool active, the **Inspector** shows the style for **New fills**
instead.

While a block is being edited, **Editing block** and **Finish editing** sit at the top.

## Parameters

The drawing's settings and its parametric side.

- **Units:** **Metric** or **Imperial**, the **Format**, **Decimals** (or **Round to** a
  fraction, and **Show 0'**), and **Angles** decimals. See [Units](units.md).
- **Variables:** name, expression, kind and slider for each; a row to add one. See
  [Parameters](../tutorials/03-parameters.md).
- **Variants:** apply, **Update**, delete, **Save current**, and **Show other variants as
  ghosts**.
- **Snapshots:** **Take snapshot** and **Restore** (this session only).
- **Pinned measurements:** live values, with **Unpin**.
- **Drafting:** **ISO 129** or **ASME Y14.5**, then scale, text height, **Arrows**,
  **Arrow size**, **Ext. gap**, **Ext. past**, text placement, **Zeros**, **Dual units**,
  **Colour** and the drawing's **Font**. See [Annotation](../tutorials/09-annotation.md).
- **Hatch patterns:** patterns imported into the drawing, and **Import .pat…**.

## Layers

Ctrl+L shows or hides it.

- Each layer is a row: its colour, name, the number of curves on it, and show and lock
  switches. Click a name to make it the current layer (new geometry goes there);
  double-click it to rename. The current layer is outlined.
- Click a row's arrow (**Colours and line settings**) to open it: **Stroke** colour
  (**Auto** is black or white to suit the theme), **Fill** colour (**Same as stroke** by
  default), **Line** weight and type, **Output** (**Printed and exported**), **Select all**,
  **Move here** and **Delete** (its geometry moves to layer 0).
- Right-click a row to select its curves or move the selection there.
- **+ New layer** adds a layer and makes it current. **Move selection here** moves what's
  picked to the current layer.
- Layer **0** always exists and can't be deleted.

Hidden layers aren't drawn, picked or snapped to, but their geometry still solves. Locked
layers are drawn and can be snapped to and dimensioned against, but not picked; the solver
holds their geometry still.

The **Underlays** section, below the layers, lists the drawing's underlays. See
[Underlays and DXF import](../tutorials/11-underlays-and-dxf.md).

Geometry takes its colour, weight and line type from its layer unless you set them on the
geometry itself (in the **Inspector**, or the top bar's **Stroke** menus).

## Sheets

Sheets, views, title blocks and output: **New sheet**, **Show sheet** / **Back to model**,
**Paper**, **Views** (with **Add view** and **Align with**), **Title block**, **Logo**,
**Revisions**, and **Output** (**PDF…**, **SVG…**, **DXF…**, **Print…**) and
**Delete sheet**. See [Sheets and output](../tutorials/10-sheets-and-output.md).

## Blocks

The drawing's blocks: a thumbnail of each, how many copies are placed, and **New block**
(a name and **Make**) when something is picked. Click a block to place copies; right-click
it to rename it, select its copies or delete it. **Add furniture library** adds a starter
set. See [Blocks](../tutorials/07-blocks.md).
