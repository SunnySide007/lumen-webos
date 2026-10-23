#!/usr/bin/env python3
"""Pulls Inter's kerning out of its GPOS table into the small table the shell
kerns its text with, using only the stdlib.

fontdue, the shell's rasteriser, kerns only from a `kern` table, and Inter 4.1
keeps its kerning in GPOS: pair adjustment lookups (type 2) under the `kern`
feature. This reads those lookups for the characters in CHARS and writes
shell/assets/fonts/Inter-SemiBold.kern, which text.rs builds into the binary.
The table is committed; regenerate it after changing the font or CHARS:

    sh tools/dev.sh make kern-table

The table, all numbers little-endian:

    magic "LKRN", version u16 (1), units per em u16,
    N u16 (characters), L u16 (left classes), R u16 (right classes),
    N characters, u16 code points in ascending order,
    N left classes, u8 each, then N right classes, u8 each,
    L x R kerning values, i16 font units, row by row (left class).

Characters that kern with the same values share a class; class 0 kerns with
nothing, and characters that kern with nothing on either side are left out.
"""

import os
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FONT = os.path.join(ROOT, 'shell', 'assets', 'fonts', 'Inter-SemiBold.ttf')
OUT = os.path.join(ROOT, 'shell', 'assets', 'fonts', 'Inter-SemiBold.kern')

MAGIC = b'LKRN'
VERSION = 1

# What Lumen draws: Basic Latin, Latin-1, Latin Extended-A (app names such as
# "Škoda" or "Łódź"), and the punctuation and symbols app names use, the
# ellipsis that ends a cut title among them.
CHARS = (
    list(range(0x20, 0x7F))
    + list(range(0xA0, 0x180))
    + [0x2013, 0x2014, 0x2018, 0x2019, 0x201A, 0x201C, 0x201D, 0x201E,
       0x2022, 0x2026, 0x2039, 0x203A, 0x20AC, 0x2122]
)

# ValueRecord fields, in the order they are stored.
X_PLACEMENT, Y_PLACEMENT, X_ADVANCE = 0x0001, 0x0002, 0x0004


class FontError(Exception):
    pass


def u16(data, at):
    return struct.unpack_from('>H', data, at)[0]


def i16(data, at):
    return struct.unpack_from('>h', data, at)[0]


def u32(data, at):
    return struct.unpack_from('>I', data, at)[0]


def tables(font):
    """The font's tables by tag, as slices."""
    count = u16(font, 4)
    found = {}
    for i in range(count):
        tag, _, offset, length = struct.unpack_from('>4sIII', font, 12 + 16 * i)
        found[tag.decode('latin-1')] = font[offset:offset + length]
    return found


def cmap(table):
    """Code point to glyph id, from the Unicode subtable (format 12 or 4)."""
    best = None
    for i in range(u16(table, 2)):
        platform, encoding, offset = struct.unpack_from('>HHI', table, 4 + 8 * i)
        fmt = u16(table, offset)
        rank = {(3, 10): 0, (0, 4): 1, (3, 1): 2, (0, 3): 3}.get((platform, encoding))
        if rank is not None and fmt in (4, 12) and (best is None or rank < best[0]):
            best = (rank, offset, fmt)
    if best is None:
        raise FontError('no Unicode cmap subtable')
    _, at, fmt = best
    glyphs = {}
    if fmt == 12:
        for i in range(u32(table, at + 12)):
            start, end, glyph = struct.unpack_from('>III', table, at + 16 + 12 * i)
            for c in range(start, end + 1):
                glyphs[c] = glyph + c - start
        return glyphs
    segs = u16(table, at + 6) // 2
    ends, starts = at + 14, at + 16 + 2 * segs
    deltas, ranges = starts + 2 * segs, starts + 4 * segs
    for s in range(segs):
        end, start = u16(table, ends + 2 * s), u16(table, starts + 2 * s)
        delta, range_at = i16(table, deltas + 2 * s), ranges + 2 * s
        range_offset = u16(table, range_at)
        for c in range(start, end + 1):
            if c == 0xFFFF:
                continue
            if range_offset == 0:
                glyph = (c + delta) & 0xFFFF
            else:
                glyph = u16(table, range_at + range_offset + 2 * (c - start))
                glyph = (glyph + delta) & 0xFFFF if glyph else 0
            if glyph:
                glyphs[c] = glyph
    return glyphs


def coverage(table, at):
    """Glyph id to coverage index."""
    fmt = u16(table, at)
    if fmt == 1:
        return {u16(table, at + 4 + 2 * i): i for i in range(u16(table, at + 2))}
    if fmt == 2:
        covered = {}
        for i in range(u16(table, at + 2)):
            start, end, index = struct.unpack_from('>HHH', table, at + 4 + 6 * i)
            for glyph in range(start, end + 1):
                covered[glyph] = index + glyph - start
        return covered
    raise FontError(f'coverage format {fmt}')


def class_def(table, at):
    """Glyph id to class; glyphs not listed are class 0."""
    fmt = u16(table, at)
    if fmt == 1:
        start = u16(table, at + 2)
        return {start + i: u16(table, at + 6 + 2 * i) for i in range(u16(table, at + 4))}
    if fmt == 2:
        classes = {}
        for i in range(u16(table, at + 2)):
            start, end, klass = struct.unpack_from('>HHH', table, at + 4 + 6 * i)
            for glyph in range(start, end + 1):
                classes[glyph] = klass
        return classes
    raise FontError(f'class definition format {fmt}')


def value_size(fmt):
    return 2 * bin(fmt & 0xFF).count('1')


def x_advance(table, at, fmt):
    """A ValueRecord's XAdvance, or 0 if it has none."""
    if not fmt & X_ADVANCE:
        return 0
    before = bin(fmt & (X_PLACEMENT | Y_PLACEMENT)).count('1')
    return i16(table, at + 2 * before)


class PairSubtable:
    """One PairPos subtable: `get(left, right)` is the kerning, or None when
    the subtable does not apply, so the lookup's next subtable is tried. A
    format 1 subtable applies only to the pairs it lists; a format 2 one to
    every pair whose first glyph it covers (class 0 values included)."""

    def __init__(self, table, at):
        self.format = u16(table, at)
        self.covered = coverage(table, at + u16(table, at + 2))
        fmt1, fmt2 = u16(table, at + 4), u16(table, at + 6)
        if fmt1 & ~(X_PLACEMENT | Y_PLACEMENT | X_ADVANCE) & 0xFF or fmt2 & 0xFF:
            # Device tables and second-glyph values are not kerning a
            # left-to-right line needs; say so rather than drop them unseen.
            print(f'kern_table: value formats {fmt1:#x}, {fmt2:#x}: '
                  'only the first glyph\'s XAdvance is kept', file=sys.stderr)
        record = value_size(fmt1) + value_size(fmt2)
        if self.format == 1:
            self.pairs = {}
            for first, index in self.covered.items():
                pair_set = at + u16(table, at + 10 + 2 * index)
                for i in range(u16(table, pair_set)):
                    rec = pair_set + 2 + i * (2 + record)
                    self.pairs[(first, u16(table, rec))] = x_advance(table, rec + 2, fmt1)
        elif self.format == 2:
            self.class1 = class_def(table, at + u16(table, at + 8))
            self.class2 = class_def(table, at + u16(table, at + 10))
            count1, count2 = u16(table, at + 12), u16(table, at + 14)
            self.values = [
                [x_advance(table, at + 16 + (c1 * count2 + c2) * record, fmt1)
                 for c2 in range(count2)]
                for c1 in range(count1)
            ]
        else:
            raise FontError(f'PairPos format {self.format}')

    def get(self, left, right):
        if left not in self.covered:
            return None
        if self.format == 1:
            return self.pairs.get((left, right))
        return self.values[self.class1.get(left, 0)][self.class2.get(right, 0)]


def kern_lookups(gpos):
    """The `kern` feature's pair lookups, for Latin (or the default script),
    each a list of subtables in order."""
    scripts, features, lookups = u16(gpos, 4), u16(gpos, 6), u16(gpos, 8)
    lang_sys = None
    for want in (b'latn', b'DFLT'):
        for i in range(u16(gpos, scripts)):
            tag, offset = struct.unpack_from('>4sH', gpos, scripts + 2 + 6 * i)
            default = u16(gpos, scripts + offset)
            if tag == want and default:
                lang_sys = scripts + offset + default
                break
        if lang_sys is not None:
            break
    if lang_sys is None:
        raise FontError('no Latin or default script in GPOS')
    wanted = set()
    for i in range(u16(gpos, lang_sys + 4)):
        feature = u16(gpos, lang_sys + 6 + 2 * i)
        tag, offset = struct.unpack_from('>4sH', gpos, features + 2 + 6 * feature)
        if tag == b'kern':
            at = features + offset
            wanted.update(u16(gpos, at + 4 + 2 * j) for j in range(u16(gpos, at + 2)))
    found = []
    for index in sorted(wanted):
        at = lookups + u16(gpos, lookups + 2 + 2 * index)
        kind, count = u16(gpos, at), u16(gpos, at + 4)
        subtables = []
        for j in range(count):
            sub = at + u16(gpos, at + 6 + 2 * j)
            sub_kind = kind
            if kind == 9:
                sub_kind = u16(gpos, sub + 2)
                sub += u32(gpos, sub + 4)
            if sub_kind == 2:
                subtables.append(PairSubtable(gpos, sub))
            else:
                # Contextual kerning, say, after a font upgrade: not read.
                print(f'kern_table: lookup {index} has a type {sub_kind} subtable, '
                      'which is skipped', file=sys.stderr)
        if subtables:
            found.append(subtables)
    return found


def pair_value(lookups, left, right):
    """The kerning between two glyphs: in each lookup the first subtable that
    applies decides, and the lookups add up."""
    total = 0
    for subtables in lookups:
        for sub in subtables:
            value = sub.get(left, right)
            if value is not None:
                total += value
                break
    return total


def kerning(font):
    """(units per em, {(left char, right char): font units}) for CHARS,
    leaving out pairs that don't kern."""
    t = tables(font)
    for tag in ('head', 'cmap', 'GPOS'):
        if tag not in t:
            raise FontError(f'no {tag} table')
    upm = u16(t['head'], 18)
    glyphs = cmap(t['cmap'])
    chars = [c for c in CHARS if c in glyphs]
    lookups = kern_lookups(t['GPOS'])
    pairs = {}
    for left in chars:
        for right in chars:
            value = pair_value(lookups, glyphs[left], glyphs[right])
            if value:
                pairs[(left, right)] = value
    return upm, pairs


def build(upm, pairs):
    """The table's bytes (module docstring)."""
    chars = sorted({c for pair in pairs for c in pair})
    if any(c > 0xFFFF for c in chars):
        raise FontError('a character beyond U+FFFF')
    rows = {c: tuple(pairs.get((c, r), 0) for r in chars) for c in chars}
    cols = {c: tuple(pairs.get((l, c), 0) for l in chars) for c in chars}

    def classes(lines):
        ids = {tuple([0] * len(chars)): 0}
        return {c: ids.setdefault(line, len(ids)) for c, line in lines.items()}, ids

    left, left_ids = classes(rows)
    right, right_ids = classes(cols)
    if len(left_ids) > 256 or len(right_ids) > 256:
        raise FontError('more than 256 classes')
    if any(not -0x8000 <= v <= 0x7FFF for v in pairs.values()):
        raise FontError('a kerning value beyond 16 bits')
    # One character of each class stands for it.
    left_of = {k: c for c, k in left.items()}
    right_of = {k: c for c, k in right.items()}
    out = bytearray(MAGIC)
    out += struct.pack('<HHHHH', VERSION, upm, len(chars), len(left_ids), len(right_ids))
    out += struct.pack(f'<{len(chars)}H', *chars)
    out += bytes(left[c] for c in chars) + bytes(right[c] for c in chars)
    for k1 in range(len(left_ids)):
        for k2 in range(len(right_ids)):
            value = 0
            if k1 and k2:
                value = pairs.get((left_of[k1], right_of[k2]), 0)
            out += struct.pack('<h', value)
    return bytes(out)


def main(argv):
    out = argv[1] if len(argv) > 1 else OUT
    with open(FONT, 'rb') as f:
        upm, pairs = kerning(f.read())
    table = build(upm, pairs)
    with open(out, 'wb') as f:
        f.write(table)
    print(f'{os.path.relpath(out, ROOT)}: {len(pairs)} pairs, {len(table)} bytes')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
