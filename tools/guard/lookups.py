#!/usr/bin/env python3
"""Summarises Lumen Guard's lookup log as a markdown table of names.

    python3 -I tools/guard/lookups.py tools/out/guard/lookups.log*

tools/guard.sh log fetches the files and runs this. Each log line is
`<unix seconds> <type> <name> <verdict>`; this counts the lookups of each
name, the types asked, when it was first and last seen (UTC), Guard's verdict
at the time, and what this checkout's profiles/glasshouse lists say now
(tools/capture/summarize.py's rules, so "candidate" rows show). Guard sits
behind connman's cache, so a count is how often connman asked the router: at
least once per answer's lifetime, not once per app lookup.
"""

import argparse
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'capture'))

from summarize import PROFILE, Lists, Seen, cell, when  # noqa: E402


class Name:
    __slots__ = ('seen', 'types', 'verdicts')

    def __init__(self):
        self.seen, self.types, self.verdicts = Seen(), set(), set()


def read_log(paths):
    """Returns ({name: Name}, lines read, lines skipped)."""
    names, read, skipped = {}, 0, 0
    for path in paths:
        with open(path, encoding='ascii', errors='replace') as f:
            for line in f:
                read += 1
                fields = line.split()
                if len(fields) != 4 or not fields[0].isdigit():
                    skipped += 1
                    continue
                ts, qtype, name, verdict = int(fields[0]), fields[1], fields[2], fields[3]
                entry = names.setdefault(name, Name())
                entry.seen.add(ts)
                entry.types.add(qtype)
                entry.verdicts.add(verdict)
    return names, read, skipped


def render(names, read, skipped, files, lists):
    lookups = sum(n.seen.count for n in names.values())
    blocked = {k: n for k, n in names.items() if 'would-block' in n.verdicts or 'block' in n.verdicts}
    first = min((n.seen.first for n in names.values()), default=None)
    last = max((n.seen.last for n in names.values()), default=None)
    out = ['# Guard lookups', '']
    out.append(f'{files} file(s), {lookups} lookups of {len(names)} names, '
               f'{when(first)} to {when(last)}; {skipped} of {read} lines skipped. '
               f'Would-block: {sum(n.seen.count for n in blocked.values())} lookups of '
               f'{len(blocked)} names.')
    out.append('Counts are connman\'s questions to the router (its cache answers the rest). '
               'Verdict is what Guard logged; List is what this checkout\'s '
               'profiles/glasshouse says now ("-" for a query Guard could not read).')
    out += ['', '| Name | Lookups | Types | First seen | Last seen | Verdict | List |',
            '|---|---:|---|---|---|---|---|']
    for name, n in sorted(names.items(), key=lambda x: (-x[1].seen.count, x[0])):
        out.append(f'| {cell(name)} | {n.seen.count} | {cell(", ".join(sorted(n.types)))} '
                   f'| {when(n.seen.first)} | {when(n.seen.last)} '
                   f'| {cell(", ".join(sorted(n.verdicts)))} '
                   f'| {"" if name == "-" else lists.status(name)} |')
    if not names:
        out.append('| none | | | | | | |')
    return '\n'.join(out) + '\n'


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n', 1)[0])
    parser.add_argument('logs', nargs='+', help='lookups.log files from tools/guard.sh log')
    parser.add_argument('--allowlist', default=os.path.join(PROFILE, 'allowlist.txt'))
    parser.add_argument('--required', default=os.path.join(PROFILE, 'required-blocks.txt'))
    parser.add_argument('--hosts', default=os.path.join(PROFILE, 'b9-aggressive.hosts'))
    args = parser.parse_args(argv)
    try:
        names, read, skipped = read_log(args.logs)
    except OSError as e:
        print(f'lookups.py: {e}', file=sys.stderr)
        return 1
    lists = Lists.load(args.allowlist, args.required, args.hosts)
    sys.stdout.write(render(names, read, skipped, len(args.logs), lists))
    return 0


if __name__ == '__main__':
    sys.exit(main())
