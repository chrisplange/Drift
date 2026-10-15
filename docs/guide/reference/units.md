# Units and length formats

Each drawing has its own display units, saved in its file. Drift stores every length in
millimetres internally, so **changing the units never moves anything**: it only changes how
values are shown and how bare numbers you type are read.

Change units with the status bar's units menu (it shows **mm**, **in**, **ft-in** and so on),
in **Parameters → Units**, or in **Edit → Preferences… → Units**. New drawings start in
millimetres.

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
| `30deg`, `30°`, `pi/4 rad` | angles (bare numbers are degrees) |
| `100mm + 2in` | mixed units in an expression |

Note that `1'-6"` (no spaces) is one length, 1 foot 6 inches. With spaces, `2' - 6"` is a
subtraction, as is `10-1/2` without an inch mark.

A unit after a term applies to the whole product before it: `100/2 mm` is 50 mm. See
[Parameters](../tutorials/03-parameters.md#expressions) for the rest of the expression
language.

## Sheet scales

Sheet views offer metric scales (10:1 down to 1:10000), architectural scales (3" = 1'-0"
down to 1/32" = 1'-0") and engineering scales (1" = 10' to 1" = 200'), with the drawing's
own system listed first. Imperial scales are written in full on title blocks and view
titles, such as `1/4" = 1'-0"`.

## Drafting units

Dimension text height, arrow size and the like are set in paper millimetres in
**Parameters → Drafting**, whatever the drawing's units. **Dual units** adds the value in a
second unit, in brackets.

## DXF

DXF files are written in the drawing's display units (inches for both fraction formats),
so lengths read the same in other programs. Imported DXF files use the units they declare;
a file that doesn't say is read in the display unit.
