#!/usr/bin/env python3
"""Draws the inkwell logo: an amber pen nib over a line of ink, on a dark rounded square.

One set of shapes (in a 512 by 512 box) makes every file, so they always agree:

    inkwell.svg       the vector original
    inkwell.png       512 by 512
    inkwell-256.png   256 by 256, the window icon the app embeds
    inkwell.ico       16, 32, 48 and 256, the icon of the Windows executable
    ../../android/res/mipmap-*/ic_launcher.png   the launcher icon of the Android app

It needs only the standard library (there is a small scanline rasterizer below), so
`python assets/logo/make_logo.py` works anywhere. Run it from the repository root.
"""

import math
import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
BOX = 512.0

# --- colors ----------------------------------------------------------------------------------
BG_TOP = (0x2D, 0x2B, 0x28)
BG_BOTTOM = (0x14, 0x13, 0x11)
NIB_LIGHT = (0xFF, 0xD2, 0x7A)
NIB_DARK = (0xD3, 0x8A, 0x1E)
INK = (0xF3, 0xEE, 0xE3)

# --- shapes (512 units) ----------------------------------------------------------------------
CORNER = 112.0

# The nib, from the tip, up the left side, across the top and down the right side: cubic Beziers.
NIB_START = (256.0, 420.0)
NIB_CURVES = [
    ((238.0, 360.0), (180.0, 320.0), (168.0, 238.0)),
    ((160.0, 186.0), (196.0, 140.0), (226.0, 104.0)),
]
NIB_TOP = [(286.0, 104.0)]  # a straight edge
NIB_CURVES_RIGHT = [
    ((316.0, 140.0), (352.0, 186.0), (344.0, 238.0)),
    ((332.0, 320.0), (274.0, 360.0), (256.0, 420.0)),
]

# The breather hole and the slit to the tip, as one keyhole cut out of the nib.
HOLE_CENTER = (256.0, 232.0)
HOLE_RADIUS = 22.0
SLIT_HALF_WIDTH = 4.5
SLIT_END = 408.0

# The line of ink the nib is drawing: two Beziers through the tip, with round ends.
INK_START = (92.0, 448.0)
INK_CURVES = [
    ((158.0, 470.0), (214.0, 432.0), (256.0, 421.0)),
    ((298.0, 410.0), (352.0, 452.0), (420.0, 438.0)),
]
INK_WIDTH = 17.0


def lerp(a, b, t):
    return a + (b - a) * t


def mix(c0, c1, t):
    t = max(0.0, min(1.0, t))
    return tuple(lerp(a, b, t) for a, b in zip(c0, c1))


def cubic(p0, p1, p2, p3, steps=40):
    out = []
    for i in range(steps + 1):
        t = i / steps
        u = 1.0 - t
        out.append(
            (
                u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0],
                u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1],
            )
        )
    return out


def chain(start, curves, steps=40):
    points = [start]
    here = start
    for c1, c2, end in curves:
        points.extend(cubic(here, c1, c2, end, steps)[1:])
        here = end
    return points


def nib_outline():
    points = chain(NIB_START, NIB_CURVES)
    points.extend(NIB_TOP)
    here = NIB_TOP[-1]
    for c1, c2, end in NIB_CURVES_RIGHT:
        points.extend(cubic(here, c1, c2, end)[1:])
        here = end
    return points


def keyhole():
    """The hole and the slit as one loop: the big arc round the hole, then down one side of the slit and up the
    other."""
    cx, cy = HOLE_CENTER
    phi = math.asin(SLIT_HALF_WIDTH / HOLE_RADIUS)
    start = math.pi / 2 - phi
    end = math.pi / 2 + phi - 2 * math.pi
    points = []
    steps = 60
    for i in range(steps + 1):
        a = lerp(start, end, i / steps)
        points.append((cx + HOLE_RADIUS * math.cos(a), cy + HOLE_RADIUS * math.sin(a)))
    points.append((cx - SLIT_HALF_WIDTH, SLIT_END))
    points.append((cx + SLIT_HALF_WIDTH, SLIT_END))
    return points


def rounded_square():
    r = CORNER
    points = []
    for (cx, cy), start in [
        ((BOX - r, r), -90.0),
        ((BOX - r, BOX - r), 0.0),
        ((r, BOX - r), 90.0),
        ((r, r), 180.0),
    ]:
        for i in range(25):
            a = math.radians(start + 90.0 * i / 24)
            points.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return points


def stroke_polygon(points, width):
    """The outline of a line through `points` of `width`, with round ends."""
    half = width / 2.0
    left, right = [], []
    for i, p in enumerate(points):
        a = points[max(i - 1, 0)]
        b = points[min(i + 1, len(points) - 1)]
        dx, dy = b[0] - a[0], b[1] - a[1]
        length = math.hypot(dx, dy) or 1.0
        nx, ny = -dy / length, dx / length
        left.append((p[0] + nx * half, p[1] + ny * half))
        right.append((p[0] - nx * half, p[1] - ny * half))

    def cap(center, toward, sign):
        base = math.atan2(toward[1], toward[0])
        return [
            (
                center[0] + half * math.cos(base + sign * math.pi * i / 12),
                center[1] + half * math.sin(base + sign * math.pi * i / 12),
            )
            for i in range(1, 12)
        ]

    # Round each end, going round the polygon: the far end from the left edge through straight ahead to the right
    # edge, the near end from the right edge through straight behind to the left edge.
    first_dir = (right[0][0] - points[0][0], right[0][1] - points[0][1])
    last_dir = (left[-1][0] - points[-1][0], left[-1][1] - points[-1][1])
    return left + cap(points[-1], last_dir, -1) + right[::-1] + cap(points[0], first_dir, -1)


# --- colors as functions of where --------------------------------------------------------------
def background(x, y):
    return mix(BG_TOP, BG_BOTTOM, y / BOX) + (1.0,)


def nib_color(x, y):
    t = ((x - 168.0) / 176.0) * 0.55 + ((y - 104.0) / 316.0) * 0.45
    return mix(NIB_LIGHT, NIB_DARK, t) + (1.0,)


def ink_color(x, y):
    return INK + (0.92,)


# --- rasterizer ---------------------------------------------------------------------------------
SUBROWS = 5


def fill(canvas, size, loops, color, rule="evenodd"):
    """Paints the region the closed `loops` (in box units) enclose onto `canvas` (a list of [r, g, b, a] rows
    of floats, `size` square). Coverage is exact along a row and sampled five times down a pixel."""
    scale = size / BOX
    edges = []
    for loop in loops:
        pts = [(x * scale, y * scale) for x, y in loop]
        for i, a in enumerate(pts):
            b = pts[(i + 1) % len(pts)]
            if a[1] != b[1]:
                edges.append((a[0], a[1], b[0], b[1]))
    if not edges:
        return
    top = max(0, int(min(min(e[1], e[3]) for e in edges)))
    bottom = min(size - 1, int(max(max(e[1], e[3]) for e in edges)))
    for py in range(top, bottom + 1):
        coverage = [0.0] * size
        touched = False
        for sub in range(SUBROWS):
            y = py + (sub + 0.5) / SUBROWS
            crossings = []
            for x0, y0, x1, y1 in edges:
                if (y0 <= y < y1) or (y1 <= y < y0):
                    crossings.append((x0 + (y - y0) * (x1 - x0) / (y1 - y0), 1 if y1 > y0 else -1))
            if not crossings:
                continue
            crossings.sort()
            spans = []
            if rule == "evenodd":
                for i in range(0, len(crossings) - 1, 2):
                    spans.append((crossings[i][0], crossings[i + 1][0]))
            else:
                winding = 0
                for i, (x, d) in enumerate(crossings[:-1]):
                    winding += d
                    if winding != 0:
                        spans.append((x, crossings[i + 1][0]))
            for xa, xb in spans:
                xa, xb = max(xa, 0.0), min(xb, float(size))
                if xb <= xa:
                    continue
                touched = True
                for px in range(int(xa), min(int(xb), size - 1) + 1):
                    overlap = min(xb, px + 1.0) - max(xa, float(px))
                    if overlap > 0:
                        coverage[px] += overlap / SUBROWS
        if not touched:
            continue
        row = canvas[py]
        for px, c in enumerate(coverage):
            if c <= 1e-4:
                continue
            r, g, b, a = color((px + 0.5) / scale, (py + 0.5) / scale)
            alpha = min(1.0, c) * a
            dst = row[px]
            out_a = alpha + dst[3] * (1.0 - alpha)
            if out_a <= 0:
                continue
            for k, src in enumerate((r, g, b)):
                dst[k] = (src * alpha + dst[k] * dst[3] * (1.0 - alpha)) / out_a
            dst[3] = out_a


def render(size):
    canvas = [[[0.0, 0.0, 0.0, 0.0] for _ in range(size)] for _ in range(size)]
    fill(canvas, size, [rounded_square()], background)
    fill(canvas, size, [stroke_polygon(chain(INK_START, INK_CURVES), INK_WIDTH)], ink_color, "nonzero")
    fill(canvas, size, [nib_outline(), keyhole()], nib_color)
    return canvas


def to_rgba(canvas):
    out = bytearray()
    for row in canvas:
        for r, g, b, a in row:
            out += bytes((round(r), round(g), round(b), round(a * 255)))
    return bytes(out)


# --- file formats ---------------------------------------------------------------------------------
def png(size, rgba):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    rows = b"".join(b"\x00" + rgba[y * size * 4 : (y + 1) * size * 4] for y in range(size))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )


def dib(size, rgba):
    """An icon image as a bottom-up 32 bit bitmap with an empty mask, which every Windows reads."""
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = bytearray()
    for y in range(size - 1, -1, -1):
        for x in range(size):
            r, g, b, a = rgba[(y * size + x) * 4 : (y * size + x) * 4 + 4]
            pixels += bytes((b, g, r, a))
    mask = bytes(((size + 31) // 32) * 4 * size)
    return header + bytes(pixels) + mask


def ico(images):
    """`images` is a list of (size, rgba); 256 and up are kept as PNG, the rest as bitmaps."""
    blobs = [(size, png(size, rgba) if size >= 256 else dib(size, rgba)) for size, rgba in images]
    out = struct.pack("<HHH", 0, 1, len(blobs))
    offset = 6 + 16 * len(blobs)
    for size, blob in blobs:
        out += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    return out + b"".join(blob for _, blob in blobs)


def svg():
    def path(start, curves):
        d = f"M{start[0]:g} {start[1]:g}"
        for c1, c2, end in curves:
            d += f" C{c1[0]:g} {c1[1]:g} {c2[0]:g} {c2[1]:g} {end[0]:g} {end[1]:g}"
        return d

    nib = (
        path(NIB_START, NIB_CURVES)
        + f" L{NIB_TOP[0][0]:g} {NIB_TOP[0][1]:g} "
        + path(NIB_TOP[0], NIB_CURVES_RIGHT)[len(f"M{NIB_TOP[0][0]:g} {NIB_TOP[0][1]:g}") :]
        + " Z"
    )
    cx, cy = HOLE_CENTER
    phi = math.asin(SLIT_HALF_WIDTH / HOLE_RADIUS)
    sx, sy = cx + HOLE_RADIUS * math.cos(math.pi / 2 - phi), cy + HOLE_RADIUS * math.sin(math.pi / 2 - phi)
    ex, ey = cx + HOLE_RADIUS * math.cos(math.pi / 2 + phi), cy + HOLE_RADIUS * math.sin(math.pi / 2 + phi)
    hole = (
        f"M{sx:.3f} {sy:.3f} A{HOLE_RADIUS:g} {HOLE_RADIUS:g} 0 1 0 {ex:.3f} {ey:.3f} "
        f"L{cx - SLIT_HALF_WIDTH:g} {SLIT_END:g} L{cx + SLIT_HALF_WIDTH:g} {SLIT_END:g} Z"
    )
    hex_ = lambda c: "#%02X%02X%02X" % c
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512" role="img" aria-label="inkwell">
  <title>inkwell</title>
  <defs>
    <linearGradient id="paper" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="{hex_(BG_TOP)}"/>
      <stop offset="1" stop-color="{hex_(BG_BOTTOM)}"/>
    </linearGradient>
    <linearGradient id="nib" gradientUnits="userSpaceOnUse" x1="168" y1="104" x2="344" y2="420">
      <stop offset="0" stop-color="{hex_(NIB_LIGHT)}"/>
      <stop offset="1" stop-color="{hex_(NIB_DARK)}"/>
    </linearGradient>
  </defs>
  <rect width="512" height="512" rx="{CORNER:g}" fill="url(#paper)"/>
  <path d="{path(INK_START, INK_CURVES)}" fill="none" stroke="{hex_(INK)}" stroke-opacity="0.92" stroke-width="{INK_WIDTH:g}" stroke-linecap="round"/>
  <path d="{nib} {hole}" fill="url(#nib)" fill-rule="evenodd"/>
</svg>
"""


def main():
    renders = {}
    # The launcher icon of the Android app, one size for each screen density.
    launcher = {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}
    for size in sorted({16, 32, 48, 256, 512, *launcher.values()}):
        renders[size] = to_rgba(render(size))
        print(f"drew {size} x {size}")
    write = lambda name, data, mode="wb": open(os.path.join(HERE, name), mode).write(data)
    write("inkwell.svg", svg(), "w")
    write("inkwell.png", png(512, renders[512]))
    write("inkwell-256.png", png(256, renders[256]))
    write("inkwell.ico", ico([(s, renders[s]) for s in (16, 32, 48, 256)]))
    for density, size in launcher.items():
        folder = os.path.join(HERE, "..", "..", "android", "res", f"mipmap-{density}")
        os.makedirs(folder, exist_ok=True)
        with open(os.path.join(folder, "ic_launcher.png"), "wb") as out:
            out.write(png(size, renders[size]))
    print("wrote inkwell.svg, inkwell.png, inkwell-256.png, inkwell.ico in", HERE)


if __name__ == "__main__":
    main()
