"""Tests for tools/bench/front.py: python3 -I -m unittest discover -s tools/tests

The log below is the B9's own (OLED55B9PUA, firmware 05.50.00), 2026-11-10:
a robot Home press with HDMI 3 in front and the shell kept hidden by
lumend, build f5419d9 on the Wayland path. The lines after the crossfade
are added to show where the window ends.
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'bench'))

import front  # noqa: E402

PRESS = 1794287716.972

LOG = '''\
[1794287710.100] drew 12 frames in 190 ms (avg 16.70 ms, worst 17.10 ms, 139 quads); 0 vsyncs skipped; idle
[1794287717.165] lifecycle: relaunch (launchType homeKey)
[1794287717.166] relaunch (Home): no key yet, 10 other events since
[1794287717.166] in front again (relaunched): drawing resumes
[1794287717.170] foreground: first frame after 5 ms
[1794287717.170] drew 1 frames in 0 ms (avg 0.00 ms, worst 0.00 ms, 137 quads); 0 vsyncs skipped; idle
[1794287717.204] wayland: window state fullscreen
[1794287717.205] window restored: no key yet, 11 other events since
[1794287717.225] front app io.github.sunnyside007.lumen.shell: last input 32.6 s ago
[1794287717.562] hero hero-ambient: 3 layers, 7085 KB; decoded in 324.5 ms, uploaded in 41.6 ms
[1794287717.603] slow frame 2: 37.52 ms = loop 0.09 + batch 0.65 + upload 0.00 + submit 36.29 + swap 0.49 ms; 0 uploads, 0 KB, 0 glyphs; faults 4 minor, 0 major
[1794287717.669] slow frame 5: 32.40 ms = loop 0.19 + batch 0.39 + upload 0.00 + submit 31.56 + swap 0.26 ms; 0 uploads, 0 KB, 0 glyphs; faults 8 minor, 0 major
[1794287717.719] slow frame 7: 33.39 ms = loop 0.09 + batch 0.38 + upload 0.00 + submit 32.65 + swap 0.27 ms; 0 uploads, 0 KB, 0 glyphs; faults 2 minor, 0 major
[1794287717.736] window focus gained: no key yet, 12 other events since
[1794287717.785] slow frame 10: 33.49 ms = loop 0.08 + batch 0.41 + upload 0.00 + submit 32.75 + swap 0.24 ms; 0 uploads, 0 KB, 0 glyphs; faults 2 minor, 0 major
[1794287718.235] drew 37 frames in 670 ms (avg 18.60 ms, worst 37.52 ms, 139 quads); first 37.52, 18.65, 14.35 ms; 6 vsyncs skipped; idle
[1794287718.238] crossfade gradient to hero-ambient: 37 frames; textures 15277 KB at most
[1794287719.900] drew 3 frames in 50 ms (avg 16.60 ms, worst 16.90 ms, 139 quads); 1 vsyncs skipped; idle
[1794287720.239] drew 9 frames in 150 ms (avg 16.70 ms, worst 17.00 ms, 139 quads); 4 vsyncs skipped; idle
[1794287721.000] slow frame 3: 40.00 ms = loop 0.10 + batch 0.40 + upload 0.00 + submit 39.00 + swap 0.50 ms; 0 uploads
'''


class Cut(unittest.TestCase):
    def setUp(self):
        self.out = front.cut(LOG, PRESS)

    def test_the_window_runs_from_the_relaunch_to_two_seconds_after_the_fade(self):
        self.assertTrue(self.out['complete'])
        self.assertEqual(self.out['relaunch_at'], 1794287717.165)
        self.assertEqual(self.out['window_ms'], 3073)
        # Before the relaunch and later than 2 s after the fade: left out.
        self.assertEqual([d['frames'] for d in self.out['drew']], [1, 37, 3])
        self.assertEqual(self.out['frames'], 41)
        self.assertEqual(self.out['skipped'], 7)

    def test_the_fades_own_drew_line(self):
        self.assertEqual(self.out['fade'], 'gradient to hero-ambient')
        self.assertEqual(self.out['crossfade_frames'], 37)
        self.assertEqual(self.out['fade_frames'], 37)
        self.assertEqual(self.out['fade_ms'], 670)
        self.assertEqual(self.out['fade_avg_ms'], 18.60)
        self.assertEqual(self.out['fade_worst_ms'], 37.52)
        self.assertEqual(self.out['fade_skipped'], 6)

    def test_the_slow_frames_splits(self):
        self.assertEqual(self.out['slow_frames'], 4)
        self.assertEqual([s['submit_ms'] for s in self.out['slow']], [36.29, 31.56, 32.65, 32.75])
        self.assertEqual(self.out['slow'][0],
                         {'frame': 2, 'ms': 37.52, 'loop_ms': 0.09, 'batch_ms': 0.65,
                          'upload_ms': 0.0, 'submit_ms': 36.29, 'swap_ms': 0.49})
        self.assertEqual(self.out['worst_submit_ms'], 36.29)

    def test_the_hero_and_target_5(self):
        self.assertEqual(self.out['hero'], 'hero-ambient')
        self.assertEqual(self.out['hero_decode_ms'], 324.5)
        self.assertEqual(self.out['hero_upload_ms'], 41.6)
        self.assertEqual(self.out['target5_ms'], 198)
        self.assertEqual(self.out['first_frame_ms'], 5)

    def test_no_crossfade_is_not_complete(self):
        text = '\n'.join(line for line in LOG.splitlines() if 'crossfade' not in line)
        out = front.cut(text, PRESS)
        self.assertFalse(out['complete'])
        self.assertNotIn('fade_frames', out)
        self.assertEqual(out['slow_frames'], 5)

    def test_no_relaunch_after_the_press(self):
        out = front.cut(LOG, PRESS + 10)
        self.assertEqual(out, {'complete': False, 'error': 'no relaunch after the press'})


if __name__ == '__main__':
    unittest.main()
