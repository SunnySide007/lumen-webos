"""Frames on disk: half-float RGB OpenEXR with its colour stated in the header.

Pixels are linear light relative to white: a value of 1.0 is white_nits cd/m².
The standard chromaticities and whiteLuminance attributes carry the primaries
and that scale, so other EXR tools read the frames the same way.
"""

import os

import numpy as np
import OpenEXR

import hdr


def write(path, rgb_nits, primaries, white_nits):
    """Writes an (H, W, 3) array of cd/m² as half floats, atomically."""
    header = {
        'compression': OpenEXR.ZIP_COMPRESSION,
        'type': OpenEXR.scanlineimage,
        'chromaticities': (*primaries.red, *primaries.green, *primaries.blue, *primaries.white),
        'whiteLuminance': float(white_nits),
    }
    pixels = (np.asarray(rgb_nits, dtype=np.float32) / white_nits).astype(np.float16)
    partial = f'{path}.partial'
    with OpenEXR.File(header, {'RGB': pixels}) as f:
        f.write(partial)
    os.replace(partial, path)


def read(path):
    """Returns (rgb in cd/m² as float32, primaries, white_nits)."""
    # Closing the file empties the header and frees the pixels it handed
    # out, so everything is copied before the block ends.
    with OpenEXR.File(path) as f:
        header = f.header()
        channels = f.channels()
        if 'RGB' in channels:
            rgb = channels['RGB'].pixels.astype(np.float32)
        else:
            rgb = np.stack([channels[c].pixels for c in 'RGB'], axis=-1).astype(np.float32)
        # EXR keeps chromaticities as 32-bit floats; rounding gives back the
        # published values, so primaries compare equal to those in hdr.py.
        c = [round(float(v), 6) for v in header['chromaticities']]
        white = float(header['whiteLuminance'])
    primaries = hdr.Primaries((c[0], c[1]), (c[2], c[3]), (c[4], c[5]), (c[6], c[7]))
    rgb *= white
    return rgb, primaries, white
