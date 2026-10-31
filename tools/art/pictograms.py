#!/usr/bin/env python3
"""Renders the input pictograms' hand-written SVGs to 240 x 240 PNGs.

    tools/studio.sh python3 -I tools/art/pictograms.py

The pictograms are shell/assets/art/icon-*.svg, written by hand: strokes only,
one colour (#EBEEF5, style.rs's TEXT), a 2-unit round stroke on a 24-unit
grid, readable at 120 px from three metres. This reads the small part of SVG
they use (path with M, L, H, V, C, A and Z, absolute or relative; line,
polyline, polygon, rect with rx, circle), refuses anything else (fills,
images, text, transforms, metadata), and draws each stroke as the pixels
within a stroke's half-width of the outline, antialiased over one pixel:
round caps and joins fall out of that. Writes icon-*.png next to the SVGs.

check() reads an SVG the same way and lists what breaks the rules; the
tests run it over every pictogram. Standard library, and numpy to draw.
"""

import argparse
import math
import os
import re
import sys
import xml.etree.ElementTree as ET

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path[:0] = [HERE]

ART = os.path.join(ROOT, 'shell', 'assets', 'art')
SIZE = 240
GRID = 24
COLOUR = '#EBEEF5'
STROKE = 2.0
SVG_NS = '{http://www.w3.org/2000/svg}'
SHAPES = ('path', 'line', 'polyline', 'polygon', 'rect', 'circle')
ROOT_ATTRIBUTES = {
    'width': '24', 'height': '24', 'viewBox': '0 0 24 24', 'fill': 'none', 'stroke': COLOUR,
    'stroke-width': '2', 'stroke-linecap': 'round', 'stroke-linejoin': 'round',
}


def check(text):
    """What in an SVG's text breaks the pictograms' rules; [] if nothing."""
    problems = []
    try:
        root = ET.fromstring(text)
    except ET.ParseError as e:
        return [f'not XML: {e}']
    if root.tag != SVG_NS + 'svg':
        problems.append(f'the root is {root.tag}, not svg')
    for key, value in ROOT_ATTRIBUTES.items():
        if root.get(key) != value:
            problems.append(f'svg {key}={root.get(key)!r}, not {value!r}')
    for key in root.attrib:
        if key not in ROOT_ATTRIBUTES and key != 'xmlns':
            problems.append(f'svg has {key}')
    allowed = {'path': {'d'}, 'line': {'x1', 'y1', 'x2', 'y2'}, 'polyline': {'points'},
               'polygon': {'points'}, 'rect': {'x', 'y', 'width', 'height', 'rx'}, 'circle': {'cx', 'cy', 'r'}}
    for element in root:
        tag = element.tag.replace(SVG_NS, '')
        if tag not in allowed:
            problems.append(f'<{tag}> is not a stroke shape')
            continue
        extra = set(element.attrib) - allowed[tag]
        if extra:
            problems.append(f'<{tag}> has {", ".join(sorted(extra))}: shapes take their style from the svg')
        if len(element):
            problems.append(f'<{tag}> has children')
    if re.search(r'<!--|<\?xml|data:|inkscape|sodipodi|metadata|<image|<text', text):
        problems.append('comments, a prolog, embedded data or editor metadata')
    return problems


# Outline: every shape becomes polylines in grid units.

def _numbers(text):
    return [float(v) for v in re.findall(r'[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?', text)]


def _arc(x0, y0, rx, ry, phi, large, sweep, x1, y1):
    """Points along an SVG arc, by the spec's conversion to a centre and
    angles (SVG 1.1, appendix F.6.5)."""
    if rx == 0 or ry == 0 or (x0, y0) == (x1, y1):
        return [(x1, y1)]
    rx, ry = abs(rx), abs(ry)
    c, s = math.cos(math.radians(phi)), math.sin(math.radians(phi))
    dx, dy = (x0 - x1) / 2, (y0 - y1) / 2
    xp, yp = c * dx + s * dy, -s * dx + c * dy
    scale = xp * xp / (rx * rx) + yp * yp / (ry * ry)
    if scale > 1:
        rx, ry = rx * math.sqrt(scale), ry * math.sqrt(scale)
    num = rx * rx * ry * ry - rx * rx * yp * yp - ry * ry * xp * xp
    root = math.sqrt(max(num, 0.0) / (rx * rx * yp * yp + ry * ry * xp * xp))
    if large == sweep:
        root = -root
    cxp, cyp = root * rx * yp / ry, -root * ry * xp / rx
    cx, cy = c * cxp - s * cyp + (x0 + x1) / 2, s * cxp + c * cyp + (y0 + y1) / 2

    def angle(ux, uy, vx, vy):
        return math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)

    start = angle(1, 0, (xp - cxp) / rx, (yp - cyp) / ry)
    span = angle((xp - cxp) / rx, (yp - cyp) / ry, (-xp - cxp) / rx, (-yp - cyp) / ry)
    if not sweep and span > 0:
        span -= 2 * math.pi
    elif sweep and span < 0:
        span += 2 * math.pi
    n = max(2, int(abs(span) / math.radians(2)))
    points = []
    for i in range(1, n + 1):
        t = start + span * i / n
        ex, ey = rx * math.cos(t), ry * math.sin(t)
        points.append((c * ex - s * ey + cx, s * ex + c * ey + cy))
    return points


def _path(d):
    """An SVG path's subpaths as [(points, closed)]."""
    tokens = re.findall(r'[MLHVCAZmlhvcaz]|[^MLHVCAZmlhvcaz]+', d)
    lines, points = [], []
    x = y = sx = sy = 0.0
    i = 0
    while i < len(tokens):
        cmd = tokens[i]
        args = _numbers(tokens[i + 1]) if i + 1 < len(tokens) and not tokens[i + 1].strip().isalpha() else []
        i += 2 if args or (i + 1 < len(tokens) and not tokens[i + 1].strip()) else 1
        rel = cmd.islower()
        op = cmd.upper()
        if op == 'Z':
            if len(points) > 1:
                lines.append((points, True))
            points, x, y = [(sx, sy)], sx, sy
            continue
        size = {'M': 2, 'L': 2, 'H': 1, 'V': 1, 'C': 6, 'A': 7}[op]
        if not args or len(args) % size:
            raise ValueError(f'path command {cmd} takes {size} numbers at a time: {args}')
        for k in range(0, len(args), size):
            a = args[k:k + size]
            if op == 'M' and k == 0:
                if len(points) > 1:
                    lines.append((points, False))
                x, y = (x + a[0], y + a[1]) if rel else (a[0], a[1])
                sx, sy = x, y
                points = [(x, y)]
            elif op in ('L', 'M'):
                x, y = (x + a[0], y + a[1]) if rel else (a[0], a[1])
                points.append((x, y))
            elif op == 'H':
                x = x + a[0] if rel else a[0]
                points.append((x, y))
            elif op == 'V':
                y = y + a[0] if rel else a[0]
                points.append((x, y))
            elif op == 'C':
                ox, oy = (x, y) if rel else (0.0, 0.0)
                p1, p2, p3 = (ox + a[0], oy + a[1]), (ox + a[2], oy + a[3]), (ox + a[4], oy + a[5])
                for j in range(1, 33):
                    t = j / 32
                    u = 1 - t
                    points.append((u ** 3 * x + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t ** 3 * p3[0],
                                   u ** 3 * y + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t ** 3 * p3[1]))
                x, y = p3
            elif op == 'A':
                ex, ey = (x + a[5], y + a[6]) if rel else (a[5], a[6])
                points += _arc(x, y, a[0], a[1], a[2], int(a[3]), int(a[4]), ex, ey)
                x, y = ex, ey
    if len(points) > 1:
        lines.append((points, False))
    return lines


def _rect(x, y, w, h, r):
    r = min(r, w / 2, h / 2)
    if r <= 0:
        return [([(x, y), (x + w, y), (x + w, y + h), (x, y + h)], True)]
    points = []
    for cx, cy, a0 in ((x + w - r, y + r, -90), (x + w - r, y + h - r, 0), (x + r, y + h - r, 90), (x + r, y + r, 180)):
        points += [(cx + r * math.cos(math.radians(a0 + t)), cy + r * math.sin(math.radians(a0 + t)))
                   for t in range(0, 91, 5)]
    return [(points, True)]


def outline(text):
    """Every stroke of an SVG as [(points in grid units, closed)]."""
    lines = []
    for element in ET.fromstring(text):
        tag = element.tag.replace(SVG_NS, '')

        def get(key, element=element):
            return float(element.get(key, 0))

        if tag == 'path':
            lines += _path(element.get('d'))
        elif tag == 'line':
            lines.append(([(get('x1'), get('y1')), (get('x2'), get('y2'))], False))
        elif tag in ('polyline', 'polygon'):
            n = _numbers(element.get('points'))
            lines.append((list(zip(n[0::2], n[1::2])), tag == 'polygon'))
        elif tag == 'rect':
            lines += _rect(get('x'), get('y'), get('width'), get('height'), get('rx'))
        elif tag == 'circle':
            cx, cy, r = get('cx'), get('cy'), get('r')
            lines.append(([(cx + r * math.cos(math.radians(t)), cy + r * math.sin(math.radians(t)))
                           for t in range(0, 360, 2)], True))
    return lines


def raster(text, size=SIZE):
    """The SVG's strokes as (size, size) coverage, 0 to 1."""
    import numpy as np

    scale = size / GRID
    half = STROKE / 2 * scale
    p = (np.arange(size) + 0.5)
    px, py = p[None, :], p[:, None]
    dist = np.full((size, size), np.inf)
    for points, closed in outline(text):
        pts = [(x * scale, y * scale) for x, y in points]
        if closed:
            pts.append(pts[0])
        if len(pts) == 1:
            pts.append(pts[0])
        for (ax, ay), (bx, by) in zip(pts, pts[1:]):
            dx, dy = bx - ax, by - ay
            length = dx * dx + dy * dy
            t = np.clip(((px - ax) * dx + (py - ay) * dy) / length, 0.0, 1.0) if length else 0.0
            dist = np.minimum(dist, np.hypot(px - ax - t * dx, py - ay - t * dy))
    return np.clip(half - dist + 0.5, 0.0, 1.0)


def render(text, size=SIZE):
    """The pictogram as (size, size, 4) uint8: the stroke colour everywhere,
    its coverage as alpha (straight)."""
    import numpy as np

    rgb = [int(COLOUR[i:i + 2], 16) for i in (1, 3, 5)]
    alpha = np.round(raster(text, size) * 255.0).astype(np.uint8)
    pixels = np.empty((size, size, 4), dtype=np.uint8)
    pixels[:, :, :3] = rgb
    pixels[:, :, 3] = alpha
    return pixels


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--art', default=ART, help='where the SVGs are and the PNGs go (default shell/assets/art)')
    args = parser.parse_args(sys.argv[1:] if argv is None else argv)
    import artkit

    names = sorted(f for f in os.listdir(args.art) if f.startswith('icon-') and f.endswith('.svg'))
    failed = False
    for name in names:
        with open(os.path.join(args.art, name)) as f:
            text = f.read()
        problems = check(text)
        if problems:
            failed = True
            print(f'{name}: ' + '; '.join(problems), file=sys.stderr)
            continue
        path = os.path.join(args.art, name[:-4] + '.png')
        artkit.write_png(path, render(text))
        print('wrote', os.path.relpath(path, ROOT))
    if failed:
        sys.exit('some pictograms break the rules')


if __name__ == '__main__':
    main()
