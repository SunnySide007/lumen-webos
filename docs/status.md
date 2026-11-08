# Status

The live status of Lumen, kept by the builder session. History, and the
reasoning behind the order, are in [specs/depth.md](specs/depth.md) and
[roadmap.md](roadmap.md). The plan of 2026-11-08 (approved by the owner):
stages 0 to 5 below. How TV work runs without asking: [safety.md, "Working
without asking"](safety.md#working-without-asking-from-2026-11-08).

Every speed number cites the TV: an LG OLED55B9PUA, webOS 4.10.2-31,
firmware 05.50.00, unless it says otherwise.

## Now (2026-11-08)

| Stage | State |
|---|---|
| 0. Rules | **Done** (324de1a). The owner chose the lab window 00:00 to 06:00 and a dark screen at night, so runs that light the screen use daytime idle windows (20 minutes on Lumen or LG's screensaver). |
| 1. Fix and measure | **First runs done** (2026-11-08, 21:48 to 22:30 UTC, the owner cleared the TV). The robot writes keys into the Magic Remote's own node (uinput devices are never read on the B9). Baselines in [device/bench.md, "Measured"](device/bench.md#measured): 3a89955 16.89 ms, start-up 541 ms, RSS 42,848 kB, Home press to Lumen 216 to 229 ms. The dither fix passes the gate with the plain look (16.94 ms) and is merged (22366d5); the dither itself costs about 12.5 ms, so the default look must drop it. |
| 2. Speed | Not started. |
| 3. Design | **Started early, at the owner's ask** (2026-11-08): the new home in its own session and worktree (`home-design`, cut from `claude/dither-fix`). Builds come to the bench; no other session touches the TV. |
| 4. Privacy | Guard G1 logging since 2026-10-31. **G2 (lane H) merged** (5180f96): default deny behind the owner's mode file, off. Trial steps 2 to 4 (backup, push, restart in log mode) run in the first night window; blocking for the family is the owner's tier C yes. |
| 5. Finish | **R3, Lumen Remote for Windows, merged** (cfe8be4), tested against lumen-ctl's fake TV. **R4, the phone's paired HTTPS door, merged** (2026-11-08, after a security review). Both TV checklists wait for the owner; R4's needs the Pixel in the same sitting, and lumen-remote is stopped again afterwards until the owner keeps it. |

## On the TV

* **Installed:** 3a89955 (binary b1605833…), the last good build; every
  bench run ends with it put back (first done 2026-11-08 21:48 UTC,
  replacing the unmeasured dither-fix build of 2026-11-07).
* **Hooks:** S1 privacy, Guard G1 (log only), `lumen-home`, `lumen-ctl`
  behind its forced key, route D (Lumen is the Home).

## Next

1. The home design's builds on the bench, in daytime lab windows:
   d7de95e's looks and pages pass, then b5f35a6 (the Control Center) with
   its own pattern. Its branch drops the dither from the default look.
2. Tonight 00:00: G2 trial steps 2 to 4 and the lean survey (read-only).
3. Guard G3 (lane K, DNS over TLS) trial after G2's nights; its last step
   is the owner's tier C yes.
4. Stage 2 (speed) from the baselines: the dither made cheap, then the
   lean track's candidates from tonight's survey.
