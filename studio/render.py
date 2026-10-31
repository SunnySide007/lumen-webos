#!/usr/bin/env python3
"""Renders a looping scene to 16-bit (half float) linear EXR frames.

    tools/studio.sh python3 studio/render.py --size 1920x1080 --seconds 10
    tools/studio.sh python3 studio/render.py --size 3840x2160 --seconds 30

Writes frame_000000.exr ... and manifest.json into --out (by default
dist/studio/<scene>-<H>p/). The manifest gains a "render" entry, with timings,
only once every frame is done; studio/encode.py refuses it until then. Frames
already rendered with the same settings and scene code are kept, so an
interrupted render resumes where it stopped.
"""

import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import sys
import time

import aurora
import exr
import fireflies
import hdr
import pool
import scenekit

SCENES = {'aurora': aurora.Aurora, 'fireflies': fireflies.Fireflies}
PRIMARIES = {'aurora': ('P3_D65', aurora.PRIMARIES), 'fireflies': ('P3_D65', fireflies.PRIMARIES)}
WHITE_NITS = 100.0  # an EXR value of 1.0
PATTERN = 'frame_%06d.exr'
FRAME_FILE = re.compile(r'frame_\d{6}\.exr(\.partial)?')
# A rendering worker's memory, measured at about 80 bytes a pixel.
RENDER_BYTES_PER_PIXEL = 96


def parse_size(text):
    match = re.fullmatch(r'(\d+)x(\d+)', text)
    if not match:
        raise argparse.ArgumentTypeError(f'{text!r} is not WIDTHxHEIGHT')
    width, height = int(match[1]), int(match[2])
    if width < 16 or height < 16 or width % 2 or height % 2:
        raise argparse.ArgumentTypeError(f'{text}: 4:2:0 video needs even sizes of 16 or more')
    return width, height


def parse_args(argv):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--scene', choices=sorted(SCENES), default='aurora')
    parser.add_argument('--size', type=parse_size, default=(1920, 1080), help='WIDTHxHEIGHT (default 1920x1080)')
    parser.add_argument('--seconds', type=float, default=10.0,
                        help='loop length (default 10); longer loops move more slowly')
    parser.add_argument('--fps', type=int, default=24, help='frames per second (default 24)')
    parser.add_argument('--out', help='output directory (default dist/studio/<scene>-<H>p)')
    parser.add_argument('--workers', type=int,
                        help='frames rendered at once (default: CPUs, at most 8, as memory allows)')
    args = parser.parse_args(argv)
    args.frames = round(args.seconds * args.fps)
    if args.fps < 1 or args.frames < 2:
        parser.error('a loop needs at least two frames')
    if args.workers is not None and args.workers < 1:
        parser.error('--workers must be at least 1')
    if args.out is None:
        args.out = os.path.join('dist', 'studio', f'{args.scene}-{args.size[1]}p')
    return args


def scene_version(scene):
    """A hash of the scene's source and the modules it draws on (shared
    helpers, primaries), so frames from older code are not reused."""
    digest = hashlib.sha256()
    for path in (sys.modules[SCENES[scene].__module__].__file__, scenekit.__file__, hdr.__file__):
        with open(path, 'rb') as f:
            digest.update(f.read())
    return digest.hexdigest()[:12]


def _write_json(path, data):
    with open(f'{path}.partial', 'w') as f:
        json.dump(data, f, indent=2)
        f.write('\n')
    os.replace(f'{path}.partial', path)


def _prepare(out, manifest):
    """Readies `out` for the frames `manifest` describes; returns the earlier
    render's timings when its frames can all be kept.

    Frames are reused only if the same settings and scene code made them, and
    frame files are deleted only where a manifest shows render.py wrote them.
    """
    os.makedirs(out, exist_ok=True)
    manifest_path = os.path.join(out, 'manifest.json')
    frames = [name for name in os.listdir(out) if FRAME_FILE.fullmatch(name)]
    previous_render = None
    if os.path.exists(manifest_path):
        with open(manifest_path) as f:
            previous = json.load(f)
        previous_render = previous.pop('render', None)
        if previous != manifest:
            previous_render = None
            for name in frames:
                os.remove(os.path.join(out, name))
    elif frames:
        sys.exit(f'{out} holds frame files that render.py did not write (no manifest.json); '
                 'choose an empty directory')
    # Until the render finishes, the manifest has no "render" entry.
    _write_json(manifest_path, manifest)
    return previous_render


_scene = None


def _start_worker(scene, width, height, frames, fps):
    global _scene
    _scene = SCENES[scene](width, height, frames, fps)


def _render_frame(job):
    index, path, primaries = job
    started = time.monotonic()
    exr.write(path, _scene.render(index), primaries, WHITE_NITS)
    return time.monotonic() - started


def main(argv=None):
    args = parse_args(sys.argv[1:] if argv is None else argv)
    width, height = args.size
    primaries_name, primaries = PRIMARIES[args.scene]
    manifest = {
        'scene': args.scene, 'scene_version': scene_version(args.scene), 'width': width, 'height': height,
        'fps': args.fps, 'frames': args.frames, 'pattern': PATTERN, 'primaries': primaries_name,
        'white_nits': WHITE_NITS,
    }
    previous_render = _prepare(args.out, manifest)
    jobs = [(i, os.path.join(args.out, PATTERN % i), primaries) for i in range(args.frames)
            if not os.path.exists(os.path.join(args.out, PATTERN % i))]
    workers = args.workers or pool.default_workers(RENDER_BYTES_PER_PIXEL * width * height)
    print(f'{args.scene}: {args.frames} frames at {width}x{height}, {args.fps} fps '
          f'({args.frames - len(jobs)} already done), {workers} workers -> {args.out}', file=sys.stderr)

    started = time.monotonic()
    worker_seconds = 0.0
    if jobs:
        init = (args.scene, width, height, args.frames, args.fps)
        with pool.executor(workers, _start_worker, init) as renderers:
            try:
                futures = [renderers.submit(_render_frame, job) for job in jobs]
                for done, future in enumerate(concurrent.futures.as_completed(futures), 1):
                    worker_seconds += future.result()
                    if done == len(jobs) or done % max(1, len(jobs) // 20) == 0:
                        print(f'  {done}/{len(jobs)} frames, {time.monotonic() - started:.0f} s', file=sys.stderr)
            except BaseException:
                renderers.shutdown(wait=True, cancel_futures=True)
                raise

    wall = time.monotonic() - started
    if jobs or previous_render is None:
        manifest['render'] = {'wall_seconds': round(wall, 1), 'workers': workers, 'frames_rendered': len(jobs),
                              'seconds_per_frame': round(worker_seconds / len(jobs), 2) if jobs else None}
    else:
        manifest['render'] = previous_render  # nothing was left to do
    _write_json(os.path.join(args.out, 'manifest.json'), manifest)
    print(f'rendered {len(jobs)} frames in {wall:.1f} s', file=sys.stderr)


if __name__ == '__main__':
    main()
