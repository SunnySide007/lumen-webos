#!/usr/bin/env python3
"""Summarises Lumen Guard's lookup log as a markdown table of names.

    python3 -I tools/guard/lookups.py tools/out/guard/lookups.log* [--owner FILE]

tools/guard.sh log fetches the files and runs this. Each log line is
`<unix seconds> <type> <name> <verdict>`; this counts the lookups of each
name, the types asked, when it was first and last seen (UTC), Guard's verdict
at the time (allow, learn, block, or would-block in log mode), and what this
checkout's G2 rules (profiles/guard, plus the owner's file if given) decide
now, with the rule that decides: tools/guard/rules.py, the same rules as
Guard's. Guard sits behind connman's cache only for the programs that still
ask connman, so a count is at least once per answer's lifetime, not once per
app lookup.
"""

import argparse
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, '..', 'capture'))
sys.path.insert(0, HERE)

from summarize import Seen, cell, when  # noqa: E402

import rules  # noqa: E402


class Name:
    __slots__ = ('seen', 'types', 'verdicts')

    def __init__(self):
        self.seen, self.types, self.verdicts = Seen(), set(), set()


# The boot hook's own probes (profiles/privacy/lumen-guard: the watchdog's
# heartbeat once a minute) end in this; they are Guard's, not the TV's.
PROBE_SUFFIX = '.lumen.invalid'
REFUSED = ('block', 'would-block')


def read_log(paths):
    """Returns ({name: Name}, lines read, lines skipped, probe lines left out)."""
    names, read, skipped, probes = {}, 0, 0, 0
    for path in paths:
        with open(path, encoding='ascii', errors='replace') as f:
            for line in f:
                read += 1
                fields = line.split()
                if len(fields) != 4 or not fields[0].isdigit():
                    skipped += 1
                    continue
                ts, qtype, name, verdict = int(fields[0]), fields[1], fields[2], fields[3]
                if name.endswith(PROBE_SUFFIX):
                    probes += 1
                    continue
                entry = names.setdefault(name, Name())
                entry.seen.add(ts)
                entry.types.add(qtype)
                entry.verdicts.add(verdict)
    return names, read, skipped, probes


def lookups_of(names):
    return sum(n.seen.count for n in names.values())


def render(names, read, skipped, probes, files, decide):
    """`decide(name)` gives (verdict, source) under this checkout's rules."""
    first = min((n.seen.first for n in names.values()), default=None)
    last = max((n.seen.last for n in names.values()), default=None)
    refused = {k: n for k, n in names.items() if n.verdicts & set(REFUSED)}
    learned = {k: n for k, n in names.items() if 'learn' in n.verdicts}
    now = {k: decide(k) for k in names if k != '-'}
    blocked_now = {k: names[k] for k, (v, _) in now.items() if v == 'block'}
    out = ['# Guard lookups', '']
    out.append(f'{files} file(s), {lookups_of(names)} lookups of {len(names)} names, '
               f'{when(first)} to {when(last)}; {skipped} of {read} lines skipped, '
               f'{probes} heartbeat probes (*{PROBE_SUFFIX}) left out. '
               f'Logged block or would-block: {lookups_of(refused)} lookups of '
               f'{len(refused)} names; learned: {lookups_of(learned)} lookups of '
               f'{len(learned)} names. These G2 rules block {lookups_of(blocked_now)} '
               f'lookups of {len(blocked_now)} names.')
    out.append('Counts are lookups that reached Guard. Logged is what Guard said at the time; '
               'G2 is what this checkout\'s rules (profiles/guard and the owner\'s file, if '
               'given) decide now, and Rule the rule that decides ("-" for a query Guard '
               'could not read).')
    out += ['', '| Name | Lookups | Types | First seen | Last seen | Logged | G2 | Rule |',
            '|---|---:|---|---|---|---|---|---|']
    for name, n in sorted(names.items(), key=lambda x: (-x[1].seen.count, x[0])):
        verdict, source = now.get(name, ('-', '-'))
        out.append(f'| {cell(name)} | {n.seen.count} | {cell(", ".join(sorted(n.types)))} '
                   f'| {when(n.seen.first)} | {when(n.seen.last)} '
                   f'| {cell(", ".join(sorted(n.verdicts)))} | {verdict} | {cell(source)} |')
    if not names:
        out.append('| none | | | | | | | |')
    return '\n'.join(out) + '\n'


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n', 1)[0])
    parser.add_argument('logs', nargs='+', help='lookups.log files from tools/guard.sh log')
    parser.add_argument('--rules', default=rules.PROFILE, help='default: profiles/guard')
    parser.add_argument('--owner', help="the owner's file, if any")
    args = parser.parse_args(argv)
    try:
        names, read, skipped, probes = read_log(args.logs)
        owner = None
        if args.owner:
            with open(args.owner, encoding='utf-8', errors='replace') as f:
                owner = rules.parse_owner(f.read())
    except OSError as e:
        print(f'lookups.py: {e}', file=sys.stderr)
        return 1
    profile = rules.Rules(args.rules)
    for w in profile.warnings:
        print(f'lookups.py: {w}', file=sys.stderr)
    sys.stdout.write(render(names, read, skipped, probes, len(args.logs),
                            lambda name: profile.decide(name, owner)))
    return 0


if __name__ == '__main__':
    sys.exit(main())
