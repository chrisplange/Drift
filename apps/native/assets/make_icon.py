"""Draw Drift's app icon: icon.png (256 px, the window icon) and icon.ico (for the
installer and the executable). Pure Python, no dependencies:

    python apps/native/assets/make_icon.py

The mark is the same drawing as `icons::logo` in drift-app: a "D" made of a line and an arc
joined at two points, on a blue rounded tile.
"""

import math
import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
TOP = (0x2e, 0x9b, 0xff)
BOTTOM = (0x00, 0x6a, 0xe6)
WHITE = (255, 255, 255)


def clamp(v, lo=0.0, hi=1.0):
    return max(lo, min(hi, v))


def render(size):
    """RGBA bytes of the icon at `size` pixels, antialiased from signed distances."""
    s = float(size)
    radius = 0.22 * s
    width = max(0.085 * s, 1.6)
    dot = max(0.075 * s, 1.4)
    lx, y0, y1 = 0.38 * s, 0.26 * s, 0.74 * s
    cy, bowl = 0.5 * s, 0.24 * s
    out = bytearray()
    for j in range(size):
        y = j + 0.5
        t = y / s
        blue = tuple(TOP[k] + (BOTTOM[k] - TOP[k]) * t for k in range(3))
        for i in range(size):
            x = i + 0.5
            # The rounded tile.
            qx = abs(x - s / 2) - (s / 2 - radius)
            qy = abs(y - s / 2) - (s / 2 - radius)
            tile = math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - radius
            alpha = clamp(0.5 - tile)
            # The stroke: the upright, then the bowl (the right half of a circle).
            upright = math.hypot(x - lx, y - clamp(y, y0, y1))
            if x >= lx:
                arc = abs(math.hypot(x - lx, y - cy) - bowl)
            else:
                arc = min(math.hypot(x - lx, y - y0), math.hypot(x - lx, y - y1))
            stroke = min(upright, arc) - width / 2
            # The two points where they join.
            points = min(math.hypot(x - lx, y - y0), math.hypot(x - lx, y - y1)) - dot
            white = clamp(0.5 - min(stroke, points))
            rgb = [blue[k] + (WHITE[k] - blue[k]) * white for k in range(3)]
            out += bytes(int(round(c)) for c in rgb) + bytes([int(round(alpha * 255))])
    return bytes(out)


def png(size, rgba):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    rows = b"".join(b"\x00" + rgba[y * size * 4:(y + 1) * size * 4] for y in range(size))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )


def ico(images):
    """An .ico holding PNG images, as (size, png bytes) pairs."""
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = len(header) + 16 * len(images)
    entries, blobs = b"", b""
    for size, data in images:
        entries += struct.pack(
            "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset
        )
        offset += len(data)
        blobs += data
    return header + entries + blobs


if __name__ == "__main__":
    images = [(n, png(n, render(n))) for n in (16, 24, 32, 48, 64, 128, 256)]
    with open(os.path.join(HERE, "icon.png"), "wb") as f:
        f.write(images[-1][1])
    with open(os.path.join(HERE, "icon.ico"), "wb") as f:
        f.write(ico(images))
    print("wrote icon.png and icon.ico")
