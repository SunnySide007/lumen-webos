"""Fireflies at the edge of a forest: a looping HDR scene made for an OLED panel.

Warm points of light drift over a meadow and flash, in front of a black
treeline under a faint deep-blue sky; grass in the foreground hides the
lowest. Almost all of the screen is black. A firefly's core reaches a few
hundred cd/m², the brightest toward the B9's peak, with a soft glow around
it, and every one of them moves and dims, so no highlight sits still.

Paths and flashes run whole numbers of cycles per loop, so the loop has no
seam. Lengths are in picture heights; colours are linear Display P3 (D65) in
cd/m².
"""

import numpy as np

import hdr
from scenekit import PEAK_NITS, TAU, harmonic, resolved_peak, soft_clip, splat

PRIMARIES = hdr.P3_D65

TREELINE = 0.58  # where the forest meets the meadow, from the top
GRASS = 0.92  # foreground grass, from the top
SKY = (0.20, 0.35, 1.00)  # deep blue
GLOW = (0.62, 1.00, 0.10)  # firefly light, yellow-green near 560 nm
MEADOW = (0.15, 0.30, 0.25)  # the faint ground the grass stands against
CORE_SIGMA = 0.0015  # a near firefly's core, in picture heights
HALO_SIGMA = 0.0120
HALO = 0.05  # the halo's peak, relative to the core's
DENSITY = 40  # fireflies per square picture height


def _coverage(edge_rows, height):
    """Per pixel, how much of it lies below `edge_rows` (one row per column)."""
    return np.clip(np.arange(height)[:, None] + 1.0 - edge_rows[None, :], 0.0, 1.0).astype(np.float32)


class Fireflies:
    """The scene. render(i) is frame i of a loop of `frames` frames."""

    def __init__(self, width, height, frames, fps, seed=7):
        self.width, self.height, self.frames = width, height, frames
        seconds = frames / fps
        rng = np.random.default_rng(seed)
        x = (np.arange(width) + 0.5) / height
        y = (np.arange(height) + 0.5) / height

        # The forest: rounded crowns with taller, pointed firs among them.
        top = TREELINE - 0.04 - 0.03 * (0.5 + 0.5 * np.sin(0.9 * x + 1.3))
        for amplitude, freq in ((0.025, 6.0), (0.012, 15.0), (0.006, 37.0)):
            top -= amplitude * np.abs(np.sin(freq * rng.uniform(0.9, 1.1) * x + rng.uniform(0.0, TAU)))
        firs = rng.uniform(0.0, width / height, int(9 * width / height))
        for centre, size in zip(firs, rng.uniform(0.04, 0.11, firs.size)):
            top = np.minimum(top, TREELINE - 0.02 - size * np.clip(1.0 - np.abs(x - centre) / (0.25 * size), 0.0, 1.0))
        # Grass: thin blades of different heights.
        blades = GRASS - 0.025 * np.abs(np.sin(97.0 * x)) ** 0.3 - 0.02 * np.abs(np.sin(41.0 * x + 1.0)) ** 4
        # The forest is a band, from the treetops down to its floor; the meadow
        # lies in front of it.
        floor = TREELINE + 0.012 + 0.004 * np.sin(2.1 * x + 0.7)
        self.trees = _coverage(top * height, height) - _coverage(floor * height, height)
        self.grass = _coverage(blades * height, height)

        # A faint sky, brightest just above the trees, where the forest shows
        # against it, and fainter still the meadow, where the grass does.
        sky = 0.05 + 1.2 * np.exp(-np.maximum(TREELINE - y, 0.0) / 0.12)
        meadow = 0.12 * np.exp(-np.maximum(y - TREELINE, 0.0) / 0.5)
        below = (y > TREELINE)[:, None]
        self.sky = np.where(below, meadow[:, None] * np.array(MEADOW)[None, :],
                            sky[:, None] * np.array(SKY)[None, :]).astype(np.float32)

        count = int(DENSITY * (width / height) * (GRASS - TREELINE + 0.15))
        self.x0 = rng.uniform(0.0, width / height, count)
        self.y0 = rng.uniform(TREELINE - 0.15, GRASS, count)
        # Nearer (lower) fireflies are larger and brighter.
        self.near = 0.4 + 0.6 * (self.y0 - (TREELINE - 0.15)) / (GRASS - TREELINE + 0.15)
        self.paths = [self._path(rng, seconds, count) for _ in range(2)]  # x, then y
        self.flash_cycles = np.array([harmonic(r, seconds) for r in rng.uniform(0.06, 0.25, count)])
        self.flash_phase = rng.uniform(0.0, TAU, count)
        self.flash_sharpness = rng.uniform(4.0, 12.0, count)
        self.peak = self.near * rng.uniform(150.0, 450.0, count)
        # The three brightest are the nearest of those clear of the grass.
        clear = np.flatnonzero(self.y0 < GRASS - 0.1)
        heroes = clear[np.argsort(self.near[clear])[-3:]]
        self.peak[heroes] = (540.0, 620.0, 700.0)[3 - heroes.size:]
        self.colour = np.array(GLOW)[None, :] * rng.uniform(0.85, 1.0, (count, 3))
        self.colour[:, 1] = 1.0

    def _path(self, rng, seconds, count):
        """A closed wandering path per firefly: three sines of whole cycles."""
        parts = []
        for amplitude, rate in ((0.05, 0.03), (0.02, 0.07), (0.008, 0.16)):
            cycles = np.array([harmonic(r, seconds) for r in rng.uniform(0.6, 1.4, count) * rate
                               * rng.choice((-1.0, 1.0), count)])
            parts.append((amplitude * rng.uniform(0.5, 1.0, count), cycles, rng.uniform(0.0, TAU, count)))
        return parts

    def render(self, index):
        """Frame `index` (taken modulo the loop) as (H, W, 3) float32 cd/m²."""
        return self.frame_at(TAU * (index % self.frames) / self.frames)

    def _fireflies(self, frame, theta):
        """Adds the fireflies at loop phase theta to frame."""
        def wander(parts):
            return sum(a * np.sin(n * theta + p) for a, n, p in parts)

        x = (self.x0 + self.near * wander(self.paths[0])) * self.height
        y = (self.y0 + self.near * wander(self.paths[1])) * self.height
        pulse = (0.5 + 0.5 * np.cos(self.flash_cycles * theta + self.flash_phase)) ** self.flash_sharpness
        light = self.peak * (0.04 + 0.96 * pulse)
        # Moving points: each gives the same light at any resolution.
        for size, share in ((CORE_SIGMA, 1.0), (HALO_SIGMA, HALO)):
            sigma = size * self.height * self.near
            splat(frame, x, y, resolved_peak(share * light, sigma), self.colour, sigma)

    def frame_at(self, theta):
        """The picture at loop phase theta: 0 and 2π are the same moment."""
        frame = self.sky[:, None, :] * (1.0 - self.trees)[:, :, None]
        self._fireflies(frame, theta)
        frame *= (1.0 - self.grass)[:, :, None]
        return soft_clip(frame, 600.0, PEAK_NITS).astype(np.float32)

    def layers_at(self, theta):
        """The picture at theta in depth layers, for stills whose layers move
        apart (parallax in the Top Shelf; tools/art/heroes.py).

        Returns (H, W) or (H, W, 3) float32 arrays in cd/m², before the
        highlights' roll-off: `back`, the sky with the black treeline;
        `light`, the fireflies; `cover`, how much the foreground grass hides;
        and `front`, the grass's own light (none). soft_clip((back + light)
        * (1 - cover) + front) is frame_at(theta), up to float rounding.
        """
        back = (self.sky[:, None, :] * (1.0 - self.trees)[:, :, None]).astype(np.float32)
        light = np.zeros_like(back)
        self._fireflies(light, theta)
        return {'back': back, 'light': light, 'cover': self.grass.astype(np.float32),
                'front': np.zeros_like(back)}
