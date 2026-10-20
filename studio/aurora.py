"""Aurora over a mountain lake: a looping HDR scene made for an OLED panel.

Curtains of green aurora with red tops and pink lower fringes ripple over a
starfield, behind a black mountain ridge, and reflect in a lake. Nearly all of
the screen is black or close to it; the curtains glow at tens to a few hundred
cd/m², and a handful of stars reach toward the B9's ~700-800 cd/m² peak.

Everything that moves is periodic in the loop: each motion runs a whole
number of cycles per loop, so frame N is frame 0 again and the loop has no
seam. Lengths are in picture heights, so every resolution frames the same
picture. Colours are linear Display P3 (D65) in cd/m².
"""

import math

import numpy as np

import hdr

PRIMARIES = hdr.P3_D65
PEAK_NITS = 800.0  # highlights roll off to this, just above the B9's peak
TAU = 2.0 * math.pi

WATERLINE = 0.78  # where the lake meets the mountains, from the top
STAR_SIGMA = 0.0005  # star size (Gaussian sigma), in picture heights

# Linear P3 colours, brightest channel 1.
GREEN = (0.10, 1.00, 0.32)  # oxygen at 557.7 nm, the body of the curtain
RED = (1.00, 0.08, 0.45)  # oxygen at 630 nm, high and faint
PINK = (1.00, 0.25, 0.80)  # nitrogen, the lower fringe of active curtains
AIRGLOW = (0.25, 1.00, 0.55)
STAR_COLOURS = ((0.70, 0.80, 1.00), (0.90, 0.93, 1.00), (1.00, 0.95, 0.85), (1.00, 0.80, 0.60))
STAR_COLOUR_WEIGHTS = (0.25, 0.35, 0.25, 0.15)


def harmonic(rate_hz, seconds):
    """The whole number of cycles per loop nearest rate_hz, at least one.

    Rounding every rate to whole cycles is what makes the loop seamless; the
    sign gives the direction of travel.
    """
    cycles = max(1, int(round(abs(rate_hz) * seconds)))
    return cycles if rate_hz >= 0 else -cycles


def erf(x):
    """Error function, Abramowitz and Stegun 7.1.26 (error under 1.5e-7)."""
    sign = np.sign(x)
    x = np.abs(x)
    t = 1.0 / (1.0 + 0.3275911 * x)
    poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))))
    return sign * (1.0 - poly * np.exp(-x * x))


def soft_clip(nits, knee, peak):
    """Leaves values below knee alone and rolls those above smoothly to peak."""
    over = np.maximum(nits - knee, 0.0)
    return np.where(nits > knee, knee + (peak - knee) * np.tanh(over / (peak - knee)), nits)


class Waves:
    """A sum of travelling sines, each a whole number of cycles per loop."""

    def __init__(self, rng, seconds, components):
        # components: (amplitude, spatial frequency in radians per picture
        # height, speed in Hz) with the frequency and speed jittered by rng.
        self.parts = []
        for amplitude, freq, rate in components:
            freq *= rng.uniform(0.85, 1.15)
            rate *= rng.uniform(0.8, 1.2) * rng.choice((-1.0, 1.0))
            self.parts.append((amplitude, freq, harmonic(rate, seconds), rng.uniform(0.0, TAU)))

    def at(self, x, theta):
        return sum(a * np.sin(f * x + n * theta + p) for a, f, n, p in self.parts)

    def slope(self, x, theta):
        return sum(a * f * np.cos(f * x + n * theta + p) for a, f, n, p in self.parts)


class Curtain:
    """One curtain: a wavy lower edge with light rising from it in rays."""

    def __init__(self, rng, seconds, base, sway, height, nits, red):
        self.base, self.height, self.nits, self.red = base, height, nits, red
        self.edge = Waves(rng, seconds, [(sway, 2.2, 0.020), (0.6 * sway, 4.5, 0.035),
                                         (0.25 * sway, 9.0, 0.060)])
        # Rays: narrow vertical striations drifting along the curtain.
        self.rays = [(Waves(rng, seconds, [(1.0, freq, rate)]), sharp, weight)
                     for freq, rate, sharp, weight in ((28.0, 0.04, 2.0, 1.0), (47.0, 0.05, 3.0, 0.8),
                                                       (83.0, 0.07, 5.0, 0.6), (131.0, 0.09, 7.0, 0.5),
                                                       (213.0, 0.11, 9.0, 0.4))]
        self.warp = Waves(rng, seconds, [(0.04, 2.3, 0.03)])
        # Where along its length the curtain is lit; it has dark gaps.
        self.envelope = Waves(rng, seconds, [(1.0, 1.7, 0.025), (0.5, 3.9, 0.04)])
        self.breath = (harmonic(0.05, seconds), rng.uniform(0.0, TAU))

    def draw(self, sky, x, y, theta, height):
        """Adds this curtain's light (cd/m²) to sky, an (rows, W, 3) array.

        x and y are the pixel centres in picture heights; height is in pixels.
        """
        lower = self.base + self.edge.at(x, theta)
        bend = np.abs(self.edge.slope(x, theta))
        warped = x + self.warp.at(x, theta)
        rays = sum(w * (0.5 + 0.5 * waves.at(warped, theta)) ** s for waves, s, w in self.rays)
        rays /= sum(w for _, _, w in self.rays)
        envelope = np.clip(0.5 + 0.5 * self.envelope.at(x, theta) / 1.5, 0.0, 1.0)
        envelope = np.clip(1.6 * envelope - 0.45, 0.0, 1.0) ** 1.5
        n, p = self.breath
        # Brightest where the curtain folds toward the viewer (steep edge).
        glow = (self.nits * (0.88 + 0.12 * math.sin(n * theta + p)) * envelope
                * (1.0 + 1.5 * np.tanh(2.5 * bend))).astype(np.float32)
        reach = (self.height * (0.5 + 1.0 * rays)).astype(np.float32)  # rays rise further than gaps
        # A bright band along the lower edge, and above it light only in rays.
        band = (0.55 + 0.45 * rays).astype(np.float32)
        streak = (0.45 * rays ** 1.5).astype(np.float32)

        # Above 9 reaches the light is under a ten-thousandth of the edge's.
        top = max(0, int((lower.min() - 9.0 * reach.max()) * height) - 1)
        rows = slice(top, sky.shape[0])
        h = (lower[None, :] - y[rows, None]).astype(np.float32)  # height above the lower edge
        above = np.maximum(h, 0.0)
        edge = np.exp(-(np.minimum(h, 0.0) / 0.004) ** 2)
        tall = np.exp(-above / reach)
        body = edge * (band * np.exp(-above / (0.3 * reach)) + streak * tall)
        tops = (self.red * streak) * tall * (1.0 - np.exp(-(above / (1.2 * reach)) ** 2))
        active = (glow / (2.5 * self.nits)) ** 2
        fringe = 0.6 * active * np.exp(-((h + 0.003) / 0.0035) ** 2)
        for c in range(3):
            sky[rows, :, c] += glow * (GREEN[c] * body + RED[c] * tops + PINK[c] * fringe)


class Aurora:
    """The scene. render(i) is frame i of a loop of `frames` frames."""

    def __init__(self, width, height, frames, fps, seed=2026):
        self.width, self.height, self.frames = width, height, frames
        seconds = frames / fps
        rng = np.random.default_rng(seed)
        self.x = ((np.arange(width) + 0.5) / height).astype(np.float32)
        self.y = ((np.arange(height) + 0.5) / height).astype(np.float32)
        self.water_row = round(WATERLINE * height)

        self.curtains = [
            Curtain(rng, seconds, base=0.63, sway=0.025, height=0.05, nits=50.0, red=0.0),
            Curtain(rng, seconds, base=0.50, sway=0.060, height=0.14, nits=120.0, red=0.3),
            Curtain(rng, seconds, base=0.31, sway=0.045, height=0.18, nits=60.0, red=0.6),
        ]
        self.ridge = self._ridge(rng)
        self._stars(rng, seconds)
        self.ripples = [(0.6, 9.0, harmonic(0.25, seconds), rng.uniform(0.0, TAU)),
                        (0.4, 23.0, -harmonic(0.4, seconds), rng.uniform(0.0, TAU))]

    def _ridge(self, rng):
        """The mountains' outline: per column, the row (in picture heights) of the ridge."""
        x = self.x.astype(np.float64)
        rise = 0.015 + 0.035 * (0.5 + 0.5 * np.sin(1.3 * x + 2.0)) * (0.5 + 0.5 * np.sin(0.55 * x + 0.3))
        for amplitude, freq in ((0.018, 3.1), (0.009, 7.3), (0.004, 17.0), (0.002, 41.0), (0.001, 97.0)):
            rise += amplitude * np.abs(np.sin(freq * rng.uniform(0.9, 1.1) * x + rng.uniform(0.0, TAU)))
        return WATERLINE - rise

    def _stars(self, rng, seconds):
        aspect = self.width / self.height
        count = int(1100 * aspect * WATERLINE)
        x = rng.uniform(0.0, aspect, count)
        y = rng.uniform(0.0, WATERLINE, count)
        # Mostly faint, a few bright, and a handful of highlights near the panel's peak.
        peak = np.minimum(0.8 * rng.uniform(0.0, 1.0, count) ** (-1.0 / 1.1), 300.0)
        heroes = rng.choice(np.flatnonzero(y < 0.45), size=5, replace=False)
        peak[heroes] = (760.0, 650.0, 560.0, 470.0, 400.0)
        # Haze dims stars near the horizon.
        peak *= np.clip((WATERLINE - y) / 0.25, 0.15, 1.0)
        colour = np.array(STAR_COLOURS)[rng.choice(len(STAR_COLOURS), count, p=STAR_COLOUR_WEIGHTS)]
        # Twinkle, and a slow fade so that no highlight sits still at full
        # brightness (the panel is an OLED). Both only ever dim a star.
        twinkle_rate = rng.uniform(0.3, 1.5, count)
        self.star_twinkle = (rng.uniform(0.1, 0.35, count),
                             np.array([harmonic(r, seconds) for r in twinkle_rate]),
                             rng.uniform(0.0, TAU, count))
        self.star_fade = (np.where(peak > 100.0, 0.4, 0.0), rng.uniform(0.0, TAU, count))
        self.star_x, self.star_y, self.star_peak, self.star_colour = x, y, peak, colour

    def _draw_stars(self, sky, theta):
        amount, cycles, phase = self.star_twinkle
        depth, fade_phase = self.star_fade
        peak = (self.star_peak * (1.0 - amount * (0.5 - 0.5 * np.sin(cycles * theta + phase)))
                * (1.0 - depth * (0.5 - 0.5 * np.cos(theta + fade_phase))))
        sigma = STAR_SIGMA * self.height
        radius = int(math.ceil(3.5 * sigma)) + 1
        offsets = np.arange(-radius, radius + 1)
        # Centred on pixels, so a star's brightest pixel is its peak at any size.
        px = np.floor(self.star_x * self.height) + 0.5
        py = np.floor(self.star_y * self.height) + 0.5
        cols = np.floor(px).astype(int)[:, None] + offsets
        rows = np.floor(py).astype(int)[:, None] + offsets
        # Each pixel gets the star's light that falls on its square.
        s = sigma * math.sqrt(2.0)
        wx = 0.5 * (erf((cols + 1 - px[:, None]) / s) - erf((cols - px[:, None]) / s))
        wy = 0.5 * (erf((rows + 1 - py[:, None]) / s) - erf((rows - py[:, None]) / s))
        centre = float(erf(np.array(0.5 / s))) ** 2  # a star centred on a pixel peaks at `peak`
        weight = (peak / centre)[:, None, None] * wy[:, :, None] * wx[:, None, :]
        r = np.broadcast_to(rows[:, :, None], weight.shape)
        c = np.broadcast_to(cols[:, None, :], weight.shape)
        inside = (r >= 0) & (r < sky.shape[0]) & (c >= 0) & (c < sky.shape[1])
        for ch in range(3):
            np.add.at(sky[:, :, ch], (r[inside], c[inside]),
                      (weight * self.star_colour[:, ch, None, None])[inside].astype(np.float32))

    def _reflect(self, sky, theta):
        """The lake: the sky and mountains mirrored, rippled and darkened."""
        rows = self.height - self.water_row
        depth = (np.arange(rows) + 0.5) / rows  # 0 at the far shore, 1 at the bottom
        source = np.clip(self.water_row - 1 - np.arange(rows), 0, self.water_row - 1)
        # Ripples crowd together toward the far shore, as perspective does.
        u = 1.0 / (depth + 0.12)
        shift = self.height * (0.0015 + 0.010 * depth) * sum(
            a * np.sin(TAU * f * u + n * theta + p) for a, f, n, p in self.ripples)
        x = np.clip(np.arange(self.width)[None, :] + shift[:, None], 0, self.width - 1)
        left = np.floor(x).astype(int)
        right = np.minimum(left + 1, self.width - 1)
        t = (x - left)[:, :, None].astype(np.float32)
        mirrored = sky[source[:, None], left] * (1 - t) + sky[source[:, None], right] * t
        reflectance = (0.2 + 0.35 * (1.0 - depth) ** 2).astype(np.float32)
        return mirrored * reflectance[:, None, None]

    def render(self, index):
        """Frame `index` (taken modulo the loop) as (H, W, 3) float32 cd/m²."""
        return self.frame_at(TAU * (index % self.frames) / self.frames)

    def frame_at(self, theta):
        """The picture at loop phase theta: 0 and 2π are the same moment."""
        sky = np.zeros((self.water_row, self.width, 3), dtype=np.float32)
        y = self.y[:self.water_row]

        glow = 2.0 * np.exp(-(WATERLINE - y) / 0.10)
        for c in range(3):
            sky[:, :, c] += (AIRGLOW[c] * glow)[:, None]
        for curtain in self.curtains:
            curtain.draw(sky, self.x, y, theta, self.height)
        self._draw_stars(sky, theta)

        # The mountains are black; their outline is antialiased.
        ridge_rows = self.ridge * self.height
        covered = np.clip(np.arange(self.water_row)[:, None] + 1.0 - ridge_rows[None, :], 0.0, 1.0)
        sky *= (1.0 - covered).astype(np.float32)[:, :, None]

        frame = np.concatenate([sky, self._reflect(sky, theta)], axis=0)
        return soft_clip(frame, 600.0, PEAK_NITS).astype(np.float32)
