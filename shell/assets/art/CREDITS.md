# Art credits

Every file in this folder is original work for Lumen, made with `tools/art/`,
CC BY-SA 4.0 (<https://creativecommons.org/licenses/by-sa/4.0/>; see
`docs/decisions.md`, record 2).

Author: _(placeholder: the owner's name, if they want it here)_

Nothing here was fetched, photographed or made by an image model, and none
of it uses a brand's marks, logos or typefaces. Each hero is drawn by code
from the scene and seed below (`tools/art/heroes.toml`) and comes out the
same, byte for byte, when drawn again in the studio container:

```sh
tools/studio.sh python3 -I tools/art/heroes.py --all
tools/studio.sh python3 -I tools/art/pictograms.py
```

## Top Shelf heroes

1920 x 640 sRGB PNGs, each with a 960 x 320 copy (`-half.png`), drawn by
`tools/art/heroes.py`.

| Hero | Scene | Seed | What it shows |
|---|---|---|---|
| `hero-ambient` | `ambient`: a still of `studio/aurora.py` | 2026 (the loop's own), frame 0 of the 30 s loop at 24 fps | The aurora over the mountain lake, the screensaver's first frame, toned as the studio's previews are. Also as layers for parallax, straight alpha: `-bg` the sky and stars, `-mid` the curtains' light, `-fg` the ridge and the lake |
| `hero-palette-0` | `palette`, shape `ember`, tile colour 0 | 4101 | A glow below the right edge, smoke curling up through it |
| `hero-palette-1` | `palette`, shape `dunes`, tile colour 1 | 4102 | Dunes in layers against a low light |
| `hero-palette-2` | `palette`, shape `rings`, tile colour 2 | 4103 | Ripples spreading from beyond the top right |
| `hero-palette-3` | `palette`, shape `canopy`, tile colour 3 | 4104 | Sun through leaves, out of focus |
| `hero-palette-4` | `palette`, shape `caustic`, tile colour 4 | 4105 | Light on the floor of still water |
| `hero-palette-5` | `palette`, shape `horizon`, tile colour 5 | 4106 | The rim of a planet against space, lit from the right |
| `hero-palette-6` | `palette`, shape `nebula`, tile colour 6 | 4107 | A cloud of gas with dark lanes across it |
| `hero-palette-7` | `palette`, shape `silk`, tile colour 7 | 4108 | Two ribbons of silk turning in the dark |
| `hero-hdmi1` | `arrival`, -9 degrees, 2400 K | 5201 | Lines of light from the right, bending into dark glass and spreading in it |
| `hero-hdmi2` | `arrival`, 5 degrees, 3600 K | 5202 | The same scene, steeper and warm white |
| `hero-hdmi3` | `arrival`, -2.5 degrees, 5600 K | 5203 | The same scene, near level and neutral |
| `hero-hdmi4` | `arrival`, 12 degrees, 12000 K | 5204 | The same scene, steepest and cold |
| `hero-live-tv` | `towers` | 6301 | Far broadcast masts on a ridge at dusk, their red lights lit |
| `hero-plex` | `beam` | 6302 | A projector's beam through a dark cinema, with dust and grain |
| `hero-homebrew` | `circuits` | 6303 | Warm light raking across a circuit board, close up |
| `hero-settings` | `panels` | 6304 | Panes of frosted glass at different depths over soft light |

The tile colours are `TILE_PALETTE` in `shell/crates/lumen-core/src/style.rs`.

## Input pictograms

Written by hand as SVG (`icon-*.svg`: strokes only, `#EBEEF5`, a 2-unit round
stroke on a 24-unit grid) and drawn to 240 x 240 PNGs with transparency
(`icon-*.png`) by `tools/art/pictograms.py`. No seed: they are drawn exactly.

| Pictogram | What it shows |
|---|---|
| `icon-hdmi` | An HDMI port, face on |
| `icon-antenna` | A set-top antenna with two rods |
| `icon-usb` | A USB-A plug |
| `icon-bluetooth` | Headphones (the Bluetooth rune is a registered mark, so it is not used) |
| `icon-input` | An arrow into a box |
| `icon-gear` | A gear, for settings |
| `icon-moon` | A crescent moon, for the ambient screen |
| `icon-shield` | A shield with a keyhole, for privacy |
