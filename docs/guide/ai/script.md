# Drift scripts

A Drift script describes a drawing the way you would describe it to a colleague: rooms and
their sizes, walls and their thickness, a door centred in a wall, a grid of columns. Names
take the place of ids. Drift's solver makes the geometry exact, and the drawing stays
parametric: change a variable later and everything tied to it follows.

AI assistants write scripts through the `run_script` tool of Drift's MCP server (see
[Drawing with AI assistants](assistants.md)). A script is one undo step, and all or nothing: if any
line fails, nothing changes, and the error says which line it was and how Drift read it.

```
# A bedroom with a door and a window
let W = 3.6 m
let D = 1.25 * W
room bed W x D at 0,0 walls 200 fixed
door d1 900 in bed.south centred
window w1 1200 in bed.north offset 600 from bed.nw
label bed "Bedroom"
hatch bed.walls ANSI31
```

Run it again with only `let W = 4 m` and the room widens; the door stays centred and the
window stays 600 from the corner.

## How it's written

- One statement per line. `#` starts a comment. End a line with `\` to carry on on the next.
- Statements start with a word (`room`, `door`, `dim`). Most then name what they make:
  `room bed …` makes `bed`.
- Add `on <layer>` to the end of a statement to put just that statement on a layer.

### Values

| Written | Means |
|---|---|
| `3600 mm`, `360 cm`, `3.6 m`, `3.6m` | 3600 mm |
| `12'6"`, `12' 6"`, `12'-6"`, `12 ft`, `150"`, `6 1/2"` | feet and inches |
| `3600`, `36` | a plain number: in the drawing's unit |
| `30°`, `30 deg` | an angle |
| `W`, `W/2 + 100`, `1.25 * W`, `(W - 2*T) / 3` | expressions of variables |

A plain number is in the drawing's unit, as when you type it into Drift: millimetres in a
metric drawing, inches in a feet-and-inches drawing (`door d1 36 in bed.south` is a 36"
door there). The same goes for a plain number added to or taken from a length: `W/2 + 6`
adds 6 inches in a feet-and-inches drawing. Multiply and divide by plain numbers as usual.
Write the units whenever a script might run in a drawing with other units: a script written
for millimetres, run in a feet-and-inches drawing, comes out 25.4 times bigger. Every result
says which unit plain numbers were read in (`plain_numbers_in`).

A door or window under 300 mm wide still runs, but the result warns about it, since it is
most likely a unit slip.

A size you write in a statement (`4200 x 3600`, `r 225`, `= 3.6 m`) becomes a driving
dimension. It keeps its text, so `W/2` stays tied to `W`. Positions (`at 0,0`,
`from 0,0 to 8 m,0`) are only rough: they show the solver where to start. A position can
also be a named point, like `bed.ne`, and then the shapes share that point.

## Statements

### Variables and layers

```
let W = 3.6 m              # a length (from its units)
let N = 4 number           # `number` or `angle` for other kinds
layer Walls color #333333 weight 0.5
layer Grid color #c03030 line center    # continuous, dashed, hidden, center, phantom
on Walls                   # new geometry goes on Walls from here on
```

`let` of a variable that exists changes it.

### Rooms

```
room bed W x D at 0,0 walls 200 fixed
room bath 2400 x D right of bed walls 200
```

`W x D` is the inside size. `at` is where the inside lower-left corner goes, and `fixed`
pins it there. `walls T` (or `walls T outside`) draws walls of thickness `T` outside the
outline. Write
`walls T inside` for walls inside the outline, or `walls T centre` for walls centred on it
(the outline is then drawn as a centre line).

`right of`, `left of`, `above` and `below` place a room against another one. The two rooms
share the wall between them: no second wall is drawn, and the new room is lined up with the
other room's bottom (or left) side. The rooms should overlap along that wall.

### Walls and openings

```
wall w1 from 0,0 to 8 m,0 thick 200            # centred on the line (the default)
wall w2 from 0,0 to 0,6 m thick 200 left       # left or right of the line, as drawn
wall w3 from 8 m,0 to 8 m,6 m thick 200 length 6 m fixed   # length as a dimension; pinned
door d1 900 in bed.south centred
window w1 1200 in bed.north offset 600 from bed.nw
opening o1 W/3 in w1.left offset 1 m from grid.B
```

A door, window or opening cuts a gap through the wall, with a jamb at each side. `centred`
keeps it in the middle of the wall however the wall changes. `offset X from P` keeps its
nearer jamb `X` from a corner or a line, such as a grid line. Doors and windows are gaps with
jambs for now: no swing arcs or glazing lines yet.

Walls drawn with `wall` stand alone. Their ends aren't joined to other walls.

### Grids and arrays

```
grid cols A..E every 6 m length 30 m at 0,0 fixed
grid rows 1..4 spacing 6 m, 7.5 m, 6 m length 24 m at 0,0
circle col r 200 at 0,0 fixed
array cols of col 5 x 6 m, 4 x 6 m        # 5 along x, 4 rows up
array studs of col 4 x 1 m up             # 4 up the y axis
array posts of col 4 by 1 m,0             # 4 in all, each moved by (1 m, 0)
array ring of col 8 around 0,0 angle 180  # 8 spread over 180°
```

Grid lines are centre lines. They print dash-dot, but they don't split rooms into separate
areas. Each one is labelled at its end. Write `every S` with a variable `S` to change the
spacing in one place later.

In an array written as `count x spacing`, the copies of a single circle or point stay in line
and at that spacing, so they follow when the spacing changes. Copies made with `by` or
`around` are placed once and are free to move.

### Plain geometry

```
point p at 0,0 fixed
line l from p to 4200,0 length 4.2 m
polyline site (0,0) (30 m,0) (30 m,20 m) (0,20 m) closed
rect r 4200 x 3600 at 0,0            # `centred at` for the centre; `about` for no dimensions
circle c r 225 at 1 m,1 m            # or `d 450`
arc a centre 0,0 from 1000,0 to 0,1000     # counter-clockwise
construction line axis from 0,0 to 0,10 m  # `construction` before any of these
```

### Relations

```
parallel a b          perpendicular a b       equal a b c        collinear a b
tangent a c           concentric c1 c2        coincident p q
horizontal l          vertical l              horizontal p q     (two points level)
symmetric p q about axis     midpoint p of l     on p c     fix p at 0,0
```

A relation that's already true is skipped, and the result says so.

### Dimensions

```
dim bed.south = 3.6 m
dim bed.sw to hall.se = 8 m horizontal     # or vertical; aligned without either
dim p to grid.A = 1200                     # a point to a line
dim r col = 225          dim d col = 450
dim angle w1.left w2.left = 90°
dim bed.north ref                          # shows the size; doesn't drive it
```

### Text, fills and hatches

```
label bed "Bedroom 1" height 250   # in the middle of the room
text "N" at 0,12 m height 250 align centre angle 90
hatch bed.walls ANSI31 scale 2 angle 45   # every stretch of the room's walls
fill bed #f0f0f0 opacity 0.5
```

### Trimming one shape by another

```
trim slab inside bay      # remove the pieces of slab that lie inside bay
trim bay outside slab     # remove the pieces of bay that lie outside slab
```

`inside` and `outside` say **what goes**. The second shape must be closed. Only its curves
cut the first, so a line drawn across both doesn't add pieces. It works as the Trim tool
does, piece by piece, so the pieces left keep their ids and relations. Two shapes trimmed
into one outline keep one fill, the top one's.

Two lines give the usual combinations:

| Want | Lines |
|---|---|
| Union | `trim A inside B`, then `trim B inside A` |
| Notch (A minus B) | `trim A inside B`, then `trim B outside A` |
| Overlap only | `trim A outside B`, then `trim B outside A` |
| A hole | nothing to trim: draw B inside A (a shape inside another is a hole in its fill) |

The second line works even though the first left A open: "inside A" means inside A as it
was before this script first trimmed it. After a trim, a name stands for what is left of
it, including the pieces trims cut off its sides.

- A piece held by a dimension that would no longer hold isn't trimmed. You get a warning,
  and the rest goes ahead. A `rect`'s sizes are measured between its corners, so they don't
  stop a trim.
- If nothing lies on that side, that's a warning, not an error.

```
rect slab about 6 m x 4 m at 0,0
circle bay r 1.2 m at 6 m,2 m
trim slab inside bay
trim bay inside slab
fill slab #f0f0f0
```

### Existing geometry

```
use e12 as north_wall
use e40, e41 as stair       # stair[1], stair[2]
```

Raw ids such as `e12` also work wherever a name does.

## Names and parts

| Made by | Parts |
|---|---|
| `line` | `.start`, `.end` |
| `polyline` | `[1]` … (its segments), `.start`, `.end`, `.inside` |
| `rect`, `room` | sides `.south .east .north .west` (or `.bottom .right .top .left`), corners `.sw .se .ne .nw`, `.width` (the south side), `.depth` (the west side), `.inside` |
| `room` with walls | also `.outer.south` … `.outer.nw` (the outside face), and `.walls` |
| `wall` | faces `.left` and `.right`, ends `.start` and `.end`, and `.axis` when centred |
| `door`, `window`, `opening` | jambs `.start` and `.end` |
| `grid` | one per axis: `grid.A`, `grid.3` |
| `circle` | `.center` |
| `arc` | `.center`, `.start`, `.end` |
| `array` | `[1]` (the original) … `[n]`; a circle's copies have `.center`: `cols[3].center` |

North is up (+y). A side runs from corner to corner counter-clockwise: south from `sw` to
`se`, north from `ne` to `nw`. Once an opening cuts a side, the pieces are `bed.south[1]`,
`bed.south[2]` and so on. `bed.south` itself still means the whole side.

Names last for one script. The result lists every name with its id (`"bed.south": "e5"`), so
a later script can pick them up with `use e5 as bed_south`.

## Errors

Errors give the line, a marker under the mistake, how Drift read the statement, and a way to
fix it. Nothing is changed.

```
line 2, column 19: room bed has no part `sout` (did you mean `south`?). Its parts: south, east, …
  2 | window w1 1200 in bed.sout offset 600 from bed.nw
    |                   ^^^^^^^^
Nothing was changed.
```

```
line 3: dimensions[0]: already implied by existing constraints: …
  2 | room bed W x 4 m at 0,0 walls 200 fixed
  3 | dim bed.south = 4 m
  meant: make the length of bed.south 4 m
  something earlier already sets this size: change that dimension or its variable instead, or add `ref` to show this one only
Nothing was changed.
```

## The `run_script` tool

| Argument | Meaning |
|---|---|
| `script` | The script. |
| `check` | Run it and report the result, then keep nothing: a dry run. |
| `explain` | Also return the tool calls it made, as `batch` takes them. |
| `render` | Attach a picture of the result. |

The result lists every name's id, how each line was read, any relations skipped as already
true, warnings (such as a door only millimetres wide), each room's area, and the sketch's
degrees of freedom.
