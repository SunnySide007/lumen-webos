"""tools/guard/rules.py: Guard's G2 rules on the PC, held to rules.rs's."""

import io
import os
import re
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, '..', '..')
sys.path.insert(0, os.path.join(ROOT, 'tools', 'guard'))

import rules  # noqa: E402

FIXTURE = os.path.join(HERE, 'fixtures', 'guard-g2')


def read(path):
    with open(path, encoding='utf-8') as f:
        return f.read()


class SharedCasesTest(unittest.TestCase):
    """The cases rules.rs's the_shared_fixture_cases reads too."""

    def test_cases(self):
        r = rules.Rules(os.path.join(FIXTURE, 'rules'))
        self.assertEqual(r.warnings, [])
        owner = rules.parse_owner(read(os.path.join(FIXTURE, 'owner.txt')))
        n = 0
        for line in read(os.path.join(FIXTURE, 'cases.txt')).splitlines():
            fields = rules.strip(line).split()
            if len(fields) != 3:
                continue
            name, want, window = fields
            got, source = r.decide(name, owner, window == 'learning')
            self.assertEqual(got, want, f'{name} ({window}): {source}')
            n += 1
        self.assertGreaterEqual(n, 20)

    def test_sources(self):
        r = rules.Rules(os.path.join(FIXTURE, 'rules'))
        owner = rules.parse_owner(read(os.path.join(FIXTURE, 'owner.txt')))
        for name, source in [('snu.lge.com', 'required'), ('ads.example', 'owner'),
                             ('www.netflix.com', 'allowlist'), ('logs.netflix.com', 'carve-outs'),
                             ('us.rdx2.lgtvsdp.com', 'list'), ('nobody.example', 'default')]:
            self.assertEqual(r.decide(name, owner)[1], source, name)


class SameAsRustTest(unittest.TestCase):
    def test_must_resolve_matches_rules_rs(self):
        src = read(os.path.join(ROOT, 'shell', 'crates', 'lumen-guard', 'src', 'rules.rs'))
        block = re.search(r'pub const MUST_RESOLVE: &\[&str\] = &\[(.*?)\];', src, re.S)
        self.assertIsNotNone(block)
        self.assertEqual(tuple(re.findall(r'"([^"]+)"', block.group(1))), rules.MUST_RESOLVE)

    def test_store_safe_checks_live_names_are_all_kept(self):
        # Every name store-safe-check.sh asks the TV for must be one Guard
        # refuses to block.
        src = read(os.path.join(ROOT, 'tools', 'store-safe-check.sh'))
        loop = re.search(r'for n in (.*?); do', src, re.S)
        self.assertIsNotNone(loop)
        live = loop.group(1).replace('\\', ' ').split()
        self.assertTrue(live)
        for name in live:
            self.assertIn(name, rules.MUST_RESOLVE)


class ShippedProfileTest(unittest.TestCase):
    def setUp(self):
        self.r = rules.Rules()

    def test_it_can_block(self):
        self.assertEqual(self.r.warnings, [])
        self.assertEqual(self.r.problems(), [])

    def test_kept_and_blocked(self):
        for name in ['us.security.lgtvsdp.com', 'us.tvsdp.lgeapi.com',
                     'lg-tv-2019.prod.partner.netflix.net', 'customerevents.netflix.com',
                     'ichnaea.netflix.com', 'tv-static.scdn.co', 'www.gstatic.com',
                     'd184dfn36gombl.cloudfront.net', 'heartbeat.lumen.invalid']:
            self.assertEqual(self.r.decide(name)[0], 'allow', name)
        for name, source in [('nsu.lge.com', 'required'), ('snu-dev.lge.com', 'required'),
                             ('unagi-na.amazon.com', 'carve-outs'),
                             # Perflyst's exact name beats the suffix both
                             # Glasshouse and hagezi list (Glasshouse's file
                             # is read first).
                             ('us.rdx2.lgtvsdp.com', 'perflyst-smarttv'),
                             ('gb.rdx2.lgtvsdp.com', 'glasshouse'),
                             ('us.nextlgsdp.com', 'perflyst-smarttv'),
                             ('aic.cdpbeacon.lgtvcommon.com', 'perflyst-smarttv'),
                             ('www.ueiwsp.com', 'default'), ('ngfts.lge.com', 'default')]:
            self.assertEqual(self.r.decide(name), ('block', source), name)

    def test_an_owner_file_can_make_it_unsafe(self):
        owner = rules.parse_owner('!=www.youtube.com\n=snu.lge.com\n')
        problems = self.r.problems(owner)
        self.assertTrue(any('www.youtube.com' in p for p in problems), problems)
        # The firmware hosts stay blocked whatever the owner writes.
        self.assertFalse(any('snu.lge.com' in p for p in problems), problems)


class CliTest(unittest.TestCase):
    def run_main(self, argv):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = rules.main(argv)
        return code, out.getvalue(), err.getvalue()

    def test_names(self):
        code, out, _ = self.run_main(['www.netflix.com', 'Nobody.Example.', '--learning'])
        self.assertEqual(code, 0)
        self.assertEqual(out.splitlines(), ['www.netflix.com allow allowlist',
                                            'Nobody.Example. learn default'])

    def test_check(self):
        self.assertEqual(self.run_main(['--check'])[0], 0)
        with tempfile.TemporaryDirectory() as d:
            owner = os.path.join(d, 'owner.txt')
            with open(owner, 'w', encoding='utf-8') as f:
                f.write('!us.lgtvsdp.com\n')
            code, out, _ = self.run_main(['--check', '--owner', owner])
            self.assertEqual(code, 1)
            self.assertIn('FAIL guard rules: us.lgtvsdp.com is not allowed (owner)', out)
            # A rules folder without its files fails too, and says why.
            code, out, err = self.run_main(['--check', '--rules', d])
            self.assertEqual(code, 1)
            self.assertIn('allowlist.txt', err)
            code, _, err = self.run_main(['x.example', '--owner', os.path.join(d, 'none')])
            self.assertEqual(code, 2)


if __name__ == '__main__':
    unittest.main()
