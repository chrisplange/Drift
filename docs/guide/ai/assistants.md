# Drawing with AI assistants

Drift comes with an MCP server, `drift-mcp`. With it, Claude (in Claude Desktop or Claude
Code) and other assistants that speak the Model Context Protocol can create and edit Drift
drawings. Turn on Drift's AI link and the assistant works on the drawing open in your
window: you watch each change as it is made, and can step in at any point. Without it, the
assistant works on `.drift` files of its own.

The server works the way Drift does. The assistant sketches rough geometry, then states what
it means: walls parallel, this side 3600, the door centred. Drift's solver then makes the
drawing exact. It stays parametric, so a dimension you change later still drives it. The
assistant also checks its own work. It can render the drawing to a picture and analyse it
for degrees of freedom, conflicts, open ends and room areas, then fix what's wrong.

## Where the server is

`drift-mcp` comes with Drift, next to the Drift program:

| Drift from | `drift-mcp` is at |
|---|---|
| The Windows installer, for you only (the default) | `C:\Users\you\AppData\Local\Programs\Drift\drift-mcp.exe` |
| The Windows installer, for all users | `C:\Program Files\Drift\drift-mcp.exe` |
| The Windows zip | `drift-mcp.exe`, in the `Drift` folder you unpacked |
| The macOS disk image | `/Applications/Drift.app/Contents/MacOS/drift-mcp` |

Replace `you` with your user name, and the folder with yours if you installed somewhere
else. Drift shows the path in **Edit → Preferences → Updates & Labs**, under **Labs**,
and **Copy Claude Desktop config** there copies a ready-made config for Claude Desktop. You don't run the server yourself: the assistant starts it. It needs no GPU (its
pictures are drawn on the CPU), and updating Drift updates it too. To check it's there,
run it from a terminal with `--version`.

Options:

| Option | Meaning |
|---|---|
| `--root <folder>` | Only read and write files inside this folder. Relative paths the assistant gives are taken from here. Recommended. |
| `--open <file.drift>` | Start with this drawing open. |
| `--no-link` | Never work on the drawing open in Drift, only on files. |

## Connecting Claude Desktop

Open Claude Desktop's settings, then **Developer → Edit Config**. This opens
`claude_desktop_config.json`: on Windows it is in `%APPDATA%\Claude\`, on macOS in
`~/Library/Application Support/Claude/`. Paste what Drift's **Copy Claude Desktop config**
copied: if the file already has servers under `mcpServers`, add Drift's entry next to them.
Or write it yourself. On Windows:

```json
{
  "mcpServers": {
    "drift": {
      "command": "C:/Users/you/AppData/Local/Programs/Drift/drift-mcp.exe",
      "args": ["--root", "C:/Users/you/Documents/Drawings"]
    }
  }
}
```

In JSON, write Windows paths with forward slashes, as here, or double each backslash
(`C:\\Program Files\\Drift\\drift-mcp.exe`). On macOS:

```json
{
  "mcpServers": {
    "drift": {
      "command": "/Applications/Drift.app/Contents/MacOS/drift-mcp",
      "args": ["--root", "/Users/you/Documents/Drawings"]
    }
  }
}
```

Restart Claude Desktop. Drift's tools then appear in the tools menu of a new chat.

## Connecting Claude Code

From a terminal, in the project where you want to use it. On Windows:

```bash
claude mcp add drift -- C:/Users/you/AppData/Local/Programs/Drift/drift-mcp.exe --root C:/Users/you/Documents/Drawings
```

On macOS:

```bash
claude mcp add drift -- /Applications/Drift.app/Contents/MacOS/drift-mcp --root ~/Documents/Drawings
```

Add `--scope project` to share it with everyone working on the project. That writes it to
`.mcp.json` at the project's root, in the same format as Claude Desktop's config:

```json
{
  "mcpServers": {
    "drift": {
      "command": "/Applications/Drift.app/Contents/MacOS/drift-mcp",
      "args": ["--root", "."]
    }
  }
}
```

Everyone who shares it needs Drift installed in the same place. `claude mcp list` shows
whether it connected.

## Working live in Drift

1. In Drift, open **Edit → Preferences → Updates & Labs** and tick **Let AI assistants edit the
   open drawing**. The status bar shows **AI link on**.
2. Ask the assistant to work on your drawing. When it connects, the status bar shows
   **Assistant connected**.

What happens then:

- Each change appears in the window as it is made. What the assistant just drew is
  selected, so you can see it.
- Every change is one undo step, listed as "Assistant: add dimensions" and so on, mixed in
  with your own. Undo one and carry on drawing.
- You can keep working while it works. A call that arrives while you're in the middle of
  an edit (dragging, say) waits until you finish.
- The assistant never replaces a drawing with unsaved changes. To start or open another
  drawing it must ask you to save first. It saves only when asked, with Drift's own save.
- Click **Assistant connected** to disconnect it, or untick the preference to turn the
  link off.

With several Drift windows open, the assistant works on the one you used last. If that
window closes while the assistant is working, its next call fails. It then reconnects or
goes back to files, rather than carrying on somewhere else. The assistant can check which
drawing it is on with the `drift_link` tool, and switch between the window and its own
files.

The link is for Drift on Windows and macOS, not the web version. To keep the server
away from your open drawings altogether, start it with `--no-link`. If it's started with
`--open <file>`, it begins on that file instead of the window.

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
| `import_file` | DWG, DXF, SVG, PDF or Illustrator, as geometry or as an underlay to trace, at a scale such as `"1:50"`. |
| `render`, `analyze` | Checking: a picture of the drawing, and its degrees of freedom, conflicts, open ends and closed areas. |
| `export` | PDF or SVG of the sheets, DXF of model space, or a PNG. |
| `run_script` | A [Drift script](script.md): rooms, walls, doors, grids, arrays, relations and dimensions written by name, in one call. |
| `undo`, `redo`, `batch` | Every change the assistant makes is one undo step, and all or nothing: a call that fails changes nothing. `batch` makes several calls one step. |
| `drift_link` | Which drawing the tools work on: the one open in Drift, or the server's own. |

The renders mark up the drawing for the assistant. Curves are coloured by how well they are
defined: black when fully defined, blue when they can still move, red when constraints
conflict. Relations show as small green tags (H, V, //, L for perpendicular, = and so on), and
every curve is labelled with its id (`e12`). The assistant can then match what it sees to
what it lists.

Coordinates are millimetres with y up, whatever the drawing's display units. Angles are in
degrees, counter-clockwise. The exception is Drift script (below): a plain number there is in the
drawing's display unit, as when you type it into Drift.

## Drift scripts

Rather than many separate tool calls, the assistant usually writes a short
[Drift script](script.md) and runs it with `run_script`:

```
let W = 3.6 m
room bed W x 1.25*W at 0,0 walls 200 fixed
door d1 900 in bed.south centred
```

The whole script is one undo step, and all or nothing. The geometry has names (`bed.south`,
`d1.start`) instead of ids, so the assistant makes fewer mistakes. If a line is wrong, the
error says which line, how Drift read it, and how to fix it, and nothing changes. You can
write scripts yourself too: ask the assistant to run one, or to show you the script it used.

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

- With `--root`, the server can't read or write files outside that folder, even when it
  works through the live link. Without it, it can use any file your account can.
- The live link is off until you turn it on. Drift then listens only on your own computer
  (localhost). It writes a random key to a file in your user data folder, and a program
  must show that key to connect, so only programs running as you can edit your drawing.
- The server only edits the drawing it has open, and every change can be undone. Nothing is
  written to disk until the assistant saves or exports, and it only writes the files it is
  asked to (and, on saving, the drawing's Blender profiles file when it has profile
  blocks).
- A drawing from a newer Drift may hold things this version doesn't know. If saving would
  lose some of them, `save_drawing` saves nothing and says so; the assistant should ask you
  before it saves again with `anyway`.
