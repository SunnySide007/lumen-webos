"""Tests for studio/verify.py.

They need numpy, OpenEXR and ffmpeg, so they skip in the build container and
run in the studio container:

    tools/studio.sh python3 -I -m unittest discover -s tools/tests -p 'test_studio*.py'
"""

import contextlib
import io
import json
import os
import shutil
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
    import verify  # noqa: E402

NEEDS_STUDIO = unittest.skipUnless(np, 'needs numpy and OpenEXR: run it with tools/studio.sh')
NEEDS_FFMPEG = unittest.skipUnless(np and shutil.which('ffmpeg'), 'needs ffmpeg: run it with tools/studio.sh')


def quietly(func, *args):
    with contextlib.redirect_stderr(io.StringIO()):
        return func(*args)


@NEEDS_STUDIO
class SeamTest(unittest.TestCase):
    def test_measures_each_step_and_the_wrap_around(self):
        frames = [np.full((2, 2), v, dtype=np.uint16) for v in (100, 103, 101, 110)]
        frames[2][0, 1] = 650
        stats = verify.loop_measure(iter(frames))
        self.assertEqual(stats.steps, [3.0, (2 * 3 + 547) / 4, (9 * 3 + 540) / 4])
        self.assertEqual(stats.seam, 10.0)
        self.assertEqual(stats.count, 4)
        self.assertEqual(stats.peak, 650)

    def test_no_frames(self):
        self.assertEqual(verify.loop_measure(iter([])), (0.0, [], 0, 0))


@NEEDS_FFMPEG
class VerifyTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def path(self, name):
        return os.path.join(self.tmp.name, name)

    def test_a_studio_loop_passes_and_gets_previews(self):
        quietly(render.main, ['--size', '128x72', '--seconds', '1', '--fps', '8', '--workers', '1',
                              '--out', self.path('loop')])
        quietly(encode.main, [self.path('loop'), '--workers', '1'])
        lines, problems = verify.run(self.path('loop.mp4'), self.path('previews'), count=3)
        self.assertEqual(problems, [])
        self.assertEqual(sorted(os.listdir(self.path('previews'))), ['loop-0.png', 'loop-1.png', 'loop-2.png'])
        report = '\n'.join(lines)
        for word in ('Main 10', 'smpte2084', 'MaxCLL', 'loop'):
            self.assertIn(word, report)

    def test_a_clip_that_does_not_loop_fails(self):
        # Eight frames getting steadily brighter: the last is far from the first.
        frames = self.path('ramp')
        os.makedirs(frames)
        for i in range(8):
            exr.write(os.path.join(frames, render.PATTERN % i), np.full((72, 128, 3), 5.0 + 20.0 * i, np.float32),
                      hdr.BT2020, 100.0)
        manifest = {'scene': 'ramp', 'width': 128, 'height': 72, 'fps': 8, 'frames': 8,
                    'pattern': render.PATTERN, 'primaries': 'BT2020', 'white_nits': 100.0,
                    'render': {'wall_seconds': 0.0}}
        with open(os.path.join(frames, 'manifest.json'), 'w') as f:
            json.dump(manifest, f)
        quietly(encode.main, [frames, '--workers', '1'])
        _, problems = verify.run(self.path('ramp.mp4'), None)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn('the loop jumps', problems[0])

    def test_a_video_without_its_encode_record_is_refused(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            verify.main([self.path('nothing.mp4')])

    def test_a_record_missing_fields_says_which(self):
        with open(self.path('old.mp4'), 'wb') as f:
            f.write(b'')
        with open(self.path('old.mp4.json'), 'w') as f:
            json.dump({'width': 16, 'height': 16, 'fps': 8, 'frames': 2}, f)
        err = io.StringIO()
        with self.assertRaises(SystemExit) as stop, contextlib.redirect_stderr(err):
            verify.main([self.path('old.mp4'), '--previews', ''])
        self.assertIn('max_cll', str(stop.exception.code))

    def test_an_unreadable_video_fails_without_a_traceback(self):
        with open(self.path('junk.mp4'), 'wb') as f:
            f.write(b'not a video')
        with open(self.path('junk.mp4.json'), 'w') as f:
            json.dump({'width': 16, 'height': 16, 'fps': 8, 'frames': 2, 'max_cll': 1, 'max_fall': 1,
                       'master_display': 'G(1,1)B(1,1)R(1,1)WP(1,1)L(2,1)'}, f)
        out = io.StringIO()
        with self.assertRaises(SystemExit) as stop, contextlib.redirect_stdout(out), \
                contextlib.redirect_stderr(io.StringIO()):
            verify.main([self.path('junk.mp4'), '--previews', ''])
        self.assertNotEqual(stop.exception.code, 0)
        self.assertIn('FAIL', out.getvalue())


if __name__ == '__main__':
    unittest.main()
