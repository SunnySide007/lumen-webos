# Status

The live status of Lumen, kept by the builder session. History, and the
reasoning behind the order, are in [specs/depth.md](specs/depth.md) and
[roadmap.md](roadmap.md). The plan of 2026-11-08 (approved by the owner):
stages 0 to 5 below. How TV work runs without asking: [safety.md, "Working
without asking"](safety.md#working-without-asking-from-2026-11-08).

Every speed number cites the TV: an LG OLED55B9PUA, webOS 4.10.2-31,
firmware 05.50.00, unless it says otherwise.

"Merged" below means reviewed and passing `make check` on the PC. A part
is **done** only once it has run on the TV; the "On the TV" column says
which.

## Now (2026-11-10)

| Stage | State | On the TV |
|---|---|---|
| 0. Rules | **Done** (324de1a). Night window 00:00 to 06:00 TV time with a dark screen; daytime idle windows; from 2026-11-10 the owner's weekday slot, 10:00 to 15:00, in which an HDMI input in front is no bar. | In force |
| 1. Fix and measure | **Done** (2026-11-08). Baselines in [device/bench.md, "Measured"](device/bench.md#measured). | Measured |
| 2. Speed | **The Home press meets its target.** `lumend` keeps the shell hidden on the Wayland path from boot and after a close; a Home press then shows Lumen 194 to 229 ms after the key goes down with a 180 ms hold, which is 14 to 49 ms after its release (LG acts on the release), against 843 to 864 ms with the shell closed. `tools/tv.sh close` and `install` hold the keeper for two minutes. Left: the lean track, lifecycle version 2, and starting the shell sooner than 157 s after a boot. | `lumend` with six parts (`s1`, `senders` acr2, `lg-home`, `icons`, `modes`, `shell`), rebooted twice |
| 3. Design | **The new home and the six design notes are merged** (9ef55e1 and a1d9a58, at 90a3880) past the gate with all 30 icons: home 17.04 ms, Pages 17.14 ms, Control Center 16.98 ms. The owner's first test with the real remote (2026-11-10): the pointer must select the tile under it; today a click opens the focused tile. With the home-design session, with the stutter on coming back in front. | Installed as the Home |
| 4. Privacy | Guard **G3's build runs in log mode**, in clear, from its own boot hook. Tried on the TV: default deny (`block trial`) with the live store-safe check and Netflix, YouTube and Plex launching; the learning window; Quad9 over TLS (`tls trial`), its fallback when port 853 is refused and its return; the clock fallback. Blocking for the family, and TLS for good, are the owner's tier C yes. | G3 binary and hook, log mode |
| 5. Finish | R3 and R4 are merged, neither verified: the Windows app never saved a TV or opened a link. The sitting for both waits for the owner. | Nothing |

## On the TV

* **Installed:** main at f5419d9 (the six design notes), as the Home. A
  Home press shows the shell lumend keeps hidden, on the Wayland path; a
  launch by a tool or after a bench is on SDL until the shell next closes.
  The old home, 3a89955, is kept in
  `tools/out/archive/tv-check-20261031T184850Z/restore/`.
* **Hooks** in `/var/lib/webosbrew/init.d`: `lumen-guard` (G3) and `lumend`
  (the door). `lumen-privacy` and `lumen-home` are retired to
  `/var/lib/lumen/lumend/retired/`; `tools/lumend.sh door remove` puts
  them back. `lumen-ctl` behind its forced key and route D are as before.
* **Not on it:** `lumen-remote` and its 8443 rule; Guard blocking; TLS.

## Next

1. The pointer on the home (the home-design session), then a bench
   pattern for coming back in front. Wayland for every launch (the IPK's
   `platform` file) once the pointer works.
2. Guard's log-mode preview over days of the family's use (G2 step 5),
   then the owner's two tier C questions with the results.
3. The R3 and R4 sitting with the owner, the TV lock held throughout.
4. The lean track from the survey: apps left running behind Lumen hold far
   more memory than any LG job (Prime Video's 134 MB, Netflix and YouTube
   246 MB together), so the first candidate is closing them, not stopping
   jobs.
