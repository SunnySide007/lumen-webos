# tools/art: the shell's art, drawn by code

Lumen's Top Shelf heroes and input pictograms are drawn by these scripts from
Lumen's own scenes and seeds: no photographs, nothing fetched, no image model,
no brand's marks. They write to `shell/assets/art/`, which `CREDITS.md` there
lists file by file (CC BY-SA 4.0).

Everything runs in the studio container (numpy and ffmpeg; `tools/studio.sh`
builds it on first use):

```sh
tools/studio.sh python3 -I tools/art/heroes.py --all           # every hero, and its stats
tools/studio.sh python3 -I tools/art/heroes.py hero-hdmi2 --out tools/out/art/try
tools/studio.sh python3 -I tools/art/pictograms.py             # icon-*.svg to icon-*.png
tools/studio.sh python3 -I tools/art/contact_sheet.py          # tools/out/art/contact-sheet.png
tools/studio.sh python3 -I tools/art/contact_sheet.py --ambient-choice
tools/studio.sh python3 -I tools/art/contact_sheet.py --crop hero-palette-1
```

## The scripts

* `heroes.py` draws each hero in `heroes.toml` (scene, seed and the scene's
  settings) as a 1920 x 640 PNG and a 960 x 320 `-half.png`, and prints each
  file's light levels and banding. `hero-ambient` is a still of the studio's
  aurora loop (`studio/aurora.py`, through its `layers_at()`), also as three
  layers for parallax: `-bg` (the sky), `-mid` (the curtains' light), `-fg`
  (the ridge and lake), RGBA with straight alpha, worked out on the encoded
  signal because the shell blends 8-bit sRGB textures as they are. Laid over
  each other they are the flat picture. `source = "fireflies"` and a
  different `frame` in the table draw it from elsewhere in the loops.
* `pictograms.py` reads the hand-written `icon-*.svg` (strokes only,
  `#EBEEF5`, a 2-unit round stroke on a 24-unit grid), refuses anything
  outside those rules (`check()`), and draws each to a 240 x 240 PNG with
  transparency.
* `contact_sheet.py` lays every hero (480 x 160) and pictogram (120 px) on one
  sheet in `tools/out/art/`, outside git. `--ambient-choice` draws the ambient
  hero from each studio scene at four frames; `--crop HERO` shows, at 400%,
  where HERO bands worst without dither, rounded beside dithered.
* `artkit.py` is what they share: colour, noise fields, finishing, dither,
  a PNG writer and reader, and the measurements the tests make.

## How a hero is made

A scene is drawn in linear light with sRGB primaries, 1.0 the UI's white, in
coordinates of picture heights, so it looks the same at any size.
`artkit.finish()` adds film grain (which leaves black black), rolls the
highlights off below white, encodes to sRGB and darkens the bottom 200 px
toward black, more on the left, where the shell's 72 px title sits.
`artkit.quantize()` takes it to 8 bits against a 64 x 64 blue-noise
threshold, so dark gradients show no bands on the TV's 8-bit UI plane. The
PNG carries an sRGB chunk and no other metadata. The same seed gives the
same bytes in the same container; numpy on another CPU may round a scene's
float32 sums differently, so expect the same picture, not always the same
bytes.

The heroes are made for an OLED in a dark room, and the tests hold them to
it: mean luminance under 20%, a dark median, few and small highlights, no
pure white, the title band dark enough for the title at 7:1 or better.

## Tests

`tools/tests/test_art.py`. The table, the SVG rules, the files' sizes and
the credits need only the standard library, so `make check` runs them. The
rest draws and measures, needs numpy and runs in the studio container:

```sh
tools/studio.sh python3 -I -m unittest discover -s tools/tests -p 'test_art*.py'
```

They check each hero's light levels and title band, that it draws again byte
for byte, that dither leaves no band longer than 64 px where rounding would,
that no two palette heroes share a shape, that the ambient layers make the
picture, and that each pictogram's PNG is its SVG (and agrees with librsvg).

## Adding a hero

Add a table to `heroes.toml` with a scene from `heroes.SCENES` and a seed no
other hero uses, draw it, look at it on the contact sheet and at a 400% crop,
run the tests, and add its row to `shell/assets/art/CREDITS.md`.
