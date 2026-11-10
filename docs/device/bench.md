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
   is in front, and that last sign is at least 20 minutes old. In the
   owner's weekday slot (Monday to Friday, 10:00 to 15:00 TV time) an HDMI
   input in front, or under LG's screensaver, is allowed too; the run
   notes it as `back to` in `run.txt` and launches that input again when
   it ends, also after a yield or an automatic undo. `--force` is for
   when the owner says the TV is free.
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

## One job at a time, and a person first

* **The TV lock.** `run`, `apps`, the lean commands and `restore` take
  `tools/out/bench/tv.lock`; a second job waits for it, up to an hour. TV
  jobs outside the bench (a Guard trial night) run under
  `tools/bench.sh with-lock CMD`. A lock whose process has gone is taken
  over, so a stopped job never blocks the next.
* **A yield shuts the window.** The TV's idle record leaves out the bench's
  own time ranges, so the robot's keys don't count as a person. A
  person's key during a run falls in such a range too, so a yield also
  writes its time to `tools/out/bench/person-at`, and the window stays
  shut for 20 minutes from it. On 2026-11-08 a run yielded to the Magic
  Remote's pointer appearing (LG's code 1198: someone picked it up), and
  the next run started 36 s later; that is the case this closes.

## The robot remote

`lumen-ctl play FILE`, `lumen-ctl key NAME` and `lumen-ctl watch-remote
SECONDS` (shell/crates/lumen-ctl/src/robot.rs). `play` writes the pattern's
key events into the Magic Remote's own input node ("LGE M-RCU - Builtin",
event3 on the B9), with fixed timing, and only falls back to a `/dev/uinput`
device when no such node exists. The ready line names the route (`via`).

A `/dev/uinput` remote does not work on the B9: LG's surface-manager opens
event0 to event5 once at boot and there is no udevd, so a device made later
is never read (smoke run 1, 2026-11-08: no key reached the shell). Writing
an `input_event` into an evdev node hands it to every reader of that node,
surface-manager included, so the shell sees the robot's keys as the remote's
(smoke run 2 on 3a89955, B9 OLED55B9PUA, firmware 05.50.00: 90 of 90 presses
shown, each 17 to 18 ms after the key).

* **Codes.** The Linux codes the Magic Remote's own node declares (its key
  map, read on 2026-11-08): arrows 103, 108, 105, 106; OK 28; Back 412;
  Home 773 (LG's code, held 150 to 200 ms before each trip through LG's
  launcher in the 2026-10-22 recordings; not yet tried from the robot);
  play 207, pause 119, stop 128, rewind 168, fast forward 208.
* **Yield.** Every input node, the remote's included, is opened read-only
  before the first key and never grabbed. Any event other than a separator
  or a scan code is a person, unless it carries a code the robot wrote
  within 250 ms (its own echo: on the remote's node every key comes back
  once). A person's key makes the robot let go of what it holds and stop
  (exit 3), and the bench puts things back. The run's last line counts the
  echoes; smoke run 2 had 180 for 90 presses, a down and an up each.
* **Repeats.** The real remote's held keys reach the shell as SDL repeats,
  the first about 510 ms after the press, then every 50 ms (B9, 2026-11-07).
  The pattern sends only presses and releases (`repeat off`); smoke run 2
  logged no `SDL repeat` lines, so the shell's own repeat (400 ms, then
  every 120 ms) is what a held robot key gives. `repeat 500 50` sends kernel
  repeats instead.

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

A number that either side lacks fails, so nothing passes unmeasured. The
RSS row is for `--settle` runs only, the ones that read memory; a settled
run whose RSS read failed still fails it. Nothing merges to `main` past a
failing gate.

## Measured

B9 (OLED55B9PUA, webOS 4.10.2-31, firmware 05.50.00), 2026-11-08, 21:48 to
22:30 UTC, in a window the owner cleared. The robot's 45 s pattern, three
runs a build, the median of each; reports in `tools/out/bench/`.

| Build | Look | Average frame | Skipped per 10 s | Start-up | RSS settled |
|---|---|---|---|---|---|
| 3a89955 (last good) | its only one | 16.89 ms | 4.82 | 541 ms | 42,848 kB |
| 86b3527 (dither fix) | plain | 16.94 ms | 7.53 | 514 ms | 42,144 kB |
| 86b3527 | -dither | 17.07 ms | 7.26 | 536 ms | |
| 86b3527 | full | 29.62 ms | 256 | 528 ms | |
| d7de95e (home design) | plain | 16.49 ms | 0.30 | 521 ms | |

Between runs of one build at 60 fps the average moved by at most 0.12 ms
and the skips by up to 4.7 per 10 s, so the skip limit's floor of 12 is
not too tight. Every press was shown within 17 to 18 ms (36 ms once in
270).

App switches (`apps`, with Lumen at 3a89955): a cold start in front after
1,710 ms (Netflix), 1,990 ms (YouTube) and 2,280 ms (plxnative); warm,
120 to 220 ms. Target 5, a Home press to Lumen in front: 216 to 229 ms in
all six.
