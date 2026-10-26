# Doors and windows

**What you'll make:** doors and windows in the walls of a small room: one placed by
pointing, one at a typed distance from a corner, a pair of windows. Then you'll slide one
along its wall, flip its swing, resize it, and see what happens to openings when their wall
changes.

Time: about 10 minutes. Start from the room in [Walls](14-walls.md), or draw four walls.

## How openings work

An **opening** (a door, a window, or a cased opening with no leaf) belongs to a wall. It
cuts the wall: the faces stop at its two **jambs**, and the wall's fill (poché) goes round it.
Its **symbol** (a door's leaf and swing, a window's frame, glass and sill) is drawn at the
wall's thickness, so a door in a 6″ wall and one in a 4½″ wall each fit their wall.

An opening has two widths:

- the **opening**: the cut in the wall (for a door, its rough opening);
- the **leaf**: what the symbol draws (for a window, its nominal size).

A door is named by its leaf (`3′0″ × 6′8″`) and cuts its rough opening (2″ wider and 2½″
taller). The types come from the drawing's **size set**: **US (imperial)** for drawings in
feet and inches, **Generic metric** for metric ones (leaves of 700 to 1000 mm, windows on a
300 mm module). You can pick another set for a drawing, and type any width: a size that isn't
in the list is never a problem.

The opening moves with its wall. Drag the wall's corners, move the wall or change its
thickness and the cut and the symbol follow.

## 1. Place a door

1. Press **Shift+O** for the **Door / window** tool (on the tool rail under **Build**, in
   the **Build** menu, or type `DOOR` in the command palette).
2. In the top bar, pick **Door – Single Swing** and a type, for example `3′0″ × 6′8″`.
3. Hover a wall. The cut and the door show where it would go, centred on the pointer and kept
   clear of the corners. The readout shows its distance from the nearer corner.
4. Press **Space** to step through the four ways it can open: hinge right or left, swinging
   into the room or out of it. The top bar shows which.
5. Click to place it.

The tool stays in hand for the next one. Each opening is one undo step.

## 2. Place one at a distance from a corner

1. Hover the wall near the corner you want to measure from.
2. Type the distance (for example `1'6"`): it is from the nearer corner's face to the
   opening's near jamb.
3. Press **Tab** to type its width too, if it isn't the type's, then **Enter**.

## 3. Windows

1. In the top bar, pick a window family, for example **Window – Double Hung**, and a type
   (`3050` is 3′0″ wide by 5′0″ tall).
2. Hover the outside wall. For a window, **Space** chooses which face is the room side (where
   the stool line is); the sill goes outside.
3. Click to place it, and again for a second one.

## 4. Change an opening

Click a door's leaf, swing or jamb to pick it. On the canvas you see its **width** and its
**offset** from the nearer corner, and two **flip arrows**:

- the arrow across the wall flips the side it opens to;
- the arrow along the wall moves its hinge to the other jamb.

**Drag** the picked opening to slide it along its wall. It stays clear of the corners and
of other openings, and snaps to the middle of the segment.

The **Inspector** shows the same and more:

- **Offset** and **Width**: type a length or a variable's name (a width from a variable
  follows it). The width grows about the opening's middle; the leaf keeps its frame.
- **Opens**: the four ways round; **Flip hinge**, **Flip side**, **Centre** and **Delete**.
- **Size set**: which sizes this drawing uses (also on the tool's bar).

**Delete** removes the opening and the wall closes up behind it.

## When the wall changes

- **Moved, stretched or re-thickened:** the opening keeps its place along the wall, measured
  from the end of the segment it is nearer.
- **Split** (Inspector, **Split after**) or **joined**: each opening goes with the part of the
  wall it is in.
- **Deleted:** its openings go too, and a notice says so. Undo brings them all back.
- **Exploded:** the wall becomes plain lines, and so does each opening's symbol.
- **Copied:** the copy has the openings too.

If a change leaves an opening where it can't be cut (across a corner, or with another wall
teed into it), the wall is drawn whole there and a notice says which opening doesn't fit.
Slide it clear and it cuts again.

## What's next

Room tags with live areas (openings are closed at the face of the wall, so a door doesn't
change a room's area), national size sets (Germany's DIN 18101 and the UK's BS 4787),
dragging an opening onto another wall, and your own blocks as door and window symbols are
coming in the next batches.
