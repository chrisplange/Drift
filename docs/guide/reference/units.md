# Units and length formats

Each drawing has its own display units, saved in its file. Drift stores every length in
millimetres internally, so **changing the units never moves anything**: it only changes how
values are shown and how bare numbers you type are read.

Change units with the status bar's units menu (it shows **mm**, **in**, **ft-in** and so on),
or in **File → Drawing settings… → Units**. New drawings start in
millimetres; the start panel's **Metric** / **Imperial** switch starts one in feet and inches
instead.

## Length formats

| System | Format | Looks like | Bare numbers mean |
|---|---|---|---|
| Metric | **Millimetres** | `12.5` | millimetres |
| Metric | **Centimetres** | `1.25` | centimetres |
| Metric | **Metres** | `0.0125` | metres |
| Imperial | **Decimal inches** | `12.375"` | inches |
| Imperial | **Inches (fractions)** | `35 1/2"`, `108"` | inches |
| Imperial | **Decimal feet** | `1.031'` | feet |
| Imperial | **Feet-inches (fractions)** | `1'-6 1/2"` | inches |

**Inches (fractions)** never splits into feet, which suits millwork and shop drawings.
**Feet-inches (fractions)** is the architectural format.

Settings for each:

- **Decimals** (0 to 8) for the decimal formats. Dimensions drop trailing zeros.
- **Round to** for the fraction formats: 1/2", 1/4", 1/8", 1/16", 1/32" or 1/64".
  Fractions are reduced (4/8 shows as 1/2).
- **Show 0'** (feet-inches): `0'-6"` instead of `6"`.
- **Angles**: decimals for angles, in degrees.

When a dimension's value is rounded for display (a metric length shown in sixteenths, for
example), it's marked with `~`. Rounding never changes the stored value, and converting
between units is exact (an inch is exactly 25.4 mm).

The grid follows the units: 1 / 10 / 100 mm steps in metric; 1/16", 1", 1' and so on in
imperial.

## Typing values

You can type any unit, whatever the display units:

| You type | Means |
|---|---|
| `25` | 25 in the display unit (inches for both fraction formats) |
| `25mm`, `2.5cm`, `0.025m` | metric lengths |
| `2in`, `3"` | inches |
| `2ft`, `2'` | feet |
| `1'6"`, `1'-6"`, `1'-6 1/2"` | feet and inches |
| `6-1/2"`, `3/8"` | fractions of an inch |
| `35 1/2` | a mixed number in the display unit |
| `W + 60`, `spacing - 10` | bare numbers added to or taken from a length are in the display unit too, and are stored with it (`W + 60 mm`); in a product a bare number is just a factor (`W * 2`) |
| `30deg`, `30°`, `pi/4 rad` | angles (bare numbers are degrees) |
| `100mm + 2in` | mixed units in an expression |

Note that `1'-6"` (no spaces) is one length, 1 foot 6 inches. With spaces, `2' - 6"` is a
subtraction, as is `10-1/2` without an inch mark.

A unit after a term applies to the whole product before it: `100/2 mm` is 50 mm. See
[Parameters](../tutorials/03-parameters.md#expressions) for the rest of the expression
language.

## Sheet scales

The drawing scale (**File → Drawing settings… → Drafting → Scale**), sheet views and
underlays share one scale menu, with the drawing's own system listed first:

| Group | Scales |
|---|---|
| Architectural | 2" = 1", 1" = 1" (full size), 6" = 1'-0", 3" = 1'-0", 1-1/2" = 1'-0", 1" = 1'-0", 3/4" = 1'-0", 1/2" = 1'-0", 3/8" = 1'-0", 1/4" = 1'-0", 3/16" = 1'-0", 1/8" = 1'-0", 3/32" = 1'-0", 1/16" = 1'-0", 1/32" = 1'-0" |
| Engineering | 1" = 10', 1" = 20', 1" = 30', 1" = 40', 1" = 50', 1" = 60', 1" = 100', 1" = 200' |
| Metric | 10:1, 5:1, 2:1, 1:1, 1:2, 1:2.5, 1:5, 1:10, 1:20, 1:25, 1:50, 1:75, 1:100, 1:125, 1:200, 1:250, 1:500, 1:1000, 1:2000, 1:2500, 1:5000, 1:10000 |

**Custom**, at the foot of the menu, takes any scale: type it and press Enter.

- `1:75`, `2:1` or just `75` (1:75).
- Paper `=` model: `3/8" = 1'-0"`, `1/2"=1'`, `1/4 = 1'` (a bare paper length is in
  inches when the model one is in feet or inches), `1" = 20'`, `2" = 1"`, `10 mm = 1 m`.

A scale from 1000:1 to 1:100000 is accepted. In a drawing in feet and inches, scales are
written the architectural way on title blocks and view titles, such as `1/4" = 1'-0"`,
`6" = 1'-0"` for half size and `1" = 1"` for full size; a custom one is too when it has a
whole number of sixteenths to the foot (`5" = 1'-0"`), and is otherwise written `1:n`.

## Drafting units

Dimension text height, arrow size and the like are set in paper millimetres in
**File → Drawing settings… → Drafting**, whatever the drawing's units. **Dual units** adds the value in a
second unit, in brackets. Text heights are cap heights: 3.5 mm text has capitals 3.5 mm
high on paper.

New text (the **Text** tool, a paste, the AI link and scripts) is the drafting text height
at the drawing scale: 3.5 mm at 1:50 makes text 175 mm high in the model, which prints
3.5 mm high on a 1:50 sheet. The **Text** tool's **Height** is that paper height; change it
there for the next texts, or click **Default** to follow the drawing again.

## DXF and DWG

DWG and DXF files are written in the drawing's display units (inches for both fraction formats),
so lengths read the same in other programs. Imported DXF files use the units they declare;
a file that doesn't say is read in the display unit.
