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
| 1. Fix and measure | **Built, waiting for a lab window.** The robot remote (`lumen-ctl play`), TV screenshots (`"shots"`), `tools/bench.sh` with the speed gate, automatic undo and the screensaver held off (`"saver":"hold"`): [device/bench.md](device/bench.md). Not yet run on the TV. |
| 2. Speed | Not started. |
| 3. Design | **Started early, at the owner's ask** (2026-11-08): the new home in its own session and worktree (`home-design`, cut from `claude/dither-fix`). Builds come to the bench; no other session touches the TV. |
| 4. Privacy | Guard G1 logging since 2026-10-31. **G2 (lane H) merged** (5180f96): default deny behind the owner's mode file, off. Trial steps 2 to 4 (backup, push, restart in log mode) run in the first night window; blocking for the family is the owner's tier C yes. |
| 5. Finish | **R3, Lumen Remote for Windows, merged** (cfe8be4), tested against lumen-ctl's fake TV. Its TV checklist waits for the owner. |

## On the TV

* **Installed:** the dither-fix build of 2026-11-07 (binary 4402139…),
  installed 2026-11-07 16:48 PST and the Home since then. It has not been
  measured. The 2026-11-08 hand-off said 3a89955 was installed; the
  bench's first look found otherwise.
* **Last good build:** 3a89955 (binary b1605833…): 16.88 ms average frame
  and 4.7 vsyncs skipped per 10 s while moving (2026-11-07). Every bench
  run ends with it put back.
* **Hooks:** S1 privacy, Guard G1 (log only), `lumen-home`, `lumen-ctl`
  behind its forced key, route D (Lumen is the Home).

## Next

1. The first bench runs, in a lab window (`tools/bench/first-runs.sh`):
   3a89955 as the baseline, then the dither fix with `{"look":"plain"}`
   (about 17 ms wanted), `{"look":"-dither"}` and the full look.
2. If the fix holds, commit it with its numbers in look-and-feel.md's note.
3. The other baselines: start-up, Home press (target 5), RSS after 5 min,
   idle CPU, app launch times.
