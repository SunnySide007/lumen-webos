#!/usr/bin/env python3
"""The bench's "coming back in front" numbers (docs/device/bench.md).

    front.py cut PRESS LOG          one Home press back to Lumen, as JSON
    front.py combine RUN.json ...   the median of the runs, as JSON

`cut` reads the shell's own log from a robot Home press (PRESS, epoch
seconds on the TV's clock) with another app in front. Its window runs from
the shell's `lifecycle: relaunch` line to 2 s after the first `crossfade …
to <hero>` line, the end of the fade to the hero. From that window it
takes the shell's own lines and computes no frame time itself:

* every `drew N frames` line; the fade's is the last one at or before the
  crossfade line;
* every `slow frame` line with its loop, batch, upload, submit and swap
  split;
* the hero's decode and upload times;
* the press to the first frame's swap (target 5).

A window with no crossfade line says so (`"complete": false`) and gives
what it found up to the log's end.
"""

import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import gate  # noqa: E402

AFTER_FADE_S = 2.0

N = r'([0-9]+(?:\.[0-9]+)?)'
STAMPED = re.compile(r'\[([0-9]+\.[0-9]+)\] (.*)')
DREW = re.compile(r'drew ([0-9]+) frames in ([0-9]+) ms \(avg ' + N + r' ms, worst ' + N +
                  r' ms, ([0-9]+) quads\);.*?([0-9]+) vsyncs skipped')
SLOW = re.compile(r'slow frame ([0-9]+): ' + N + r' ms = loop ' + N + r' \+ batch ' + N +
                  r' \+ upload ' + N + r' \+ submit ' + N + r' \+ swap ' + N + r' ms')
HERO = re.compile(r'hero (\S+): .*decoded in ' + N + r' ms, uploaded in ' + N + r' ms')
FADE = re.compile(r'crossfade (\S+) to (\S+): ([0-9]+) frames')
FIRST = re.compile(r'(?:foreground|relaunch): first frame after ([0-9]+) ms')


def cut(text, press):
    """One Home press's numbers from the log `text`."""
    lines = []
    for raw in text.splitlines():
        m = STAMPED.match(raw)
        if m and float(m.group(1)) >= press:
            lines.append((float(m.group(1)), m.group(2)))
    start = next((t for t, said in lines if said.startswith('lifecycle: relaunch')), None)
    if start is None:
        return {'complete': False, 'error': 'no relaunch after the press'}
    lines = [(t, said) for t, said in lines if t >= start]
    fade_at = next((t for t, said in lines if FADE.match(said)), None)
    out = {'complete': fade_at is not None, 'relaunch_at': start}
    if fade_at is not None:
        lines = [(t, said) for t, said in lines if t <= fade_at + AFTER_FADE_S]
        out['window_ms'] = round((fade_at + AFTER_FADE_S - start) * 1000)

    drew, slow = [], []
    for t, said in lines:
        m = FIRST.match(said)
        if m and 'target5_ms' not in out:
            out['target5_ms'] = round((t - press) * 1000)
            out['first_frame_ms'] = int(m.group(1))
        m = DREW.match(said)
        if m:
            drew.append({'at': t, 'frames': int(m.group(1)), 'ms': int(m.group(2)),
                         'avg_ms': float(m.group(3)), 'worst_ms': float(m.group(4)),
                         'quads': int(m.group(5)), 'skipped': int(m.group(6)), 'line': said})
        m = SLOW.match(said)
        if m:
            slow.append({'frame': int(m.group(1)), 'ms': float(m.group(2)),
                         'loop_ms': float(m.group(3)), 'batch_ms': float(m.group(4)),
                         'upload_ms': float(m.group(5)), 'submit_ms': float(m.group(6)),
                         'swap_ms': float(m.group(7))})
        m = HERO.match(said)
        if m and 'hero' not in out:
            out['hero'] = m.group(1)
            out['hero_decode_ms'] = float(m.group(2))
            out['hero_upload_ms'] = float(m.group(3))
        m = FADE.match(said)
        if m and 'fade' not in out:
            out['fade'] = f'{m.group(1)} to {m.group(2)}'
            out['crossfade_frames'] = int(m.group(3))

    out['drew'] = drew
    out['slow'] = slow
    out['frames'] = sum(d['frames'] for d in drew)
    out['skipped'] = sum(d['skipped'] for d in drew)
    out['slow_frames'] = len(slow)
    if slow:
        out['worst_slow_ms'] = max(s['ms'] for s in slow)
        out['worst_submit_ms'] = max(s['submit_ms'] for s in slow)
    if fade_at is not None:
        fades = [d for d in drew if d['at'] <= fade_at]
        if fades:
            fade = fades[-1]
            out['fade_frames'] = fade['frames']
            out['fade_ms'] = fade['ms']
            out['fade_avg_ms'] = fade['avg_ms']
            out['fade_worst_ms'] = fade['worst_ms']
            out['fade_skipped'] = fade['skipped']
    return out


def main(argv):
    if len(argv) == 3 and argv[0] == 'cut':
        with open(argv[2], encoding='utf-8', errors='replace') as f:
            print(json.dumps(cut(f.read(), float(argv[1])), indent=2, sort_keys=True))
        return 0
    if len(argv) >= 2 and argv[0] == 'combine':
        runs = [json.loads(gate.read(p)) for p in argv[1:]]
        out = gate.combine(runs)
        out['complete'] = all(r.get('complete') for r in runs)
        print(json.dumps(out, indent=2, sort_keys=True))
        return 0
    print(__doc__.strip(), file=sys.stderr)
    return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
