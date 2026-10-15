"""Draw Drift's app icon. Pure Python, no dependencies:

    python apps/native/assets/make_icon.py

It writes, beside this file:

- icon.png      256 px, the window icon
- icon.ico      16 to 256 px, for the Windows executable and installer
- icon-mac.png  1024 px with the margin macOS icons carry, for the .icns

The mark is the same drawing as `icons::logo` in drift-app: a bold white Swiss sans-serif
"D" (a straight stem, flat bars and a squarish super-elliptic bowl), centred on a flat
charcoal tile with a hairline rim.
"""

import math
import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
TILE = (0x16, 0x18, 0x1C)
RIM = (0x2C, 0x30, 0x37)
WHITE = (0xF4, 0xF5, 0xF7)


def clamp(v, lo=0.0, hi=1.0):
    return max(lo, min(hi, v))


def contour(x0, y0, y1, xb, rx, ry, cy, n=2.6, k=96):
    """A D's closed contour: the stem side, the top bar, a super-elliptic bowl (exponent
    `n`, squarer than a circle) and the bottom bar, with sharp corners."""
    pts = [(xb, y1), (x0, y1), (x0, y0), (xb, y0)]
    for i in range(1, k):
        t = -math.pi / 2 + math.pi * i / k
        c, sn = math.cos(t), math.sin(t)
        pts.append(
            (
                xb + rx * abs(c) ** (2 / n) * (1 if c >= 0 else -1),
                cy + ry * abs(sn) ** (2 / n) * (1 if sn >= 0 else -1),
            )
        )
    pts.append((xb, y1))
    return pts


def letter(s):
    """The D's outer and inner contours in an `s`-sized tile, centred on it. Keep in step
    with `icons::logo`."""
    x0, y0, y1 = 0.305 * s, 0.245 * s, 0.755 * s
    h = y1 - y0
    w = 0.80 * h
    ry, rx = h / 2, 0.60 * w
    xb, cy = x0 + w - rx, (y0 + y1) / 2
    t = 0.15 * s  # the stroke weight
    outer = contour(x0 - t / 2, y0 - t / 2, y1 + t / 2, xb, rx + t / 2, ry + t / 2, cy)
    inner = contour(x0 + t / 2, y0 + t / 2, y1 - t / 2, xb, rx - t / 2, ry - t / 2, cy)
    xs, ys = [p[0] for p in outer], [p[1] for p in outer]
    dx = s / 2 - (min(xs) + max(xs)) / 2
    dy = s / 2 - (min(ys) + max(ys)) / 2
    return [[(x + dx, y + dy) for x, y in ring] for ring in (outer, inner)]


def seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    l2 = dx * dx + dy * dy
    t = 0.0 if l2 == 0 else clamp(((px - ax) * dx + (py - ay) * dy) / l2)
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def inside(px, py, ring):
    c = False
    for (ax, ay), (bx, by) in zip(ring, ring[1:]):
        if (ay > py) != (by > py) and px < ax + (py - ay) * (bx - ax) / (by - ay):
            c = not c
    return c


def render(size, margin=0.0):
    """RGBA bytes of the icon at `size` pixels, antialiased from signed distances.
    `margin` is the clear space round the tile, as a fraction of the size."""
    off = margin * size
    s = float(size) - 2.0 * off
    radius = 0.225 * s
    rim = max(s / 256, 1.0)
    outer, inner = letter(s)
    segs = list(zip(outer, outer[1:])) + list(zip(inner, inner[1:]))
    xs, ys = [p[0] for p in outer], [p[1] for p in outer]
    box = (min(xs) - 1, max(xs) + 1, min(ys) - 1, max(ys) + 1)
    out = bytearray()
    for j in range(size):
        y = j + 0.5 - off
        for i in range(size):
            x = i + 0.5 - off
            # The rounded tile, with a hairline rim so it reads on dark docks.
            qx = abs(x - s / 2) - (s / 2 - radius)
            qy = abs(y - s / 2) - (s / 2 - radius)
            tile = math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - radius
            alpha = clamp(0.5 - tile)
            if alpha <= 0.0:
                out += b"\x00\x00\x00\x00"
                continue
            edge = clamp(0.5 - abs(tile + 0.6 * rim) + 0.5 * rim)
            rgb = [TILE[k] + (RIM[k] - TILE[k]) * edge for k in range(3)]
            # The letter: inside the outer contour and outside the inner one.
            if box[0] <= x <= box[1] and box[2] <= y <= box[3]:
                d = min(seg_dist(x, y, *a, *b) for a, b in segs)
                ink = inside(x, y, outer) and not inside(x, y, inner)
                white = clamp(0.5 + (d if ink else -d))
                rgb = [rgb[k] + (WHITE[k] - rgb[k]) * white for k in range(3)]
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
