# The sketch corpus

The solver bar's item 8 (#187): drawings as people make them, swept drag by drag. The
random generator behind the heavy matrix and the censuses (`plan()` in
`crates/drift-edit/src/drag_props.rs`) draws lines and arcs and eleven kinds of relation;
these hold every relation and dimension kind, circles, ellipses and curves.

| Group | Sketches |
|---|---|
| `profile-` | sections and outlines: slot, L angle, T, channel, rounded rectangle, keyhole, D, stepped shaft, dovetail, I beam, ogee, a filleted corner |
| `bracket-` | plates with holes: L bracket, bolt circle, symmetric flange, gusset, slotted plate, hinge leaf, shelf bracket, clamp, lug, a bushing (a ring with its wall set as a gap) |
| `linkage-` | mechanisms with one or two freedoms: four-bar, crank and slider, scissor, pantograph, toggle, bell crank, cam and follower, Watt's linkage, belt, piston |
| `plan-` | rooms with walls, doors, windows and openings, free-standing walls, a corridor, a column grid |
| `pattern-` | rows, grids and rings of holes, a mirrored pair, evenly spaced slats, a hexagon |
| others | ellipses with points riding on them, curves meeting smoothly, arcs on one circle, point-to-line dimensions |
| `real-bar-shelves` | a real drawing (447 points and curves, 73 relations), geometry and relations only |

Each sketch is a Drift script (`<name>.driftscript`) and the drawing it makes
(`<name>.drift`). The sweep (`crates/drift-edit/src/drag_props/corpus.rs`) reads the
drawings. Most are left with a few degrees of freedom on purpose: a fully defined sketch has
nothing to drag.

## Adding or changing a sketch

Write or edit the script, then build the drawings and bring the baseline up to date:

```
DRIFT_CORPUS_WRITE=1 cargo test -p drift-app --lib corpus_build
python scripts/solver-bar/corpus.py run --update
```

`cargo test -p drift-app --lib corpus_build` (in the normal test run) fails when a drawing
isn't what its script makes, when a sketch is fully defined, or when the corpus no longer
holds every relation kind. The kinds come from `Constraint` itself (a match with no catch-all
arm), so a new relation kind doesn't compile until it is named in `corpus_build.rs`, and
then fails the test until a sketch holds it. `DRIFT_CORPUS_LIST=1` with `--nocapture` prints each sketch's
size and degrees of freedom, and the count of each relation kind.

`--update` takes a whole sweep only: it refuses one narrowed by `CORPUS_ONLY`,
`CORPUS_TARGET`, `CORPUS_PATH` or `CORPUS_PRIVATE_ONLY`, or one that misses a target the
baseline has.

## The baseline on each platform

`corpus-baseline.txt` is measured on one Windows PC. Round-off decides a drag at a fold and
libm differs between platforms, so `corpus-baseline.<platform>.txt` holds the rows that
differ elsewhere, and `run` says which files it judged against. The Linux rows are CI's
(`Sketch corpus (ubuntu)`, pinned to ubuntu-24.04); write them from that job's record, with
no Linux machine needed:

```
gh run download <run> -n solver-bar-corpus-linux -D target/corpus-linux
python scripts/solver-bar/corpus.py baseline --from target/corpus-linux/record.txt --platform linux
```

Only one machine per platform is measured. Another Windows CPU, a Mac, a new runner image,
toolchain or glibc may differ by a row or two the same way; such rows come from that
machine's record with `baseline --from`, not from a code change.

A script's `#>` lines are comments to the script language and instructions to the builder,
for what the language has no statement for:

```
#> el = add_geometry {"items":[{"type":"ellipse","center":[0,0],"major":[40,0],"minor_radius":25}]}
#> add_relations {"relations":[{"type":"on_curve","point":"$pa","curve":"$el"}]}
#> constraint smooth $ca.b $ca.c2 $cb.c1
```

The first two are tool calls as the AI link takes them, with `$name` standing for a name of
the script, or one bound by `name = add_geometry …` (the first shape made; `name.center`
and the like for its points). `constraint` adds a relation no tool makes: `smooth`,
`corner`, `equal_distance`, `midpoint_of`, `fix_radius`, `fix_x`, `fix_y`.

Two things the script language does that shape a sketch: a line drawn level or plumb, or
within about a degree of it, is given that relation; and `fixed` on a line pins its start
only.

A real drawing comes in as a fixture, with none of its file's own details (project, logo,
fonts, sheets, texts, fills):

```
DRIFT_CORPUS_IMPORT=<file.drift> DRIFT_CORPUS_AS=real-<name> cargo test -p drift-app --lib corpus_import -- --ignored
```

## Private drawings

Set `DRIFT_CORPUS_PRIVATE` to a folder of `.drift` files and the sweep takes them too, as
`private-` and eight hex digits of a hash of the file's contents (so a drawing keeps its number
when others are added or removed, and `diff` of two records compares the same drawing). They
are never committed, never named, and their results are
reported in aggregate only: this repository is public.
