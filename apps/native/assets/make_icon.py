"""Draw Drift's app icon. Pure Python, no dependencies:

    python apps/native/assets/make_icon.py

It writes, beside this file:

- icon.png      256 px, the window icon
- icon.ico      16 to 256 px, for the Windows executable and installer
- icon-mac.png  1024 px with the margin macOS icons carry, for the .icns

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


def render(size, margin=0.0):
    """RGBA bytes of the icon at `size` pixels, antialiased from signed distances.
    `margin` is the clear space round the tile, as a fraction of the size."""
    off = margin * size
    s = float(size) - 2.0 * off
    radius = 0.22 * s
    width = max(0.085 * s, 1.6)
    dot = max(0.075 * s, 1.4)
    lx, y0, y1 = 0.38 * s, 0.26 * s, 0.74 * s
    cy, bowl = 0.5 * s, 0.24 * s
    out = bytearray()
    for j in range(size):
        y = j + 0.5 - off
        t = clamp(y / s)
        blue = tuple(TOP[k] + (BOTTOM[k] - TOP[k]) * t for k in range(3))
        for i in range(size):
            x = i + 0.5 - off
            # The rounded tile.
            qx = abs(x - s / 2) - (s / 2 - radius)
            qy = abs(y - s / 2) - (s / 2 - radius)
            tile = math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - radius
            alpha = clamp(0.5 - tile)
            if alpha <= 0.0:
                out += b"\x00\x00\x00\x00"
                continue
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


def dib(size, rgba):
    """An icon image as a 32-bit bitmap (what the resource compiler expects below 256 px):
    header, rows bottom-up as BGRA, then an all-clear 1-bit mask."""
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    rows = bytearray()
    for y in range(size - 1, -1, -1):
        row = rgba[y * size * 4:(y + 1) * size * 4]
        for x in range(size):
            r, g, b, a = row[x * 4:x * 4 + 4]
            rows += bytes((b, g, r, a))
    mask = bytes(((size + 31) // 32) * 4 * size)
    return header + bytes(rows) + mask


def ico(images):
    """An .ico from (size, image bytes) pairs."""
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
    sizes = (16, 24, 32, 48, 64, 128, 256)
    drawn = {n: render(n) for n in sizes}
    with open(os.path.join(HERE, "icon.png"), "wb") as f:
        f.write(png(256, drawn[256]))
    # Bitmaps for the small sizes, PNG for 256 (the form Windows tools all accept).
    images = [(n, png(n, drawn[n]) if n == 256 else dib(n, drawn[n])) for n in sizes]
    with open(os.path.join(HERE, "icon.ico"), "wb") as f:
        f.write(ico(images))
    # macOS icons sit inside a margin of about a tenth of the canvas.
    with open(os.path.join(HERE, "icon-mac.png"), "wb") as f:
        f.write(png(1024, render(1024, margin=0.098)))
    print("wrote icon.png, icon.ico and icon-mac.png")
