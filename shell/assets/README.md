# Shell assets

## art/

The Top Shelf heroes (`hero-*.png`) and the input pictograms (`icon-*.svg`,
`icon-*.png`), original work for Lumen under CC BY-SA 4.0, drawn by code with
`tools/art/` (its README says how). `art/CREDITS.md` lists each file with the
script and seed that made it. Regenerate them with:

```sh
tools/studio.sh python3 -I tools/art/heroes.py --all
tools/studio.sh python3 -I tools/art/pictograms.py
```

`make ipk` puts every hero, full size and half (`-half`), in the app's
`art/` folder, with `CREDITS.md`; the shell reads them from there, and the
simulator from this folder (`tools/sim.ps1` sets `LUMEN_ART`). The full set
is about 13 MB, the halves 3.4 MB: both go, so the TV can show either with
the `half` look switch (docs/specs/look-and-feel.md).

## art-owner/

Your own Top Shelf art: a PNG named for the app it is for,
`art-owner/<app id>.png` (`com.webos.app.hdmi1.png`, `netflix.png`), shown
over that app's tile in place of Lumen's hero. The folder is git-ignored, so
the art stays yours; `make ipk` packs it into the app's `art-owner/` folder
if it is there, and Lumen never fetches art from anywhere.

- 1920x640, or 960x320, scaled up when drawn. Any other size is left out,
  with a line in the log saying so.
- Opaque: any transparency is laid over black. At most 8 MB a file.
- The app ID is the one the shell's log names when it launches the app
  (`launch <app id>: answered in …`). A file whose name is not an app ID is
  left out, with a line in the log.

The simulator reads this folder too (`LUMEN_ART_OWNER`).

## fonts/

`Inter-SemiBold.ttf` is Inter 4.1 by Rasmus Andersson and the Inter Project
Authors, unmodified, under the SIL Open Font License 1.1 (`Inter-LICENSE.txt`,
which names no Reserved Font Name). `shell/.../text.rs` builds it into the
binary, so the IPK carries `Inter-LICENSE.txt` beside it (the Makefile's
`ipk` rule).

It comes from `extras/ttf/` in the release zip,
<https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip>, fetched
on 2026-10-20. GitHub publishes no digest for that asset, so these are the
hashes seen then:

| File | SHA-256 |
|---|---|
| `Inter-4.1.zip` | `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e` |
| `Inter-SemiBold.ttf` | `78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3` |
| `Inter-LICENSE.txt` (the zip's `LICENSE.txt`) | `262481e844521b326f5ecd053e59b98c8b2da78c8ee1bdbb6e8174305e54935a` |

`Inter-SemiBold.kern` is Inter's kerning, read out of that font's GPOS table
by `tools/font/kern_table.py` (the table's layout is in its docstring), so it
is derived from Inter and under the same licence. `text.rs` builds it in
beside the font. Regenerate it after changing the font:

```sh
sh tools/dev.sh make kern-table
```

## mock-icons/

Placeholder pictograms for the simulator's mock apps (play, note, tv, input,
globe, gear), drawn by `tools/mock-icons.py`. They are original drawings,
part of Lumen and under its licence. Regenerate them with:

```sh
sh tools/dev.sh python3 -I tools/mock-icons.py
```
