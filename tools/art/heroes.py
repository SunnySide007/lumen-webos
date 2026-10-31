#!/usr/bin/env python3
"""Draws the Top Shelf heroes: full-bleed 1920 x 640 art above the app row.

    tools/studio.sh python3 -I tools/art/heroes.py --all
    tools/studio.sh python3 -I tools/art/heroes.py hero-hdmi2 --out tools/out/art/try

Each hero is a scene drawn by code from a seed in heroes.toml, written as an
8-bit sRGB PNG and a 960 x 320 half-size copy (-half.png) into --out (by
default shell/assets/art). They are made for an OLED in a dark room: a
near-black base, a few small highlights, nothing pure white, and the bottom
200 px darkened so the shell's 72 px title reads over them. The ambient hero,
a still of the studio's aurora loop, also comes as three layers for
parallax (-bg, -mid, -fg). Prints each file's light levels and banding.
"""

import argparse
import math
import os
import sys
import time
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path[:0] = [HERE, os.path.join(ROOT, 'studio')]

import numpy as np  # noqa: E402

import artkit  # noqa: E402
import aurora  # noqa: E402
import fireflies  # noqa: E402
import hdr  # noqa: E402
from artkit import Fbm, Waves, grid, smoothstep  # noqa: E402
from scenekit import PEAK_NITS, soft_clip  # noqa: E402

TABLE = os.path.join(HERE, 'heroes.toml')
OUT = os.path.join(ROOT, 'shell', 'assets', 'art')
SOURCES = {'aurora': aurora.Aurora, 'fireflies': fireflies.Fireflies}

# style.rs, TILE_PALETTE: tile colours for apps that bring none.
TILE_PALETTE = ((196, 58, 58), (214, 120, 46), (190, 152, 44), (64, 156, 92),
                (44, 140, 160), (58, 104, 196), (112, 82, 190), (170, 70, 140))


# Palette: one slow field per tile colour, each a different shape.

def tint(field, hue, peak):
    """A field (0 to about 1) in one hue: deeper and more saturated where it
    is dim, paler where it is bright, with `peak` the brightest channel at 1."""
    base = artkit.normalized(hue)
    deep = artkit.with_luminance(base ** 1.6, artkit.luminance(base))
    pale = artkit.with_luminance(base ** 0.55, artkit.luminance(base))
    t = field[:, :, None]
    colour = deep + (base - deep) * smoothstep(0.0, 0.45, t) + (pale - base) * smoothstep(0.5, 1.1, t)
    return colour * t * peak


def ember(rng, x, y, aspect):
    """A glow below the right edge, with smoke curling up through it."""
    cx = aspect * rng.uniform(0.72, 0.82)
    glow = np.exp(-(np.hypot((x - cx) / 1.3, (y - 1.15) / 0.8) / 0.6) ** 1.3)
    w1, w2, w3 = Waves(rng, 1.6, count=6), Waves(rng, 1.2, count=6), Fbm(rng, 1.1, octaves=2, gain=0.4)
    # Smoke rises: its features are stretched upward and lean with the draught.
    wx = x + 0.14 * w1(x, y) + 0.25 * (1.0 - y)
    wy = y + 0.10 * w2(x, y)
    wisps = np.clip(1.0 - np.abs(w3(wx / 0.7, wy / 2.2)) / 1.2, 0.0, 1.0) ** 4
    return glow * (0.45 + 0.75 * wisps)


def dunes(rng, x, y, aspect):
    """Dunes in layers against a low light, the far ones paler."""
    sky = 0.55 * np.exp(-np.abs(y - 0.42) / 0.16) * np.exp(-((x - aspect * rng.uniform(0.25, 0.45)) / 1.4) ** 2)
    field = sky + 0.0 * x
    for k, (base, light) in enumerate(((0.40, 0.55), (0.50, 0.38), (0.62, 0.24), (0.76, 0.12))):
        wave = Waves(rng, rng.uniform(1.4, 2.2), count=3)
        ripple = Waves(rng, 0.35, count=4)
        crest = base + 0.05 * wave(x, 0.0 * x) + 0.008 * ripple(x, 0.0 * x)
        depth = y - crest
        inside = smoothstep(-0.0025, 0.0025, depth)
        rim = np.exp(-np.maximum(depth, 0.0) / 0.012)
        body = np.exp(-np.maximum(depth, 0.0) / (0.10 + 0.04 * k))
        lit = 0.5 + 0.5 * np.tanh(2.0 * wave(x + 0.02, 0.0 * x) - 2.0 * wave(x - 0.02, 0.0 * x))
        field = field * (1.0 - inside) + inside * light * (0.85 * rim * (0.4 + 0.6 * lit) + 0.25 * body)
    return field


def rings(rng, x, y, aspect):
    """Ripples spreading from beyond the top right, fading as they go."""
    cx, cy = aspect * rng.uniform(0.74, 0.84), rng.uniform(-0.3, -0.18)
    wobble = Waves(rng, 1.5, count=5)
    r = np.hypot(x - cx, (y - cy) * 1.15) + 0.006 * wobble(x, y)
    # Rings spread further apart as they travel, and fade.
    phase = (r / 0.15) ** 0.85
    crest = (0.5 + 0.5 * np.cos(2.0 * math.pi * phase)) ** 3
    fade = np.exp(-r / 0.55)
    second = np.hypot(x - cx + rng.uniform(0.9, 1.3), y - cy + 0.1)
    beat = 0.5 + 0.5 * np.cos(2.0 * math.pi * second / 0.24)
    return fade * (0.10 + 0.9 * crest * (0.6 + 0.4 * beat))


def canopy(rng, x, y, aspect):
    """Sun through leaves, out of focus: soft discs of light, many faint and
    a few bright, gathered near the top where the sun comes in."""
    sx, sy = aspect * rng.uniform(0.3, 0.42), rng.uniform(0.0, 0.12)
    field = 0.06 * np.exp(-((x - sx) ** 2 / 1.1 + (y - sy) ** 2 / 0.22))
    for _ in range(120):
        cx = sx + rng.normal(0.0, 0.7)
        cy = sy + abs(rng.normal(0.0, 0.26))
        r = math.exp(rng.uniform(math.log(0.014), math.log(0.075)))
        d = np.hypot(x - cx, y - cy) / r
        disc = 1.0 - smoothstep(0.8, 1.0, d)
        rim = np.exp(-((d - 0.9) / 0.07) ** 2) * disc
        light = (math.exp(rng.uniform(math.log(0.03), 0.0)) ** 1.4 * (0.03 / r) ** 0.6
                 * math.exp(-((cx - sx) ** 2 / 1.3 + (cy - sy) ** 2 / 0.35)))
        field = field + light * (0.7 * disc + 0.35 * rim)
    return field


def caustic(rng, x, y, aspect):
    """Light on the floor of still water, a slow net of bright lines."""
    width, height = x.shape[1], y.shape[0]
    # The water's surface is a sum of swells. Light falls straight down
    # through it and each ray is bent by the slope where it crosses, so it
    # lands displaced; where the surface's curvature focuses the rays, they
    # crowd into bright lines. Trace jittered rays and count where they land.
    waves = []
    for _ in range(6):
        angle = rng.uniform(0.0, 2.0 * math.pi)
        k = 2.0 * math.pi / rng.uniform(0.3, 0.6)
        waves.append((0.5 * rng.uniform(0.7, 1.0) / k, k * math.cos(angle), k * math.sin(angle),
                      rng.uniform(0.0, 2.0 * math.pi)))
    rays = 4
    landed = np.zeros(height * width)
    for _ in range(rays):
        px = (np.arange(width)[None, :] + rng.uniform(0.0, 1.0, (height, width))) / height
        py = (np.arange(height)[:, None] + rng.uniform(0.0, 1.0, (height, width))) / height
        qx, qy = px.copy(), py.copy()
        for a, kx, ky, p in waves:
            c = a * np.cos(kx * px + ky * py + p)
            qx += c * kx / math.hypot(kx, ky)
            qy += c * ky / math.hypot(kx, ky)
        col = np.floor(qx * height).astype(int)
        row = np.floor(qy * height).astype(int)
        inside = (col >= 0) & (col < width) & (row >= 0) & (row < height)
        landed += np.bincount((row * width + col)[inside], minlength=height * width)
    density = np.maximum(artkit.blur(landed.reshape(height, width) / rays, 0.9 * height / artkit.HEIGHT), 0.0)
    lines = 0.035 * density ** 1.45 + 0.025 * np.maximum(artkit.blur(density, 0.03 * height), 0.0) ** 2
    light = np.exp(-(((x - aspect * rng.uniform(0.45, 0.6)) / 1.25) ** 2 + ((y + 0.1) / 0.7) ** 2))
    return light * lines


def horizon(rng, x, y, aspect):
    """The rim of a planet against space, lit from the right."""
    cx, cy, radius = aspect * rng.uniform(0.3, 0.42), 2.25, 1.85
    dx, dy = x - cx, y - cy
    d = np.hypot(dx, dy) - radius
    side = smoothstep(-0.9, 0.9, dx / radius * 2.0)  # the sunlit side
    rim = np.where(d > 0.0, np.exp(-d / 0.022), np.exp(d / 0.006))
    air = np.exp(-np.maximum(d, 0.0) / 0.2) * (d > -0.002)
    ground = 0.06 * np.exp(np.minimum(d, 0.0) / 0.12) * (d <= 0.0)
    return (0.25 + 0.75 * side) * (0.9 * rim + 0.22 * air) + ground * side


def nebula(rng, x, y, aspect):
    """A cloud of gas, folded on itself, across a diagonal, with dark lanes."""
    w1, w2 = Fbm(rng, 2.0, octaves=2), Fbm(rng, 2.0, octaves=2)
    cloud = Fbm(rng, 1.1, octaves=5, gain=0.52, count=12)
    dust = Fbm(rng, 0.6, octaves=3, gain=0.5, count=12)
    wx, wy = x + 0.22 * w1(x, y), y + 0.22 * w2(x, y)
    # Log-normal: dark almost everywhere, glowing where the gas is thick.
    density = np.exp(1.35 * (cloud(wx, wy) - 1.5))
    lanes = 1.0 - 0.7 * smoothstep(0.5, 1.6, dust(wx, wy))
    band = np.exp(-((y - 0.1 - 0.3 * x / aspect - 0.06 * w1(x * 0.5, y)) / 0.3) ** 2)
    return band * density * lanes


def silk(rng, x, y, aspect):
    """Ribbons of silk drawn across the dark, folding as they go."""
    field = 0.0 * x
    flat = 0.0 * x
    for k, (base, half_width, light) in enumerate(((0.26, 0.085, 1.0), (0.46, 0.06, 0.6))):
        path = Waves(rng, rng.uniform(2.4, 3.2), count=3)
        centre = base + 0.11 * path(x, flat) - 0.07 * (x - aspect / 2) / aspect
        # The ribbon turns slowly about its length: its width is the cosine
        # of the turn, and it shines where its face turns to the light.
        turn = 1.4 * Waves(rng, 2.0, count=3)(x, flat) + rng.uniform(0.0, 6.3)
        across = half_width * (0.12 + 0.88 * np.abs(np.cos(turn)))
        d = (y - centre) / across
        body = np.exp(-d ** 4)
        facing = np.maximum(np.cos(turn - 0.5), 0.0)
        sheen = np.exp(-((d - 0.55 * np.sin(turn)) / 0.3) ** 2) * facing ** 3
        ribbon = light * body * (0.18 + 0.45 * facing ** 1.5 + 0.55 * sheen)
        field = np.maximum(field, ribbon)
    return field


SHAPES = {'ember': ember, 'dunes': dunes, 'rings': rings, 'canopy': canopy,
          'caustic': caustic, 'horizon': horizon, 'nebula': nebula, 'silk': silk}


def palette(spec, width, height):
    rng = np.random.default_rng(spec['seed'])
    x, y = grid(width, height)
    field = SHAPES[spec['shape']](rng, x, y, width / height)
    hue = artkit.rgb8(*TILE_PALETTE[spec['colour']])
    # Equal light for every hue: a blue at full channel is far dimmer than a gold.
    peak = spec.get('peak', 0.42) * (0.3 / artkit.luminance(artkit.normalized(hue))) ** 0.5
    image = tint(np.broadcast_to(field, (height, width)), hue, peak)
    stars(rng, image, spec.get('stars', 0))
    return image


def stars(rng, image, count, bright=4):
    """Faint stars, most barely there and a few brighter, kept out of the
    title's band; points under a pixel across, at any size."""
    if not count:
        return
    h, w = image.shape[:2]
    points = np.column_stack([rng.uniform(0.0, w, count), rng.uniform(0.0, 0.62 * h, count)])
    peak = np.minimum(0.012 * rng.uniform(0.0, 1.0, count) ** -0.9, 0.12)
    peak[rng.choice(count, bright, replace=False)] = rng.uniform(0.25, 0.4, bright)
    colour = np.array([artkit.blackbody(t) for t in rng.uniform(4500.0, 11000.0, count)])
    artkit.splat(image, points, peak, colour, 0.45 * h / artkit.HEIGHT)


# Inputs: lines of light arriving from the right through dark glass.

def arrival(spec, width, height):
    """Lines of light from beyond the right edge meet a pane of dark glass,
    bend at its face and travel on inside it, dimming and spreading."""
    rng = np.random.default_rng(spec['seed'])
    aspect = width / height
    x, y = grid(width, height)
    light = artkit.blackbody(spec['kelvin'])
    angle = math.radians(spec['angle'])
    face = aspect * rng.uniform(0.70, 0.80)
    lean = math.tan(math.radians(rng.uniform(-5.0, 5.0)))
    inside = 1.0 - smoothstep(-0.0015, 0.0015, x - face - (y - 0.5) * lean)
    sigma = 0.9 / artkit.HEIGHT  # a line's core, in picture heights
    middle = rng.uniform(0.22, 0.4)
    lines = np.zeros((height, width))
    within = np.zeros((height, width))
    glints = []
    for _ in range(int(rng.integers(10, 16))):
        fy = float(np.clip(middle + 0.13 * rng.standard_normal(), 0.04, 0.64))
        fx = face + (fy - 0.5) * lean
        a = angle + math.radians(rng.normal(0.0, 1.5))
        bright = math.exp(rng.uniform(math.log(0.12), 0.0))
        # Outside, in air: crisp, the same brightness all the way in.
        ox, oy = x - fx, y - fy
        d_out = np.abs(ox * math.sin(a) + oy * math.cos(a))
        lines += (1.0 - inside) * bright * (np.exp(-0.5 * (d_out / sigma) ** 2)
                                            + 0.05 * np.exp(-0.5 * (d_out / (6.0 * sigma)) ** 2))
        # Inside, the line bends toward the face's normal (glass, n = 1.5),
        # dims as the glass absorbs it and spreads as it scatters.
        b = math.asin(math.sin(a) / 1.5)
        s = np.maximum(-ox * math.cos(b) + oy * math.sin(b), 0.0)
        d_in = np.abs(ox * math.sin(b) + oy * math.cos(b))
        spread = sigma + 0.011 * s
        fade = np.exp(-s / rng.uniform(0.7, 1.4)) * sigma / spread
        glow = fade * bright * (np.exp(-0.5 * (d_in / spread) ** 2)
                                + 0.06 * np.exp(-0.5 * (d_in / (7.0 * spread)) ** 2))
        lines += inside * glow
        within += inside * glow
        glints.append((fx * height, fy * height, bright))
    haze = 0.3 * artkit.blur(within, 0.025 * height) + 0.15 * artkit.blur(within, 0.12 * height)
    # Where each line meets the face, light runs a little way along it.
    edge = np.zeros((height, width))
    along = np.exp(-0.5 * ((x - face - (y - 0.5) * lean) / (0.8 / artkit.HEIGHT)) ** 2)
    for gx, gy, bright in glints:
        edge += 0.07 * bright * along * np.exp(-np.abs(y - gy / height) / 0.035)
    smoke = 0.85 + 0.15 * Waves(rng, 1.4)(x, y)
    sheen = 0.004 * inside * np.exp(-(((x - 0.45 * face) * 0.8 + (y - 0.25) * 0.6) / 0.35) ** 2)
    total = (lines + haze + edge) * smoke + sheen
    image = total[:, :, None] * light * spec.get('peak', 0.55)
    pale = artkit.normalized(light ** 0.35)
    for gx, gy, bright in glints:
        artkit.splat(image, [(gx, gy)], 0.4 * bright, pale, 1.1 * height / artkit.HEIGHT)
        artkit.splat(image, [(gx, gy)], 0.025 * bright, light, 12.0 * height / artkit.HEIGHT)
    return image


# Live TV, Plex, Homebrew Channel, Settings: one scene each.

def coverage_x(x_px, left, right):
    """How much of each pixel column [x, x + 1) lies between left and right (pixels)."""
    return np.clip(np.minimum(x_px + 1.0, right) - np.maximum(x_px, left), 0.0, 1.0)


def line_cover(px, py, ax, ay, bx, by, width):
    """Coverage of a straight line `width` pixels wide, from a to b (pixels)."""
    d, _ = artkit.segment(px, py, ax, ay, bx, by)
    return np.clip(width / 2.0 - d + 0.5, 0.0, 1.0) * min(width, 1.0)


def towers(spec, width, height):
    """Far masts on a ridge at dusk, their red lights lit, under the last of
    the afterglow."""
    rng = np.random.default_rng(spec['seed'])
    aspect = width / height
    x, y = grid(width, height)
    horizon = spec.get('horizon', 0.66)
    sun = aspect * rng.uniform(0.62, 0.8)
    up = np.maximum(horizon - y, 0.0)
    zenith = np.array((0.0014, 0.0024, 0.0090))
    dusk = np.array((0.0080, 0.0058, 0.0200))
    glow = artkit.blackbody(2000) * 0.12
    near_sun = np.exp(-((x - sun) / 1.1) ** 2)
    sky = (zenith + (dusk - zenith) * np.exp(-up / 0.3)[:, :, None]
           + glow * (np.exp(-up / 0.085) * (0.2 + 0.8 * near_sun))[:, :, None])
    # Thin clouds low in the west, their undersides catching the light.
    clouds = Fbm(rng, 0.9, octaves=4, count=10)
    layer = np.exp(-((y - horizon + rng.uniform(0.13, 0.18)) / 0.05) ** 2)
    cloud = smoothstep(0.3, 1.4, clouds(x / 3.5, y * 6.0)) * layer
    sky = (sky * (1.0 - 0.6 * cloud[:, :, None])
           + artkit.blackbody(2500) * 0.07 * (cloud * (0.2 + 0.8 * near_sun))[:, :, None])
    stars(rng, sky, 70, bright=2)
    # The land: a low ridge, black, with the lights of a far town on it.
    ridge = horizon - 0.012 - 0.02 * (0.5 + 0.5 * Waves(rng, 1.6, count=3)(x, 0.0 * x))
    px = (np.arange(width) + 0.5)[None, :]
    py = (np.arange(height) + 0.5)[:, None]
    land = np.clip(py - ridge * height + 0.5, 0.0, 1.0)
    image = sky * (1.0 - land)[:, :, None]
    cover = np.zeros((height, width))
    lights = []
    scale = height / artkit.HEIGHT
    for cx in sorted(rng.uniform(0.25, 0.97, int(rng.integers(4, 6))) * aspect):
        tall = rng.uniform(0.16, 0.42)
        base = float(np.interp(cx, x[0], ridge[0])) + 0.004
        c, t, b = cx * height, (base - tall) * height, base * height
        foot = (0.6 + 4.0 * tall) * scale  # half the mast's width at its foot
        taper = 0.4 + 0.6 * np.clip((py - t) / (b - t), 0.0, 1.0)
        body = coverage_x(px, c - foot * taper, c + foot * taper)
        # A lattice: solid girders every few pixels, open bracing between.
        girders = np.abs(((py - t) / (5.0 * scale)) % 1.0 - 0.5) < 0.18
        body = body * np.where(girders, 1.0, 0.6) * np.clip(py - t, 0.0, 1.0) * np.clip(b - py + 0.5, 0.0, 1.0)
        cover = np.maximum(cover, body)
        cover = np.maximum(cover, line_cover(px, py, c, t - 0.07 * tall * height, c, t, 0.9 * scale))
        # Guy wires, from three heights out to anchors on the ground.
        for share in (0.3, 0.6, 0.9):
            hy = b - share * (b - t)
            for side in (-1.0, 1.0):
                ax = c + side * (b - hy) * rng.uniform(0.6, 0.9)
                cover = np.maximum(cover, 0.8 * line_cover(px, py, c, hy, ax, b, 0.6 * scale))
        for k, share in enumerate((1.0, 0.66, 0.33)):
            on = rng.uniform(0.35, 1.0) if k else 1.0
            lights.append((c, t + (1.0 - share) * (b - t) - (2.0 * scale if k == 0 else 0.0), on * (1.0 - 0.45 * k)))
    image *= (1.0 - cover)[:, :, None]
    red = np.array((1.0, 0.035, 0.02))
    for cx, cy, on in lights:
        artkit.splat(image, [(cx, cy)], 0.6 * on, red, 1.1 * scale)
        artkit.splat(image, [(cx, cy)], 0.04 * on, red, 8.0 * scale)
    count = 150
    town = rng.uniform(0.05, 0.95, count) * width
    town_y = np.interp(town / height, x[0], ridge[0]) * height + rng.uniform(1.0, 6.0, count) * scale
    warm = np.array([artkit.blackbody(t) for t in rng.uniform(2000.0, 3400.0, count)])
    artkit.splat(image, np.column_stack([town, town_y]), 2.5 * rng.uniform(0.01, 0.07, count) ** 1.2,
                 warm, 0.6 * scale)
    return image


def beam(spec, width, height):
    """A projector's beam through the dark of a cinema, dust turning in it."""
    rng = np.random.default_rng(spec['seed'])
    aspect = width / height
    x, y = grid(width, height)
    sx, sy = aspect * rng.uniform(0.86, 0.92), rng.uniform(0.07, 0.12)
    dx, dy = -0.2 * aspect - sx, rng.uniform(0.62, 0.75) - sy
    n = math.hypot(dx, dy)
    dx, dy = dx / n, dy / n
    vx, vy = x - sx, y - sy
    along = vx * dx + vy * dy
    across = vx * dy - vy * dx
    radius = 0.006 + np.maximum(along, 0.0) * math.tan(math.radians(spec.get('spread', 9.5)))
    edge = 1.0 - smoothstep(0.45, 1.0, np.abs(across) / radius)
    # Shafts: the picture's light and shade, spread by the lens into rays.
    angle = across / np.maximum(along, 1e-3)
    shafts = 0.7 + 0.3 * np.tanh(1.2 * Fbm(rng, 0.05, octaves=2, count=10)(angle, 0.0 * angle))
    smoke = Fbm(rng, 0.8, octaves=4, count=10)
    swirl = Waves(rng, 1.4, count=6)
    haze = 0.3 + 0.7 * smoothstep(-1.0, 1.8, smoke(x + 0.2 * swirl(x, y), y + 0.2 * swirl(y, x)))
    density = edge * shafts * haze * (along > 0.0) / (np.maximum(along, 0.0) + 0.2)
    image = density[:, :, None] * artkit.blackbody(spec.get('kelvin', 5200)) * spec.get('peak', 0.06)
    # The port the light leaves by, and dust in the beam, some out of focus.
    scale = height / artkit.HEIGHT
    artkit.splat(image, [(sx * height, sy * height)], 0.7, artkit.normalized(artkit.blackbody(6000) ** 0.5),
                 2.4 * scale)
    artkit.splat(image, [(sx * height, sy * height)], 0.06, artkit.blackbody(5200), 26.0 * scale)
    count = 900
    motes = np.column_stack([rng.uniform(0.0, width, count), rng.uniform(0.0, height, count)])
    lit = density[np.clip(motes[:, 1].astype(int), 0, height - 1), np.clip(motes[:, 0].astype(int), 0, width - 1)]
    keep = lit > 0.3
    soft = rng.uniform(0.0, 1.0, count)[keep] ** 3
    artkit.splat(image, motes[keep], 0.07 * lit[keep] * rng.uniform(0.3, 1.0, keep.sum()) / (1.0 + 6.0 * soft),
                 artkit.blackbody(4200), (0.6 + 4.0 * soft) * scale)
    return image


def _route(rng, start, heading, runs):
    """A trace's centre line from `start` (grid units): straight runs,
    horizontal, vertical or diagonal, bending 45 degrees between them."""
    steps = ((1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1))
    path = [start]
    for _ in range(runs):
        dx, dy = steps[heading]
        n = int(rng.integers(3, 15))
        path.append((path[-1][0] + dx * n, path[-1][1] + dy * n))
        heading = (heading + int(rng.choice((-1, 1)))) % 8
    return path


def _offset(path, distance):
    """The path moved sideways by `distance`, mitred at each bend."""
    pts = np.array(path, dtype=np.float64)
    out = []
    for i in range(len(pts)):
        normals = []
        for a, b in ((i - 1, i), (i, i + 1)):
            if a >= 0 and b < len(pts):
                d = (pts[b] - pts[a]) / np.hypot(*(pts[b] - pts[a]))
                normals.append(np.array((-d[1], d[0])))
        n = sum(normals)
        n = n / np.hypot(*n)
        out.append(tuple(pts[i] + n * distance / max(float(np.dot(n, normals[0])), 0.3)))
    return out


def _cells(line):
    """The grid cells a polyline (grid units) passes through."""
    cells = set()
    for (ax, ay), (bx, by) in zip(line, line[1:]):
        n = int(4 * max(abs(bx - ax), abs(by - ay))) + 1
        for i in range(n + 1):
            cells.add((round(ax + (bx - ax) * i / n), round(ay + (by - ay) * i / n)))
    return cells


def _bus(rng, start, heading, lanes, taken, cols, rows):
    """Lanes of traces side by side along one route, shortened until they
    cross nothing already on the board; None if too little is left."""
    path = _route(rng, start, heading, int(rng.integers(2, 6)))
    while len(path) >= 2:
        lines = [_offset(path, k - (lanes - 1) / 2.0) for k in range(lanes)]
        cells = set().union(*(_cells(line) for line in lines))
        inside = all(-2 <= cx <= cols + 2 and -2 <= cy <= rows + 2 for cx, cy in cells)
        if inside and not cells & taken:
            taken |= {(cx + i, cy + j) for cx, cy in cells for i in (-1, 0, 1) for j in (-1, 0, 1)
                      if (i, j) == (0, 0) or lanes == 1}
            return lines
        path = path[:-1]
    return None


def circuits(spec, width, height):
    """Warm light raking across a circuit board, close up: buses of traces
    fanning out of a chip and running on, pads and vias, and the board
    beyond the plane of focus soft."""
    rng = np.random.default_rng(spec['seed'])
    aspect = width / height
    x, y = grid(width, height)
    pitch = 0.02 * height  # the grid the traces run on, in pixels
    cols, rows = int(width / pitch), int(height / pitch)
    px = (np.arange(width) + 0.5)[None, :]
    py = (np.arange(height) + 0.5)[:, None]
    taken = set()
    # The chip, a square package with pins on all four sides.
    side = int(rng.integers(9, 12))
    c0, r0 = int(cols * rng.uniform(0.42, 0.55)), int(rows * rng.uniform(0.12, 0.25))
    taken |= {(c, r) for c in range(c0 - 2, c0 + side + 2) for r in range(r0 - 2, r0 + side + 2)}
    lines = []
    sides = ((6, [(c0 + i, r0 - 2) for i in range(side)]),
             (2, [(c0 + i, r0 + side + 1) for i in range(side)]),
             (4, [(c0 - 2, r0 + i) for i in range(side)]),
             (0, [(c0 + side + 1, r0 + i) for i in range(side)]))
    for heading, pins in sides:
        i = 0
        while i < side:
            lanes = int(rng.integers(2, 6))
            group = pins[i:i + lanes]
            i += lanes + int(rng.integers(0, 2))
            if len(group) < 2 or rng.uniform() < 0.25:
                continue
            mid = ((group[0][0] + group[-1][0]) / 2.0, (group[0][1] + group[-1][1]) / 2.0)
            bus = _bus(rng, mid, heading, len(group), taken, cols, rows)
            if bus:
                lines += bus
    # More buses across the board, then single traces in what room is left.
    for attempt in range(500):
        lanes = int(rng.choice((1, 1, 1, 2, 3, 4, 6, 8))) if attempt < 350 else 1
        start = (int(rng.integers(0, cols)), int(rng.integers(0, rows)))
        bus = _bus(rng, start, int(rng.choice((0, 2, 4, 6))), lanes, taken, cols, rows)
        if bus:
            lines += bus
    dist = np.full((height, width), 1e9)
    ends = []
    for line in lines:
        pts = [(gx * pitch + pitch / 2, gy * pitch + pitch / 2) for gx, gy in line]
        for (ax, ay), (bx, by) in zip(pts, pts[1:]):
            x0, x1 = int(max(min(ax, bx) - 4, 0)), int(min(max(ax, bx) + 4, width))
            y0, y1 = int(max(min(ay, by) - 4, 0)), int(min(max(ay, by) + 4, height))
            if x0 < x1 and y0 < y1:
                d, _ = artkit.segment(px[:, x0:x1], py[y0:y1], ax, ay, bx, by)
                dist[y0:y1, x0:x1] = np.minimum(dist[y0:y1, x0:x1], d)
        ends.append(pts[-1])
    half = 0.24 * pitch
    metal = np.clip(half - dist + 0.5, 0.0, 1.0)
    free = [(c, r) for c in range(cols) for r in range(rows) if (c, r) not in taken]
    vias = [((c + 0.5) * pitch, (r + 0.5) * pitch) for c, r in
            (free[i] for i in rng.choice(len(free), min(len(free), 70), replace=False))]
    for (cx, cy), outer in [(e, 2.2 * half) for e in ends] + [(v, 1.6 * half) for v in vias]:
        x0, x1 = int(max(cx - 2 * outer, 0)), int(min(cx + 2 * outer + 1, width))
        y0, y1 = int(max(cy - 2 * outer, 0)), int(min(cy + 2 * outer + 1, height))
        if x0 < x1 and y0 < y1:
            r = np.hypot(px[:, x0:x1] - cx, py[y0:y1] - cy)
            ring = np.clip(outer - r + 0.5, 0.0, 1.0) * (1.0 - 0.85 * np.clip(0.45 * outer - r + 0.5, 0.0, 1.0))
            metal[y0:y1, x0:x1] = np.maximum(metal[y0:y1, x0:x1], ring)
    # The package and its pins.
    bx0, by0 = c0 * pitch + 0.2 * pitch, r0 * pitch + 0.2 * pitch
    size = side * pitch - 0.4 * pitch
    box = rounded_box(px, py, bx0, by0, bx0 + size, by0 + size, 0.15 * pitch)
    chip = np.clip(0.5 - box, 0.0, 1.0)
    along = np.abs((((px - bx0) / pitch) % 1.0) - 0.5) < 0.22
    down = np.abs((((py - by0) / pitch) % 1.0) - 0.5) < 0.22
    reach = 1.4 * pitch  # how far the pins stand out of the package
    top_bottom = _between(py, by0 - reach, by0) | _between(py, by0 + size, by0 + size + reach)
    left_right = _between(px, bx0 - reach, bx0) | _between(px, bx0 + size, bx0 + size + reach)
    pins = ((along & _between(px, bx0, bx0 + size) & top_bottom)
            | (down & _between(py, by0, by0 + size) & left_right))
    metal = np.maximum(metal, pins.astype(np.float64))
    # A warm pool of light from the upper right, raking low across the
    # board: the gloss of the solder mask shows a band of it, and the edges
    # of the copper that face it shine.
    lx, ly = aspect * rng.uniform(0.72, 0.85), rng.uniform(-0.2, 0.0)
    pool = np.exp(-(((x - lx) / 1.15) ** 2 + ((y - ly) / 0.6) ** 2))
    gy, gx = np.gradient(artkit.blur(metal, 1.2 * height / artkit.HEIGHT))
    towards = np.maximum(np.hypot(lx - x, ly - y), 1e-6)
    facing = np.clip(-(gx * (lx - x) + gy * (ly - y)) / towards, 0.0, None)
    gloss = np.exp(-((x * 0.5 + y - rng.uniform(0.8, 1.0)) / 0.16) ** 2)
    weave = 0.8 + 0.2 * np.cos(2.0 * math.pi * (x - y) * height / 6.0) * np.cos(2.0 * math.pi * (x + y) * height / 6.0)
    copper = np.array((1.0, 0.6, 0.32))
    board = np.array((0.16, 0.30, 0.20))
    m, g = metal[:, :, None], gloss[:, :, None]
    bevel = np.clip(-box / (0.12 * pitch), 0.0, 1.0)
    lit_edge = -(np.gradient(box, axis=1) * (lx - x) + np.gradient(box, axis=0) * (ly - y))
    rim = (bevel < 1.0) * chip * np.clip(lit_edge, 0.0, None) / towards
    image = pool[:, :, None] * (m * copper * (0.12 + 0.75 * g) + 2.5 * facing[:, :, None] * copper * (0.25 + g)
                                + (1.0 - m) * board * (0.05 + 0.25 * g) * weave[:, :, None])
    package = (chip * pool * (0.01 + 0.03 * gloss))[:, :, None] * np.array((0.6, 0.6, 0.65))
    image = image * (1.0 - chip[:, :, None]) + package
    image += (0.4 * rim * pool)[:, :, None] * copper
    image = image * artkit.blackbody(spec.get('kelvin', 2700)) * spec.get('peak', 0.6)
    # A shallow focus: sharp along a diagonal band, soft away from it.
    focus = np.exp(-((x * 0.3 - y + rng.uniform(0.0, 0.25)) / 0.3) ** 2)[:, :, None]
    near, far = 5.0 * height / artkit.HEIGHT, 14.0 * height / artkit.HEIGHT
    soft = 0.6 * artkit.blur(image, near) + 0.4 * artkit.blur(image, far)
    return image * focus + soft * (1.0 - focus)


def _between(v, lo, hi):
    return (v > lo) & (v < hi)


def rounded_box(px, py, x0, y0, x1, y1, r):
    """Signed distance to a rounded rectangle, in pixels (negative inside)."""
    cx, cy, hx, hy = (x0 + x1) / 2, (y0 + y1) / 2, (x1 - x0) / 2 - r, (y1 - y0) / 2 - r
    qx, qy = np.abs(px - cx) - hx, np.abs(py - cy) - hy
    return np.hypot(np.maximum(qx, 0.0), np.maximum(qy, 0.0)) + np.minimum(np.maximum(qx, qy), 0.0) - r


def panels(spec, width, height):
    """Panes of frosted glass at different depths over soft coloured lights:
    each scatters what lies behind it into a milky blur, a little brighter
    than the dark around it, and catches the light along its top edge."""
    rng = np.random.default_rng(spec['seed'])
    aspect = width / height
    x, y = grid(width, height)
    px = (np.arange(width) + 0.5)[None, :]
    py = (np.arange(height) + 0.5)[:, None]
    cool = np.array((0.22, 0.6, 1.0))
    violet = np.array((0.62, 0.3, 1.0))
    teal = np.array((0.15, 0.85, 0.8))
    image = np.zeros((height, width, 3))
    for colour, (cx, cy, r, light) in ((cool, (aspect * rng.uniform(0.66, 0.76), 0.18, 0.36, 0.16)),
                                       (violet, (aspect * rng.uniform(0.3, 0.4), 0.28, 0.34, 0.11)),
                                       (teal, (aspect * rng.uniform(0.86, 0.94), 0.5, 0.22, 0.07))):
        image += light * colour * np.exp(-((x - cx) ** 2 + (y - cy) ** 2) / (r * r))[:, :, None]
    scale = height / artkit.HEIGHT
    count = int(rng.integers(4, 6))
    for k in range(count):
        near = (k + 1) / count  # the last is the nearest
        w = (0.3 + 0.32 * near) * height * rng.uniform(0.9, 1.1)
        h = w * rng.uniform(0.55, 0.68)
        cx = aspect * height * (0.32 + 0.6 * (k + rng.uniform(0.2, 0.8)) / count)
        y0 = height * (0.04 + (0.2 if k % 2 else 0.0) + rng.uniform(0.0, 0.08))
        x0 = cx - w / 2
        r = (0.03 + 0.02 * near) * height
        d = rounded_box(px, py, x0, y0, x0 + w, y0 + h, r)
        inside = np.clip(0.5 - d, 0.0, 1.0)[:, :, None]
        # The pane frosts what is behind it and casts a soft shadow around
        # its lower edge onto what is behind.
        frosted = artkit.blur(image, (16.0 + 24.0 * near) * scale) * 1.3 + 0.006 * np.array((0.9, 0.95, 1.0))
        drop = np.clip(0.5 - rounded_box(px, py - 18 * near * scale, x0, y0, x0 + w, y0 + h, r), 0.0, 1.0)
        image *= (1.0 - 0.55 * artkit.blur(drop, (10.0 + 18.0 * near) * scale))[:, :, None]
        depth = np.maximum(py - y0, 0.0) / h
        sheen = (0.012 * np.exp(-depth / 0.3))[:, :, None]
        edge = np.exp(-((d + 1.0 * scale) / (0.8 * scale)) ** 2)
        top = np.exp(-depth / 0.08) * (0.4 + 0.6 * smoothstep(x0, x0 + 0.5 * w, px))
        image = (image * (1.0 - inside) + inside * (frosted + sheen)
                 + ((0.025 + 0.12 * near * top) * edge)[:, :, None])
    return image


# Ambient: a still of one of the studio's loops, in layers for parallax.

def hable(x):
    """John Hable's filmic curve, as ffmpeg's tonemap filter has it."""
    a, b, c, d, e, f = 0.15, 0.50, 0.10, 0.20, 0.02, 0.30
    return (x * (x * a + b * c) + d * e) / (x * (x * a + b) + d * f) - e / f


def tone_map(nits, peak_nits, exposure=1.0):
    """Linear Display P3 in cd/m² to the sRGB signal, as the studio's SDR
    previews do it (studio/verify.py, TONEMAP): 100 cd/m² is 1, BT.709
    primaries, Hable's curve on the brightest channel, scaled so that
    `peak_nits` maps to white, which keeps each colour's hue."""
    to_709 = np.array(hdr.conversion_matrix(hdr.P3_D65, hdr.BT709))
    rgb = (np.asarray(nits, dtype=np.float64) * exposure / 100.0) @ to_709.T
    signal = np.maximum(rgb.max(axis=-1, keepdims=True), 1e-6)
    return artkit.srgb_encode(np.clip(rgb * hable(signal) / hable(peak_nits / 100.0) / signal, 0.0, 1.0))


def over(colour, alpha, under):
    return colour * alpha[:, :, None] + under * (1.0 - alpha[:, :, None])


def ambient(spec, width, height):
    """The still as {'': picture, '-bg', '-mid', '-fg': RGBA layers}, each
    encoded sRGB, the layers straight (not premultiplied) alpha.

    The shell blends 8-bit sRGB textures as they are, so the layers are
    worked out on the encoded signal: -bg is the sky, opaque; -mid is the
    scene's light (the aurora's curtains, the fireflies), the colour and
    alpha that, laid over the sky without its stars, give the sky with that
    light; -fg is what stands in front. The picture is the three laid over
    each other, so the flat copy and the layers at rest look the same.
    """
    make = SOURCES[spec['source']]
    frames = round(spec['seconds'] * spec['fps'])
    seed = {'seed': spec['seed']} if 'seed' in spec else {}
    scene = make(width, height, frames, spec['fps'], **seed)
    parts = scene.layers_at(2.0 * math.pi * spec['frame'] / frames)
    peak, exposure = spec.get('peak_nits', 725.0), spec.get('exposure', 1.0)

    def tone(nits):
        return tone_map(soft_clip(nits, 600.0, PEAK_NITS), peak, exposure)

    back = tone(parts['back'])
    # The sky with its stars smoothed away, so no star is copied into -mid.
    smooth = artkit.blur(parts['back'], 2.0 * height / artkit.HEIGHT)
    under, lit = tone(smooth), tone(smooth + parts['light'])
    # The least alpha whose colour stays in range where the light brightens
    # the sky. Where the curve darkens a near-black channel of the sky as the
    # light comes in (it keeps hues by scaling the brightest channel), the
    # colour clips at 0 instead: a fraction of a code, not worth an alpha.
    rise = np.where(lit > under, (lit - under) / np.maximum(1.0 - under, 1e-6), 0.0)
    alpha = np.clip(rise.max(axis=2), 0.0, 1.0)
    alpha = np.where(alpha < 1e-4, 0.0, alpha)
    light = np.where(alpha[:, :, None] > 0.0,
                     (lit - (1.0 - alpha[:, :, None]) * under) / np.maximum(alpha[:, :, None], 1e-4), 0.0)
    cover = np.clip(parts['cover'].astype(np.float64), 0.0, 1.0)
    front = np.where(cover[:, :, None] > 0.0, tone(parts['front'] / np.maximum(cover, 1e-6)[:, :, None]), 0.0)
    scrim = artkit.title_scrim(height, width)[:, :, None]
    back, light, front = back * scrim, np.clip(light, 0.0, 1.0) * scrim, front * scrim
    picture = over(front, cover, over(light, alpha, back))
    return {'': picture,
            '-bg': np.concatenate([back, np.ones((height, width, 1))], axis=2),
            '-mid': np.concatenate([light, alpha[:, :, None]], axis=2),
            '-fg': np.concatenate([front, cover[:, :, None]], axis=2)}


SCENES = {'ambient': ambient, 'palette': palette, 'arrival': arrival, 'towers': towers, 'beam': beam,
          'circuits': circuits, 'panels': panels}


def load_table(path=TABLE):
    with open(path, 'rb') as f:
        return tomllib.load(f)


def render(name, spec, width=artkit.WIDTH, height=artkit.HEIGHT):
    """The hero `name` as {suffix: encoded float image}: '' for the picture,
    and for a layered hero '-bg', '-mid', '-fg' (RGBA, straight alpha)."""
    if spec['scene'] == 'ambient':
        return ambient(spec, width, height)
    linear = SCENES[spec['scene']](spec, width, height)
    rng = np.random.default_rng([spec['seed'], 1])
    return {'': artkit.finish(rng, linear, spec.get('grain', 0.06))}


def half(encoded):
    """The half-size copy of one image: averaged in linear light, or for a
    layer with alpha, averaged premultiplied on the encoded signal, as the
    shell blends it."""
    if encoded.shape[2] == 3:
        return artkit.srgb_encode(artkit.downsample(artkit.srgb_decode(encoded)))
    alpha = encoded[:, :, 3:]
    small = artkit.downsample(np.concatenate([encoded[:, :, :3] * alpha, alpha], axis=2))
    a = small[:, :, 3:]
    return np.concatenate([np.where(a > 0.0, small[:, :, :3] / np.maximum(a, 1e-9), 0.0), a], axis=2)


def halves(images):
    """The half-size copies of a hero's images. A layered hero's picture is
    its half-size layers laid over each other, as at full size, so the flat
    copy and the layers agree at either size."""
    small = {suffix: half(encoded) for suffix, encoded in images.items()}
    if '-bg' in small:
        bg, mid, fg = small['-bg'], small['-mid'], small['-fg']
        small[''] = over(fg[:, :, :3], fg[:, :, 3], over(mid[:, :, :3], mid[:, :, 3], bg[:, :, :3]))
    return small


def write(name, images, out):
    """Writes the files of one hero; returns [(file, pixels, encoded)]."""
    written = []
    small = halves(images)
    for suffix, encoded in images.items():
        for tail, picture in (('', encoded), ('-half', small[suffix])):
            pixels = artkit.quantize(picture)
            path = os.path.join(out, f'{name}{suffix}{tail}.png')
            artkit.write_png(path, pixels)
            written.append((path, pixels, picture))
    return written


def report(path, pixels, encoded):
    """One line on a written file: its size, light levels and banding."""
    head = f'{os.path.basename(path):26} {os.path.getsize(path) / 1e6:5.2f} MB'
    if pixels.shape[2] == 4:
        alpha = pixels[:, :, 3] / 255.0
        return f'{head}  layer: alpha mean {alpha.mean():.3f}, opaque {(pixels[:, :, 3] == 255).mean():.1%}'
    s = artkit.light_stats(pixels)
    before, after = artkit.banding(encoded, pixels)
    return (f'{head}  mean {s["mean"]:.3f} (linear {s["mean_linear"]:.3f})  p99 {s["p99"]:.3f}  '
            f'max {s["max"]:.3f}  band {s["band_mean"]:.3f}->{s["band_end"]:.3f}  '
            f'title {s["contrast"]:.1f}:1  bands {before}->{after} px')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('names', nargs='*', help='heroes to draw, by file name (hero-hdmi1)')
    parser.add_argument('--all', action='store_true', help='draw every hero in the table')
    parser.add_argument('--out', default=OUT, help='output directory (default shell/assets/art)')
    parser.add_argument('--table', default=TABLE, help='the table of heroes and seeds (default heroes.toml)')
    args = parser.parse_args(sys.argv[1:] if argv is None else argv)
    table = load_table(args.table)
    names = list(table) if args.all else args.names
    unknown = [n for n in names if n not in table]
    if unknown or not names:
        parser.error(f'unknown heroes: {", ".join(unknown)}' if unknown else 'name heroes, or --all')
    os.makedirs(args.out, exist_ok=True)
    for name in names:
        started = time.monotonic()
        files = write(name, render(name, table[name]), args.out)
        for path, pixels, encoded in files:
            if '-half' not in path:
                print(report(path, pixels, encoded))
        print(f'  {name}: {time.monotonic() - started:.1f} s', file=sys.stderr)


if __name__ == '__main__':
    main()
