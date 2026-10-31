# studio: HDR content pipeline (Phase 2)

Original screensaver loops, made for an OLED panel and released under
CC BY-SA 4.0. Two scenes so far (`render.py --scene`):

* `aurora`: green curtains with pink lower fringes over a starfield, a black
  mountain ridge, and a lake that mirrors the lowest of them.
* `fireflies`: warm points of light drifting and flashing over a meadow, in
  front of a black treeline under a faint deep-blue sky.

* **Design for the panel:** near-black scenes with low average light and small
  highlights. An OLED dims large bright areas (ABL), but a small highlight can
  reach the B9's ~700–800 nit peak next to perfect black. Think aurora,
  starfields, bioluminescence, ink in water, neon rain.
* **OLED care:** everything bright moves or fades. Stars twinkle, and the
  brightest dim through the loop; fireflies wander and flash. No highlight
  sits still at full power.

## Making a loop

Everything runs in the studio container (`studio/Dockerfile`: Ubuntu's ffmpeg
with 10-bit x265, numpy, OpenEXR). `tools/studio.sh` builds it on first use,
like `tools/dev.sh`. Output goes to `dist/studio/`, which git ignores.

```sh
tools/studio.sh python3 studio/render.py --size 1920x1080 --seconds 10   # EXR frames
tools/studio.sh python3 studio/encode.py dist/studio/aurora-1080p        # HDR10 video
tools/studio.sh python3 studio/verify.py dist/studio/aurora-1080p.mp4    # checks, previews
```

1. **Render** (`render.py`; scenes in `aurora.py` and `fireflies.py`, shared
   timing and highlight helpers in `scenekit.py`): procedural frames as
   half-float linear EXR in Display P3, 1.0 = 100 cd/m². The EXR header says
   so (`chromaticities`, `whiteLuminance`). Every motion runs a whole number
   of cycles per loop, so frame N is frame 0 again: the loop has no seam. An
   interrupted render resumes where it stopped.
2. **Encode** (`encode.py`, maths in `hdr.py`): measures MaxCLL and MaxFALL
   from the frames, converts them to BT.2020, the PQ curve and 10-bit Y'CbCr
   4:2:0, and encodes HEVC Main10 with x265. The stream carries HDR10 SEI on
   every keyframe: a P3-D65 1000 cd/m² mastering display and the measured
   light levels. GOPs are closed and two seconds long, and the bitrate stays
   within Main tier level 4.1 (HD) or 5.1 (UHD). Writes `<name>.mp4` and
   `<name>.mp4.json` (metadata, settings, timings).
3. **Verify** (`verify.py`, rules in `checks.py`): ffprobe must see HEVC
   Main 10, yuv420p10le, BT.2020 / PQ / BT.2020 NCL in narrow range, and the
   HDR10 SEI on every keyframe with the measured values. Decoded, the step
   from the last frame back to the first must look like any other step, and
   no luma may be brighter than the MaxCLL the stream claims.
   Writes tone-mapped SDR previews to `dist/studio/previews/`; they show the
   look, not what the TV shows.

Options: `render.py --help`, `encode.py --help` (quality `--crf`, `--preset`,
`.mkv` output). Every motion is rounded to whole cycles per loop, so short
loops move faster and lose some variety; 30 s or more looks calmer.

A 4K, 30 s loop (720 frames) took about 4 minutes to render and 3 to measure
and encode on a 16-thread PC with Docker's default 8 GB, and its frames take
about 10 GB under `dist/`. Both scripts size their worker count to the memory
they see; `--workers` overrides it.

## Tests

`hdr.py` and `checks.py` use the standard library only, so `make check` tests
them in the build container. The rest needs numpy, OpenEXR and ffmpeg; those
tests skip there and run here:

```sh
tools/studio.sh python3 -I -m unittest discover -s tools/tests -p 'test_studio*.py'
```

## On the TV

Watched on the B9 (OLED55B9PUA, webOS 4.10.2, firmware 05.50.00) on
2026-10-20: `aurora-2160p.mp4` in LG's player, started by `tv/hdr-check.sh`
([tv/README.md](tv/README.md) has the steps).

* The TV went from `expert1` / `sdr` to `hdrCinemaBright` / `hdr` 4 s after
  the player started, and showed its HDR logo. The MP4 as it is (`hev1`,
  HDR10 metadata in the stream's SEI only, no `mdcv` or `clli` boxes) is
  enough; the `.mkv` was not needed.
* By eye: the stars sparkle against true black, and no banding showed in the
  glow above the mountains.
* The player plays the loop once and does not repeat. Three minutes after
  the start, the TV was back on the input it showed before (HDMI 2, in SDR),
  with the player in the background. A seamless repeat is the screensaver's
  job, in `ambient/`.

## Still to do

* Watch `aurora-2160p-60s.mp4` and `fireflies-2160p.mp4` on the TV the same
  way.
* Dolby Vision Profile 8.1, an RPU added with `dovi_tool`.
* Dither before the 10-bit quantization, if a faint glow bands on the panel
  (none seen in aurora).
* More scenes.
