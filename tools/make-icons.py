#!/usr/bin/env python3
"""Draws the Lumen app icons (80 and 130 px) as PNGs, using only the stdlib.

The mark is a soft lens of light on a near-black tile: a placeholder until the
project has real artwork, but deterministic, so the committed PNGs can always be
regenerated from this file:

    python3 tools/make-icons.py
"""

import math
import os
import struct
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'packaging', 'shell')
SUPERSAMPLE = 4

TILE = (11, 15, 28)
INNER = (120, 200, 255)   # cool cyan at the lens centre
OUTER = (150, 110, 255)   # violet at its rim


def mix(a, b, t):
    return tuple(x + (y - x) * t for x, y in zip(a, b))


def rounded_rect_coverage(x, y, size, radius):
    """1 inside a rounded square of side `size`, 0 outside (point sample)."""
    cx = min(max(x, radius), size - radius)
    cy = min(max(y, radius), size - radius)
    return 1.0 if (x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2 else 0.0


def shade(x, y, size):
    """RGBA (floats 0..1 alpha) for one sample at (x, y) in a `size` tile."""
    cover = rounded_rect_coverage(x, y, size, size * 0.22)
    if cover == 0.0:
        return (0.0, 0.0, 0.0, 0.0)
    nx, ny = x / size - 0.5, y / size - 0.5
    r = math.hypot(nx, ny)
    ring = math.exp(-((r - 0.27) / 0.045) ** 2)          # the lens rim
    core = math.exp(-(r / 0.16) ** 2) * 0.55             # its inner glow
    glint = math.exp(-(((nx + 0.11) / 0.06) ** 2 + ((ny + 0.13) / 0.035) ** 2)) * 0.9
    light = min(1.0, ring + core)
    color = mix(TILE, mix(INNER, OUTER, min(1.0, r / 0.3)), light)
    color = mix(color, (255, 255, 255), min(1.0, glint))
    return (color[0], color[1], color[2], cover)


def render(size):
    pixels = bytearray()
    n = SUPERSAMPLE
    for py in range(size):
        pixels.append(0)  # PNG filter: none
        for px in range(size):
            acc = [0.0, 0.0, 0.0, 0.0]
            for sy in range(n):
                for sx in range(n):
                    r, g, b, a = shade(px + (sx + 0.5) / n, py + (sy + 0.5) / n, size)
                    acc[0] += r * a
                    acc[1] += g * a
                    acc[2] += b * a
                    acc[3] += a
            alpha = acc[3] / (n * n)
            if acc[3] > 0:
                rgb = [c / acc[3] for c in acc[:3]]
            else:
                rgb = [0.0, 0.0, 0.0]
            pixels.extend(int(round(min(255, max(0, c)))) for c in rgb)
            pixels.append(int(round(alpha * 255)))
    return bytes(pixels)


def png(size, raw):
    def chunk(kind, data):
        return (struct.pack('>I', len(data)) + kind + data
                + struct.pack('>I', zlib.crc32(kind + data) & 0xFFFFFFFF))
    header = struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0)  # 8-bit RGBA
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header)
            + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))


def main():
    os.makedirs(OUT, exist_ok=True)
    for size, name in ((80, 'icon80.png'), (130, 'icon130.png')):
        path = os.path.join(OUT, name)
        with open(path, 'wb') as f:
            f.write(png(size, render(size)))
        print('wrote', os.path.relpath(path, ROOT))


if __name__ == '__main__':
    main()
