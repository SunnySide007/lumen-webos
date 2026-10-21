#!/usr/bin/env python3
"""Draws the simulator's placeholder app icons as PNGs, using only the stdlib.

Six white pictograms on transparent 128 px squares: play, note, tv, input,
globe and gear. The mock launcher hands them to its apps until the TV's own
launcher reads real icons (milestone B3). They are original, deterministic
drawings, so the committed PNGs can always be regenerated from this file:

    python3 tools/mock-icons.py
"""

import math
import os
import struct
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'shell', 'assets', 'mock-icons')
SIZE = 128
SUPERSAMPLE = 4


def seg_dist(px, py, ax, ay, bx, by):
    """Distance from (px, py) to the segment from a to b."""
    dx, dy = bx - ax, by - ay
    t = ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)
    t = min(1.0, max(0.0, t))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def in_polygon(px, py, pts):
    """Even-odd test against a closed polygon."""
    inside = False
    for (ax, ay), (bx, by) in zip(pts, pts[1:] + pts[:1]):
        if (ay > py) != (by > py) and px < ax + (py - ay) * (bx - ax) / (by - ay):
            inside = not inside
    return inside


def rounded_polygon(px, py, pts, r):
    """Inside `pts` grown by `r`, which rounds its corners."""
    if in_polygon(px, py, pts):
        return True
    edges = zip(pts, pts[1:] + pts[:1])
    return min(seg_dist(px, py, ax, ay, bx, by) for (ax, ay), (bx, by) in edges) <= r


def box_dist(px, py, cx, cy, hw, hh, r):
    """Signed distance to a rounded rectangle (negative inside)."""
    qx = abs(px - cx) - hw + r
    qy = abs(py - cy) - hh + r
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    return outside + min(max(qx, qy), 0.0) - r


def ellipse(px, py, cx, cy, rx, ry, angle=0.0):
    c, s = math.cos(angle), math.sin(angle)
    x, y = px - cx, py - cy
    u, v = x * c + y * s, -x * s + y * c
    return (u / rx) ** 2 + (v / ry) ** 2 <= 1.0


def play(x, y):
    return rounded_polygon(x, y, [(-20, -30), (-20, 30), (32, 0)], 7)


def note(x, y):
    heads = (ellipse(x, y, -22, 30, 15, 11, -0.35)
             or ellipse(x, y, 26, 22, 15, 11, -0.35))
    stems = (-9 <= x <= -2 and -30 <= y <= 30) or (39 <= x <= 46 and -38 <= y <= 22)
    beam = in_polygon(x, y, [(-9, -30), (46, -40), (46, -26), (-9, -16)])
    return heads or stems or beam


def tv(x, y):
    d = box_dist(x, y, 0, -8, 50, 34, 9)
    screen = -8 <= d <= 0
    neck = -4 <= x <= 4 and 26 <= y <= 34
    base = rounded_polygon(x, y, [(-22, 34), (22, 34), (22, 38), (-22, 38)], 2)
    return screen or neck or base


def input_(x, y):
    d = box_dist(x, y, 16, 0, 34, 42, 9)
    frame = -8 <= d <= 0 and not (x < -6 and abs(y) < 16)
    shaft = -54 <= x <= 6 and abs(y) <= 5
    head = rounded_polygon(x, y, [(2, -17), (2, 17), (24, 0)], 2)
    return frame or shaft or head


def globe(x, y):
    r = math.hypot(x, y)
    rim = 41 <= r <= 48
    if r > 48:
        return rim
    meridian = ellipse(x, y, 0, 0, 22, 48) and not ellipse(x, y, 0, 0, 15, 41)
    equator = abs(y) <= 3.5
    return rim or meridian or equator


def gear(x, y):
    r = math.hypot(x, y)
    teeth = 46 + 7 * max(-1.0, min(1.0, 2.4 * math.cos(8 * math.atan2(y, x))))
    return 17 <= r <= teeth


SHAPES = {
    'play': play,
    'note': note,
    'tv': tv,
    'input': input_,
    'globe': globe,
    'gear': gear,
}


def render(shape):
    """White pixels whose alpha is the shape's coverage, row by row."""
    pixels = bytearray()
    n = SUPERSAMPLE
    half = SIZE / 2
    for py in range(SIZE):
        pixels.append(0)  # PNG filter: none
        for px in range(SIZE):
            hits = sum(
                shape(px + (sx + 0.5) / n - half, py + (sy + 0.5) / n - half)
                for sy in range(n) for sx in range(n))
            pixels.extend((255, 255, 255, round(hits * 255 / (n * n))))
    return bytes(pixels)


def png(raw):
    def chunk(kind, data):
        return (struct.pack('>I', len(data)) + kind + data
                + struct.pack('>I', zlib.crc32(kind + data) & 0xFFFFFFFF))
    header = struct.pack('>IIBBBBB', SIZE, SIZE, 8, 6, 0, 0, 0)  # 8-bit RGBA
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header)
            + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, shape in SHAPES.items():
        path = os.path.join(OUT, name + '.png')
        with open(path, 'wb') as f:
            f.write(png(render(shape)))
        print('wrote', os.path.relpath(path, ROOT))


if __name__ == '__main__':
    main()
