# Drawing with AI assistants

Drift comes with an MCP server, `drift-mcp`. With it, Claude (in Claude Desktop or Claude
Code) and other assistants that speak the Model Context Protocol can create and edit Drift
drawings. The assistant works on a `.drift` file. You open that file in Drift to see it and
carry on by hand.

The server works the way Drift does. The assistant sketches rough geometry, then states what
it means: walls parallel, this side 3600, the door centred. Drift's solver then makes the
drawing exact. It stays parametric, so a dimension you change later still drives it. The
assistant also checks its own work. It can render the drawing to a picture and analyse it
for degrees of freedom, conflicts, open ends and room areas, then fix what's wrong.

> **Phase 1.** The server edits drawing files on its own. It is not connected to a running
> Drift window yet, so to see the assistant's work, save the drawing and open it in Drift.
> A live link and an assistant panel inside Drift come later.

## Building the server

`drift-mcp` is part of the Drift source tree. Build it with Rust (1.92 or later):

```bash
cargo build --release -p drift-mcp
```

The program is `target/release/drift-mcp` (`drift-mcp.exe` on Windows). Copy it anywhere
you like. It needs no other files, and no GPU: its pictures are drawn on the CPU.

Options:

| Option | Meaning |
|---|---|
| `--root <folder>` | Only read and write files inside this folder. Relative paths the assistant gives are taken from here. Recommended. |
| `--open <file.drift>` | Start with this drawing open. |

## Connecting Claude Desktop

Open Claude Desktop's settings, then **Developer → Edit Config**. This opens
`claude_desktop_config.json`: on Windows it is in `%APPDATA%\Claude\`, on macOS in
`~/Library/Application Support/Claude/`. Add Drift under `mcpServers`:

```json
{
  "mcpServers": {
    "drift": {
      "command": "C:/Tools/drift-mcp.exe",
      "args": ["--root", "C:/Users/you/Documents/Drawings"]
    }
  }
}
```

On macOS or Linux, use paths like `/Users/you/bin/drift-mcp` and
`/Users/you/Documents/Drawings`. In JSON, write Windows paths with forward slashes, or
double each backslash (`C:\\Tools\\drift-mcp.exe`).

Restart Claude Desktop. Drift's tools then appear in the tools menu of a new chat.

## Connecting Claude Code

From a terminal, in the project where you want to use it:

```bash
claude mcp add drift -- C:/Tools/drift-mcp.exe --root C:/Users/you/Documents/Drawings
```

Add `--scope project` to share it with everyone working on the project. That writes it to
`.mcp.json` at the project's root, in the same format as Claude Desktop's config:

```json
{
  "mcpServers": {
    "drift": {
      "command": "C:/Tools/drift-mcp.exe",
      "args": ["--root", "."]
    }
  }
}
```

`claude mcp list` shows whether it connected.

## What the assistant can do

| Tools | For |
|---|---|
| `new_drawing`, `open_drawing`, `save_drawing`, `drawing_info` | Files and an overview: units, scale, layers, variables, degrees of freedom. |
| `list_entities`, `list_constraints`, `measure` | Reading the drawing: ids, coordinates, lengths, relations, dimensions. |
| `add_geometry` | Points, lines, polylines, rectangles, regular polygons, circles, arcs, ellipses and Bézier curves. Shapes that share a point are joined. |
| `add_relations` | Horizontal, vertical, parallel, perpendicular, equal, tangent, concentric, collinear, coincident, symmetric, midpoint, point on curve and fix. |
| `add_dimensions`, `set_dimension`, `set_variables`, `remove_constraints` | Driving (or reference) dimensions, with values such as `3600`, `"3.6 m"`, `"12'6\""` or expressions of variables like `"W/2"`. |
| `move_point`, `transform`, `offset`, `trim_extend_split`, `fillet_chamfer`, `delete`, `set_construction` | Editing: move, rotate, scale, mirror, arrays, offset, trim, extend, split, fillets and chamfers. |
| `layers`, `add_fill`, `add_text` | Layers, solid fills and hatches, and text. |
| `import_file` | SVG, PDF, Illustrator or DXF, as geometry or as an underlay to trace, at a scale such as `"1:50"`. |
| `render`, `analyze` | Checking: a picture of the drawing, and its degrees of freedom, conflicts, open ends and closed areas. |
| `export` | PDF or SVG of the sheets, DXF of model space, or a PNG. |
| `undo`, `redo`, `batch` | Every change the assistant makes is one undo step, and all or nothing: a call that fails changes nothing. `batch` makes several calls one step. |

The renders mark up the drawing for the assistant. Curves are coloured by how well they are
defined: black when fully defined, blue when they can still move, red when constraints
conflict. Relations show as small green tags (H, V, //, L for perpendicular, = and so on), and
every curve is labelled with its id (`e12`). The assistant can then match what it sees to
what it lists.

Coordinates are millimetres with y up, whatever the drawing's display units. Angles are in
degrees, counter-clockwise.

## Example prompts

- "Start a new drawing at 1:50. Draw a 4200 × 3600 room with 200 mm walls. Make it fully
  defined, with the inside lower-left corner at the origin, and render it."
- "Open `flat.drift`. What's the area of each room? Is anything under-defined?"
- "Add a 900 mm door opening centred in the north wall of the bedroom, and dimension it
  from the corner."
- "Make the room width a variable W = 3600 and the length 1.25 × W. Then try W = 4000 and
  show me."
- "Import `site-plan.pdf` at 1:200 as an underlay on a layer called Site. Trace the
  boundary as a closed polyline and tell me its area."
- "Hatch the walls with ANSI31 and put the room names in the middle of each room."
- "Export the drawing as a PDF on an A3 sheet."

For good results, ask for intent and let the assistant constrain it: "the walls are
parallel", "the window is centred", "these two rooms are the same width". This works better
than giving coordinates. Ask it to render and analyse before it finishes.

## Safety

- With `--root`, the server can't read or write files outside that folder. Without it, it
  can use any file your account can.
- The server only edits the drawing it has open, and every change can be undone. Nothing is
  written to disk until the assistant saves or exports, and it only writes the files it is
  asked to.
