# Trying a loop on the TV

Does the B9 switch into HDR for a studio loop? These steps play one in LG's
own video player, `com.webos.app.photovideo` (the webOS 3–5 player, present
on the B9), and read the picture settings while it plays.

Everything is temporary, as [docs/safety.md](../../docs/safety.md) asks: the
loop goes to root's stage on the TV (`/var/lib/lumen/stage/hdr`), the script
copies it to `/var/lib/lumen-hdr` for the player, the last step removes both,
and the only thing started is LG's player. No setting is changed. Root
writes nothing into `/tmp`, which every app's jail shares
([CONTRIBUTING.md](../../CONTRIBUTING.md)). Run the commands from the
repository root in Git Bash, with `.env` set up
([docs/setup.md](../../docs/setup.md)).

## 1. Copy a loop to the TV

The loops are in `dist/studio/` (`tools/studio.sh python3 studio/verify.py
<loop>` checks one first). `aurora-2160p.mp4` is 4K, 30 s and 8 MB; the
calmer `aurora-2160p-60s.mp4` and the second scene, `fireflies-2160p.mp4`,
go the same way: copy each in turn as `loop.mp4`.

```sh
tools/tv.sh push dist/studio/aurora-2160p.mp4 /var/lib/lumen/stage/hdr/loop.mp4
```

`push` checks that the stage is root's alone, makes the folder and checks
that the size on the TV matches. `/var` had about 270 MB free on the B9 on
2026-10-31, and a loop takes twice its size there until the clean-up.

## 2. Play it and read the dynamic range

```sh
tools/tv.sh script studio/tv/hdr-check.sh
```

It copies the loop to `/var/lib/lumen-hdr` (a folder anyone may read and
only root may write, since LG's player may not run as root), closes the
player if it is open, prints the picture settings'
`pictureMode` and `dynamicRange`, starts the player, and polls for up to 30 s
until the dynamic range leaves `sdr`. The last line is the verdict:

| Result | Exit |
| --- | --- |
| `RESULT: HDR (...) after N s` | 0 |
| `RESULT: still SDR ...` | 1 |
| `RESULT: the player did not start` or `... is not in front` | 2 |
| `RESULT: inconclusive` (the TV was in HDR before the player started) | 3 |

Close any app that is showing HDR first: a Netflix title in Dolby Vision
left the B9 in `dolbyHdr`, and the check stopped as inconclusive.

Today, in SDR, the TV reports `expert2` and `sdr`. In HDR, LG uses a
separate set of picture modes (the `hdr...` names in `psmListForExpert`), so
expect `pictureMode` to change as well. Whether `dynamicRange` follows LG's
own player is itself untested: if the screen clearly shows HDR (an HDR logo,
the HDR picture modes in the menu) while the script says SDR, the screen
wins; note both.

While it plays, look at the TV:

* LG sets usually show an HDR logo in a corner when HDR playback starts.
* The stars should sparkle well above the curtains, against true black.
* Note any banding in the faint glow above the mountains.
* At the end of the loop, does the player stop, or repeat? A seamless repeat
  needs the player's repeat option, or the screensaver work in `ambient/`.

## 3. Clean up

```sh
tools/tv.sh close com.webos.app.photovideo
tools/tv.sh run 'rm -rf /var/lib/lumen/stage/hdr /var/lib/lumen-hdr'
```

Both folders are on `/var`, which a reboot keeps, so run this whatever the
result.

## If it does not work

* **The player will not open the file.** The 2026-10-20 run played from
  `/tmp`; this is the first from `/var/lib/lumen-hdr`. Keep the player's
  answer and `tools/tv.sh run 'ls -ldn /var/lib/lumen-hdr /var/lib/lumen-hdr/*'`,
  clean up, and try the USB stick (the last item) to tell the file from the
  path.

* **It plays, but stays SDR.** Try the Matroska version: it carries the same
  stream, and some players read HDR metadata from the container. Encode it
  if `dist/studio/aurora-2160p.mkv` does not exist yet, then copy it as
  `loop.mkv` (remove `loop.mp4` first: the script prefers it; it closes the
  player from the first run itself):

  ```sh
  tools/studio.sh python3 studio/encode.py dist/studio/aurora-2160p --out dist/studio/aurora-2160p.mkv
  tools/tv.sh run 'rm -f /var/lib/lumen/stage/hdr/loop.mp4'
  tools/tv.sh push dist/studio/aurora-2160p.mkv /var/lib/lumen/stage/hdr/loop.mkv
  tools/tv.sh script studio/tv/hdr-check.sh
  ```

* **Still SDR, or the launch fails.** Copy the `.mp4` to a USB stick and
  open it from the TV's Photo & Video app. If the TV shows HDR that way, the
  file is right and the launch from the shell is what needs work.

Whatever happens, keep the script's output: it is the measurement that goes
under "On the TV" in studio/README.md.
