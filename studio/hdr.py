"""HDR10 colour maths for the studio pipeline.

The PQ curve (SMPTE ST 2084), BT.2020 Y'CbCr and narrow-range code values,
RGB primaries conversions, and the HDR10 static metadata: the mastering
display (SMPTE ST 2086) and the content light levels MaxCLL and MaxFALL
(CTA-861.3).

Standard library only, so its tests run in the build container, which has no
numpy. Functions documented as "arithmetic only" also take numpy arrays.
"""

import math
from collections import namedtuple

# SMPTE ST 2084 constants, written as the standard defines them.
M1 = 2610 / 16384
M2 = 2523 / 4096 * 128
C1 = 3424 / 4096
C2 = 2413 / 4096 * 32
C3 = 2392 / 4096 * 32
PQ_PEAK = 10000.0  # cd/m² at signal 1.0

# BT.2020 non-constant-luminance luma coefficients.
KR = 0.2627
KB = 0.0593
KG = 1.0 - KR - KB


def pq_from_linear(y):
    """PQ inverse EOTF of luminance normalized to 1.0 = 10000 cd/m².

    Arithmetic only, so y may be a numpy array; values must already lie in
    [0, 1] (clip first: a negative base raised to M1 is not a real number).
    """
    p = y ** M1
    return ((C1 + C2 * p) / (1.0 + C3 * p)) ** M2


def pq_encode(nits):
    """Absolute luminance in cd/m² to a PQ signal in [0, 1], clamped."""
    return pq_from_linear(min(max(nits / PQ_PEAK, 0.0), 1.0))


def pq_decode(signal):
    """PQ EOTF: a signal in [0, 1] (clamped) to absolute luminance in cd/m²."""
    e = min(max(signal, 0.0), 1.0) ** (1.0 / M2)
    return PQ_PEAK * (max(e - C1, 0.0) / (C2 - C3 * e)) ** (1.0 / M1)


NarrowRange = namedtuple('NarrowRange', 'black luma_scale chroma_mid chroma_scale low luma_high chroma_high')


def narrow_range(bits=10):
    """Code values of narrow ("limited", "tv") range video at a bit depth."""
    d = 1 << (bits - 8)
    return NarrowRange(16 * d, 219 * d, 128 * d, 224 * d, 16 * d, 235 * d, 240 * d)


def luma_code(signal, bits=10):
    """A Y' signal in [0, 1] to its narrow-range code value."""
    r = narrow_range(bits)
    return min(max(math.floor(r.black + r.luma_scale * signal + 0.5), r.low), r.luma_high)


def chroma_code(signal, bits=10):
    """A Cb or Cr signal in [-0.5, 0.5] to its narrow-range code value."""
    r = narrow_range(bits)
    return min(max(math.floor(r.chroma_mid + r.chroma_scale * signal + 0.5), r.low), r.chroma_high)


def ycbcr_from_rgb(r, g, b):
    """BT.2020 non-constant-luminance R'G'B' to Y'CbCr. Arithmetic only."""
    y = KR * r + KG * g + KB * b
    return y, (b - y) / (2.0 * (1.0 - KB)), (r - y) / (2.0 * (1.0 - KR))


# RGB primaries and white point as CIE 1931 xy chromaticities.
Primaries = namedtuple('Primaries', 'red green blue white')
D65 = (0.3127, 0.3290)
BT709 = Primaries((0.640, 0.330), (0.300, 0.600), (0.150, 0.060), D65)
P3_D65 = Primaries((0.680, 0.320), (0.265, 0.690), (0.150, 0.060), D65)
BT2020 = Primaries((0.708, 0.292), (0.170, 0.797), (0.131, 0.046), D65)


def _xyz(xy):
    x, y = xy
    if y <= 0.0:
        raise ValueError(f'chromaticity {xy} has no luminance')
    return (x / y, 1.0, (1.0 - x - y) / y)


def _determinant(m):
    return (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))


def inverse(m):
    """Inverse of a 3x3 matrix given as rows."""
    det = _determinant(m)
    if abs(det) < 1e-12:
        raise ValueError('matrix is singular')
    cofactors = [[(m[(r + 1) % 3][(c + 1) % 3] * m[(r + 2) % 3][(c + 2) % 3]
                   - m[(r + 1) % 3][(c + 2) % 3] * m[(r + 2) % 3][(c + 1) % 3])
                  for c in range(3)] for r in range(3)]
    return [[cofactors[c][r] / det for c in range(3)] for r in range(3)]


def multiply(a, b):
    """Product of two 3x3 matrices given as rows."""
    return [[sum(a[r][k] * b[k][c] for k in range(3)) for c in range(3)] for r in range(3)]


def apply(m, rgb):
    """A 3x3 matrix applied to one RGB triple."""
    return tuple(sum(m[r][k] * rgb[k] for k in range(3)) for r in range(3))


def rgb_to_xyz(primaries):
    """The normalized primary matrix: linear RGB to CIE XYZ, white at Y = 1."""
    columns = [_xyz(p) for p in (primaries.red, primaries.green, primaries.blue)]
    p = [[columns[c][r] for c in range(3)] for r in range(3)]
    try:
        scale = apply(inverse(p), _xyz(primaries.white))
    except ValueError:
        raise ValueError(f'primaries {primaries} do not span a gamut') from None
    return [[p[r][c] * scale[c] for c in range(3)] for r in range(3)]


def conversion_matrix(src, dst):
    """Linear RGB in src primaries to linear RGB in dst primaries.

    Both must share a white point: this does no chromatic adaptation.
    """
    if src.white != dst.white:
        raise ValueError(f'white points differ: {src.white} and {dst.white}')
    return multiply(inverse(rgb_to_xyz(dst)), rgb_to_xyz(src))


def master_display(primaries, peak_nits, black_nits):
    """The mastering display (SMPTE ST 2086) in x265's --master-display form.

    Chromaticities are in units of 0.00002 and luminance in 0.0001 cd/m², in
    the order green, blue, red, white point, then maximum and minimum. A
    black of 0 is allowed, as ST 2086 and HEVC allow it.
    """
    if not 0.0 <= black_nits < peak_nits <= PQ_PEAK:
        raise ValueError(f'no display has peak {peak_nits} and black {black_nits} cd/m²')

    def xy(c):
        return f'({round(c[0] * 50000)},{round(c[1] * 50000)})'

    return (f'G{xy(primaries.green)}B{xy(primaries.blue)}R{xy(primaries.red)}'
            f'WP{xy(primaries.white)}L({round(peak_nits * 10000)},{round(black_nits * 10000)})')


def content_light_level(frames):
    """MaxCLL and MaxFALL (CTA-861.3), in whole cd/m², rounded up.

    frames yields (max, average) per frame, where both are taken over the
    frame's pixels of max(R, G, B) in cd/m², in the encoded BT.2020 signal.
    Differences under a millionth are float noise from summing the frame, so
    they neither fail the checks nor round a whole number up.
    """
    noise = 1e-6
    max_cll = max_fall = None
    for peak, average in frames:
        if not (math.isfinite(peak) and math.isfinite(average)
                and 0.0 <= average <= peak * (1.0 + noise) and peak <= PQ_PEAK * (1.0 + noise)):
            raise ValueError(f'impossible frame light levels: max {peak}, average {average}')
        max_cll = peak if max_cll is None else max(max_cll, peak)
        max_fall = average if max_fall is None else max(max_fall, average)
    if max_cll is None:
        raise ValueError('no frames')
    max_cll = math.ceil(max_cll * (1.0 - noise))
    return max_cll, min(math.ceil(max_fall * (1.0 - noise)), max_cll)


# Fixed by HDR10; x265_params refuses to let extra options change them.
_HDR10_FIELDS = ('colorprim', 'transfer', 'colormatrix', 'range', 'chromaloc',
                 'hdr10', 'hdr10-opt', 'repeat-headers', 'master-display', 'max-cll')


def x265_params(max_cll, max_fall, master, extra=None):
    """x265 options (ffmpeg's -x265-params form) for an HDR10 stream.

    BT.2020 / PQ / narrow range, and HDR10 SEI (mastering display, content
    light level) repeated with the headers at every keyframe, so a player can
    start, or loop, at any of them. Chroma is flagged left-sited (type 0,
    HEVC's default), which is how studio/encode.py downsamples it; a
    different downsampler must change this flag with it.
    """
    fields = [('colorprim', 'bt2020'), ('transfer', 'smpte2084'), ('colormatrix', 'bt2020nc'),
              ('range', 'limited'), ('chromaloc', 0), ('hdr10', 1), ('hdr10-opt', 1),
              ('repeat-headers', 1), ('master-display', master), ('max-cll', f'{max_cll},{max_fall}')]
    for key, value in (extra or {}).items():
        if key in _HDR10_FIELDS:
            raise ValueError(f'{key} is fixed by HDR10')
        fields.append((key, value))
    for key, value in fields:
        if ':' in f'{key}{value}' or '=' in f'{key}{value}':
            raise ValueError(f'separator in x265 option {key}={value}')
    return ':'.join(f'{key}={value}' for key, value in fields)
