# Trying a loop on the TV

Does the B9 switch into HDR for a studio loop? These steps play one in LG's
own video player, `com.webos.app.photovideo` (the webOS 3–5 player, present
on the B9), and read the picture settings while it plays.

Everything is temporary, as [docs/safety.md](../../docs/safety.md) asks: the
loop goes to `/tmp` on the TV, which is RAM, and the only thing started is
LG's player. No setting is changed. Run the commands from the repository root
in Git Bash, with `.env` set up ([docs/setup.md](../../docs/setup.md)).

## 1. Copy a loop to the TV

The loops are in `dist/studio/` (`tools/studio.sh python3 studio/verify.py
<loop>` checks one first). `aurora-2160p.mp4` is 4K, 30 s and 8 MB.

```sh
tools/tv.sh run 'mkdir -p /tmp/lumen-hdr && cat > /tmp/lumen-hdr/loop.mp4' < dist/studio/aurora-2160p.mp4
tools/tv.sh run 'ls -l /tmp/lumen-hdr'
```

The size on the TV should match `ls -l dist/studio/aurora-2160p.mp4`.

## 2. Play it and read the dynamic range

```sh
tools/tv.sh script studio/tv/hdr-check.sh
```

It prints the picture settings' `pictureMode` and `dynamicRange` before and
eight seconds after the player starts, then `RESULT: HDR (...)` or
`RESULT: still SDR`. Today, in SDR, the TV reports `expert2` and `sdr`. In
HDR, LG uses a separate set of picture modes (the `hdr...` names in
`psmListForExpert`), so expect `pictureMode` to change as well.

While it plays, look at the TV:

* LG sets usually show an HDR logo in a corner when HDR playback starts.
* The stars should sparkle well above the curtains, against true black.
* Note any banding in the faint glow above the mountains.
* At the end of the loop, does the player stop, or repeat? A seamless repeat
  needs the player's repeat option, or the screensaver work in `ambient/`.

## 3. Clean up

```sh
tools/tv.sh close com.webos.app.photovideo
tools/tv.sh run 'rm -rf /tmp/lumen-hdr'
```

## If it does not work

* **The player will not open the file.** Try a folder on the apps
  partition (1.8 GB free) instead of `/tmp`, then delete it afterwards:

  ```sh
  tools/tv.sh run 'mkdir -p /media/developer/temp/lumen-hdr && cat > /media/developer/temp/lumen-hdr/loop.mp4' < dist/studio/aurora-2160p.mp4
  tools/tv.sh run 'LUMEN_HDR_DIR=/media/developer/temp/lumen-hdr sh -s' < studio/tv/hdr-check.sh
  tools/tv.sh run 'rm -rf /media/developer/temp/lumen-hdr'
  ```

* **It plays, but stays SDR.** Try the Matroska version: it carries the same
  stream, and some players read HDR metadata from the container. Encode it
  if `dist/studio/aurora-2160p.mkv` does not exist yet, then copy it as
  `loop.mkv` (remove `loop.mp4` first: the script prefers it):

  ```sh
  tools/studio.sh python3 studio/encode.py dist/studio/aurora-2160p --out dist/studio/aurora-2160p.mkv
  tools/tv.sh run 'rm -f /tmp/lumen-hdr/loop.mp4 && cat > /tmp/lumen-hdr/loop.mkv' < dist/studio/aurora-2160p.mkv
  tools/tv.sh script studio/tv/hdr-check.sh
  ```

* **Still SDR, or the launch fails.** Copy the `.mp4` to a USB stick and
  open it from the TV's Photo & Video app. If the TV shows HDR that way, the
  file is right and the launch from the shell is what needs work.

Whatever happens, keep the script's output: it is the measurement for
studio/README.md's "watch it on the TV" item.
