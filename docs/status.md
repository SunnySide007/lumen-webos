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
| 2. Speed | **Under way.** `lumend` runs from boot with `s1`, `senders` (acr2), `lg-home`, `icons` and `modes`; its `shell`, `link` and `screen-off` parts are off. The Wayland path works once every seat is bound: first frame 436 ms against SDL's 520 ms, 40.5 MB against 49.4 MB, frames level. The Home key acts on its release: 41 to 51 ms from the release with the shell alive or started hidden, 663 to 684 ms with it closed. | `lumend` installed (door, five parts), rebooted once; Wayland by launch parameter only |
| 3. Design | **The new home is merged** (9ef55e1, at 8a4d844) past the gate: home 17.01 ms, Pages 17.07 ms, Control Center 16.58 ms. The home-design session builds the six design notes next. | Installed as the Home |
| 4. Privacy | Guard **G3's build runs in log mode**, in clear, from its own boot hook. Tried on the TV: default deny (`block trial`) with the live store-safe check and Netflix, YouTube and Plex launching; the learning window; Quad9 over TLS (`tls trial`), its fallback when port 853 is refused and its return; the clock fallback. Blocking for the family, and TLS for good, are the owner's tier C yes. | G3 binary and hook, log mode |
| 5. Finish | R3 and R4 are merged, neither verified: the Windows app never saved a TV or opened a link. The sitting for both waits for the owner. | Nothing |

## On the TV

* **Installed:** main with every seat bound (the commit after 9ef55e1),
  the new home, as the Home on the SDL path. The last good build before
  it, 3a89955, is kept in `tools/out/archive/tv-check-20261031T184850Z/restore/`.
* **Hooks** in `/var/lib/webosbrew/init.d`: `lumen-guard` (G3) and `lumend`
  (the door). `lumen-privacy` and `lumen-home` are retired to
  `/var/lib/lumen/lumend/retired/`; `tools/lumend.sh door remove` puts
  them back. `lumen-ctl` behind its forced key and route D are as before.
* **Not on it:** `lumen-remote` and its 8443 rule; Guard blocking; TLS.

## Next

1. `lumend`'s `shell` part: the shell sends `leaving` before a deliberate
   exit, lumend launches it hidden on the Wayland path at boot and after
   a close. Then Wayland for every launch, after the owner has used the
   real remote on it (Back, the pointer, a click).
2. Guard's log-mode preview over days of the family's use (G2 step 5),
   then the owner's two tier C questions with the results.
3. The R3 and R4 sitting with the owner, the TV lock held throughout.
4. The lean track from the survey: apps left running behind Lumen hold far
   more memory than any LG job (Prime Video's 134 MB, Netflix and YouTube
   246 MB together), so the first candidate is closing them, not stopping
   jobs.
