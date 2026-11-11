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
   measured. After an install the bench makes the icon copy if the new
   build has none (the shell's own script, as root) and launches the build
   once without timing it, until that start's icon copy has ended; only
   then do the timed runs begin. `run.txt` lists the apps running behind
   Lumen, and each run's numbers carry its icon count (`icons`): runs with
   different counts do not compare. The robot's `lumen-ctl` and the pattern go into
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

## Coming back in front

`tools/bench.sh front` measures the frames the shell draws when a Home
press brings it back over another app: the hero is decoded again (the
shell frees it while hidden) and fades in over the gradient.

1. The shell is closed and launched with `--params` (default
   `{"platform":"wayland"}`, the path a Home press gets from `lumend`'s
   hidden shell) plus `"saver":"hold"` and `"exitAfter"`. The run waits for
   the first frame and for the start's icon copy to end.
2. **Each trip** (3 by default): the other app (`--from`, default
   `netflix`) is launched and stays in front for 8 s; the robot presses
   Home for 180 ms; 8 s later the shell's log is read from the press on.
3. **The window** is the log's own: from the shell's `lifecycle: relaunch`
   line to 2 s after its first `crossfade … to <hero>` line.
   `tools/bench/front.py` takes from it the shell's `drew N frames` lines
   (the fade's is the last one before the crossfade line), each `slow
   frame` line with its loop, batch, upload, submit and swap split, the
   hero's decode and upload times, and the press to the first frame
   (target 5). It computes no frame time itself.
4. **Afterwards:** the other app is closed if it was not running before,
   and the run ends like any other (the last good build back, a plain
   relaunch, the HDMI input back in front).

The report holds `front<N>.log`, `front<N>.json`, `summary.json` (the
medians) and, in `run.txt`, the apps running behind Lumen. There is no
gate for this pattern yet: its first numbers are the baseline a limit will
come from.

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
  play 207, pause 119, stop 128, rewind 168, fast forward 208. Added
  from the same key map on 2026-11-11 for the Remote: Info 358, Exit 174,
  channel up and down 402 and 403, red, green, yellow and blue 398 to 401,
  and the numbers (1 to 9 are 2 to 10, 0 is 11). Pressed the same day
  through `lumen-ctl serve --keys inject` with Lumen's shell in front (B9
  OLED55B9PUA, firmware 05.50.00, the SDL path): each of 1, 5, 0, the four
  colours, Info, channel up, channel down and Exit reached the shell, 78 to
  79 ms a press from the PC. The shell's log names them as the numbers'
  own key codes (0x31, 0x35, 0x30) and SDL scancodes 486 to 489 for red,
  green, yellow and blue, 499 for Info, 480 and 481 for channel up and
  down, and 505 for Exit; Lumen stayed in front after every one. What they
  do inside LG's apps and Live TV is not tried. `menu` is the gear, 139
  (b9.md); sent the same day with Netflix in front, the TV took it in 78 ms,
  and the owner saw LG's settings panel open ("Yes, it opens").
* **The pointer.** `serve --keys inject` also takes `pointer.move` and
  `pointer.click` for the Remote's touchpad, written into the same node the
  way the Magic Remote sends its own (b9.md): LG's 1198 when the pointer
  appears, its place as `ABS_X` and `ABS_Y` scaled to the node's own range
  (read with `EVIOCGABS`) from where the kernel last had it, `BTN_LEFT`
  (272) for a click, and LG's 1199 once it has rested 5 s. Built and tested
  on the PC on 2026-11-11; **not yet tried on the TV**, so whether LG's
  compositor draws a pointer for it is not known.
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

### 2026-11-10: the new home, the two paths and the Home key

On the B9 (OLED55B9PUA, webOS 4.10.2-31, firmware 05.50.00), 02:45 to
03:40 UTC, the owner having given the TV over (`--force`), Netflix and
YouTube closed behind Lumen. Medians of three runs.

| Build, pattern | Average frame | Skipped per 10 s | Start-up | Gate |
|---|---|---|---|---|
| 3a89955, home (the baseline again) | 16.84 ms | 2.14 | 502 ms | |
| 8a4d844, home, full look | 17.01 ms | 0.00 | 549 ms | pass |
| 8a4d844, pages-43s | 17.07 ms | 7.84 | 537 ms | pass |
| 8a4d844, control-45s | 16.58 ms | 0.69 | 573 ms | pass |
| bc053bd, pages-43s (before the fills) | 19.34 to 19.75 ms | 77 to 88 | | fail |
| f19929a, home, dither from a 4x4 texture | 18.98 ms | 60.52 | 555 ms | fail |
| main with every seat bound, SDL path | 16.70 ms | 0.45 | 562 ms | pass |
| the same build, `{"platform":"wayland"}` | 17.00 ms | 1.30 | 440 ms | pass |

The two paths, five cold starts each, the same build: the Wayland path's
first frame came after 422 to 502 ms (median 436) and the SDL path's
after 504 to 529 ms (median 520); 8 s in, the shell held 40.5 MB on the
Wayland path and 49.4 MB on SDL.

**The Home key acts on its release.** A robot Home press with YouTube in
front and the shell alive behind it, timed from the key going down to the
swap of Lumen's frame: 91 ms for a 40 ms hold, 217 ms for 180 ms and
441 ms for 400 ms, twice each. So the TV takes 41 to 51 ms from the
release, of which the shell's frame is 2 to 3 ms, and the 216 to 229 ms
above was the bench's own 180 ms hold plus that. `apps` now says the hold
and the time after the release.

**A build measured with apps running behind it reads slower.** The same
9d5a988 read 16.92 ms on 2026-11-09 and 17.21 to 17.28 ms on 2026-11-10,
with the baseline unchanged; closing Netflix and YouTube (246 MB more
available) brought the skips back but not the average. Not explained.

**A shell started hidden answers Home in the time of a running one.**
With HDMI 3 in front, a 180 ms robot Home press, three times each: with
the shell closed, Lumen's first frame came 843 to 864 ms after the key
went down (663 to 684 ms after its release, a cold start on the SDL
path); with the shell started hidden on the Wayland path
(`{"platform":"wayland","hidden":true}`, the input staying in front),
216 to 218 ms (36 to 38 ms after the release). That is what `lumend`'s
`shell` part is for.

**The B9 lists two `wl_seat`s, and the remote's keys come on the second.**
With only the first bound, the Wayland path drew and heard Home (through
its luna registration) but no arrow reached it; with every seat bound,
all 90 keys of the bench's pattern did, in each of three runs. Its key stamps are on a clock of the compositor's
own, 2.9 s behind CLOCK_MONOTONIC, so the Wayland path times a key from
when the shell reads it.

**With `lumend` keeping the shell hidden** (its `shell` part, the Wayland
path), 2026-11-10 05:05 to 05:20 UTC, HDMI 3 in front, a 180 ms robot
Home press: 222, 194 and 195 ms from the key going down (42, 14 and 15 ms
from its release); after a reboot, 229, 195 and 198 ms (49, 15 and 18 ms).
The first press after a hidden start is the slower one: its frame takes
31 to 33 ms where the later ones take 2 to 5 ms. After that boot lumend
started the shell 157 s in: it waits two minutes for an app to come in
front, and the input was in front before lumend was up.

The six design notes, 90a3880, every run with 30 of 30 icons: home, full
look 17.04 ms and 1.34 skipped per 10 s, start-up 551 ms; pages-43s
17.14 ms and 10.20; control-45s 16.98 ms and 1.71. The gate passes on all
three. 0a537bf, before icon tiles were drawn as fills, read 17.92 ms and
34.67 on pages. A run straight after an install starts while the first
icon copy is still going and read 590 to 728 ms to its first frame; the
same build without a reinstall starts in 550 to 582 ms.


**The pointer build, 57c4831** (hover moves the focus, a click opens what
is under it), 05:22 to 05:55 UTC, forced, Netflix and YouTube running
behind Lumen, every run with 30 of 30 icons: home, full look 17.02 ms and
0.00 skipped per 10 s, start-up 579 ms; pages-43s 17.12 ms and 9.67,
start-up 600 ms; control-45s 16.50 ms and 0.00, start-up 552 ms. The gate
passes on all three.

**A build installed over the last good one starts with 4 of 30 icons.**
The first Pages and Control Center runs of 57c4831 drew 4 icons (17.13 ms
and 16.51 ms; they do not count). The last good IPK is made from the TV's
folder, icon copy included, so the installer removes the copy with it; the
shell refreshes only a copy that exists, and `lumend` makes one only when
it starts. The bench now makes the copy after an install and launches once
untimed. The untimed launch did not bring the start-up down: the three
Pages starts after it read 613, 539 and 600 ms.

**Coming back in front** (`tools/bench.sh front`), 57c4831 on the Wayland
path, Netflix in front, a 180 ms robot Home press, three trips: the fade
drew 36, 35 and 35 frames in 680, 699 and 695 ms with 7, 8 and 8 vsyncs
skipped (median 8), and 3, 4 and 3 slow frames of 25 to 74 ms, all of it
submit time (the worst 70.10, 56.34 and 72.36 ms; upload at most 0.02 ms,
swap under 1 ms). The hero was decoded in 344, 324 and 324 ms, and
Lumen's first frame came 192, 201 and 194 ms after the key went down.
With 4 icons the fade read the same (8 skipped, worst submit 73.56 ms),
so the icon tiles are not the cause. This is the baseline for the fix.

**A first fix for coming back in front did not help.** f6d2025 draws each
hero layer once right after its upload, before the fade (06:07 to 06:13
UTC, the same conditions): the fade drew 34, 35 and 33 frames in 637, 696
and 686 ms with 9, 8 and 10 vsyncs skipped (median 9), and four slow
frames a trip, the worst submit 48.67, 64.68 and 88.39 ms. The draw before
the fade took 50.6 ms on the first trip and 3.9 and 6.1 ms after, so the
hero texture's first draw is not where the time goes. Its full look read
17.01 ms and 0.45 skipped per 10 s. Not merged.

**The fix that did: the fade waits for the window's focus.** In every slow
trip above, the 50 to 90 ms frames came before the shell's `window focus
gained` line, which LG's compositor sends 530 to 590 ms after the Home
press. 06:29 to 06:55 UTC, in a lab window, the same pattern:

| Build | Fade | Vsyncs skipped | Slow frames, worst submit | Key down to first frame |
|---|---|---|---|---|
| 57c4831 (before) | 35 frames in 695 ms | 8 | 3, 70.10 ms | 194 ms |
| b98f274: the fade waits for the focus | 32 frames in 532 ms | 2 | 1, 32.24 ms | 230 ms |
| 50e27b6: and the gradient is a fill | 32 frames in 513 ms | 1 | 0 | 217 ms |
| a433623: and a fade after a wait starts whole | 41 frames in 678 ms | 2 | 1, 44.52 ms at most | 211 ms |

The fade waited 136 to 263 ms for the focus, so the hero's art shows that
much later than before. The first two rows' fades were cut short by the
wait (a433623 fixes that). a433623 is merged: its home, full look, read
17.01 ms and 0.90 skipped per 10 s; Pages 17.11 ms and 9.42 and the
Control Center 16.50 ms and 0.58 were read on 50e27b6, every run with 30
of 30 icons and past the gate.
