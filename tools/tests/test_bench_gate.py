"""Tests for tools/bench/gate.py: python3 -I -m unittest discover -s tools/tests

The frame-stats.sh text below is the B9's 2026-11-07 run of lane J's build
with {"look":"plain"} (OLED55B9PUA, firmware 05.50.00), cut to the sections
gate.py reads.
"""

import json
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'bench'))

import gate  # noqa: E402

STATS = '''\
windows (fps lines): 24, 1065 frames, 24.3 s
  average frame    22.80 ms (43.9 fps)
  worst frame      49.41 ms
bursts (drew ... idle): 43, 2021 frames, 42.8 s
  average frame    21.61 ms (46.3 fps)
  worst frame      49.41 ms
  burst worst      p50 38.14  p95 42.12  p99 49.41 ms
  missed vsync     42 of 43 bursts had a frame over 25 ms
  skip count       43 of 43 bursts carry it
  vsyncs skipped   563 in all, in 31 of 43 bursts
  per 10 s         131.69 over 42.8 s of motion
  worst burst      64 skipped, 210 frames at 1794098154.961
key repeats (SDL repeat lines): 40 lines from 8 holds
key presses (key ...: lines): 46 shown, 0 not shown, 0 more not timed
  shown after      p50 1  p95 33  max 33 ms
  target 2         46 of 46 within 33 ms
'''

LOG = '''\
[1794095823.499] start: sdl 125, gl 193, renderer 261, assets 433, apps 498, first frame 536 ms
[1794095864.367] key down scancode 80 keycode 0x40000050 (Some(Move(Left)))
[1794095864.934] key down scancode 79 keycode 0x4000004f (Some(Move(Right)))
[1794095900.000] start: sdl 1, gl 1, renderer 1, assets 1, apps 1, first frame 9 ms
'''


class RunNumbers(unittest.TestCase):
    def test_reads_the_bursts_keys_and_start_up(self):
        play = ['{"ready":"Lumen robot remote"}',
                '{"done":96,"first":1.0,"last":46.0,"late_ms":3,"echoes":0}']
        n = gate.run_numbers(STATS, LOG, play, rss_kb=61000, cpu_pct=0.4)
        self.assertEqual(n['bursts'], 43)
        self.assertEqual(n['frames'], 2021)
        self.assertEqual(n['avg_frame_ms'], 21.61)
        self.assertEqual(n['worst_frame_ms'], 49.41)
        self.assertEqual(n['skipped_per_10s'], 131.69)
        self.assertEqual(n['skipped'], 563)
        self.assertEqual(n['bursts_with_miss'], 42)
        self.assertEqual((n['key_p50_ms'], n['key_p95_ms'], n['key_max_ms']), (1.0, 33.0, 33.0))
        self.assertEqual(n['startup_ms'], 536.0, 'the first start line only')
        self.assertEqual(n['platform'], 'sdl')
        self.assertEqual(n['key_downs'], 2)
        self.assertEqual(n['robot_presses'], 96)
        self.assertEqual((n['rss_kb'], n['idle_cpu_pct']), (61000, 0.4))
        self.assertNotIn('yielded', n)

    def test_a_yield_is_kept(self):
        n = gate.run_numbers('', '', ['{"yield":{"node":"event3"},"played":4}'])
        self.assertEqual(n['yielded'], {'node': 'event3'})
        self.assertNotIn('avg_frame_ms', n)

    def test_a_wayland_start_line(self):
        log = '[1.0] start: wayland 40, gl 90, renderer 120, assets 300, apps 350, first frame 410 ms\n'
        self.assertEqual(gate.startup_from_log(log), {'platform': 'wayland', 'startup_ms': 410.0})


class Combine(unittest.TestCase):
    def test_medians_over_the_runs(self):
        s = gate.combine([{'avg_frame_ms': 16.9, 'platform': 'sdl'},
                          {'avg_frame_ms': 17.4, 'platform': 'sdl'},
                          {'avg_frame_ms': 16.8, 'yielded': {}}])
        self.assertEqual(s['runs'], 3)
        self.assertEqual(s['avg_frame_ms'], 16.9)
        self.assertEqual(s['avg_frame_ms_all'], [16.9, 17.4, 16.8])
        self.assertEqual(s['platform'], 'sdl')
        self.assertTrue(s['yielded'])


class Gate(unittest.TestCase):
    BASE = {'avg_frame_ms': 16.88, 'skipped_per_10s': 4.7, 'startup_ms': 540, 'rss_kb': 60000}

    def test_the_handoffs_limits(self):
        good = {'avg_frame_ms': 17.1, 'skipped_per_10s': 12.0, 'startup_ms': 600, 'rss_kb': 65120}
        rows = gate.check(good, self.BASE)
        self.assertTrue(all(ok for *_, ok in rows), rows)
        limits = {name: limit for name, _, limit, _ in rows}
        self.assertAlmostEqual(limits['average frame (ms)'], 17.18)
        self.assertEqual(limits['vsyncs skipped per 10 s'], 12.0)

    def test_each_limit_can_fail(self):
        for key, value in [('avg_frame_ms', 17.19), ('skipped_per_10s', 12.1),
                           ('startup_ms', 601), ('rss_kb', 65121)]:
            bad = {'avg_frame_ms': 16.9, 'skipped_per_10s': 4.0, 'startup_ms': 500,
                   'rss_kb': 60000, key: value}
            self.assertFalse(all(ok for *_, ok in gate.check(bad, self.BASE)), key)

    def test_the_ceiling_and_the_skip_factor(self):
        base = {'avg_frame_ms': 17.1, 'skipped_per_10s': 20.0, 'startup_ms': 500}
        rows = {name: limit for name, _, limit, _ in gate.check({}, base)}
        self.assertEqual(rows['average frame (ms)'], 17.2)
        self.assertEqual(rows['vsyncs skipped per 10 s'], 30.0)

    def test_nothing_passes_unmeasured_and_a_yield_fails(self):
        rows = gate.check({'yielded': True}, self.BASE)
        self.assertFalse(any(ok for *_, ok in rows))
        self.assertIn('not measured', gate.table(rows))


class Params(unittest.TestCase):
    def test_merges_later_wins(self):
        self.assertEqual(gate.main(['params', '{"look":"plain"}', '{"saver":"hold","look":"x"}']), 0)

    def test_refuses_a_list(self):
        with open(os.devnull, 'w') as null:
            old, sys.stderr = sys.stderr, null
            try:
                self.assertEqual(gate.main(['params', '[1]']), 2)
            finally:
                sys.stderr = old


if __name__ == '__main__':
    unittest.main()
