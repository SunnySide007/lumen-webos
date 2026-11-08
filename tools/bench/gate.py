#!/usr/bin/env python3
"""The bench's numbers and its speed gate (docs/device/bench.md).

    gate.py params JSON [JSON ...]          merge launch parameters, later wins
    gate.py run --stats F --log F [--play F] [--rss KB] [--cpu PCT]
                                            one run's numbers, as JSON
    gate.py combine RUN.json [RUN.json ...] the median of the runs, as JSON
    gate.py check SUMMARY.json BASELINE.json
                                            the gate: a table, exit 0 if
                                            every limit holds, 1 if not

`run` reads tools/frame-stats.sh's text (its bursts and key presses
sections) for the frames, and the run's own log lines (from its launch on)
for the start-up. It never computes frame times itself, so the numbers
always match what frame-stats.sh prints.

The limits are the handoff's (2026-11-08), to tune once the noise is known:
average frame at most the baseline + 0.3 ms and at most 17.2 ms; vsyncs
skipped per 10 s at most max(12, 1.5 x the baseline's); start-up at most
600 ms; RSS at most the baseline + 5 MB.
"""

import json
import re
import statistics
import sys

AVG_OVER_BASE_MS = 0.3
AVG_CEILING_MS = 17.2
SKIP_FLOOR = 12.0
SKIP_FACTOR = 1.5
STARTUP_MS = 600.0
RSS_OVER_BASE_KB = 5 * 1024

NUMBER = r'([0-9]+(?:\.[0-9]+)?)'


def section(text, title):
    """The lines of frame-stats.sh's section starting with `title`."""
    out, inside = [], False
    for line in text.splitlines():
        if line.startswith(title):
            inside = True
            out.append(line)
        elif inside and line.startswith('  '):
            out.append(line)
        elif inside:
            break
    return out


def find(lines, pattern):
    for line in lines:
        m = re.search(pattern, line)
        if m:
            return m
    return None


def frames_from_stats(text):
    """The bursts and key presses numbers frame-stats.sh printed."""
    out = {}
    bursts = section(text, 'bursts (')
    head = find(bursts, r'^bursts \(drew \.\.\. idle\): ([0-9]+), ([0-9]+) frames, ' + NUMBER + ' s')
    if head:
        out['bursts'] = int(head.group(1))
        out['frames'] = int(head.group(2))
        out['motion_s'] = float(head.group(3))
    m = find(bursts, r'average frame +' + NUMBER + ' ms')
    if m:
        out['avg_frame_ms'] = float(m.group(1))
    m = find(bursts, r'worst frame +' + NUMBER + ' ms')
    if m:
        out['worst_frame_ms'] = float(m.group(1))
    m = find(bursts, r'missed vsync +([0-9]+) of ([0-9]+) bursts')
    if m:
        out['bursts_with_miss'] = int(m.group(1))
    m = find(bursts, r'per 10 s +' + NUMBER)
    if m:
        out['skipped_per_10s'] = float(m.group(1))
    m = find(bursts, r'vsyncs skipped +([0-9]+) in all')
    if m:
        out['skipped'] = int(m.group(1))
    keys = section(text, 'key presses (')
    m = find(keys, r'shown after +p50 ' + NUMBER + r' +p95 ' + NUMBER + r' +max ' + NUMBER)
    if m:
        out['key_p50_ms'] = float(m.group(1))
        out['key_p95_ms'] = float(m.group(2))
        out['key_max_ms'] = float(m.group(3))
    return out


def startup_from_log(text):
    """The first `start:` line's first frame, in ms, and its platform."""
    for line in text.splitlines():
        m = re.search(r'\] start: (sdl|wayland) .*first frame ([0-9]+) ms', line)
        if m:
            return {'platform': m.group(1), 'startup_ms': float(m.group(2))}
    return {}


def keys_in_log(text):
    downs = [line for line in text.splitlines() if '] key down scancode ' in line]
    return {'key_downs': len(downs)}


def run_numbers(stats_text, log_text, play_lines=(), rss_kb=None, cpu_pct=None):
    out = {}
    out.update(frames_from_stats(stats_text))
    out.update(startup_from_log(log_text))
    out.update(keys_in_log(log_text))
    for line in play_lines:
        line = line.strip()
        if not line.startswith('{'):
            continue
        try:
            said = json.loads(line)
        except ValueError:
            continue
        if 'done' in said:
            out['robot_presses'] = said['done']
            out['robot_late_ms'] = said.get('late_ms')
            out['robot_echoes'] = said.get('echoes')
        if 'yield' in said:
            out['yielded'] = said['yield']
    if rss_kb is not None:
        out['rss_kb'] = rss_kb
    if cpu_pct is not None:
        out['idle_cpu_pct'] = cpu_pct
    return out


def combine(runs):
    """The median of each number over the runs that have it."""
    keys = sorted({k for r in runs for k, v in r.items() if isinstance(v, (int, float))
                   and not isinstance(v, bool)})
    out = {'runs': len(runs)}
    for k in keys:
        values = [r[k] for r in runs if isinstance(r.get(k), (int, float))
                  and not isinstance(r.get(k), bool)]
        if values:
            out[k] = statistics.median(values)
            out[k + '_all'] = values
    platforms = sorted({r['platform'] for r in runs if 'platform' in r})
    if platforms:
        out['platform'] = ','.join(platforms)
    if any('yielded' in r for r in runs):
        out['yielded'] = True
    return out


def check(summary, baseline):
    """The gate's rows: (name, value, limit, passed); a number either side
    lacks is a row that fails, so nothing passes unmeasured."""
    rows = []

    def row(name, value, limit, ok):
        rows.append((name, value, limit, bool(ok) if value is not None and limit is not None
                     else False))

    avg, base_avg = summary.get('avg_frame_ms'), baseline.get('avg_frame_ms')
    limit = None if base_avg is None else min(base_avg + AVG_OVER_BASE_MS, AVG_CEILING_MS)
    row('average frame (ms)', avg, limit, avg is not None and limit is not None and avg <= limit)
    skip, base_skip = summary.get('skipped_per_10s'), baseline.get('skipped_per_10s')
    limit = None if base_skip is None else max(SKIP_FLOOR, SKIP_FACTOR * base_skip)
    row('vsyncs skipped per 10 s', skip, limit,
        skip is not None and limit is not None and skip <= limit)
    start = summary.get('startup_ms')
    row('start-up (ms)', start, STARTUP_MS, start is not None and start <= STARTUP_MS)
    if 'rss_kb' in summary or 'rss_kb' in baseline:
        rss, base_rss = summary.get('rss_kb'), baseline.get('rss_kb')
        limit = None if base_rss is None else base_rss + RSS_OVER_BASE_KB
        row('RSS after 5 min (kB)', rss, limit,
            rss is not None and limit is not None and rss <= limit)
    if summary.get('yielded'):
        rows.append(('yielded to a person', 'yes', 'no', False))
    return rows


def table(rows):
    lines = ['| Limit | Value | Allowed | |', '|---|---|---|---|']
    for name, value, limit, ok in rows:
        def show(v):
            if v is None:
                return 'not measured'
            return f'{v:.2f}' if isinstance(v, float) else str(v)
        lines.append(f'| {name} | {show(value)} | {show(limit)} | {"pass" if ok else "FAIL"} |')
    return '\n'.join(lines)


def read(path):
    with open(path, encoding='utf-8', errors='replace') as f:
        return f.read()


def main(argv):
    if not argv:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    cmd, rest = argv[0], argv[1:]
    if cmd == 'params':
        merged = {}
        for text in rest:
            said = json.loads(text) if text.strip() else {}
            if not isinstance(said, dict):
                print('gate.py: launch parameters must be JSON objects', file=sys.stderr)
                return 2
            merged.update(said)
        print(json.dumps(merged, separators=(',', ':')))
        return 0
    if cmd == 'run':
        opts = {'--stats': None, '--log': None, '--play': None, '--rss': None, '--cpu': None}
        it = iter(rest)
        for flag in it:
            if flag not in opts:
                print(f'gate.py: unknown flag {flag}', file=sys.stderr)
                return 2
            opts[flag] = next(it, None)
        if not opts['--stats'] or not opts['--log']:
            print('gate.py run needs --stats and --log', file=sys.stderr)
            return 2
        play = read(opts['--play']).splitlines() if opts['--play'] else []
        rss = int(opts['--rss']) if opts['--rss'] else None
        cpu = float(opts['--cpu']) if opts['--cpu'] else None
        print(json.dumps(run_numbers(read(opts['--stats']), read(opts['--log']), play, rss, cpu),
                         indent=2, sort_keys=True))
        return 0
    if cmd == 'combine':
        print(json.dumps(combine([json.loads(read(p)) for p in rest]), indent=2, sort_keys=True))
        return 0
    if cmd == 'check' and len(rest) == 2:
        rows = check(json.loads(read(rest[0])), json.loads(read(rest[1])))
        print(table(rows))
        return 0 if all(ok for *_, ok in rows) else 1
    print(__doc__.strip(), file=sys.stderr)
    return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
