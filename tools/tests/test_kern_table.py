"""Tests for tools/font/kern_table.py: python3 -I -m unittest discover -s tools/tests

The synthetic subtables are built in code; the rest reads the committed Inter
SemiBold and the table made from it.
"""

import os
import struct
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'font'))

import kern_table  # noqa: E402
from kern_table import PairSubtable, build, pair_value  # noqa: E402


def read_table(data):
    """The committed format, decoded: (units per em, {(left, right): value})."""
    assert data[:4] == b'LKRN'
    version, upm, n, lefts, rights = struct.unpack_from('<HHHHH', data, 4)
    assert version == 1
    chars = struct.unpack_from(f'<{n}H', data, 14)
    left = data[14 + 2 * n:14 + 3 * n]
    right = data[14 + 3 * n:14 + 4 * n]
    values = struct.unpack_from(f'<{lefts * rights}h', data, 14 + 4 * n)
    assert len(data) == 14 + 4 * n + 2 * lefts * rights
    pairs = {}
    for i, a in enumerate(chars):
        for j, b in enumerate(chars):
            value = values[left[i] * rights + right[j]]
            if value:
                pairs[(a, b)] = value
    return upm, pairs


def format1():
    """Glyph 5 then glyph 7 kerns -50; nothing else is listed."""
    header = struct.pack('>HHHHHH', 1, 18, 0x0004, 0, 1, 12)
    pair_set = struct.pack('>HHh', 1, 7, -50)
    cover = struct.pack('>HHH', 1, 1, 5)
    return header + pair_set + cover


def format2():
    """Glyphs 5 and 6 are covered; 5 is left class 1, 7 and 8 right class 1.
    Values carry an XPlacement before the XAdvance."""
    header = struct.pack('>HHHHHHHH', 2, 32, 0x0005, 0, 40, 50, 2, 2)
    values = struct.pack('>8h', 0, 0, 0, 0, 0, 0, 99, -30)
    cover = struct.pack('>HHHH', 1, 2, 5, 6)
    class1 = struct.pack('>HHHHH', 1, 5, 2, 1, 0)
    class2 = struct.pack('>HHHHH', 2, 1, 7, 8, 1)
    return header + values + cover + class1 + class2


class PairSubtables(unittest.TestCase):
    def test_glyph_pairs_apply_only_to_their_pairs(self):
        sub = PairSubtable(format1(), 0)
        self.assertEqual(sub.get(5, 7), -50)
        self.assertIsNone(sub.get(5, 8), 'a covered glyph with no such pair')
        self.assertIsNone(sub.get(6, 7), 'not covered')

    def test_class_pairs_apply_to_every_covered_first_glyph(self):
        sub = PairSubtable(format2(), 0)
        self.assertEqual(sub.get(5, 7), -30, 'the XAdvance, not the XPlacement')
        self.assertEqual(sub.get(5, 8), -30)
        self.assertEqual(sub.get(5, 9), 0, 'right class 0')
        self.assertEqual(sub.get(6, 7), 0, 'left class 0')
        self.assertIsNone(sub.get(4, 7), 'not covered')

    def test_the_first_subtable_that_applies_decides_and_lookups_add_up(self):
        lookup = [PairSubtable(format1(), 0), PairSubtable(format2(), 0)]
        self.assertEqual(pair_value([lookup], 5, 7), -50, 'the exception wins')
        self.assertEqual(pair_value([lookup], 5, 8), -30, 'then the classes')
        self.assertEqual(pair_value([lookup, lookup], 5, 7), -100)
        self.assertEqual(pair_value([lookup], 4, 7), 0)


class Table(unittest.TestCase):
    def test_classes_keep_every_pair(self):
        pairs = {(65, 86): -154, (86, 65): -154, (84, 111): -160, (89, 111): -202,
                 (65, 84): -179}
        upm, back = read_table(build(2048, pairs))
        self.assertEqual(upm, 2048)
        self.assertEqual(back, pairs)

    def test_no_pairs_is_an_empty_table(self):
        self.assertEqual(read_table(build(1000, {})), (1000, {}))


class Inter(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        with open(kern_table.FONT, 'rb') as f:
            cls.upm, cls.pairs = kern_table.kerning(f.read())

    def test_known_pairs_kern_tighter(self):
        self.assertEqual(self.upm, 2048)
        # Inter 4.1 SemiBold, read 2026-10-22. A parser change that moves
        # these is a bug, not a stale table.
        expected = {'AV': -154, 'To': -160, 'Ty': -128, 'Yo': -202, 'LT': -190, 'r.': -128}
        for pair, value in expected.items():
            self.assertEqual(self.pairs.get((ord(pair[0]), ord(pair[1])), 0), value, pair)
        self.assertNotIn((ord('H'), ord('H')), self.pairs)
        self.assertEqual(len(self.pairs), 15259)

    def test_the_committed_table_matches_the_font(self):
        with open(kern_table.OUT, 'rb') as f:
            committed = f.read()
        self.assertEqual(committed, build(self.upm, self.pairs),
                         'stale: run `sh tools/dev.sh make kern-table`')
        self.assertEqual(read_table(committed), (self.upm, self.pairs))


if __name__ == '__main__':
    unittest.main()
