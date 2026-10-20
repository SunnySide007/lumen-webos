"""Shared pieces for studio scenes: loop timing, highlights and their roll-off.

Scenes give lengths in picture heights and colours as linear Display P3 in
cd/m², and every motion runs a whole number of cycles per loop.
"""

import math

import numpy as np

TAU = 2.0 * math.pi
PEAK_NITS = 800.0  # highlights roll off to this, just above the B9's ~700-800 peak


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


def resolved_peak(peak, sigma):
    """The centre pixel of a Gaussian point whose continuous peak is `peak`.

    sigma is the point's size in pixels. A point smaller than a pixel spreads
    its light over that pixel, so its pixel is dimmer than `peak`, while the
    light it gives the picture stays the same at any resolution; from about
    a pixel up, the centre pixel reaches `peak` (93% at sigma 1).
    """
    sigma = np.asarray(sigma, dtype=np.float64)
    return peak * 2.0 * math.pi * sigma ** 2 * erf(0.5 / (sigma * math.sqrt(2.0))) ** 2


def splat(image, px, py, peak, colour, sigma):
    """Adds Gaussian points of light to image, an (rows, W, 3) array.

    px and py are the points' centres in pixels (pixel i spans [i, i + 1)),
    peak their brightness in cd/m² (what a point centred on a pixel gives
    that pixel), colour an (N, 3) array, and sigma the Gaussian's size in
    pixels, one for all or one per point. Each pixel gets the light that
    falls on its square.
    """
    if np.size(px) == 0:
        return
    sigma = np.broadcast_to(np.asarray(sigma, dtype=np.float64), np.shape(px))
    radius = int(math.ceil(3.5 * sigma.max())) + 1
    offsets = np.arange(-radius, radius + 1)
    cols = np.floor(px).astype(int)[:, None] + offsets
    rows = np.floor(py).astype(int)[:, None] + offsets
    s = (sigma * math.sqrt(2.0))[:, None]
    wx = 0.5 * (erf((cols + 1 - px[:, None]) / s) - erf((cols - px[:, None]) / s))
    wy = 0.5 * (erf((rows + 1 - py[:, None]) / s) - erf((rows - py[:, None]) / s))
    centre = erf(0.5 / s[:, 0]) ** 2
    weight = (peak / centre)[:, None, None] * wy[:, :, None] * wx[:, None, :]
    r = np.broadcast_to(rows[:, :, None], weight.shape)
    c = np.broadcast_to(cols[:, None, :], weight.shape)
    inside = (r >= 0) & (r < image.shape[0]) & (c >= 0) & (c < image.shape[1])
    for ch in range(3):
        np.add.at(image[:, :, ch], (r[inside], c[inside]),
                  (weight * colour[:, ch, None, None])[inside].astype(np.float32))
