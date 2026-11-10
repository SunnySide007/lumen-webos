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
| 0. Rules | **Done** (324de1a). The lab window is 00:00 to 06:00 TV time with a dark screen, so runs that light the screen use daytime idle windows (20 minutes on Lumen or LG's screensaver). | In force |
| 1. Fix and measure | **Done** (2026-11-08; report `tools/out/reports/stage1-20261108.md`). The robot writes keys into the Magic Remote's own node. Baselines in [device/bench.md, "Measured"](device/bench.md#measured): 3a89955 16.89 ms, start-up 541 ms, RSS 42,848 kB, Home press to Lumen 216 to 229 ms. The dither fix is merged (22366d5); the dither itself costs about 12.5 ms, so the default look drops it. | Measured |
| 2. Speed | Not started. Lane L (platform layer) and lane M (`lumend`) are merged; none of their TV steps has run. | Nothing |
| 3. Design | The new home in its own session and worktree (`claude/home-design-6d71ec`). 9d5a988's home passes the gate (16.92 ms); Pages (20.38 ms) and the Control Center (23.50 ms) fail it. bc053bd removes their full-screen dims and waits for a daytime window, then the f19929a pair (dither from a 4x4 texture). The branch merges only past the gate. | 9d5a988 measured; bc053bd queued |
| 4. Privacy | Guard G1 logging since 2026-10-31. G2 (default deny, 5180f96) and G3 (Quad9 over DNS over TLS, acc15cb) are merged, both off. G2's trial steps 2 to 4 are queued for the night window of 2026-11-10 (08:00 UTC). Blocking for the family, and TLS on for good, are the owner's tier C yes. | G1 only |
| 5. Finish | R3 (Lumen Remote for Windows, cfe8be4) and R4 (the phone's paired HTTPS door) are merged. Neither is verified: on 2026-11-09 no request from the Windows app reached the TV (the app has no saved TV and never opened a link). The sitting for both waits for the owner. | Nothing |

## On the TV

* **Installed:** 3a89955 (binary b1605833…), the last good build; every
  bench run ends with it put back.
* **Hooks:** S1 privacy, Guard G1 (log only), `lumen-home`, `lumen-ctl`
  behind its forced key, route D (Lumen is the Home).
* **Not on it:** `lumend`, the Wayland path, Guard G2 and G3,
  `lumen-remote` and its 8443 rule.

## Next

Drain what is merged but unmeasured before building more:

1. The bench chain in daytime windows: bc053bd (pages, control, full),
   then f19929a (dither, full). Merge the home design once the gate passes.
2. Night of 2026-11-10: G2 trial steps 2 to 4 and the lean survey; steps 5
   to 8 on later nights, then G3's trial.
3. The R3 and R4 sitting with the owner, the TV lock held throughout.
4. Stage 2 (speed): lane L's TV steps 1 to 12, `lumend`'s parts one at a
   time, the shell kept resident, Home press under 150 ms, then the lean
   track's candidates from the survey.
