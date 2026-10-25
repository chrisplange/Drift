# Blocks

**What you'll make:** a 12' × 10' bedroom furnished from Drift's block library, and a block
of your own. You'll place copies, turn them as you place them, and edit a block so that
every copy updates.

![A bedroom with a door, a window, a queen bed with nightstands, a desk, a desk chair and a dresser placed from the Blocks panel's library](../images/blocks.png)

Time: about 10 minutes.

## What a block is

A block is geometry drawn once and placed many times, like an AutoCAD block or a Revit
family. Each copy has its own position, angle, scale, mirror and layer. Edit the block and
every copy changes.

Copies are rigid: they aren't solved, so a drawing can hold thousands without slowing the
sketch down.

A group is one thing; a block is a thing you reuse. Group what belongs together in one
place (a desk and its chair); make a block of what you'll place again and again (every desk
in the office). One turns into the other, see [Groups and blocks](#groups-and-blocks).

## 1. Draw the room

1. Start a new sketch (Ctrl+N), and set the units to **ft-in** at the bottom right.
2. Press R, click a first corner, type `12'`, Tab, `10'`, and press Enter.
3. Press Shift+F to fit it in the window.

## 2. The block library

Open the **Blocks** panel (the cube icon, at the end of the panel tabs). Under **Library**,
pick a category: **Doors**, **Windows**, **Kitchen**, **Bath**, **Furniture**, or **Laundry
and utility**. Each family (**Door – Single Swing**, **Base Cabinet**, **Bed**…) shows its
standard US sizes as tiles. The search field at the top searches every category, and the
drawing's own blocks: try `queen` or `36 door`.

Nothing is added to the drawing until you use it, so the list of the drawing's blocks
stays short. **Add all** adds every block shown without placing them.

The blocks are drawn to real sizes and clearances in plan, legible at 1/4" = 1'-0".
Swings, glass and bedding are thin lines; wall cabinets and door heads are hidden lines;
countertops and vanity tops have a light fill.

## 3. Place copies

1. Pick **Furniture** and click the **Queen with Nightstands** tile under **Bed**. It's
   added to the drawing, and the **Place block** tool (I) starts: a copy follows the
   pointer, snapping by its base point.
2. Click the middle of the top wall to place it. The tool keeps going, so you can place
   more.
3. Search `desk`, and click **Desk – 48" × 24"** (a tile, or the row once it's in the
   drawing). Before you click to place it against the right wall, press R to turn it 90°.
   Press R again for each further quarter turn, and X to mirror it.
4. Place a **Chair – Desk** at the desk, a **Dresser**, a **Door – Single Swing – 32"** and a
   **Window – Double Hung – 36"** the same way.
5. Press Esc to finish.

The panel lists how many copies of each block are placed. **Purge unused** deletes the
blocks that have none (Ctrl+Z brings them back).

### Base points

Each block is named "Family – Type", and every type of a family has the same base point,
so replacing a copy with another size (**Definition** in the **Inspector**) keeps it in place:

| Blocks | Base point |
|---|---|
| Doors, openings, windows | The hinge side (or left end) of the opening, on the room-side face of the wall. The wall lies behind the base point, and doors swing into the room. Mirror a door for the other hand. |
| Cabinets, appliances, counters, tubs, showers, washers, dryers, furnaces | The back-left corner, on the wall. |
| Toilets, vanities, water heaters | The middle of the back, on the wall, so they stay on the plumbing line. |
| Beds, sofas, chairs, desks, dressers | The middle of the back (a bed's head wall). |
| Tables, islands, stools, free-standing tub | The centre. |

Blocks face down in the block (the wall at the top), so turn them with R to face the
room.

## 4. Change a copy

Pick a copy with the **Select** tool: click its lines or anywhere inside its outline. It
moves, turns and copies like anything else (drag, G, R, Shift+D, Delete).

The **Inspector** shows the copy, laid out as it shows a group:

- **Name:** the block's name, which every copy shares.
- **Definition:** pick another block to replace this one with it; it says how many copies
  the block has.
- **Edit in place**, **Explode** (turn the copy back into ordinary, connected geometry) and
  **Explode to group** (the same, as a group named after the block).
- **Blender profile** (see [Profiles in Blender](13-profiles-in-blender.md)).
- **Placement:** **X**, **Y**, **Angle** (with 0, 90, 180 and 270 buttons), **Scale** and
  **Mirror**.
- **Layer.**
- **Set base point**, **Select all copies** and **Delete**.

A click picks a copy while the top bar's **Blocks** pick filter is on (see
[Pick filters](04-editing.md#pick-filters)). Turn it off and a click picks a line inside a
copy instead.

Tools snap to copies: to the corners, side middles and centre of each copy's box, its
insertion point, and the ends, middles and centres of its own curves.

## 5. Make your own block

1. Draw something: a 600 × 600 rectangle with a circle inside, for a table and a lamp.
2. Pick it all (drag a box round it).
3. Under **New block** at the top of the **Blocks** panel, type a name (or keep the
   suggested one) and click **Make**. You can also use **Edit → Make block**, or **Make
   block** in the top bar's **Actions** menu or the right-click menu.
4. Drift asks for the block's **base point**, the point a copy is placed by. Click a snap
   point: the table's back-left corner, say. Press Enter to use the middle of what you
   picked instead. Esc gives up, and nothing is made.

A copy takes the geometry's place. Fills inside the outline go into the block too: each
copy fills its area, the hatch turning, mirroring and (for model patterns) scaling with the
copy.

### Set base point

To change a block's base point later, pick a copy and click **Set base point** in the
**Inspector** (or right-click the block in the **Blocks** panel). The copy's current base
point shows as a small cross. Click the new one, on any snap point of the copy: an end, a
corner, a middle or a centre. Enter puts it in the middle of the block.

The block's geometry moves relative to its base point, and every copy's insertion point
moves with it, so nothing moves on the drawing. It's one undo step.

Choose the point that matters: the back corner that sits against a wall, a door's hinge,
or the axis a profile turns about. Exporting a block uses its base point as its origin.

## 6. Edit a block in place

1. Double-click a copy (or pick it and click **Edit in place**).
2. The copy becomes ordinary geometry, framed as the block being edited. Its base point
   shows as a small cross. Change it, add to it, delete from it; anything you draw joins
   the block. **Set base point** at the top of the **Inspector** moves the base point, as
   above.
3. Press Esc, click outside, or click **Finish editing** at the top of the **Inspector**.

Every copy updates.

Right-click a block in the **Blocks** panel to rename it, select its copies or delete it.
Deleting a block deletes its copies.

## Groups and blocks

A group is one thing; a block is a thing you reuse. The **Inspector** shows both the same
way: a name, what it holds (a group's members, a copy's block), **Edit in place**, and
**Ungroup** or **Explode**.

- **Make block from group:** pick a group and click **Make block** in the **Inspector**
  (also on the right-click menu, **Edit → Make block from group** and the command
  palette). The block is named after the group (with a number if a block already has the
  name), its base point is the middle of the group, and it keeps the group's fills. A copy
  takes the group's place.
- **Explode to group:** pick block copies and click **Explode to group**. Each copy becomes
  ordinary geometry in a group named after its block, with its fills; shapes drawn filled
  are shapes again.

Each is one undo step. Groups don't hold other groups, so a group to make a block of is one
whole group, picked or being edited.

## Blocks and other programs

DXF export writes blocks and their copies as BLOCK and INSERT entities, and DXF import reads
them back as blocks. See [Underlays and DXF import](11-underlays-and-dxf.md).

## What you learned

- The **Library** holds doors, windows, kitchen, bath, furniture and utility blocks in
  standard US sizes; a click adds one and places it. **Make** turns a selection into a
  block.
- Clicking a block starts placing it; R turns and X mirrors the next copy.
- **Make** asks for the base point; **Set base point** moves it later without moving any
  copy.
- Double-click a copy to edit the block; every copy follows.
- A group is one thing; a block is a thing you reuse. **Make block** and **Explode to
  group** turn one into the other.

## Next

[Construction geometry](08-construction-geometry.md).
