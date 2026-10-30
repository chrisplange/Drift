//! The tools' names, descriptions and JSON schemas, and how to use them together.

use serde_json::{Value, json};

/// A tool as an assistant sees it.
#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON schema of the arguments (an object).
    pub schema: Value,
    /// Looks without changing the drawing or any file.
    pub read_only: bool,
}

/// How to work with the tools, for the assistant's instructions.
pub const INSTRUCTIONS: &str = "\
Drift is a parametric 2D CAD sketcher. These tools create and edit one Drift drawing at a time.

Coordinates and lengths are millimetres with y up; angles are degrees counter-clockwise from +x. \
Ids: entities e12 (points and curves), constraints c3, layers l1, fills f2, texts t4.

Work the way Drift is meant to be used, not by computing exact coordinates:
1. To build or change geometry, write a Drift script and call run_script: rooms, walls, openings, \
grids, arrays, relations and dimensions by name, in one call that is one undo step (its description \
has the language). Use the single tools for looking, and for small fixes to existing ids: sketch \
approximate geometry with add_geometry (rough coordinates are fine; shapes that share a coordinate \
or a point id are joined).
2. State intent with relations and dimensions, in the script or with add_relations (horizontal, perpendicular, parallel, equal, coincident, \
symmetric, ...) and add_dimensions (lengths, distances, radii, angles; values may be numbers in \
mm or text with units and variables such as \"3.6 m\", \"W/2\", \"12'6\\\"\"). The solver moves the \
geometry to satisfy them.
3. Check: call analyze (degrees of freedom, conflicts, open ends, closed areas) and render (a \
PNG with ids, relation tags and colours: blue can still move, black is fully defined, red \
conflicts). Fix what's wrong and repeat. A drawing is done when analyze says it is fully defined \
(fix one point, e.g. a corner at the origin, to pin the whole sketch in place).

Every change is one undo step and all or nothing: a failed call changes nothing. Use undo to \
step back, and batch to make several calls one step. Save with save_drawing.";

fn coord() -> Value {
    json!({ "description": "[x, y] in mm", "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 })
}

fn point_ref() -> Value {
    json!({ "description": "[x, y] in mm, or the id of an existing point (\"e5\")", "anyOf": [
        { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
        { "type": "string" }
    ]})
}

fn id(what: &str) -> Value {
    json!({ "type": "string", "description": what })
}

fn ids(what: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": what })
}

fn value() -> Value {
    json!({ "description": "A number (mm, or degrees for angles) or text as typed in Drift: units, variables, expressions (\"3600\", \"3.6 m\", \"W/2 + 100\", \"12'6\\\"\")",
            "anyOf": [{ "type": "number" }, { "type": "string" }] })
}

fn object(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

/// Every tool.
pub fn tools() -> Vec<ToolSpec> {
    let layer = json!({ "type": "string", "description": "Layer id (\"l2\") or name" });
    let region = json!({ "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4,
        "description": "[min_x, min_y, max_x, max_y] in mm" });
    vec![
        ToolSpec {
            name: "new_drawing",
            description: "Start a new, empty drawing (the current one is discarded unless saved).",
            schema: object(
                json!({
                    "units": { "type": "string", "enum": ["mm", "cm", "m", "in", "ft", "ft-in", "in-frac"], "description": "Display units (coordinates stay mm)" },
                    "scale": { "type": "number", "description": "Drawing scale n of 1:n, for printed text and dimension sizes (e.g. 50)" }
                }),
                &[],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "open_drawing",
            description: "Open a .drift file. Returns the same summary as drawing_info.",
            schema: object(json!({ "path": { "type": "string" } }), &["path"]),
            read_only: false,
        },
        ToolSpec {
            name: "save_drawing",
            description: "Save the drawing as a .drift file (to `path`, or where it was opened or last saved). If the drawing came from a newer Drift and saving would lose some of what that build wrote (things this version doesn't know, on something since deleted), nothing is saved and the error says so: tell the user, and save with `anyway` only if they agree.",
            schema: object(
                json!({
                    "path": { "type": "string" },
                    "anyway": { "type": "boolean", "description": "Save even though things from a newer Drift will be lost." }
                }),
                &[],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "drawing_info",
            description: "Summary of the drawing: units, scale, bounds, counts, layers, variables, sheets, degrees of freedom.",
            schema: object(json!({}), &[]),
            read_only: true,
        },
        ToolSpec {
            name: "undo",
            description: "Undo the last change(s). Each tool call that changed the drawing is one step.",
            schema: object(json!({ "steps": { "type": "integer", "minimum": 1 } }), &[]),
            read_only: false,
        },
        ToolSpec {
            name: "redo",
            description: "Redo undone change(s).",
            schema: object(json!({ "steps": { "type": "integer", "minimum": 1 } }), &[]),
            read_only: false,
        },
        ToolSpec {
            name: "list_entities",
            description: "List curves (and optionally points) with their ids, defining point ids, coordinates, lengths, radii and layers.",
            schema: object(
                json!({
                    "kind": { "type": "string", "enum": ["point", "line", "circle", "arc", "ellipse", "ellipse_arc", "bezier"] },
                    "layer": layer,
                    "within": region,
                    "ids": ids("Only these entities"),
                    "include_points": { "type": "boolean", "description": "Also list points (default false)" },
                    "limit": { "type": "integer", "description": "Most to return (default 200)" }
                }),
                &[],
            ),
            read_only: true,
        },
        ToolSpec {
            name: "list_constraints",
            description: "List relations and dimensions (type, entities by role, value, expression, driven), all or those on given entities.",
            schema: object(
                json!({
                    "entities": ids("Only constraints on these entities"),
                    "type": { "type": "string", "description": "Only this type, e.g. distance or horizontal" }
                }),
                &[],
            ),
            read_only: true,
        },
        ToolSpec {
            name: "measure",
            description: "Measure without adding a dimension: a curve's length/radius/angle; distance between points (ids or coordinates), from a point to a line or circle; angle (and gap, if parallel) between two lines; or the area of the closed region around `inside`.",
            schema: object(
                json!({
                    "a": point_ref(), "b": point_ref(),
                    "inside": coord()
                }),
                &[],
            ),
            read_only: true,
        },
        ToolSpec {
            name: "add_geometry",
            description: "Add geometry; one undo step. Item types and their fields: \
point {at}; line {from, to}; polyline {points, closed}; rectangle {corner, width, height} or {from, to}; \
polygon {center, radius, sides, rotation} (regular, on a construction circle); circle {center, radius}; \
arc {center, start, end} (counter-clockwise) or {start, through, end}; ellipse {center, major (end of the major semi-axis), minor_radius}; \
bezier {from, c1, c2, to}. Positions are [x, y] or existing point ids; equal coordinates share one point, so shapes join. \
Lines within 1 degree of level or plumb get horizontal/vertical relations (auto_relations). Returns the new ids by role (a rectangle's sides bottom/right/top/left and corners).",
            schema: object(
                json!({
                    "items": { "type": "array", "items": { "type": "object", "properties": {
                        "type": { "type": "string", "enum": ["point", "line", "polyline", "rectangle", "polygon", "circle", "arc", "ellipse", "bezier"] },
                        "at": point_ref(), "from": point_ref(), "to": point_ref(),
                        "points": { "type": "array", "items": point_ref() }, "closed": { "type": "boolean" },
                        "corner": coord(), "width": { "type": "number" }, "height": { "type": "number" },
                        "center": point_ref(), "radius": { "type": "number" }, "sides": { "type": "integer" }, "rotation": { "type": "number" },
                        "start": point_ref(), "end": point_ref(), "through": coord(),
                        "major": coord(), "minor_radius": { "type": "number" },
                        "c1": coord(), "c2": coord()
                    }, "required": ["type"] } },
                    "layer": layer,
                    "construction": { "type": "boolean", "description": "Make the curves construction geometry (reference only; doesn't print)" },
                    "auto_relations": { "type": "boolean", "description": "Default true" },
                    "join": { "type": "boolean", "description": "Share points with existing points at the same coordinates (default true)" }
                }),
                &["items"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "add_relations",
            description: "Add geometric relations; one undo step. Types and fields: coincident {a, b points}; horizontal / vertical {line} or {a, b points}; \
parallel, perpendicular, equal, collinear {a, b lines} (equal and concentric/coradial/tangent also take circles and arcs); tangent {a, b} (b may also be an elliptical arc or curve that a ends on: tangent at that end); \
concentric, coradial {a, b}; on_curve {point, curve}; symmetric {a, b points, axis line}; midpoint {point, line}; fix {point, at?} (pins it; defaults to where it is). \
Relations already implied are skipped; one that conflicts fails the call.",
            schema: object(
                json!({
                    "relations": { "type": "array", "items": { "type": "object", "properties": {
                        "type": { "type": "string", "enum": ["coincident", "horizontal", "vertical", "parallel", "perpendicular", "equal", "tangent", "concentric", "coradial", "collinear", "on_curve", "symmetric", "midpoint", "fix"] },
                        "a": { "type": "string" }, "b": { "type": "string" }, "line": { "type": "string" },
                        "point": { "type": "string" }, "curve": { "type": "string" }, "axis": { "type": "string" },
                        "at": coord()
                    }, "required": ["type"] } }
                }),
                &["relations"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "add_dimensions",
            description: "Add driving dimensions, which move the geometry to their values; one undo step. Types: length {line}; distance {a, b} (points or lines' midpoints; a point and a line gives the perpendicular distance); \
horizontal / vertical {a, b} or {line}; point_line {point, line}; radius / diameter {curve}; angle {a, b lines} (counter-clockwise from a to b). \
`value` links to variables when it uses them. A dimension that would over-define the sketch fails unless driven: true (a reference that only shows the measurement).",
            schema: object(
                json!({
                    "dimensions": { "type": "array", "items": { "type": "object", "properties": {
                        "type": { "type": "string", "enum": ["length", "distance", "horizontal", "vertical", "point_line", "radius", "diameter", "angle"] },
                        "a": { "type": "string" }, "b": { "type": "string" }, "line": { "type": "string" },
                        "point": { "type": "string" }, "curve": { "type": "string" },
                        "value": value(),
                        "driven": { "type": "boolean" },
                        "label_offset": { "description": "Where the label sits, as an offset in mm from the measured thing", "type": "array", "items": { "type": "number" } }
                    }, "required": ["type"] } }
                }),
                &["dimensions"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "set_dimension",
            description: "Change a dimension's value (or expression), or make it driven/driving.",
            schema: object(
                json!({ "id": id("Dimension id, e.g. c7"), "value": value(), "driven": { "type": "boolean" } }),
                &["id"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "remove_constraints",
            description: "Remove relations or dimensions by id.",
            schema: object(json!({ "ids": ids("Constraint ids") }), &["ids"]),
            read_only: false,
        },
        ToolSpec {
            name: "set_variables",
            description: "Create or change variables (parameters) that dimensions can use, e.g. W = 3600, D = W * 0.75; or remove them. Dimensions linked to a variable follow it.",
            schema: object(
                json!({
                    "set": { "type": "array", "items": { "type": "object", "properties": {
                        "name": { "type": "string" }, "value": value(),
                        "kind": { "type": "string", "enum": ["length", "angle", "number"] }
                    }, "required": ["name", "value"] } },
                    "remove": { "type": "array", "items": { "type": "string" } }
                }),
                &[],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "move_point",
            description: "Drag a point to a location; its relations pull the rest along. Fails if dimensions hold it elsewhere.",
            schema: object(
                json!({ "point": id("Point id"), "to": coord() }),
                &["point", "to"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "transform",
            description: "Move, rotate, scale or mirror curves, or array them. ops: move {by}; rotate {angle, about?}; scale {factor (or [fx, fy]), about?} (about defaults to the centre); \
mirror {axis line, keep_original (default true)}; linear_array {by, count}; circular_array {about, count, angle (total, default 360)}. `copy` transforms a copy (move, rotate, scale).",
            schema: object(
                json!({
                    "op": { "type": "string", "enum": ["move", "rotate", "scale", "mirror", "linear_array", "circular_array"] },
                    "ids": ids("Curves to transform"),
                    "by": coord(), "angle": { "type": "number" }, "about": coord(),
                    "factor": { "anyOf": [{ "type": "number" }, { "type": "array", "items": { "type": "number" } }] },
                    "axis": id("Mirror line"), "keep_original": { "type": "boolean" },
                    "count": { "type": "integer" }, "copy": { "type": "boolean" }
                }),
                &["op", "ids"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "offset",
            description: "Offset a curve and the chain it joins (e.g. a wall outline) by a distance towards `side`.",
            schema: object(
                json!({ "id": id("A curve of the chain"), "distance": { "type": "number" }, "side": coord() }),
                &["id", "distance", "side"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "trim_extend_split",
            description: "trim: remove the piece of a curve around `at` between its crossings with other curves; extend: lengthen the end nearest `at` to the next curve; split: cut it in two at `at`. \
`against` (curve ids): only those cut, or are reached by an extend (a curve of a shape stands for the whole shape). \
`remove: inside|outside` with `by` (curves of a closed shape) and no `at`: remove every piece of the shape of `id` (and `ids`) lying inside or outside that shape, as Trim would piece by piece. \
`and_remove` then does the same to the shape `by`, against the first shape as it was. Union: remove inside, and_remove inside. The first minus `by`: inside, outside. The overlap: outside, outside. \
Two filled shapes trimmed into one outline keep one fill, the top one's.",
            schema: object(
                json!({
                    "id": id("Curve"),
                    "at": coord(),
                    "mode": { "type": "string", "enum": ["trim", "extend", "split"] },
                    "against": { "type": "array", "items": { "type": "string" }, "description": "Curve ids: only these cut (trim) or are reached (extend)" },
                    "remove": { "type": "string", "enum": ["inside", "outside"], "description": "Trim the shape of `id` where it lies inside or outside `by` (no `at`)" },
                    "by": { "type": "array", "items": { "type": "string" }, "description": "Curve ids of the closed shape to trim by" },
                    "ids": { "type": "array", "items": { "type": "string" }, "description": "More curves trimmed with `id`" },
                    "and_remove": { "type": "string", "enum": ["inside", "outside"], "description": "Then trim `by` where it lies inside or outside the first shape as it was" },
                    "inside_of": { "type": "array", "items": { "type": "array", "items": coord() }, "description": "The inside of `by` as closed polygons, when `by` is no longer closed (trimmed open by an earlier call)" }
                }),
                &["id"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "fillet_chamfer",
            description: "Round (radius) or cut (chamfer: a distance or [d1, d2]) the corner where two curves meet, or several corners in one step. Dimensions and relations on a corner stay: the corner is kept as a construction point held where the two curves cross (a `corner` relation), and a midpoint or equal length on a side keeps measuring it from corner to corner.",
            schema: object(
                json!({
                    "corner": { "description": "A corner ([x, y] in mm, or a point id), or a list of corners", "anyOf": [
                        point_ref(),
                        { "type": "array", "items": point_ref(), "minItems": 1 }
                    ]},
                    "radius": { "type": "number" },
                    "chamfer": { "anyOf": [{ "type": "number" }, { "type": "array", "items": { "type": "number" } }] }
                }),
                &["corner"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "delete",
            description: "Delete entities (with their relations), constraints, fills or texts by id.",
            schema: object(json!({ "ids": ids("Ids: e.., c.., f.., t..") }), &["ids"]),
            read_only: false,
        },
        ToolSpec {
            name: "set_construction",
            description: "Make curves construction geometry (reference lines that don't print or bound areas), or normal again. `style` centre_line gives dash-dot lines that print but don't bound areas (grids, axes).",
            schema: object(
                json!({ "ids": ids("Curves"), "construction": { "type": "boolean" },
                        "style": { "type": "string", "enum": ["solid", "construction", "centre_line"], "description": "Instead of `construction`" } }),
                &["ids"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "layers",
            description: "Create, change or delete layers, move entities onto one, or choose the current layer (where new geometry goes).",
            schema: object(
                json!({
                    "create": { "type": "array", "items": { "type": "object", "properties": {
                        "name": { "type": "string" }, "color": { "type": "string", "description": "#rrggbb" },
                        "weight_mm": { "type": "number" }, "line_type": { "type": "string", "enum": ["continuous", "dashed", "hidden", "center", "phantom"] },
                        "visible": { "type": "boolean" }, "locked": { "type": "boolean" }
                    }, "required": ["name"] } },
                    "update": { "type": "array", "items": { "type": "object", "description": "{layer, ...properties as in create}" } },
                    "delete": { "type": "array", "items": { "type": "string" } },
                    "assign": { "type": "object", "properties": { "layer": { "type": "string" }, "ids": { "type": "array", "items": { "type": "string" } } } },
                    "current": layer
                }),
                &[],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "add_fill",
            description: "Fill or hatch the closed area around a point. Built-in hatch patterns: ANSI31 (diagonal, the default) to ANSI38, BRICK, BLOCK, CONCRETE, DOTS, EARTH, GLASS, GRID, INSULATION, SAND, STEEL, WOOD; the drawing may have imported more. Hatch `scale` multiplies the pattern's spacing. An area that is already filled gets no second fill: its fill takes the new style, and the result says `restyled`.",
            schema: object(
                json!({
                    "at": coord(), "kind": { "type": "string", "enum": ["solid", "hatch"] },
                    "pattern": { "type": "string" }, "scale": { "type": "number" }, "angle": { "type": "number" },
                    "color": { "type": "string", "description": "#rrggbb" }, "opacity": { "type": "number" },
                    "layer": layer
                }),
                &["at"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "add_text",
            description: "Add a line (or lines) of text. Height is the capital height in mm (default: readable when printed at the drawing scale).",
            schema: object(
                json!({
                    "text": { "type": "string" }, "at": coord(), "height": { "type": "number" },
                    "angle": { "type": "number" }, "align": { "type": "string", "enum": ["left", "center", "right"] },
                    "layer": layer
                }),
                &["text", "at"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "import_file",
            description: "Import an SVG, PDF, Illustrator (.ai), DXF or DWG file as editable geometry or as an underlay to trace over. `scale` is model mm per paper mm (50 or \"1:50\" for a plan drawn at 1:50); for DXF and DWG it multiplies the file's own units.",
            schema: object(
                json!({
                    "path": { "type": "string" },
                    "scale": { "anyOf": [{ "type": "number" }, { "type": "string" }] },
                    "as": { "type": "string", "enum": ["geometry", "underlay"] },
                    "page": { "type": "integer", "description": "PDF/AI page (default 1)" },
                    "offset": coord(), "layer": layer
                }),
                &["path"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "render",
            description: "Look at the drawing: returns a PNG of the whole drawing or a region, with dimensions, relation tags, entity id labels and definition colours, plus the pixel scale to map pixels back to coordinates. Use it after changes to check the result.",
            schema: object(
                json!({
                    "region": region,
                    "width": { "type": "integer", "description": "Pixels along the longer side (default 1024)" },
                    "dimensions": { "type": "boolean", "description": "Default true" },
                    "relations": { "type": "boolean", "description": "Relation tags (default true)" },
                    "labels": { "type": "string", "enum": ["none", "curves", "all"], "description": "Entity id labels: curves (default) or curves and points" },
                    "dof_colors": { "type": "boolean", "description": "Colour curves by definition (default true)" },
                    "points": { "type": "boolean", "description": "Dots at curve ends (default true)" }
                }),
                &[],
            ),
            read_only: true,
        },
        ToolSpec {
            name: "analyze",
            description: "Check the sketch: degrees of freedom and which entities can still move, conflicting and redundant constraints, collapsed or overlapping curves, open ends, and closed regions with areas (mm² and m²), perimeters, centroids and boundary curves.",
            schema: object(json!({}), &[]),
            read_only: true,
        },
        ToolSpec {
            name: "export",
            description: "Export to a file: pdf (every sheet; with no sheets, one A3 sheet with the drawing fitted), svg (one sheet, `sheet` number from 1), dwg or dxf (model space at full size, AutoCAD 2018: layers, blocks, fills and hatches), dxf-r12 (the same as DXF R12, lines and arcs only, for laser cutters and old software) or png (as render).",
            schema: object(
                json!({
                    "path": { "type": "string" },
                    "format": { "type": "string", "enum": ["pdf", "svg", "dwg", "dxf", "dxf-r12", "png"], "description": "Default: from the file's extension" },
                    "sheet": { "type": "integer" },
                    "region": region, "width": { "type": "integer" }
                }),
                &["path"],
            ),
            read_only: false,
        },
        ToolSpec {
            name: "run_script",
            description: super::script::DESCRIPTION,
            schema: super::script::schema(),
            read_only: false,
        },
        ToolSpec {
            name: "batch",
            description: "Run several tool calls in order as one undo step; if one fails, none of them apply. Each call is {tool, args}.",
            schema: object(
                json!({
                    "calls": { "type": "array", "items": { "type": "object", "properties": {
                        "tool": { "type": "string" }, "args": { "type": "object" }
                    }, "required": ["tool"] } }
                }),
                &["calls"],
            ),
            read_only: false,
        },
    ]
}
