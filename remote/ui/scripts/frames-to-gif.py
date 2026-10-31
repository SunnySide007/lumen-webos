# SPDX-License-Identifier: GPL-3.0-or-later
"""Turns the recording's screencast frames into a GIF.

    python scripts/frames-to-gif.py <capture dir> [fps] [width]

Reads <dir>/recording-frames/frames.json (written by e2e/capture.spec.ts) and
writes <dir>/recording-volume.gif: at most `fps` frames a second (15), scaled
to `width` pixels (720), each shown for as long as Chromium showed it. Needs
Pillow.
"""
import json
import sys
from pathlib import Path

from PIL import Image


def main() -> None:
    if not 2 <= len(sys.argv) <= 4:
        sys.exit(__doc__)
    base = Path(sys.argv[1])
    fps = float(sys.argv[2]) if len(sys.argv) > 2 else 15.0
    width = int(sys.argv[3]) if len(sys.argv) > 3 else 720
    src = base / "recording-frames"
    meta = json.loads((src / "frames.json").read_text())
    frames = meta["frames"]
    if not frames:
        sys.exit("no frames")

    # Keep a frame once 1/fps has passed since the last one kept; it stays on
    # screen until the next one kept.
    kept = []
    for f in frames:
        if not kept or f["t"] - kept[-1]["t"] >= 1 / fps:
            kept.append(f)
    times = [f["t"] for f in kept] + [meta["end"]]

    images, durations = [], []
    for i, f in enumerate(kept):
        img = Image.open(src / f["file"]).convert("RGB")
        img = img.resize((width, round(img.height * width / img.width)), Image.Resampling.LANCZOS)
        images.append(img.quantize(colors=200, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
        durations.append(max(20, round((times[i + 1] - times[i]) * 1000)))
    out = base / "recording-volume.gif"
    images[0].save(out, save_all=True, append_images=images[1:], duration=durations, loop=0, optimize=True)
    print(f"{out}: {len(images)} frames, {sum(durations) / 1000:.1f} s, {out.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
