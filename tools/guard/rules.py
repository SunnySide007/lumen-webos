#!/usr/bin/env python3
"""Lumen Guard's G2 rules on the PC: what Guard decides for a name.

    python3 -I tools/guard/rules.py [--rules DIR] [--owner FILE] [--learning] NAME...
    python3 -I tools/guard/rules.py --check [--rules DIR] [--owner FILE]

The same rules as shell/crates/lumen-guard/src/rules.rs, read from the same
files (profiles/guard by default, and the owner's file if given), so
tools/guard.sh log, tools/store-safe-check.sh and the owner can ask what Guard
would do without the TV. tools/tests/test_guard_rules.py holds the two to the
same cases (tools/tests/fixtures/guard-g2).

Each NAME prints `name verdict source`: verdict allow, block or learn; source
the rule that decided (required, owner, allowlist, carve-outs, the imported
file's name, or default). --check prints nothing and exits 0 when Guard could
block with these rules (every name it must resolve is allowed and every
firmware host blocked), else 1 with a line per problem.
"""

import argparse
import ipaddress
import os
import re
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..')
PROFILE = os.path.join(ROOT, 'profiles', 'guard')
ALLOWLIST, CARVE_OUTS, REQUIRED, IMPORTED = (
    'allowlist.txt', 'carve-outs.txt', 'required-blocks.txt', 'imported')

# rules.rs's MUST_RESOLVE: the store, DRM, the paid apps (store-safe-check's
# live names) and the boot hook's heartbeat.
MUST_RESOLVE = (
    'us.lgtvsdp.com', 'lgtvsdp.com', 'us.lgeapi.com', 'www.netflix.com',
    'www.amazon.com', 'atv-ps.amazon.com', 'plex.tv', 'www.googleapis.com',
    'www.youtube.com', 'spclient.wg.spotify.com', 'heartbeat.lumen.invalid',
)

PLAIN = re.compile(r'^[a-z0-9_-]+(\.[a-z0-9_-]+)*$')


def strip(line):
    return line.split('#', 1)[0].strip().lower()


def plain(name):
    return len(name) <= 253 and PLAIN.match(name) is not None


def sinkhole(addr):
    return addr.is_unspecified or addr.is_loopback


class Table:
    """Exact and suffix rules, each ('allow' | 'block', source)."""

    def __init__(self):
        self.exact, self.suffix = {}, {}

    def __len__(self):
        return len(self.exact) + len(self.suffix)

    def insert(self, name, exact, act, source):
        if not plain(name):
            return
        rules = self.exact if exact else self.suffix
        # The same rule both ways allows, whichever came first.
        if name not in rules or (act == 'allow' and rules[name][0] != 'allow'):
            rules[name] = (act, source)

    def find(self, name):
        if name in self.exact:
            return self.exact[name]
        rest = name
        while True:
            if rest in self.suffix:
                return self.suffix[rest]
            if '.' not in rest:
                return None
            rest = rest.split('.', 1)[1]

    def add_allow_syntax(self, text, act, source, bangs=False):
        for line in text.splitlines():
            rule, this = strip(line), act
            if bangs and rule.startswith('!'):
                rule, this = rule[1:], 'block'
            if rule.startswith('='):
                self.insert(rule[1:], True, this, source)
            elif rule:
                self.insert(rule, False, this, source)

    def add_block_list(self, text, source):
        for line in text.splitlines():
            fields = strip(line).split()
            if not fields:
                continue
            try:
                addr = ipaddress.ip_address(fields[0])
            except ValueError:
                addr = None
            if addr is not None:
                if not sinkhole(addr):
                    continue
                fields = fields[1:]
            for field in fields:
                if field.startswith('*.'):
                    self.insert(field[2:], False, 'block', source)
                else:
                    self.insert(field, True, 'block', source)


def parse_owner(text):
    table = Table()
    table.add_allow_syntax(text, 'allow', 'owner', bangs=True)
    return table


def read(path, warnings):
    try:
        with open(path, encoding='utf-8', errors='replace') as f:
            return f.read()
    except OSError as e:
        warnings.append(f'{path}: {e.strerror}')
        return ''


class Rules:
    def __init__(self, directory=PROFILE):
        self.warnings = []
        self.lists = Table()
        self.lists.add_allow_syntax(read(os.path.join(directory, ALLOWLIST), self.warnings),
                                    'allow', 'allowlist')
        self.allow_rules = len(self.lists)
        self.lists.add_allow_syntax(read(os.path.join(directory, CARVE_OUTS), self.warnings),
                                    'block', 'carve-outs')
        imported = os.path.join(directory, IMPORTED)
        if os.path.isdir(imported):
            for name in sorted(os.listdir(imported)):
                if name.endswith('.txt'):
                    self.lists.add_block_list(read(os.path.join(imported, name), self.warnings),
                                              name[:-4])
        self.required = {n for n in map(strip, read(os.path.join(directory, REQUIRED),
                                                    self.warnings).splitlines()) if plain(n)}

    def decide(self, name, owner=None, learning=False):
        """(verdict, source) for `name`, as rules.rs's Rules::verdict."""
        if name in self.required:
            return 'block', 'required'
        found = (owner.find(name) if owner else None) or self.lists.find(name)
        if found:
            return found
        return ('learn' if learning else 'block'), 'default'

    def problems(self, owner=None):
        out = []
        if self.allow_rules == 0:
            out.append('allowlist.txt holds no allow rule')
        out += [f'{n} is not allowed ({self.decide(n, owner)[1]})'
                for n in MUST_RESOLVE if self.decide(n, owner)[0] != 'allow']
        out += [f'firmware host {n} is not blocked' for n in sorted(self.required)
                if self.decide(n, owner)[0] != 'block']
        return out


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n', 1)[0])
    parser.add_argument('names', nargs='*')
    parser.add_argument('--rules', default=PROFILE, help='default: profiles/guard')
    parser.add_argument('--owner', help="the owner's file, if any")
    parser.add_argument('--learning', action='store_true', help='as if a window were open')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args(argv)
    rules = Rules(args.rules)
    owner = None
    if args.owner:
        try:
            with open(args.owner, encoding='utf-8', errors='replace') as f:
                owner = parse_owner(f.read())
        except OSError as e:
            print(f'rules.py: {args.owner}: {e.strerror}', file=sys.stderr)
            return 2
    # imported/ may be absent; the three files may not.
    for w in rules.warnings:
        print(f'rules.py: {w}', file=sys.stderr)
    if args.check:
        problems = rules.problems(owner)
        for p in problems:
            print(f'FAIL guard rules: {p}')
        return 1 if problems or rules.warnings else 0
    for name in args.names:
        verdict, source = rules.decide(name.lower().rstrip('.'), owner, args.learning)
        print(f'{name} {verdict} {source}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
