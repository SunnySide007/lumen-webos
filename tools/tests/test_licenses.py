"""Tests for tools/licenses.py: python3 -I -m unittest discover -s tools/tests"""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))

from licenses import Bad, met, problems  # noqa: E402


def package(name, license, deps=(), member=False):
    return {
        'id': f'{"path+file:///work/shell/crates/" if member else "registry+"}{name}#0.1.0',
        'name': name,
        'version': '0.1.0',
        'license': license,
        'dependencies': [{'name': d, 'kind': None, 'path': None} for d in deps],
    }


def metadata(*packages):
    return {
        'packages': list(packages),
        'workspace_members': [p['id'] for p in packages if p['id'].startswith('path+')],
    }


class Expressions(unittest.TestCase):
    def test_or_needs_one_side_and_and_needs_both(self):
        self.assertTrue(met('MIT OR Apache-2.0'))
        self.assertTrue(met('GPL-2.0 OR MIT'))
        self.assertFalse(met('GPL-2.0 AND MIT'))
        self.assertTrue(met('(MIT OR Apache-2.0) AND Unicode-3.0'))
        self.assertFalse(met('(MIT OR Apache-2.0) AND SSPL-1.0'))
        self.assertTrue(met('Apache-2.0 WITH LLVM-exception OR MIT'))
        self.assertFalse(met('Apache-2.0 WITH Some-exception'))
        self.assertTrue(met('MIT/Apache-2.0'))
        self.assertFalse(met('AGPL-3.0-only'))

    def test_what_isnt_spdx_is_refused(self):
        for bad in ['MIT OR', '(MIT', 'MIT AND AND ISC', 'MIT; ISC', '']:
            with self.assertRaises(Bad, msg=bad):
                met(bad)


class Workspace(unittest.TestCase):
    THIRD = '| [libc](x) | ... |\n| `serde_json` | ... |'

    def test_a_clean_workspace_passes(self):
        m = metadata(
            package('lumend', 'GPL-3.0-or-later', ['libc', 'serde_json'], member=True),
            package('libc', 'MIT OR Apache-2.0'),
            package('serde_json', 'MIT OR Apache-2.0'),
            package('itoa', 'MIT OR Apache-2.0'),
        )
        self.assertEqual(problems(m, self.THIRD), [])

    def test_names_a_bad_license_a_missing_one_and_an_unlisted_dependency(self):
        m = metadata(
            package('lumend', 'GPL-3.0-or-later', ['libc', 'leftpad'], member=True),
            package('libc', 'MIT OR Apache-2.0'),
            package('leftpad', 'SSPL-1.0'),
            package('nolicense', None),
        )
        found = problems(m, self.THIRD)
        self.assertIn('leftpad 0.1.0: SSPL-1.0 is not on the list', found)
        self.assertIn('nolicense 0.1.0: no SPDX license', found)
        self.assertIn('leftpad: a direct dependency THIRD-PARTY.md does not name', found)
        self.assertEqual(len(found), 3)

    def test_the_repositorys_own_file_names_every_direct_dependency(self):
        root = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..')
        with open(os.path.join(root, 'THIRD-PARTY.md'), encoding='utf-8') as f:
            text = f.read()
        m = metadata(
            package('x', 'GPL-3.0-or-later',
                    ['fontdue', 'libc', 'libloading', 'png', 'serde_json'], member=True),
            *[package(n, 'MIT') for n in ['fontdue', 'libc', 'libloading', 'png', 'serde_json']],
        )
        self.assertEqual(problems(m, text), [])


if __name__ == '__main__':
    unittest.main()
