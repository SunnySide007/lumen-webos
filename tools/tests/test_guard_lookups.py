"""Tests for tools/guard/lookups.py: python3 -I -m unittest discover -s tools/tests

The logs here are written in the format lumen-guard writes, with example names.
"""

import io
import os
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'guard'))

import lookups  # noqa: E402

T0 = 1793318400  # 2026-10-30 00:00:00Z


class GuardLogTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)

    def write(self, name, text):
        path = os.path.join(self.dir.name, name)
        with open(path, 'w', encoding='ascii') as f:
            f.write(text)
        return path

    def profile(self):
        return ['--allowlist', self.write('allow.txt', 'netflix.com\n'),
                '--required', self.write('req.txt', 'snu.lge.com\n'),
                '--hosts', self.write('h.hosts', '# 0.0.0.0\tmaybe.example\n')]

    def run_main(self, *logs):
        with redirect_stdout(io.StringIO()) as out:
            code = lookups.main([*logs, *self.profile()])
        self.assertEqual(code, 0)
        return out.getvalue()

    def test_counts_names_across_both_files(self):
        old = self.write('lookups.log.1', f'{T0} A www.netflix.com allow\n'
                                          f'{T0 + 1} AAAA www.netflix.com allow\n')
        new = self.write('lookups.log', f'{T0 + 60} A www.netflix.com allow\n'
                                        f'{T0 + 61} A snu.lge.com would-block\n'
                                        f'{T0 + 62} A maybe.example allow\n'
                                        f'{T0 + 63} - - allow\n')
        report = self.run_main(old, new)
        self.assertIn('2 file(s), 6 lookups of 4 names', report)
        self.assertIn('Would-block: 1 lookups of 1 names.', report)
        self.assertIn('| www.netflix.com | 3 | A, AAAA | 2026-10-30 00:00:00Z | 2026-10-30 00:01:00Z '
                      '| allow | allow |', report)
        self.assertIn('| snu.lge.com | 1 | A |', report)
        self.assertIn('| would-block | block |', report)
        self.assertIn('| maybe.example | 1 | A |', report)
        self.assertIn('| allow | candidate |', report)
        self.assertIn('| - | 1 | - |', report)
        # Most looked-up first.
        self.assertLess(report.index('www.netflix.com'), report.index('snu.lge.com'))

    def test_bad_lines_are_skipped_and_counted(self):
        log = self.write('lookups.log', f'{T0} A a.example allow\nhalf a line\n'
                                        f'{T0} A two words allow extra\nnot-a-time A b.example allow\n')
        report = self.run_main(log)
        self.assertIn('1 lookups of 1 names', report)
        self.assertIn('3 of 4 lines skipped', report)

    def test_names_stay_in_one_cell(self):
        log = self.write('lookups.log', f'{T0} TXT evil|name allow\n')
        self.assertIn('| evil\\|name | 1 | TXT |', self.run_main(log))

    def test_an_empty_log_says_so(self):
        report = self.run_main(self.write('lookups.log', ''))
        self.assertIn('0 lookups of 0 names', report)
        self.assertIn('| none |', report)

    def test_a_missing_file_fails(self):
        with redirect_stderr(io.StringIO()) as err:
            self.assertEqual(lookups.main([os.path.join(self.dir.name, 'nope.log')]), 1)
        self.assertIn('lookups.py:', err.getvalue())


if __name__ == '__main__':
    unittest.main()
