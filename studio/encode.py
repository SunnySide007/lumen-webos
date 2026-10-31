#!/usr/bin/env python3
"""Encodes rendered EXR frames as an HDR10 video: HEVC Main10, BT.2020 / PQ.

    tools/studio.sh python3 studio/encode.py dist/studio/aurora-1080p

Reads the frames twice. The first pass measures MaxCLL and MaxFALL, which go
into the stream's content light level SEI. The second grades each frame to
BT.2020 primaries and the PQ curve as 10-bit narrow-range Y'CbCr 4:2:0 and
pipes it to x265. The mastering display is a P3-D65 monitor of 1000 cd/m²,
the usual HDR10 grading target, which holds the scenes' P3 colours.

Writes <frames>.mp4 (or --out, .mp4 or .mkv) and, beside it, <video>.json with
the metadata, settings and timings that studio/verify.py checks against.
"""

import argparse
import json
import os
import subprocess
import sys
import threading
import time

import numpy as np

import exr
import hdr
import pool

MASTER_PRIMARIES = hdr.P3_D65
MASTER_PEAK_NITS = 1000.0
MASTER_BLACK_NITS = 0.0001


def to_bt2020(rgb, primaries):
    """Linear light in cd/m² from `primaries` to BT.2020, clipped to PQ's range."""
    matrix = np.asarray(hdr.conversion_matrix(primaries, hdr.BT2020), dtype=np.float32)
    return np.clip(rgb @ matrix.T, 0.0, hdr.PQ_PEAK)


def light_levels(rgb2020):
    """(max, mean) over the frame of max(R, G, B) in cd/m², for CTA-861.3."""
    brightest = rgb2020.max(axis=2)
    return float(brightest.max()), float(brightest.mean(dtype=np.float64))


def downsample(plane):
    """4:2:0 chroma sited left (HEVC chroma location 0).

    Each chroma sample sits on an even luma column, halfway between two rows:
    columns are filtered [1, 2, 1] / 4 around it, rows averaged in pairs. The
    border column repeats past the left edge.
    """
    rows = 0.5 * (plane[0::2] + plane[1::2])
    left = np.concatenate([rows[:, :1], rows[:, 1:-1:2]], axis=1)
    return 0.25 * left + 0.5 * rows[:, 0::2] + 0.25 * rows[:, 1::2]


def yuv420_codes(rgb2020, bits=10):
    """BT.2020 linear cd/m² to PQ Y'CbCr 4:2:0 narrow-range planes (uint16)."""
    signal = hdr.pq_from_linear(np.clip(rgb2020 / hdr.PQ_PEAK, 0.0, 1.0))
    y, cb, cr = hdr.ycbcr_from_rgb(signal[..., 0], signal[..., 1], signal[..., 2])
    r = hdr.narrow_range(bits)

    def codes(values, offset, scale, high):
        return np.clip(np.floor(offset + scale * values + 0.5), r.low, high).astype('<u2')

    return (codes(y, r.black, r.luma_scale, r.luma_high),
            codes(downsample(cb), r.chroma_mid, r.chroma_scale, r.chroma_high),
            codes(downsample(cr), r.chroma_mid, r.chroma_scale, r.chroma_high))


def x265_extra(width, height, fps):
    """x265 settings beyond HDR10: closed two-second GOPs and HEVC level limits.

    Closed GOPs let a player start, or loop, cleanly at any keyframe. The
    bitrate caps keep the stream inside Main tier level 4.1 (HD) or 5.1 (UHD),
    the levels HD and 4K players are built for (the B9 is still to be tried).
    aq-mode 3 spends bits on dark areas, where these scenes live.
    """
    uhd = width > 1920 or height > 1080
    cap = 40000 if uhd else 20000  # kbit/s
    return {'keyint': 2 * fps, 'min-keyint': fps, 'open-gop': 0, 'aq-mode': 3,
            'level-idc': '5.1' if uhd else '4.1', 'vbv-maxrate': cap, 'vbv-bufsize': cap,
            'log-level': 'error'}


def ffmpeg_command(out, width, height, fps, params, crf, preset):
    container = ['-movflags', '+faststart'] if out.endswith('.mp4') else []
    return ['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y',
            '-f', 'rawvideo', '-pix_fmt', 'yuv420p10le', '-s', f'{width}x{height}', '-r', str(fps), '-i', '-',
            '-c:v', 'libx265', '-preset', preset, '-crf', str(crf), '-x265-params', params,
            '-color_primaries', 'bt2020', '-color_trc', 'smpte2084', '-colorspace', 'bt2020nc',
            '-color_range', 'tv', '-chroma_sample_location', 'left', '-an', *container, out]


# Rows converted at once. A whole 4K frame in float32 temporaries costs a few
# hundred MB per worker, too much beside x265 in Docker's default 8 GB.
BAND = 256
# A grading worker's memory: the frame as read (half and float32) plus a band.
GRADE_BYTES_PER_PIXEL = 32
# x265's own memory at preset slow, about 2 GB for 4K, kept free of workers.
X265_BYTES_PER_PIXEL = 240


def _bands(height, band):
    if band % 2:
        raise ValueError('bands must hold whole pairs of rows, for 4:2:0 chroma')
    return range(0, height, band)


def measure(rgb, primaries, band=BAND):
    """light_levels of the frame in BT.2020, converted a band of rows at a time."""
    peak = total = 0.0
    for top in _bands(rgb.shape[0], band):
        part = to_bt2020(rgb[top:top + band], primaries)
        part_peak, part_mean = light_levels(part)
        peak = max(peak, part_peak)
        total += part_mean * part.shape[0] * part.shape[1]
    return peak, total / (rgb.shape[0] * rgb.shape[1])


def grade(rgb, primaries, band=BAND):
    """yuv420_codes of the frame in BT.2020, converted a band of rows at a time."""
    height, width = rgb.shape[:2]
    y = np.empty((height, width), dtype='<u2')
    cb = np.empty((height // 2, width // 2), dtype='<u2')
    cr = np.empty_like(cb)
    for top in _bands(height, band):
        y[top:top + band], cb[top // 2:(top + band) // 2], cr[top // 2:(top + band) // 2] = \
            yuv420_codes(to_bt2020(rgb[top:top + band], primaries))
    return y, cb, cr


def _measure(path):
    rgb, primaries, _ = exr.read(path)
    return measure(rgb, primaries)


def _grade(path):
    rgb, primaries, _ = exr.read(path)
    return grade(rgb, primaries)


def _drop_numa_noise(stream):
    # x265 asks for NUMA memory placement, which Docker's seccomp profile
    # refuses; libnuma reports each refusal. Pass everything else through.
    for line in stream:
        if b'set_mempolicy: Operation not permitted' not in line:
            sys.stderr.write(line.decode(errors='replace'))
            sys.stderr.flush()


def parse_args(argv):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('frames', help='a directory written by studio/render.py')
    parser.add_argument('--out', help='output video, .mp4 or .mkv (default <frames>.mp4)')
    parser.add_argument('--crf', type=float, default=16.0, help='x265 quality, lower is better (default 16)')
    parser.add_argument('--preset', default='slow', help='x265 preset (default slow)')
    parser.add_argument('--workers', type=int,
                        help='frames read and graded at once (default: CPUs, at most 8, as memory allows)')
    args = parser.parse_args(argv)
    if args.out is None:
        args.out = os.path.normpath(args.frames) + '.mp4'
    if not args.out.endswith(('.mp4', '.mkv')):
        parser.error('--out must end in .mp4 or .mkv')
    if args.workers is not None and args.workers < 1:
        parser.error('--workers must be at least 1')
    return args


def _load_frames(directory):
    """The render's manifest and frame paths, refusing an unfinished render."""
    with open(os.path.join(directory, 'manifest.json')) as f:
        manifest = json.load(f)
    if 'render' not in manifest:
        sys.exit(f'{directory}: the render has not finished; run studio/render.py again')
    paths = [os.path.join(directory, manifest['pattern'] % i) for i in range(manifest['frames'])]
    missing = [p for p in paths if not os.path.exists(p)]
    if missing:
        sys.exit(f'{len(missing)} frames are missing, the first {missing[0]}')
    return manifest, paths


def _progress(done, count, started):
    if done == count or done % max(1, count // 20) == 0:
        print(f'  {done}/{count} frames, {time.monotonic() - started:.0f} s', file=sys.stderr)


def _write_json(path, data):
    with open(path, 'w') as f:
        json.dump(data, f, indent=2)
        f.write('\n')


def partial_path(out):
    """Where ffmpeg writes `out` until it is finished; keeps the extension,
    from which ffmpeg picks the container."""
    base, ext = os.path.splitext(out)
    return f'{base}.partial{ext}'


def _encode(executor, window, paths, out, cmd, describe):
    """Pipes the graded frames to `cmd`, an ffmpeg writing partial_path(out),
    and returns the record describe(size, seconds) makes for the video.

    The video and its record (<out>.json) replace any earlier pair only once
    ffmpeg has finished; on any failure ffmpeg is stopped, the partial file
    removed, and an earlier video and record are left as they were.
    """
    partial = partial_path(out)
    record, partial_record = f'{out}.json', f'{out}.json.partial'
    started = time.monotonic()
    ffmpeg = subprocess.Popen(cmd, stdin=subprocess.PIPE, stderr=subprocess.PIPE)
    noise = threading.Thread(target=_drop_numa_noise, args=(ffmpeg.stderr,), daemon=True)
    noise.start()
    finished = False
    try:
        try:
            for done, planes in enumerate(pool.ordered(executor, _grade, paths, window), 1):
                for plane in planes:
                    ffmpeg.stdin.write(plane.tobytes())
                _progress(done, len(paths), started)
            ffmpeg.stdin.close()
        except BrokenPipeError:
            pass  # ffmpeg stopped early; its exit status and messages explain why
        if ffmpeg.wait():
            sys.exit(f'ffmpeg failed with status {ffmpeg.returncode}')
        info = describe(os.path.getsize(partial), time.monotonic() - started)
        _write_json(partial_record, info)
        os.replace(partial, out)
        os.replace(partial_record, record)
        finished = True
    finally:
        if ffmpeg.poll() is None:
            ffmpeg.kill()
            ffmpeg.wait()
        noise.join()
        ffmpeg.stderr.close()
        if not ffmpeg.stdin.closed:
            try:
                ffmpeg.stdin.close()
            except BrokenPipeError:
                pass
        if not finished:
            for name in (partial, partial_record):
                if os.path.exists(name):
                    os.remove(name)
    return info


def main(argv=None):
    args = parse_args(sys.argv[1:] if argv is None else argv)
    manifest, paths = _load_frames(args.frames)
    width, height, fps, count = manifest['width'], manifest['height'], manifest['fps'], manifest['frames']
    workers = args.workers or pool.default_workers(GRADE_BYTES_PER_PIXEL * width * height,
                                                   reserve=X265_BYTES_PER_PIXEL * width * height)
    with pool.executor(workers) as frames:
        try:
            started = time.monotonic()
            max_cll, max_fall = hdr.content_light_level(pool.ordered(frames, _measure, paths, 2 * workers))
            measured = time.monotonic() - started
            print(f'{count} frames: MaxCLL {max_cll}, MaxFALL {max_fall} cd/m² ({measured:.1f} s)',
                  file=sys.stderr)
            master = hdr.master_display(MASTER_PRIMARIES, MASTER_PEAK_NITS, MASTER_BLACK_NITS)
            params = hdr.x265_params(max_cll, max_fall, master, x265_extra(width, height, fps))
            cmd = ffmpeg_command(partial_path(args.out), width, height, fps, params, args.crf, args.preset)

            def describe(size, encoded):
                return {
                    'video': os.path.basename(args.out), 'scene': manifest['scene'], 'width': width,
                    'height': height, 'fps': fps, 'frames': count, 'max_cll': max_cll, 'max_fall': max_fall,
                    'master_display': master, 'x265_params': params, 'crf': args.crf, 'preset': args.preset,
                    'bytes': size, 'mbit_per_second': round(size * 8 / (count / fps) / 1e6, 2),
                    'workers': workers,
                    'seconds': {'render': manifest['render']['wall_seconds'], 'measure': round(measured, 1),
                                'encode': round(encoded, 1)},
                }

            info = _encode(frames, 2 * workers, paths, args.out, cmd, describe)
        except BaseException:
            frames.shutdown(wait=True, cancel_futures=True)
            raise
    print(f'{args.out}: {info["bytes"] / 1e6:.1f} MB, {info["mbit_per_second"]} Mbit/s, '
          f'encoded in {info["seconds"]["encode"]} s', file=sys.stderr)


if __name__ == '__main__':
    main()
