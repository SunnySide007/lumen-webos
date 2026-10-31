"""Shared pieces for Lumen's art: colour, fields, finishing, dither, PNG and the checks.

Heroes are drawn as linear light with sRGB (BT.709) primaries, where 1.0 is
the UI's white, in coordinates of picture heights: x runs from 0 to the
aspect ratio, y from 0 at the top to 1 at the bottom. The same picture can
then be drawn at any size. Finishing (finish()) darkens the bottom band for
the title, adds film grain and encodes to sRGB; quantize() dithers the result
to 8 bits with blue noise, so a dark gradient on the TV's 8-bit UI plane
shows no bands. Everything is seeded: the same seed gives the same bytes.

Standard library and numpy.
"""

import math
import struct
import zlib

import numpy as np

# The shell's title colour (style.rs, TEXT) and the shelf's size.
TEXT = (235, 238, 245)
WIDTH, HEIGHT = 1920, 640
TITLE_BAND = 200  # the bottom rows (at full size) that darken for the title

# BT.709 luminance weights, for linear and for encoded (luma) values.
LUMA = np.array((0.2126, 0.7152, 0.0722))


# Colour.

def srgb_encode(linear):
    """Linear light in [0, 1] (clipped) to the sRGB signal."""
    x = np.clip(linear, 0.0, 1.0)
    return np.where(x <= 0.0031308, 12.92 * x, 1.055 * np.power(x, 1.0 / 2.4) - 0.055)


def srgb_decode(signal):
    """The sRGB signal in [0, 1] (clipped) to linear light."""
    v = np.clip(signal, 0.0, 1.0)
    return np.where(v <= 0.04045, v / 12.92, np.power((v + 0.055) / 1.055, 2.4))


def rgb8(r, g, b):
    """An 8-bit sRGB colour as linear light."""
    return srgb_decode(np.array((r, g, b)) / 255.0)


def luminance(linear):
    """Relative luminance of (..., 3) linear sRGB."""
    return np.asarray(linear)[..., :3] @ LUMA


def normalized(colour):
    """A colour scaled so its brightest channel is 1."""
    colour = np.asarray(colour, dtype=np.float64)
    return colour / colour.max()


def with_luminance(colour, y):
    """A colour scaled to relative luminance y."""
    colour = np.asarray(colour, dtype=np.float64)
    return colour * (y / luminance(colour))


def blackbody(kelvin):
    """The colour of a black body at `kelvin` (1667 to 25000 K) as linear sRGB,
    brightest channel 1. Kim et al.'s cubic fit to the Planckian locus, then
    CIE xyY to BT.709; colours below about 1900 K fall outside sRGB and clip."""
    t = float(kelvin)
    if not 1667.0 <= t <= 25000.0:
        raise ValueError(f'{kelvin} K is outside 1667-25000 K')
    if t <= 4000.0:
        x = -0.2661239e9 / t ** 3 - 0.2343589e6 / t ** 2 + 0.8776956e3 / t + 0.179910
    else:
        x = -3.0258469e9 / t ** 3 + 2.1070379e6 / t ** 2 + 0.2226347e3 / t + 0.240390
    if t <= 2222.0:
        y = -1.1063814 * x ** 3 - 1.34811020 * x ** 2 + 2.18555832 * x - 0.20219683
    elif t <= 4000.0:
        y = -0.9549476 * x ** 3 - 1.37418593 * x ** 2 + 2.09137015 * x - 0.16748867
    else:
        y = 3.0817580 * x ** 3 - 5.87338670 * x ** 2 + 3.75112997 * x - 0.37001483
    xyz = np.array((x / y, 1.0, (1.0 - x - y) / y))
    to_rgb = np.array(((3.2404542, -1.5371385, -0.4985314),
                       (-0.9692660, 1.8760108, 0.0415560),
                       (0.0556434, -0.2040259, 1.0572252)))
    return normalized(np.maximum(to_rgb @ xyz, 0.0))


# Fields.

def grid(width, height):
    """Pixel centres in picture heights: x as (1, W), y as (H, 1)."""
    x = ((np.arange(width) + 0.5) / height)[None, :]
    y = ((np.arange(height) + 0.5) / height)[:, None]
    return x, y


def smoothstep(edge0, edge1, x):
    t = np.clip((x - edge0) / (edge1 - edge0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def blur(image, sigma):
    """Gaussian blur of an (H, W) or (H, W, C) array; sigma in pixels.

    Done in the frequency domain with the edges padded by reflection, so the
    picture does not wrap around.
    """
    if sigma <= 0.0:
        return np.array(image, dtype=np.float64)
    image = np.asarray(image, dtype=np.float64)
    pad = min(int(math.ceil(3.0 * sigma)), max(image.shape[:2]) - 1)
    widths = [(pad, pad), (pad, pad)] + [(0, 0)] * (image.ndim - 2)
    padded = np.pad(image, widths, mode='reflect')
    h, w = padded.shape[:2]
    fy = np.fft.fftfreq(h)[:, None]
    fx = np.fft.rfftfreq(w)[None, :]
    transfer = np.exp(-2.0 * (math.pi * sigma) ** 2 * (fx ** 2 + fy ** 2))
    if image.ndim == 3:
        transfer = transfer[:, :, None]
    out = np.fft.irfft2(np.fft.rfft2(padded, axes=(0, 1)) * transfer, s=(h, w), axes=(0, 1))
    return out[pad:pad + image.shape[0], pad:pad + image.shape[1]]


class Waves:
    """Smooth noise as a sum of sines in random directions, about `scale`
    picture heights from crest to crest, with unit standard deviation.

    It is a function of position rather than a grid of samples, so it can be
    warped (called at moved coordinates) and drawn at any size.
    """

    def __init__(self, rng, scale, count=10, spread=0.35):
        angle = rng.uniform(0.0, 2.0 * math.pi, count)
        k = 2.0 * math.pi / scale * rng.uniform(1.0 - spread, 1.0 + spread, count)
        self.kx, self.ky = k * np.cos(angle), k * np.sin(angle)
        self.phase = rng.uniform(0.0, 2.0 * math.pi, count)
        self.norm = math.sqrt(2.0 / count)

    def __call__(self, x, y):
        total = 0.0
        for kx, ky, p in zip(self.kx, self.ky, self.phase):
            total = total + np.sin(kx * x + ky * y + p)
        return self.norm * total


class Fbm:
    """Waves over octaves, each half the size and `gain` the strength of the last."""

    def __init__(self, rng, scale, octaves=4, gain=0.5, count=8):
        self.layers = [(gain ** k, Waves(rng, scale / 2 ** k, count)) for k in range(octaves)]
        self.norm = 1.0 / math.sqrt(sum(a * a for a, _ in self.layers))

    def __call__(self, x, y):
        return self.norm * sum(a * w(x, y) for a, w in self.layers)


def noise(rng, width, height, scale):
    """Smooth Gaussian noise with features about `scale` picture heights
    across: white noise blurred, with unit standard deviation."""
    field = blur(rng.standard_normal((height, width)), max(scale * height / 2.0, 0.3))
    return field / max(field.std(), 1e-12)


def fbm(rng, width, height, scale, octaves=4, gain=0.5):
    """Noise summed over octaves, each half the size of the one before."""
    total = np.zeros((height, width))
    amplitude = 1.0
    for _ in range(octaves):
        total += amplitude * noise(rng, width, height, scale)
        scale /= 2.0
        amplitude *= gain
    return total / math.sqrt(sum(gain ** (2 * k) for k in range(octaves)))


def segment(px, py, ax, ay, bx, by):
    """Distance from points (px, py) to the segment a-b, and how far along
    it (0 to 1) the nearest point lies."""
    dx, dy = bx - ax, by - ay
    t = np.clip(((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy), 0.0, 1.0)
    return np.hypot(px - ax - t * dx, py - ay - t * dy), t


def splat(image, points, peak, colour, sigma):
    """Adds Gaussian points of light to an (H, W, 3) image.

    points is (N, 2) in pixels (pixel i spans [i, i + 1)); peak, colour
    ((N, 3) or (3,)) and sigma (pixels) are per point or shared.
    """
    points = np.asarray(points, dtype=np.float64).reshape(-1, 2)
    n = len(points)
    peak = np.broadcast_to(np.asarray(peak, dtype=np.float64), (n,))
    sigma = np.broadcast_to(np.asarray(sigma, dtype=np.float64), (n,))
    colour = np.broadcast_to(np.asarray(colour, dtype=np.float64), (n, 3))
    h, w = image.shape[:2]
    for (px, py), p, s, c in zip(points, peak, sigma, colour):
        r = int(math.ceil(4.0 * s)) + 1
        x0, x1 = max(int(px) - r, 0), min(int(px) + r + 1, w)
        y0, y1 = max(int(py) - r, 0), min(int(py) + r + 1, h)
        if x0 >= x1 or y0 >= y1:
            continue
        xs = np.arange(x0, x1) + 0.5 - px
        ys = np.arange(y0, y1) + 0.5 - py
        g = np.exp(-(ys[:, None] ** 2 + xs[None, :] ** 2) / (2.0 * s * s))
        image[y0:y1, x0:x1] += p * g[:, :, None] * c


def downsample(image, factor=2):
    """Box filter by an integer factor (in linear light, for colour)."""
    h, w = image.shape[0] // factor, image.shape[1] // factor
    trimmed = image[:h * factor, :w * factor]
    return trimmed.reshape(h, factor, w, factor, *image.shape[2:]).mean(axis=(1, 3))


# Finishing.

def shoulder(linear, knee=0.35, ceiling=0.8):
    """Rolls values above `knee` smoothly toward `ceiling`, per channel, so
    no highlight reaches pure white; below the knee nothing changes."""
    over = np.maximum(linear - knee, 0.0)
    span = ceiling - knee
    return np.where(linear > knee, knee + span * np.tanh(over / span), linear)


def title_scrim(height, width=None, band=TITLE_BAND / HEIGHT, floor=0.10):
    """The factor (on the encoded signal) that darkens the bottom band toward
    black for the shell's title: 1 above the band, `floor` at the bottom
    edge, and a little darker toward the left, where the title sits.

    band is a fraction of the height. Returns (H, 1) or, with width, (H, W).
    """
    y = (np.arange(height) + 0.5) / height
    t = smoothstep(1.0 - band, 1.0, y)[:, None]
    fall = 1.0 - (1.0 - floor) * t ** 0.8
    if width is None:
        return fall
    x = ((np.arange(width) + 0.5) / width)[None, :]
    return fall * (1.0 - 0.35 * t * (1.0 - smoothstep(0.0, 0.6, x)))


def grain(rng, linear, amount, size=0.7):
    """Film grain: each pixel's light scaled by 1 + amount x noise, so black
    stays black and the grain follows the light, the same for all channels."""
    h, w = linear.shape[:2]
    field = blur(rng.standard_normal((h, w)), size)
    field /= max(field.std(), 1e-12)
    return linear * np.maximum(1.0 + amount * field, 0.0)[:, :, None]


def finish(rng, linear, grain_amount=0.0, scrim=True):
    """Linear art to the encoded signal: grain, highlights rolled off (after
    the grain, so it cannot push one past the ceiling), and the bottom band
    darkened for the title. Returns (H, W, 3) in [0, 1]."""
    out = np.maximum(linear, 0.0)
    if grain_amount:
        out = grain(rng, out, grain_amount)
    encoded = srgb_encode(shoulder(out))
    if scrim:
        encoded = encoded * title_scrim(encoded.shape[0], encoded.shape[1])[:, :, None]
    return encoded


# Dither.

_BLUE_NOISE = {}


def blue_noise(size=64, seed=1):
    """A (size, size) blue-noise threshold map in (0, 1), tiling seamlessly.

    Ulichney's void-and-cluster method on a torus. The energy sums a Gaussian
    (sigma 1.5) scaled to integers, so ties between equal voids break the
    same way on every machine.
    """
    key = (size, seed)
    if key in _BLUE_NOISE:
        return _BLUE_NOISE[key]
    n = size * size
    d = np.minimum(np.arange(size), size - np.arange(size))
    kernel = np.round(np.exp(-(d[:, None] ** 2 + d[None, :] ** 2) / (2.0 * 1.5 ** 2)) * (1 << 20)).astype(np.int64)
    shifted = [None] * n

    def bump(i):
        if shifted[i] is None:
            shifted[i] = np.roll(kernel, (i // size, i % size), axis=(0, 1)).ravel()
        return shifted[i]

    rng = np.random.default_rng(seed)
    pattern = np.zeros(n, dtype=bool)
    pattern[rng.choice(n, n // 10, replace=False)] = True
    energy = np.zeros(n, dtype=np.int64)
    for i in np.flatnonzero(pattern):
        energy += bump(i)
    big = np.iinfo(np.int64).max

    def tightest(p, e):
        return int(np.argmax(np.where(p, e, -big)))

    def emptiest(p, e):
        return int(np.argmin(np.where(p, big, e)))

    # Move points from the tightest cluster to the largest void until stable.
    while True:
        c = tightest(pattern, energy)
        pattern[c] = False
        energy -= bump(c)
        v = emptiest(pattern, energy)
        pattern[v] = True
        energy += bump(v)
        if v == c:
            break

    rank = np.zeros(n, dtype=np.int64)
    ones = int(pattern.sum())
    p, e = pattern.copy(), energy.copy()
    for r in range(ones - 1, -1, -1):
        c = tightest(p, e)
        p[c] = False
        e -= bump(c)
        rank[c] = r
    p, e = pattern.copy(), energy.copy()
    for r in range(ones, n):
        v = emptiest(p, e)
        p[v] = True
        e += bump(v)
        rank[v] = r
    thresholds = ((rank + 0.5) / n).reshape(size, size)
    _BLUE_NOISE[key] = thresholds
    return thresholds


def thresholds_for(height, width, offset=(0, 0)):
    """The blue-noise map tiled over (height, width), shifted by offset."""
    tile = blue_noise()
    s = tile.shape[0]
    rows = (np.arange(height) + offset[0]) % s
    cols = (np.arange(width) + offset[1]) % s
    return tile[rows[:, None], cols[None, :]]


def quantize(encoded, dither=True, offset=(0, 0)):
    """An encoded image in [0, 1] to 8 bits.

    With dither, each pixel adds the same blue-noise threshold to all its
    channels before rounding down, so a smooth gradient turns into a fine,
    even mix of neighbouring codes rather than bands, without coloured
    speckle; the mean stays exact and black stays black. Alpha, if there is
    a fourth channel, gets a threshold of its own.
    """
    encoded = np.clip(np.asarray(encoded, dtype=np.float64), 0.0, 1.0)
    if not dither:
        return np.floor(encoded * 255.0 + 0.5).astype(np.uint8)
    h, w, c = encoded.shape
    t = thresholds_for(h, w, offset)[:, :, None]
    codes = encoded * 255.0
    out = np.floor(codes[:, :, :3] + t)
    if c == 4:
        alpha_t = thresholds_for(h, w, (offset[0] + 23, offset[1] + 41))[:, :, None]
        out = np.concatenate([out, np.floor(codes[:, :, 3:] + alpha_t)], axis=2)
    return np.clip(out, 0, 255).astype(np.uint8)


# PNG.

def _chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xFFFFFFFF)


def png_bytes(pixels):
    """An 8-bit RGB or RGBA PNG of an (H, W, 3 or 4) uint8 array.

    It carries an sRGB chunk and nothing else: no time, no text, so the same
    pixels give the same bytes. Each row takes whichever of the five PNG
    filters leaves the smallest sum of magnitudes, the usual heuristic.
    """
    pixels = np.ascontiguousarray(pixels, dtype=np.uint8)
    h, w, c = pixels.shape
    if c not in (3, 4):
        raise ValueError(f'{c} channels: PNG here is RGB or RGBA')
    raw = pixels.reshape(h, w * c).astype(np.int16)
    up = np.vstack([np.zeros((1, w * c), dtype=np.int16), raw[:-1]])
    left = np.hstack([np.zeros((h, c), dtype=np.int16), raw[:, :-c]])
    upleft = np.hstack([np.zeros((h, c), dtype=np.int16), up[:, :-c]])
    estimate = left + up - upleft
    pa, pb, pc = np.abs(estimate - left), np.abs(estimate - up), np.abs(estimate - upleft)
    paeth = np.where((pa <= pb) & (pa <= pc), left, np.where(pb <= pc, up, upleft))
    candidates = np.stack([raw, raw - left, raw - up, raw - (left + up) // 2, raw - paeth]) & 0xFF
    signed = np.where(candidates > 127, 256 - candidates, candidates)
    choice = np.argmin(signed.sum(axis=2), axis=0)
    rows = candidates[choice, np.arange(h)].astype(np.uint8)
    body = np.hstack([choice.astype(np.uint8)[:, None], rows]).tobytes()
    header = struct.pack('>IIBBBBB', w, h, 8, 6 if c == 4 else 2, 0, 0, 0)
    return (b'\x89PNG\r\n\x1a\n' + _chunk(b'IHDR', header) + _chunk(b'sRGB', b'\x00')
            + _chunk(b'IDAT', zlib.compress(body, 9)) + _chunk(b'IEND', b''))


def write_png(path, pixels):
    with open(path, 'wb') as f:
        f.write(png_bytes(pixels))


def read_png(data):
    """Pixels of an 8-bit, non-interlaced RGB or RGBA PNG, as (H, W, C) uint8.

    Enough for the files png_bytes() writes and for checking them; not a
    general PNG reader (no palettes, grey, 16 bits or interlacing).
    """
    if data[:8] != b'\x89PNG\r\n\x1a\n':
        raise ValueError('not a PNG')
    pos, idat, header = 8, [], None
    while pos < len(data):
        (length,) = struct.unpack('>I', data[pos:pos + 4])
        kind, body = data[pos + 4:pos + 8], data[pos + 8:pos + 8 + length]
        if zlib.crc32(kind + body) & 0xFFFFFFFF != struct.unpack('>I', data[pos + 8 + length:pos + 12 + length])[0]:
            raise ValueError(f'bad CRC in {kind!r}')
        if kind == b'IHDR':
            header = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            idat.append(body)
        pos += 12 + length
    w, h, depth, colour_type, _, _, interlace = header
    if depth != 8 or colour_type not in (2, 6) or interlace:
        raise ValueError(f'unsupported PNG: depth {depth}, colour type {colour_type}, interlace {interlace}')
    c = 4 if colour_type == 6 else 3
    stream = np.frombuffer(zlib.decompress(b''.join(idat)), dtype=np.uint8).reshape(h, 1 + w * c)
    kinds = stream[:, 0].astype(np.int32)
    if kinds.max(initial=0) > 4:
        raise ValueError(f'unknown PNG filter {kinds.max()}')
    filtered = stream[:, 1:].reshape(h, w, c).astype(np.int32)
    # A pixel depends on the ones to its left, above and above-left, so all
    # pixels on one anti-diagonal can be undone at once. out is padded with
    # a row and a column of zeros, which the filters take as outside.
    out = np.zeros((h + 1, w + 1, c), dtype=np.int32)
    for d in range(h + w - 1):
        r = np.arange(max(0, d - w + 1), min(h, d + 1))
        i = d - r
        a, b, ul = out[r + 1, i], out[r, i + 1], out[r, i]
        p = a + b - ul
        pa, pb, pc = np.abs(p - a), np.abs(p - b), np.abs(p - ul)
        paeth = np.where((pa <= pb) & (pa <= pc), a, np.where(pb <= pc, b, ul))
        k = kinds[r][:, None]
        guess = np.select([k == 1, k == 2, k == 3, k == 4], [a, b, (a + b) // 2, paeth], 0)
        out[r + 1, i + 1] = (filtered[r, i] + guess) & 0xFF
    return out[1:, 1:].astype(np.uint8)


# Checks.

def light_stats(pixels, band=TITLE_BAND / HEIGHT):
    """How bright an 8-bit sRGB image is, as the heroes' rules measure it.

    mean and p99 are of luma (BT.709 weights on the encoded signal, 0 to 1);
    mean_linear is of relative luminance in linear light; band_mean is the
    luma mean of the bottom band, band_end that of its last tenth, and
    band_p99_linear the 99th percentile of linear luminance there, which sets
    the contrast of the title (contrast) against 99% of the band's pixels.
    white counts pixels at (255, 255, 255).
    """
    rgb = np.asarray(pixels)[:, :, :3]
    encoded = rgb / 255.0
    y_luma = encoded @ LUMA
    y_linear = srgb_decode(encoded) @ LUMA
    h = rgb.shape[0]
    top = h - int(round(band * h))
    tail = h - max(1, int(round(band * h / 10)))
    text = float(rgb8(*TEXT) @ LUMA)
    p99_band = float(np.percentile(y_linear[top:], 99))
    return {
        'mean': float(y_luma.mean()),
        'median': float(np.median(y_luma)),
        'p99': float(np.percentile(y_luma, 99)),
        'max': float(y_luma.max()),
        'mean_linear': float(y_linear.mean()),
        'band_mean': float(y_luma[top:].mean()),
        'band_end': float(y_luma[tail:].mean()),
        'band_p99_linear': p99_band,
        'contrast': (text + 0.05) / (p99_band + 0.05),
        'white': int(np.all(rgb == 255, axis=2).sum()),
    }


def longest_band(signal, codes, low=1.5, high=12.0, spread=0.5, ends=8):
    """The longest flat run in the dark gradients of an image: a run of one
    8-bit code, down a column or along a row, from whose one end to the
    other the signal it was made from rises or falls by at least `spread`
    codes, so a gradient was flattened into a band. Each end is the mean of
    the run's own first or last `ends` pixels (fewer in a short run), so
    neither grain nor an edge beyond the run makes a flat patch a gradient.
    Only pixels whose signal lies between `low` and `high` codes count: the
    dark end, where 8 bits band first, but above black, where a run of
    zeros is just black.

    signal is the float image in codes (0 to 255) and codes the 8-bit image
    made from it, both 2-D (one channel). Returns the length in pixels.
    """
    return longest_band_at(signal, codes, low, high, spread, ends)[0]


def longest_band_at(signal, codes, low=1.5, high=12.0, spread=0.5, ends=8):
    """longest_band(), and where that run is: (length, row, column, axis),
    axis 0 for a run down a column and 1 along a row; (0, None, None, None)
    if there is none."""
    signal = np.asarray(signal, dtype=np.float64)
    codes = np.asarray(codes).astype(np.int64)
    dark = (signal >= low) & (signal <= high)
    best = (0, None, None, None)
    for axis, (s, q, m) in enumerate(((signal.T, codes.T, dark.T), (signal, codes, dark))):
        length = s.shape[1]
        s, q, m = s.reshape(-1), q.reshape(-1), m.reshape(-1)
        # A run ends where the code changes, at the end of a line, or where
        # the signal leaves the dark range (those pixels are runs of their own).
        q = np.where(m, q, -1 - np.arange(q.size))
        starts = np.ones(q.size, dtype=bool)
        starts[1:] = q[1:] != q[:-1]
        starts[::length] = True
        index = np.flatnonzero(starts)
        run = np.diff(np.append(index, q.size))
        k = np.clip(run // 4, 1, ends)
        total = np.concatenate([[0.0], np.cumsum(s)])
        head = (total[index + k] - total[index]) / k
        tail = (total[index + run] - total[index + run - k]) / k
        banded = np.flatnonzero((np.abs(tail - head) >= spread) & m[index])
        if banded.size:
            i = banded[np.argmax(run[banded])]
            if run[i] > best[0]:
                line, start = divmod(int(index[i]), length)
                best = (int(run[i]), start, line, 0) if axis == 0 else (int(run[i]), line, start, 1)
    return best


def banding(encoded, pixels):
    """The longest band (longest_band()) in any colour channel of an image,
    rounded to 8 bits without dither and as `pixels`, its dithered 8 bits."""
    plain = quantize(encoded, dither=False)
    codes = np.asarray(encoded)[:, :, :3] * 255.0
    before = max(longest_band(codes[:, :, c], plain[:, :, c]) for c in range(3))
    after = max(longest_band(codes[:, :, c], pixels[:, :, c]) for c in range(3))
    return before, after
