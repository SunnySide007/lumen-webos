# The bench

`tools/bench.sh` measures a Lumen build on the B9 the same way every time,
with no one at the remote, and checks it against the last good build. It is
the TV lab of the 2026-11-08 plan (Stage 1). What it may do without asking
is tier A of [docs/safety.md, "Working without
asking"](../safety.md#working-without-asking-from-2026-11-08): install,
launch and close Lumen builds, robot keys, screenshots and log reads, the
screensaver held off during a run, and the last good build always put back.

Every number below cites the TV it came from. Numbers from the simulator
are layout only.

## How a run goes

1. **A lab window.** `tools/bench.sh window` reads, without changing
   anything: the TV's hour, the app in front, the power state, and from
   Lumen's own log the last sign of a person (a key, the pointer, a click,
   a change of the app in front or the power state, but not LG's
   screensaver coming). A run starts only if the hour is 06:00 or later
   (the screen stays dark at night), Lumen or LG's screensaver over Lumen
   is in front, and that last sign is at least 20 minutes old. `--force`
   is for when the owner says the TV is free.
2. **The build.** `--ipk` installs a build; otherwise the installed one is
   measured. The robot's `lumen-ctl` and the pattern go into
   `/var/lib/lumen/stage/`, root's own folder. The installed `lumen-ctl`
   behind the Remote's forced key is never touched.
3. **Each run** (3 by default): close the shell, launch it with the
   parameters asked for plus the bench's own:
   * `"saver":"hold"`: the shell answers LG's screensaver request
     `ack:false` for this launch only. LG's settings stay as they are.
   * `"shots":[4,58]`: the first settled frame 4 s and 58 s after the
     start is saved as a PNG beside the log and pulled to the PC. It shows
     the UI plane only, never the video plane.
   * `"exitAfter"`: 600 s (900 s with `--settle`), so a shell left with
     the screensaver held off leaves on its own if the bench dies.
4. **Automatic undo.** No `start: … first frame` line within 10 s of the
   launch: the last good build goes back (exit 4).
5. **The robot remote** plays `tools/bench/pattern-45s.txt` through
   `lumen-ctl play`: holds and taps over the grid for 45.4 s, no OK, no
   Back, no Home.
6. **The window for the numbers** runs from the log's own first to its last
   `key down`, never from a clock on the PC or a message.
   `tools/frame-stats.sh` summarises it; `tools/bench/gate.py` reads its
   text, so the bench's numbers are always frame-stats.sh's.
7. **With `--settle`**, the last run then stays settled for 5 minutes and
   reads the shell's RSS and its CPU over 30 s.
8. **Afterwards**, always, even after an error or a yield: the last good
   build is put back if another was installed (its binary's SHA-256 is
   checked), and the shell is relaunched with no parameters, so LG's
   screensaver works again.

The report goes to `tools/out/bench/<UTC stamp>-<name>/`: `run.txt` (commit,
build, parameters, model and firmware), each run's log, frame-stats output,
the robot's lines and screenshots, `run<N>.json`, `summary.json` (the
median of the runs) and `gate.md`.

## App launch times and target 5

`tools/bench.sh apps` (in a lab window) takes each app, by default Netflix,
YouTube and plx-native, through: a cold launch (closed first if it runs),
5 s in front, a robot Home press, then a warm launch and Home again.

* **Launch time** runs from the launch call to the app manager naming the
  app as the one in front, polled about every 50 ms on the TV's
  `/proc/uptime` (10 ms steps). It misses the app's own loading once in
  front (a splash, a profile screen) and the panel's queue. `luna-send
  -i`'s life events are logged but not timed, since its piped output may
  be buffered.
* **Target 5** runs from the robot's Home press (its own stamp on the TV's
  clock, written microseconds after the event; the kernel's stamp on the
  robot's node is the same clock) to the stamp of the shell's next
  `foreground: first frame after …` line, logged right after that frame's
  swap. The panel's queue after the swap (up to two refreshes) is stated,
  not measured.
* **Automatic undo:** a Home press that leaves Lumen out of front brings it
  back by a launch; if that fails too, the run stops (exit 4).
* An HDMI input is left out for now: switching to one may wake other
  devices over HDMI-CEC, which the family would notice.

## The lean track

Stage 2 of the plan stops what the "just a TV" contract never uses, one
thing at a time, and keeps a change only if the bench shows app launch
times or free memory measurably improve. The B9 had about 310 MB available
and 156 MB in swap with Lumen in front (b9.md, "Memory").

* `tools/bench.sh lean-survey` (read-only, any lab window): memory, every
  upstart job with its main process's resident memory, SAM's keep-alive
  apps, the running apps and the 40 largest processes, to
  `tools/out/lean/survey-<stamp>.txt`.
* `profiles/lean/candidates.txt` lists the jobs that may be stopped, each
  with why; it starts empty and is filled from the survey.
  `profiles/lean/never-touch.txt` refuses any job whose name holds one of
  its words (picture, sound, inputs, remote, network, apps, store, DRM,
  root access, Lumen), whatever the candidates say.
* `tools/bench.sh lean-plan` is the dry run: each candidate, its memory in
  the latest survey, and the command that would stop it, or why it is
  refused.
* `tools/bench.sh lean-try JOB` (tier B, a lab window, a backup from the
  last 24 hours): notes the job in `/var/lib/lumen/lean/stopped`, stops it
  with `initctl stop` until the next boot, and logs the memory available
  before and 10 s after to `tools/out/lean/changes.tsv` with its undo.
* `tools/bench.sh lean-undo [JOB]` starts again only what that record
  lists. A reboot undoes every stop.

A stop that helps becomes lasting only through a boot hook, tested like any
other (safety rule 3).

## The robot remote

`lumen-ctl play FILE`, `lumen-ctl key NAME` and `lumen-ctl watch-remote
SECONDS` (shell/crates/lumen-ctl/src/robot.rs). `play` makes a virtual
remote, "Lumen robot remote", through `/dev/uinput` (the kernel is 4.4, so
the old `uinput_user_dev` set-up), waits 1.5 s for the compositor to find
it, then plays the pattern with fixed timing.

* **Codes.** The Linux codes the Magic Remote's own node declares (its key
  map, read on 2026-11-08): arrows 103, 108, 105, 106; OK 28; Back 412;
  Home 773 (LG's code, held 150 to 200 ms before each trip through LG's
  launcher in the 2026-10-22 recordings; not yet tried from the robot);
  play 207, pause 119, stop 128, rewind 168, fast forward 208.
* **Yield.** Every other input node is opened read-only before the robot's
  exists and never grabbed. Any event there other than a separator or a
  scan code is a person: the robot lets go of what it holds and stops
  (exit 3), and the bench puts things back. An event on another node with a
  code the robot sent within 250 ms is counted as an echo instead, in case
  LG's input service mirrors new devices; the run's last line counts them.
* **Repeats.** The real remote's held keys reach the shell as SDL repeats,
  the first about 510 ms after the press, then every 50 ms (B9, 2026-11-07).
  The pattern sends only presses and releases (`repeat off`); whether LG
  makes those repeats for a robot's held key too is for the first run's
  `SDL repeat` lines to show. `repeat 500 50` sends kernel repeats instead.

## The speed gate

`tools/bench.sh gate DIR` (and the end of every run) compares the run's
`summary.json` with `tools/out/bench/baseline.json`, the last good build's.
The limits are the plan's, to be tuned once the noise between runs is known:

| Limit | Allowed |
|---|---|
| Average frame while moving | at most the baseline + 0.3 ms, and at most 17.2 ms |
| Vsyncs skipped per 10 s of motion | at most max(12, 1.5 × the baseline's) |
| Start-up to the first frame | at most 600 ms |
| RSS after 5 min settled | at most the baseline + 5 MB |
| Home press to Lumen (target 5) | from the new method, once measured |

A number that either side lacks fails, so nothing passes unmeasured.
Nothing merges to `main` past a failing gate.

## Measured

None yet: the first runs are the next lab window's.
