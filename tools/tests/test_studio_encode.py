"""Tests for studio/encode.py.

They need numpy, OpenEXR and ffmpeg, so they skip in the build container and
run in the studio container:

    tools/studio.sh python3 -I -m unittest discover -s tools/tests -p 'test_studio*.py'
"""

import contextlib
import io
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'studio'))

try:
    import numpy as np
    import OpenEXR  # noqa: F401
except ImportError:
    np = None
else:
    import encode  # noqa: E402
    import exr  # noqa: E402
    import hdr  # noqa: E402
    import render  # noqa: E402

NEEDS_STUDIO = unittest.skipUnless(np, 'needs numpy and OpenEXR: run it with tools/studio.sh')
NEEDS_FFMPEG = unittest.skipUnless(np and shutil.which('ffmpeg'), 'needs ffmpeg: run it with tools/studio.sh')


def flat(nits, height=4, width=6):
    return np.full((height, width, 3), nits, dtype=np.float32)


# A saturated P3 orange-red, far from grey, so that swapped chroma planes or a
# matrix applied in the wrong domain change the codes.
COLOUR = (300.0, 40.0, 8.0)


def reference_codes(p3_nits):
    """The colour's (Y, Cb, Cr) codes and BT.2020 light, from hdr.py's scalar maths."""
    rgb2020 = [max(c, 0.0) for c in hdr.apply(hdr.conversion_matrix(hdr.P3_D65, hdr.BT2020), p3_nits)]
    y, cb, cr = hdr.ycbcr_from_rgb(*(hdr.pq_encode(c) for c in rgb2020))
    return (hdr.luma_code(y), hdr.chroma_code(cb), hdr.chroma_code(cr)), rgb2020


def quietly(func, *args):
    with contextlib.redirect_stderr(io.StringIO()):
        return func(*args)


@NEEDS_STUDIO
class ConversionTest(unittest.TestCase):
    def test_grey_lands_on_the_pq_code_values(self):
        for nits, code in ((0.0, 64), (100.0, 509), (1000.0, 723), (10000.0, 940)):
            y, cb, cr = encode.yuv420_codes(flat(nits))
            self.assertTrue((y == code).all(), (nits, np.unique(y)))
            self.assertTrue((cb == 512).all() and (cr == 512).all())

    def test_a_colour_matches_the_scalar_maths(self):
        rgb = np.empty((4, 6, 3), dtype=np.float32)
        rgb[...] = COLOUR
        planes = encode.yuv420_codes(encode.to_bt2020(rgb, hdr.P3_D65))
        want, _ = reference_codes(COLOUR)
        for plane, code, name in zip(planes, want, 'Y Cb Cr'.split()):
            self.assertTrue((plane == code).all(), (name, np.unique(plane), code))
        self.assertNotEqual(want[1], want[2])

    def test_codes_are_ten_bit_planes(self):
        y, cb, cr = encode.yuv420_codes(flat(50.0))
        self.assertEqual(y.dtype, np.uint16)
        self.assertEqual((y.shape, cb.shape, cr.shape), ((4, 6), (2, 3), (2, 3)))

    def test_light_above_pq_peak_is_clipped(self):
        y, _, _ = encode.yuv420_codes(flat(50000.0))
        self.assertTrue((y == 940).all())

    def test_p3_is_converted_to_bt2020(self):
        rgb = np.zeros((2, 2, 3), dtype=np.float32)
        rgb[..., 1] = 100.0  # pure P3 green
        out = encode.to_bt2020(rgb, hdr.P3_D65)
        column = [row[1] * 100.0 for row in hdr.conversion_matrix(hdr.P3_D65, hdr.BT2020)]
        self.assertTrue(np.allclose(out[0, 0], column, rtol=1e-5))

    def test_bt2020_input_is_left_alone(self):
        rgb = np.random.default_rng(1).uniform(0.0, 500.0, (2, 2, 3)).astype(np.float32)
        self.assertTrue(np.allclose(encode.to_bt2020(rgb, hdr.BT2020), rgb))

    def test_negative_light_from_gamut_conversion_is_clipped(self):
        rgb = np.zeros((2, 2, 3), dtype=np.float32)
        rgb[..., 0] = 100.0  # P3 red has a slightly negative BT.2020 blue
        self.assertGreaterEqual(encode.to_bt2020(rgb, hdr.P3_D65).min(), 0.0)

    def test_bands_give_the_same_frame_as_one_pass(self):
        # Converting a band of rows at a time bounds memory at 4K; it must not
        # change a single code value or the light levels.
        rgb = np.random.default_rng(2).uniform(0.0, 900.0, (12, 8, 3)).astype(np.float32)
        whole = encode.yuv420_codes(encode.to_bt2020(rgb, hdr.P3_D65))
        for band in (2, 4, 6, 100):
            banded = encode.grade(rgb, hdr.P3_D65, band=band)
            for a, b in zip(banded, whole):
                self.assertTrue(np.array_equal(a, b), band)
            peak, average = encode.measure(rgb, hdr.P3_D65, band=band)
            want = encode.light_levels(encode.to_bt2020(rgb, hdr.P3_D65))
            self.assertAlmostEqual(peak, want[0], places=3)
            self.assertAlmostEqual(average, want[1], places=3)

    def test_bands_must_keep_chroma_row_pairs_together(self):
        with self.assertRaises(ValueError):
            encode.grade(np.zeros((4, 4, 3), np.float32), hdr.BT2020, band=3)

    def test_light_levels_are_max_and_mean_of_brightest_channel(self):
        rgb = np.zeros((4, 5, 3), dtype=np.float32)
        rgb[1, 2] = (10.0, 750.0, 20.0)
        rgb[3, 4] = (40.0, 0.0, 0.0)
        peak, average = encode.light_levels(rgb)
        self.assertAlmostEqual(peak, 750.0)
        self.assertAlmostEqual(average, 790.0 / 20)


@NEEDS_STUDIO
class ChromaSitingTest(unittest.TestCase):
    # HEVC chroma location type 0 ("left"), as hdr.x265_params flags it: each
    # chroma sample sits on an even luma column, halfway between two rows.
    def test_even_column_impulse_stays_in_its_chroma_column(self):
        plane = np.zeros((2, 8), dtype=np.float32)
        plane[:, 2] = 1.0
        self.assertTrue(np.allclose(encode.downsample(plane), [[0.0, 0.5, 0.0, 0.0]]))

    def test_odd_column_impulse_splits_between_neighbours(self):
        plane = np.zeros((2, 8), dtype=np.float32)
        plane[:, 3] = 1.0
        self.assertTrue(np.allclose(encode.downsample(plane), [[0.0, 0.25, 0.25, 0.0]]))

    def test_rows_are_averaged_in_pairs(self):
        plane = np.zeros((4, 2), dtype=np.float32)
        plane[1] = 1.0
        self.assertTrue(np.allclose(encode.downsample(plane), [[0.5], [0.0]]))

    def test_edges_repeat_the_border_pixel(self):
        plane = np.ones((2, 6), dtype=np.float32)
        self.assertTrue(np.allclose(encode.downsample(plane), 1.0))


@NEEDS_STUDIO
class CommandTest(unittest.TestCase):
    def test_ffmpeg_command_tags_hdr10(self):
        cmd = encode.ffmpeg_command('out.mp4', 3840, 2160, 24, 'colorprim=bt2020', crf=18, preset='slow')
        joined = ' '.join(cmd)
        self.assertIn('-f rawvideo -pix_fmt yuv420p10le -s 3840x2160 -r 24 -i -', joined)
        self.assertIn('-c:v libx265', joined)
        self.assertIn('-x265-params colorprim=bt2020', joined)
        for option in ('-color_primaries bt2020', '-color_trc smpte2084', '-colorspace bt2020nc',
                       '-color_range tv', '-chroma_sample_location left', '-crf 18', '-preset slow'):
            self.assertIn(option, joined)
        self.assertEqual(cmd[-1], 'out.mp4')

    def test_x265_options_fit_the_resolution(self):
        uhd = encode.x265_extra(3840, 2160, 24)
        hd = encode.x265_extra(1920, 1080, 24)
        self.assertEqual((uhd['level-idc'], uhd['vbv-maxrate']), ('5.1', 40000))
        self.assertEqual((hd['level-idc'], hd['vbv-maxrate']), ('4.1', 20000))
        # Closed GOPs of two seconds, so a player can loop at any keyframe.
        self.assertEqual((hd['keyint'], hd['open-gop']), (48, 0))


def write_frames(directory, frames, width=64, height=36, colour=COLOUR, finished=True):
    """A render.py-style directory of flat-colour P3 frames."""
    os.makedirs(directory)
    rgb = np.empty((height, width, 3), dtype=np.float32)
    rgb[...] = colour
    for i in range(frames):
        exr.write(os.path.join(directory, render.PATTERN % i), rgb, hdr.P3_D65, 100.0)
    manifest = {'scene': 'flat', 'width': width, 'height': height, 'fps': 8, 'frames': frames,
                'pattern': render.PATTERN, 'primaries': 'P3_D65', 'white_nits': 100.0}
    if finished:
        manifest['render'] = {'wall_seconds': 0.0}
    with open(os.path.join(directory, 'manifest.json'), 'w') as f:
        json.dump(manifest, f)


@NEEDS_FFMPEG
class EndToEndTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def path(self, name):
        return os.path.join(self.tmp.name, name)

    def test_decoded_colour_and_light_levels_match_the_scalar_maths(self):
        write_frames(self.path('flat'), 3)
        quietly(encode.main, [self.path('flat'), '--workers', '2'])
        raw = subprocess.run(['ffmpeg', '-v', 'error', '-i', self.path('flat.mp4'), '-frames:v', '1',
                              '-f', 'rawvideo', '-pix_fmt', 'yuv420p10le', '-'],
                             check=True, capture_output=True).stdout
        codes = np.frombuffer(raw, dtype='<u2')
        y, cb, cr = codes[:64 * 36], codes[64 * 36:64 * 36 + 32 * 18], codes[64 * 36 + 32 * 18:]
        want, rgb2020 = reference_codes(COLOUR)
        for plane, code, name in zip((y, cb, cr), want, 'Y Cb Cr'.split()):
            self.assertLessEqual(np.abs(plane.astype(int) - code).max(), 1, (name, np.unique(plane), code))
        with open(self.path('flat.mp4.json')) as f:
            info = json.load(f)
        self.assertEqual((info['max_cll'], info['max_fall']), (math.ceil(max(rgb2020)),) * 2)

    def test_a_failed_encode_leaves_no_video(self):
        write_frames(self.path('flat'), 2)
        with self.assertRaises(SystemExit):
            quietly(encode.main, [self.path('flat'), '--workers', '1', '--preset', 'nonsense'])
        self.assertEqual(sorted(os.listdir(self.tmp.name)), ['flat'])

    def test_an_unfinished_render_is_refused(self):
        write_frames(self.path('flat'), 2, finished=False)
        with self.assertRaises(SystemExit):
            quietly(encode.main, [self.path('flat'), '--workers', '1'])
        self.assertFalse(os.path.exists(self.path('flat.mp4')))

    def test_a_tiny_loop_encodes_as_hdr10(self):
        with tempfile.TemporaryDirectory() as tmp:
            frames = os.path.join(tmp, 'frames')
            video = os.path.join(tmp, 'loop.mp4')
            quietly(render.main, ['--size', '128x72', '--seconds', '0.5', '--fps', '8', '--workers', '1',
                                  '--out', frames])
            quietly(encode.main, [frames, '--out', video, '--workers', '1'])
            with open(video + '.json') as f:
                info = json.load(f)
            probe = json.loads(subprocess.run(
                ['ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_streams', '-show_frames',
                 '-read_intervals', '%+#1', '-of', 'json', video],
                check=True, capture_output=True, text=True).stdout)
        stream = probe['streams'][0]
        self.assertEqual((stream['codec_name'], stream['profile'], stream['pix_fmt']),
                         ('hevc', 'Main 10', 'yuv420p10le'))
        self.assertEqual(stream['nb_frames'], '4')
        side = {s['side_data_type']: s for s in probe['frames'][0]['side_data_list']}
        light = side['Content light level metadata']
        self.assertEqual((light['max_content'], light['max_average']), (info['max_cll'], info['max_fall']))
        self.assertGreater(info['max_cll'], 0)
        self.assertEqual(info['frames'], 4)


if __name__ == '__main__':
    unittest.main()
