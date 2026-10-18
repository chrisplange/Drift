"""Write the website's logo and favicon as SVG, from the same drawing as the app icon:

    python website/scripts/make_logo.py

The D's contours come from apps/native/assets/make_icon.py, so the logo stays in step
with the icon. It writes, into website/public:

- logo.svg     the white D on its charcoal tile with a hairline rim
- logo-mark.svg  the D alone in `currentColor`, for use inline
"""

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "apps", "native", "assets"))

import make_icon  # noqa: E402

S = 256


def path_data():
    rings = make_icon.letter(S)
    parts = []
    for ring in rings:
        pts = ring[:-1] if ring[0] == ring[-1] else ring
        head, *rest = pts
        parts.append(
            f"M{head[0]:.1f} {head[1]:.1f}"
            + "".join(f"L{x:.1f} {y:.1f}" for x, y in rest)
            + "Z"
        )
    return "".join(parts)


def hexc(rgb):
    return "#%02x%02x%02x" % rgb


def main():
    d = path_data()
    out = os.path.join(ROOT, "website", "public")
    tile = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {S} {S}">'
        f'<rect x="1" y="1" width="{S - 2}" height="{S - 2}" rx="{S * 0.225:.1f}" '
        f'fill="{hexc(make_icon.TILE)}" stroke="{hexc(make_icon.RIM)}" stroke-width="2"/>'
        f'<path fill="{hexc(make_icon.WHITE)}" fill-rule="evenodd" d="{d}"/></svg>\n'
    )
    mark = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {S} {S}">'
        f'<path fill="currentColor" fill-rule="evenodd" d="{d}"/></svg>\n'
    )
    for name, text in (("logo.svg", tile), ("logo-mark.svg", mark)):
        with open(os.path.join(out, name), "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        print("wrote", name)


if __name__ == "__main__":
    main()
