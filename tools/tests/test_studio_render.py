"""Tests for the studio scenes (studio/aurora.py, fireflies.py, scenekit.py), exr.py and render.py.

They need numpy and OpenEXR, so they skip in the build container and run in
the studio container:

    tools/studio.sh python3 -I -m unittest discover -s tools/tests -p 'test_studio*.py'
"""

import contextlib
import io
import json
import os
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
    import exr  # noqa: E402
    import hdr  # noqa: E402
    import render  # noqa: E402
    import scenekit  # noqa: E402

NEEDS_STUDIO = unittest.skipUnless(np, 'needs numpy and OpenEXR: run it with tools/studio.sh')


@NEEDS_STUDIO
class SceneTest(unittest.TestCase):
    """Every scene: seamless, made for an OLED, reproducible, the same at any size."""

    SCENES = sorted(render.SCENES.items()) if np else []

    def test_last_frame_meets_the_first(self):
        for name, make in self.SCENES:
            with self.subTest(scene=name):
                scene = make(160, 90, frames=48, fps=24)
                first = scene.render(0)
                # Every motion must run whole cycles: phase 2π is phase 0 again.
                # (render() takes the index modulo the loop, so it cannot show this.)
                self.assertTrue(np.allclose(scene.frame_at(scenekit.TAU), scene.frame_at(0.0), atol=1e-3))
                step = np.abs(scene.render(1) - first).mean()
                seam = np.abs(first - scene.render(47)).mean()
                self.assertGreater(step, 0.0, 'the scene should move')
                self.assertLess(seam, 2.0 * step)

    def test_made_for_oled(self):
        for name, make in self.SCENES:
            with self.subTest(scene=name):
                scene = make(320, 180, frames=24, fps=24)
                for index in (0, 8, 16):
                    frame = scene.render(index)
                    self.assertEqual(frame.shape, (180, 320, 3))
                    self.assertEqual(frame.dtype, np.float32)
                    self.assertTrue(np.isfinite(frame).all())
                    self.assertGreaterEqual(frame.min(), 0.0)
                    brightest = frame.max(axis=2)
                    self.assertLessEqual(brightest.max(), scenekit.PEAK_NITS)
                    self.assertLess(brightest.mean(), 25.0, 'average light stays low')
                    self.assertGreater((brightest < 1.0).mean(), 0.4, 'most of the screen is near black')

    def test_highlights_reach_toward_the_panel_peak(self):
        # At 1080p, the size these loops are made for, or larger: smaller
        # pictures blur the brightest points into their neighbours.
        for name, make in self.SCENES:
            with self.subTest(scene=name):
                scene = make(1920, 1080, frames=24, fps=24)
                self.assertGreater(max(scene.render(i).max() for i in range(0, 24, 4)), 500.0)

    def test_frames_are_reproducible(self):
        for name, make in self.SCENES:
            with self.subTest(scene=name):
                a = make(160, 90, frames=24, fps=24).render(5)
                b = make(160, 90, frames=24, fps=24).render(5)
                self.assertTrue(np.array_equal(a, b))

    def test_the_look_does_not_depend_on_resolution(self):
        for name, make in self.SCENES:
            with self.subTest(scene=name):
                small = make(320, 180, frames=24, fps=24).render(3).mean()
                large = make(640, 360, frames=24, fps=24).render(3).mean()
                self.assertAlmostEqual(small / large, 1.0, delta=0.15)

    def test_harmonics_are_whole_cycles_per_loop(self):
        self.assertEqual(scenekit.harmonic(0.05, 10.0), 1)
        self.assertEqual(scenekit.harmonic(0.05, 30.0), 2)
        self.assertEqual(scenekit.harmonic(-0.1, 30.0), -3)
        self.assertEqual(scenekit.harmonic(0.001, 10.0), 1)

    def test_splat_puts_a_centred_point_at_its_peak(self):
        image = np.zeros((9, 9, 3), dtype=np.float32)
        scenekit.splat(image, np.array([4.5]), np.array([4.5]), np.array([700.0]),
                       np.array([[1.0, 0.5, 0.25]]), np.array([1.2]))
        self.assertAlmostEqual(float(image[4, 4, 0]), 700.0, places=2)
        self.assertAlmostEqual(float(image[4, 4, 2]), 175.0, places=2)
        # All its light lands in the image, bar the far tails the image edge cuts.
        self.assertAlmostEqual(float(image[..., 0].sum()), 700.0 / scenekit.erf(np.array(0.5 / (1.2 * 2 ** 0.5))) ** 2,
                               delta=7.0)

    def test_a_resolved_peak_keeps_a_points_light_at_any_size(self):
        big = scenekit.resolved_peak(700.0, 4.0)
        self.assertAlmostEqual(float(big), 700.0, delta=7.0)
        for sigma in (0.2, 0.5, 1.0, 3.0):
            image = np.zeros((41, 41, 3), dtype=np.float32)
            scenekit.splat(image, np.array([20.5]), np.array([20.5]), np.array([scenekit.resolved_peak(1.0, sigma)]),
                           np.ones((1, 3)), np.array([sigma]))
            # A Gaussian of peak 1 holds 2πσ² of light, in pixel units.
            self.assertAlmostEqual(float(image[..., 0].sum()), 2 * np.pi * sigma ** 2, delta=0.01, msg=sigma)


@NEEDS_STUDIO
class ExrTest(unittest.TestCase):
    def test_round_trip_keeps_pixels_and_colour_metadata(self):
        rgb = np.linspace(0.0, 750.0, 4 * 6 * 3, dtype=np.float32).reshape(4, 6, 3)
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, 'f.exr')
            exr.write(path, rgb, hdr.P3_D65, white_nits=100.0)
            back, primaries, white = exr.read(path)
        self.assertEqual(back.shape, rgb.shape)
        self.assertTrue(np.allclose(back, rgb, rtol=1e-3, atol=1e-3))
        self.assertEqual(white, 100.0)
        for got, want in zip(primaries, hdr.P3_D65):
            self.assertAlmostEqual(got[0], want[0], places=6)
            self.assertAlmostEqual(got[1], want[1], places=6)

    def test_values_are_stored_relative_to_white(self):
        rgb = np.full((2, 2, 3), 250.0, dtype=np.float32)
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, 'f.exr')
            exr.write(path, rgb, hdr.BT2020, white_nits=100.0)
            with OpenEXR.File(path) as f:
                stored = f.channels()['RGB'].pixels.copy()
        self.assertEqual(stored.dtype, np.float16)
        self.assertTrue(np.allclose(stored, 2.5))


@NEEDS_STUDIO
class RenderCliTest(unittest.TestCase):
    def test_renders_frames_and_a_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = os.path.join(tmp, 'loop')
            render.main(['--size', '64x36', '--seconds', '0.25', '--fps', '8', '--workers', '1', '--out', out])
            files = sorted(os.listdir(out))
            with open(os.path.join(out, 'manifest.json')) as f:
                manifest = json.load(f)
        self.assertEqual(files, ['frame_000000.exr', 'frame_000001.exr', 'manifest.json'])
        self.assertEqual(manifest['scene'], 'aurora')
        self.assertEqual((manifest['width'], manifest['height']), (64, 36))
        self.assertEqual((manifest['fps'], manifest['frames']), (8, 2))
        self.assertEqual(manifest['pattern'], 'frame_%06d.exr')
        self.assertEqual(manifest['primaries'], 'P3_D65')
        self.assertEqual(manifest['white_nits'], 100.0)

    def render_quietly(self, out, *extra):
        with contextlib.redirect_stderr(io.StringIO()):
            render.main(['--size', '64x36', '--seconds', '0.5', '--fps', '8', '--out', out, *extra])

    def test_workers_in_parallel_render_the_same_frames(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.render_quietly(os.path.join(tmp, 'one'), '--workers', '1')
            self.render_quietly(os.path.join(tmp, 'two'), '--workers', '2')
            for i in range(4):
                a, _, _ = exr.read(os.path.join(tmp, 'one', render.PATTERN % i))
                b, _, _ = exr.read(os.path.join(tmp, 'two', render.PATTERN % i))
                self.assertTrue(np.array_equal(a, b), i)

    def test_a_render_resumes_with_the_missing_frames(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.render_quietly(tmp, '--workers', '1')
            kept = os.stat(os.path.join(tmp, render.PATTERN % 0)).st_mtime_ns
            os.remove(os.path.join(tmp, render.PATTERN % 2))
            self.render_quietly(tmp, '--workers', '1')
            self.assertEqual(os.stat(os.path.join(tmp, render.PATTERN % 0)).st_mtime_ns, kept)
            self.assertTrue(os.path.exists(os.path.join(tmp, render.PATTERN % 2)))
            with open(os.path.join(tmp, 'manifest.json')) as f:
                self.assertEqual(json.load(f)['render']['frames_rendered'], 1)

    def test_frames_from_other_scene_code_are_rendered_again(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.render_quietly(tmp, '--workers', '1')
            path = os.path.join(tmp, 'manifest.json')
            with open(path) as f:
                manifest = json.load(f)
            manifest['scene_version'] = 'older'
            with open(path, 'w') as f:
                json.dump(manifest, f)
            self.render_quietly(tmp, '--workers', '1')
            with open(path) as f:
                manifest = json.load(f)
        self.assertEqual(manifest['scene_version'], render.scene_version('aurora'))
        self.assertEqual(manifest['render']['frames_rendered'], 4)

    def test_frame_files_it_did_not_write_are_left_alone(self):
        with tempfile.TemporaryDirectory() as tmp:
            theirs = os.path.join(tmp, 'frame_000000.exr')
            with open(theirs, 'w') as f:
                f.write('someone else')
            with self.assertRaises(SystemExit):
                self.render_quietly(tmp, '--workers', '1')
            with open(theirs) as f:
                self.assertEqual(f.read(), 'someone else')

    def test_bad_sizes_are_refused(self):
        for size in ('1920', '0x1080', 'axb', '1921x1080'):
            with self.assertRaises(SystemExit, msg=size), contextlib.redirect_stderr(io.StringIO()):
                render.parse_args(['--size', size])

    def test_a_loop_needs_two_frames(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            render.parse_args(['--seconds', '0.01', '--fps', '24'])


if __name__ == '__main__':
    unittest.main()
