# Blocks

**What you'll make:** a 4 × 3 m room furnished from Drift's furniture library, and a block
of your own. You'll place copies, turn them as you place them, and edit a block so that
every copy updates.

![A room with a double bed, a desk and a chair placed from the Blocks panel](../images/blocks.png)

Time: about 10 minutes.

## What a block is

A block is geometry drawn once and placed many times, like an AutoCAD block or a Revit
family. Each copy has its own position, angle, scale, mirror and layer. Edit the block and
every copy changes.

Copies are rigid: they aren't solved, so a drawing can hold thousands without slowing the
sketch down.

## 1. Draw the room

1. Start a new sketch (Ctrl+N).
2. Press R, click a first corner, type `4000`, Tab, `3000`, and press Enter.
3. Press Shift+F to fit it in the window.

## 2. Add the furniture library

1. Open the **Blocks** panel (the cube icon, at the end of the panel tabs).
2. Click **Add furniture library**. You get a chair, a 1400 desk, a double bed, a sofa, a
   WC, a basin and a 900 door, all in millimetres.

## 3. Place copies

1. Click **Double bed** in the list. The **Place block** tool (I) starts, and a copy follows
   the pointer, snapping by its base point.
2. Click inside the room to place it. The tool keeps going, so you can place more.
3. Click **Desk 1400**. Before you click to place it, press R to turn it 90°. Press R again
   for each further quarter turn, and X to mirror it.
4. Place a **Chair** at the desk the same way.
5. Press Esc to finish.

The panel lists how many copies of each block are placed.

## 4. Change a copy

Pick a copy with the **Select** tool: click its lines or anywhere inside its outline. It
moves, turns and copies like anything else (drag, G, R, Shift+D, Delete).

The **Inspector** shows the copy:

- **Block:** pick another block to replace this one with it.
- **Placement:** **X**, **Y**, **Angle** (with 0, 90, 180 and 270 buttons), **Scale** and
  **Mirror**.
- **Layer.**
- **Edit block**, **Explode** (turn the copy back into ordinary, connected geometry) and
  **Select all copies**.

Tools snap to copies: to the corners, side middles and centre of each copy's box, its
insertion point, and the ends, middles and centres of its own curves.

## 5. Make your own block

1. Draw something: a 600 × 600 rectangle with a circle inside, for a table and a lamp.
2. Pick it all (drag a box round it).
3. Under **New block** at the top of the **Blocks** panel, type a name (or keep the
   suggested one) and click **Make**. You can also use **Edit → Make block**, or **Make
   block** in the top bar's **Actions** menu or the right-click menu.

The block's base point is the middle of what you picked, and a copy takes the geometry's
place. Fills inside the outline go into the block too: each copy fills its area, the hatch
turning, mirroring and (for model patterns) scaling with the copy.

## 6. Edit a block in place

1. Double-click a copy (or pick it and click **Edit block**).
2. The copy becomes ordinary geometry, framed as the block being edited. Change it, add to
   it, delete from it; anything you draw joins the block.
3. Press Esc, click outside, or click **Finish editing** at the top of the **Inspector**.

Every copy updates.

Right-click a block in the **Blocks** panel to rename it, select its copies or delete it.
Deleting a block deletes its copies.

## Blocks and other programs

DXF export writes blocks and their copies as BLOCK and INSERT entities, and DXF import reads
them back as blocks. See [Underlays and DXF import](11-underlays-and-dxf.md).

## What you learned

- **Add furniture library** gives you a starting set; **Make** turns a selection into a
  block.
- Clicking a block starts placing it; R turns and X mirrors the next copy.
- Double-click a copy to edit the block; every copy follows.

## Next

[Construction geometry](08-construction-geometry.md).
