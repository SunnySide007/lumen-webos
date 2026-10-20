#!/usr/bin/env python3
"""Checks a loop from studio/encode.py the way the TV will meet it.

    tools/studio.sh python3 studio/verify.py dist/studio/aurora-1080p.mp4

* ffprobe: HEVC Main 10, yuv420p10le, BT.2020 / PQ / BT.2020 NCL, narrow
  range, and the size, rate and length the encode recorded in <video>.json.
* Every keyframe carries the HDR10 SEI (mastering display and content light
  level) with the values the encode measured.
* Seamless loop: decoded, the step from the last frame to the first is no
  bigger than an ordinary step between frames.
* Light level: no decoded luma is brighter than the MaxCLL the stream claims,
  a check that does not trust encode.py's own measurement.
* Tone-mapped SDR previews (PNG), by default in dist/studio/previews/, to
  judge the look on any screen. They are not what the TV shows.

Exits non-zero if any check fails.
"""

import argparse
import json
import os
import statistics
import subprocess
import sys
from collections import namedtuple

import numpy as np

import checks

# PQ to linear light (1.0 = 100 cd/m²), BT.709 primaries, Hable's curve, sRGB.
TONEMAP = ('zscale=transfer=linear:npl=100,format=gbrpf32le,zscale=primaries=bt709,'
           'tonemap=tonemap=hable:desat=0,zscale=transfer=iec61966-2-1,format=rgb24')


RECORD_FIELDS = ('width', 'height', 'fps', 'frames', 'max_cll', 'max_fall', 'master_display')


class Unreadable(Exception):
    """ffprobe or ffmpeg could not read the video."""


def _probe(video, *options):
    try:
        out = subprocess.run(['ffprobe', '-v', 'error', '-select_streams', 'v:0', *options, '-of', 'json', video],
                             check=True, capture_output=True, text=True).stdout
    except subprocess.CalledProcessError as e:
        raise Unreadable(f'ffprobe could not read it: {(e.stderr or "").strip() or e}') from None
    return json.loads(out)


def _load_record(video):
    """The <video>.json encode.py wrote, refusing a missing or incomplete one."""
    record = video + '.json'
    if not os.path.exists(record):
        raise FileNotFoundError(f'{record} is missing: verify checks videos from studio/encode.py')
    try:
        with open(record) as f:
            expect = json.load(f)
    except json.JSONDecodeError as e:
        raise ValueError(f'{record} is not JSON ({e}); encode the video again') from None
    missing = [field for field in RECORD_FIELDS if field not in expect]
    if missing:
        raise ValueError(f'{record} lacks {", ".join(missing)}; encode the video again')
    return expect


def luma_frames(video, width, height):
    """Decodes the video's luma plane, frame by frame, as (H, W) uint16 code values."""
    size = width * height * 2
    cmd = ['ffmpeg', '-v', 'error', '-i', video, '-map', '0:v:0', '-vf', 'extractplanes=y',
           '-f', 'rawvideo', '-pix_fmt', 'gray10le', '-']
    with subprocess.Popen(cmd, stdout=subprocess.PIPE) as ffmpeg:
        while True:
            data = ffmpeg.stdout.read(size)
            if len(data) < size:
                break
            yield np.frombuffer(data, dtype='<u2').reshape(height, width)
    if ffmpeg.returncode:
        raise Unreadable('ffmpeg could not decode it')


LoopStats = namedtuple('LoopStats', 'seam steps count peak')


def loop_measure(frames):
    """Mean absolute differences between consecutive frames (steps) and from
    the last frame back to the first (seam), with the frame count and the
    brightest code value seen (peak)."""
    first = previous = None
    steps = []
    count = peak = 0
    for frame in frames:
        peak = max(peak, int(frame.max()))
        frame = frame.astype(np.float32)
        if previous is None:
            first = frame
        else:
            steps.append(float(np.abs(frame - previous).mean()))
        previous = frame
        count += 1
    seam = float(np.abs(first - previous).mean()) if count else 0.0
    return LoopStats(seam, steps, count, peak)


def write_previews(video, out_dir, count, seconds):
    os.makedirs(out_dir, exist_ok=True)
    stem = os.path.splitext(os.path.basename(video))[0]
    paths = []
    for k in range(count):
        path = os.path.join(out_dir, f'{stem}-{k}.png')
        at = (k + 0.5) * seconds / count
        subprocess.run(['ffmpeg', '-v', 'error', '-y', '-ss', f'{at:.3f}', '-i', video, '-frames:v', '1',
                        '-vf', TONEMAP, path], check=True, capture_output=True, text=True)
        paths.append(path)
    return paths


def run(video, previews_dir, count=4):
    """Returns (report lines, problems) for one video.

    Raises FileNotFoundError or ValueError when its encode record is missing
    or incomplete; a video ffmpeg cannot read is reported as a failed check.
    """
    expect = _load_record(video)
    lines, problems = [], []

    def check(name, found, detail):
        lines.append(f'{"FAIL" if found else "PASS"} {name}: {detail}')
        lines.extend(f'     {p}' for p in found)
        problems.extend(found)

    seconds = expect['frames'] / expect['fps']
    lines.append(f'{os.path.basename(video)}: {expect["width"]}x{expect["height"]}, {expect["fps"]} fps, '
                 f'{expect["frames"]} frames ({seconds:g} s), {os.path.getsize(video) / 1e6:.1f} MB')

    try:
        _check_video(video, expect, check)
    except Unreadable as e:
        check('readable', [str(e)], 'ffprobe and ffmpeg')
        return lines, problems

    if previews_dir:
        try:
            for path in write_previews(video, previews_dir, count, seconds):
                lines.append(f'preview {path}')
        except subprocess.CalledProcessError as e:
            check('previews', [f'ffmpeg could not write them: {(e.stderr or "").strip() or e}'], previews_dir)
    return lines, problems


def _check_video(video, expect, check):
    """The stream, HDR10 metadata, loop and light checks; raises Unreadable."""
    streams = _probe(video, '-show_streams')['streams']
    if not streams:
        raise Unreadable('it has no video stream')
    stream = streams[0]
    check('stream', checks.stream_problems(stream, expect),
          f'{stream.get("codec_name")} {stream.get("profile")}, {stream.get("pix_fmt")}, '
          f'{stream.get("color_primaries")} / {stream.get("color_transfer")} / {stream.get("color_space")}, '
          f'{stream.get("color_range")} range, chroma {stream.get("chroma_location")}')

    keyframes = _probe(video, '-skip_frame', 'nokey', '-show_frames',
                       '-show_entries', 'frame=pts_time,side_data_list')['frames']
    check('HDR10 metadata', checks.keyframe_problems(keyframes, expect),
          f'on {len(keyframes)} keyframes: mastering display {expect["master_display"]}, '
          f'MaxCLL {expect["max_cll"]}, MaxFALL {expect["max_fall"]} cd/m²')

    stats = loop_measure(luma_frames(video, expect['width'], expect['height']))
    found = checks.loop_problems(stats.seam, stats.steps)
    if stats.count != expect['frames']:
        found.append(f'decoded {stats.count} frames, the encode wrote {expect["frames"]}')
    median = statistics.median(stats.steps) if stats.steps else 0.0
    check('seamless loop', found, f'last to first frame {stats.seam:.2f} code values, consecutive '
          f'frames {median:.2f} (median), max {max(stats.steps, default=0.0):.2f}')
    check('light level', checks.light_problems(stats.peak, expect['max_cll']),
          f'brightest decoded luma code {stats.peak}; MaxCLL {expect["max_cll"]} cd/m²')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('video', help='a video written by studio/encode.py')
    parser.add_argument('--previews', default=os.path.join('dist', 'studio', 'previews'),
                        help='where SDR previews go (default dist/studio/previews); "" for none')
    parser.add_argument('--count', type=int, default=4, help='how many previews (default 4)')
    args = parser.parse_args(sys.argv[1:] if argv is None else argv)
    try:
        lines, problems = run(args.video, args.previews, args.count)
    except (FileNotFoundError, ValueError) as e:
        sys.exit(str(e))
    print('\n'.join(lines))
    if problems:
        sys.exit(f'{len(problems)} problem(s)')


if __name__ == '__main__':
    main()
