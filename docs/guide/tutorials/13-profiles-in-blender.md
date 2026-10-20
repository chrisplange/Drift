# Profiles in Blender

**What you'll make:** a crown moulding swept along a wall and a turned vase, both drawn in
Drift and modelled in Blender. When you change a profile in Drift and save, the Blender
model updates.

Time: about 15 minutes. You need Blender 4.2 or later.

## How it works

You draw each profile in Drift as a [block](07-blocks.md) and mark it as a **Blender
profile**. Each time you save the drawing, Drift writes the marked blocks' curves to a
small file beside it. For `Trim.drift`, the file is `Trim.drift-profiles.json`.

The **Drift Profiles** add-on reads that file in Blender. Each profile becomes a curve, with
its origin at the block's base point. You then use the curve in one of two ways:

- **Sweep:** as a bevel object, for mouldings, rails and trim.
- **Lathe:** with a Screw modifier, for turned shapes such as balusters, vases and newel
  posts.

Reload replaces only the curve's shape. The object, its position, its modifiers, its
materials, and every sweep that uses it stay as they are.

## 1. Draw the profiles

1. Start a new sketch (Ctrl+N), in **mm**.
2. Draw the moulding's section as one closed outline: lines, arcs and curves joined end to
   end. A closed area inside it, such as a round groove, becomes a hole.
3. Select it, open the **Blocks** panel and click **Make**. Name it `Crown`, then click its
   bottom-back corner as the base point (the point that will run along the wall).
4. For the vase, draw half of its outline with the axis on the left. Start on the axis at the
   bottom, go out and round the bulge, and come back to the axis at the top. Leave it open:
   don't draw the axis itself. Make it a block named `Vase`, with its base point on the axis
   at the bottom.

Construction and centre lines are left out of the profile, so use them for guides and the
axis. Points, dimensions, text and fills are left out too.

### Base points

The block's base point is the profile's origin in Blender:

- A **sweep** runs the base point along the path.
- A **lathe** spins round the base point's vertical line, so put it on the axis.

To move a base point later, select a copy and click **Set base point** in the Inspector. The
copies stay where they are on the drawing. In Blender, the curve's origin moves to the new
point.

## 2. Mark them as Blender profiles

Select a copy of `Crown`, and tick **Blender profile** in the Inspector. Do the same for
`Vase`. You can also tick it in the Blocks panel: right-click the block. A marked block's
row reads **· Blender profile**.

Save the drawing as `Trim.drift`. The status line reads *Saved Trim.drift and 2 Blender
profiles*, and `Trim.drift-profiles.json` appears beside it.

Only marked blocks are written. If you unmark them all and save, Drift deletes the profiles
file.

**File → Export → Blender profiles…** writes the same file anywhere you like. This also works
in the browser, where Drift can't write beside the drawing.

## 3. Install the add-on

1. Download `drift_profiles-<version>.zip` from the
   [release](https://github.com/chrisplange/drift-releases/releases) that matches your copy
   of Drift. It is attached to every release.
2. In Blender, open **Edit → Preferences → Get Extensions**. Open the menu at the top
   right, choose **Install from Disk…**, and pick the zip.

The add-on adds **File → Import → Drift Profiles**, and a **Drift** tab to the 3D view's
sidebar (press N).

## 4. Import

1. Choose **File → Import → Drift Profiles (.drift-profiles.json)**, and pick
   `Trim.drift-profiles.json`.
2. A dialog lists the profiles. Untick any you don't want, and choose how each is laid:
   - **XY · sweep:** flat, for a bevel object. This is the default for closed profiles.
   - **XZ · lathe:** standing up, to spin round Z. The profile's x is the radius. This is
     the default for open profiles.
3. Click **OK**.

Each profile becomes a curve object named after its block, at the 3D cursor. Sizes follow
the scene's unit scale (**Scene → Units**), so 40 mm in Drift is 40 mm in Blender.

## 5. Sweep the moulding

1. Add a path: **Add → Curve → Path**, or draw the run of wall as a curve.
2. In the path's **Object Data** properties, open **Geometry → Bevel**, choose **Object**,
   and pick `Crown`.
3. Turn on **Fill Caps** to close the ends.

The moulding runs along the path, with the profile's base point on it. To flip or turn it,
rotate the `Crown` object, or set the path's **Tilt**.

## 6. Lathe the vase

1. Select `Vase` and add a **Screw** modifier (**Add Modifier → Generate → Screw**).
2. Set **Axis** to **Z**, and **Screw** to 0.
3. Raise **Steps** for a smoother surface.

## 7. Change a profile in Drift

1. In Blender, open the **Drift** tab and tick **Auto-reload**.
2. In Drift, edit `Crown` (double-click a copy), change it, and save (Ctrl+S). You can save
   while the block is still open for editing.

Within about a second, the moulding in Blender takes the new shape. The tab's status line
says what was updated, and when.

Without Auto-reload, click **Reload All**, or select the profile objects and click
**Selected**.

A profile is matched by its block, not its name. If you rename a block in Drift, the
Blender object is renamed too, unless you renamed it yourself in Blender.

If you delete a block, or unmark it, the Blender object stays. The Drift tab lists it under
**Not in Drift's file any more**. Mark the block again and save to bring it back.

## Good to know

- The profiles file is saved with the drawing, so a profile updates only when you save.
  Updating while you drag is planned.
- Arcs, circles and ellipses become Bézier curves, accurate to within a few millionths of
  their size.
- The add-on never reads the `.drift` file itself, only the profiles file.
- Each Blender object remembers its file's full path. If you move the drawing, import the
  profiles again from the new place.
