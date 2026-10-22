---
name: lumen-tv-check
description: Checks a Lumen build on the owner's LG OLED B9 the same way every milestone does. Builds the IPK, installs and launches it, collects frame statistics from the app's log for a set number of seconds, closes the app and puts back what was installed before, then reports average, worst and percentile frame times with the TV's model and firmware. Use when a milestone's "done when" needs a measurement on the TV (hello-gles at 60 fps, the home screen's frame time while moving, a milestone's closing check) or when asked to "check it on the TV" or "measure the frame times". Every TV step waits for the owner's yes.
---

# Lumen TV check

One check, run the same way by every milestone, so numbers from different
milestones compare. It builds the IPK, installs it, launches it, reads the
frame statistics the app logs, then closes the app and puts the TV back as it
was. [CLAUDE.md](../../../CLAUDE.md) requires that "performance claims cite a
measurement on a real TV, with model and firmware"; the simulator proves
layout, never frame rates. [docs/safety.md](../../../docs/safety.md) applies to
everything here.

## Rules

1. **Ask before every TV step.** A TV step is any command that reaches the TV,
   which here means every `tools/tv.sh` call. They are grouped into four:
   **A** look and back up, **B** install, **C** launch and measure, **D** close
   and restore. Before each, say in chat what it will run and change, and wait
   for a clear yes. The repo's permission prompt on `tools/tv.sh` is not the
   ask; it is a second gate. Building, making the restore IPK and reading the
   log on this PC need no ask.
2. **Start only when the owner says the TV is free.** The app takes over the
   screen, and other sessions may be using the TV (pairing a pad, capturing).
3. **Nothing persistent.** No boot hooks, no settings, no reboot, no file on the
   TV beyond the app's own directory and its log. Run only the TV commands
   listed here; anything else is a new ask.
4. **Once step B has run, always finish with step D,** even if a step failed or
   the owner stops early. If the restore cannot be verified, say so plainly.
5. **Report numbers; don't rule on the milestone.** The milestone's own text
   says what they mean and where they are recorded. Another branch's docs are
   the owner's call.

## Setup

Use Git Bash, from the root of the checkout under test (the milestone's
worktree or the main checkout). Shell state does not persist between tool
calls, so repeat the assignments at the top of every command that uses them.

```sh
APP=io.github.sunnyside007.lumen.shell        # APP_ID in the Makefile
OUT=dist/tv-check/<UTC stamp, e.g. 20261022T031500Z>   # one folder per check; dist/ is git-ignored
STATS=tools/frame-stats.sh
[ -f .env ] || export LUMEN_ENV_FILE=/c/Claude/WebOS/.env   # a worktree has no .env of its own
```

* `STATS`: a checkout from before P0 has no `tools/frame-stats.sh`. Use the
  helper from the checkout that has it, by its full path; it only reads a log.
* Settings: 30 seconds of statistics after 5 seconds that are skipped (shader
  compile, first frames). Change them only if the milestone says so, and report
  what was used.
* Other apps (RetroArch in P3) use steps A, B and D unchanged with their own
  `APP`; the statistics step reads only Lumen's log lines.

## 1. Build (on this PC)

```sh
git log --oneline -1; git status --short
find shell \( -name target -o -name target-host \) -prune -o \( -name '*.rs' -o -name Cargo.toml -o -name Cargo.lock \) -print | xargs touch
sh tools/dev.sh make ipk
```

* Docker Desktop must be running (`docker info`).
* The `touch` is deliberate. Every checkout builds at `/work` against the one
  `lumen-target` volume, and cargo judges a crate fresh by file times, so a
  checkout whose files are older than another checkout's last build could
  reuse that build's output. Confirm the build prints `Compiling lumen-shell`.
* The IPK is `dist/${APP}_<version in shell/Cargo.toml>_arm.ipk`. Every build
  has that same name, so it says nothing about which commit it is; the report
  records the commit instead.

## 2. Step A: look, and back up what is there (ask first)

Read-only on the TV.

```sh
sh tools/tv.sh check
sh tools/tv.sh run 'ls -d /media/developer/apps/usr/palm/applications/'$APP' 2>/dev/null || echo NOT-INSTALLED; pidof lumen-shell || echo NOT-RUNNING; if pidof tcpdump >/dev/null; then echo "a capture (tcpdump) is running"; fi' </dev/null
```

* `check` prints the model and firmware for the report (`modelName`,
  `firmwareVersion`).
* Every `tv.sh` call prints OpenSSH's "no post-quantum key exchange" warning on
  stderr. It is harmless; leave it out of the report.
* If the app is running, stop and ask the owner; don't close it unasked.
* Use `pidof`, never `ps`, to see whether the app runs. The B9's `ps` is
  procps, which with no options lists only the caller's terminal's processes,
  so it never shows an app.
* Name anything else of ours that loads the TV in the report, such as Phase 1's
  DNS capture (`tcpdump`). It ran through this skill's first runs.
* **If it is installed,** copy it to this PC and make a restore IPK from the
  copy, before anything is installed. All builds share one app ID and version,
  so no IPK file on the PC can be trusted to match what is on the TV.

  ```sh
  mkdir -p "$OUT"
  sh tools/tv.sh run 'tar -cf - -C /media/developer/apps/usr/palm/applications '$APP </dev/null > "$OUT/previous.tar"
  tar -tvf "$OUT/previous.tar"          # must list $APP/appinfo.json and $APP/lumen-shell
  sh tools/dev.sh sh -c "mkdir /tmp/prev && tar -xf $OUT/previous.tar -C /tmp/prev && ares-package --no-minify --outdir $OUT/restore /tmp/prev/$APP"
  ls -l "$OUT"/restore/*.ipk
  ```

  Repackaging an app directory this way gave the same control fields, files,
  modes and binary SHA-256 as the original IPK in an off-TV test. If the
  listing is wrong or no restore IPK appears, stop. Don't install over an app
  that can't be put back. The owner may name an IPK to restore instead.

  `ares-package` makes every folder 0777, and an install applies the IPK's
  folder modes to the shared `applications` folder too. That is how the TV
  was before, so the restore keeps it. Once `tools/ipk/fix_modes.py` (shell
  branch) is on `main`, run it on the restore IPK as `make ipk` does there.
* **If it is not installed,** the restore is removing it.

## 3. Step B: install (ask first)

```sh
sh tools/tv.sh install dist/${APP}_<version>_arm.ipk
```

Expect `installed`. On `install failed` or `install timeout`, go to step D.

## 4. Step C: launch and measure (ask first; the owner is at the remote)

Agree first on what the owner does on the remote while the clock runs:

* **hello-gles** (any build from `main`): nothing. It animates by itself.
* **The home screen:** a settled shell stops drawing, so with no motion there
  are no statistics at all. The owner moves for the whole window, the same way
  each time: hold RIGHT along row 0 to its end, DOWN through every row, then
  LEFT and UP back, repeating. No OK (it may launch a real app), and no BACK
  on the top row (it closes the shell). A build with the key-diagnostics
  lines takes a click while the pointer shows as OK, so no clicks either.
* **Focus and keys (B9, 2026-10-21):** after `tv.sh launch`, no remote key
  reached the shell until the owner woke the Magic Remote's pointer. The shell
  also lost focus to LG's UI twice with nothing on our side, and every key
  then went elsewhere. The shell logs both: `the window lost focus: drawing
  stops` and `in front again (<cause>): drawing resumes` (the B2 and B3 builds
  print no cause). So the owner wakes the pointer and checks that an arrow
  moves the focus before the clock starts. B1's run, which got keys, was opened
  by hand from LG's app list; try that if keys still don't arrive.
* **The window, and who is in front (B9, 2026-10-22):** when keys may be
  missing, don't take the window from a `T0` read. A `T0` only says when the
  owner meant to start. Take it from the log's own first and last `key down`
  after the launch, and use those two stamps as `--since` and `--until`. Don't
  confirm that the shell is the app in front by eye either. The log says so:
  `key down`, `fps` and `drew` lines are coming in, and no `the window lost
  focus: drawing stops` has come since the last `in front again`.
* **A build that lists apps through `luna-send`** (B3's first version) shows
  no apps when launched as an app. Inside the app's jail `luna-send` fails
  with `Permission denied` (B9, 2026-10-21), so there is nothing to move
  through. Builds that call luna through the library (c500289 on) list
  30 tiles from inside the jail (2026-10-22); measure those, or a build with
  mock tiles (B2's).

Then, quickly one after the other:

```sh
T0=$(sh tools/tv.sh run 'date +%s' </dev/null | tr -d '\r')      # the TV's clock; note the number
sh tools/tv.sh launch $APP                                       # expect "returnValue": true
sh tools/tv.sh run 'sleep 35; date +%s' </dev/null               # 5 s warm-up + 30 s; prints END
```

For the shell, keep the first reading as `LAUNCH0` and read the clock again as
`T0` once the owner confirms that keys reach it (or take the window from the
log, as in the 2026-10-22 note above), then run the wait. Write "start" when
the wait begins and "stop" when it returns. If the harness blocks the `sleep`,
run that call in the background. Then fetch the whole log (the app appends to
it across runs; the lookup is the one `tools/tv.sh log` uses for a jailed
`/tmp`) and summarise the window:

```sh
sh tools/tv.sh run 'n=0; for f in /tmp/lumen-shell.log /tmp/lumen-shell.fallback.log; do [ -f "$f" ] && tail -n 20000 "$f" && n=1; done; [ "$n" = 1 ] || { f=$(find /var/palm/jail -name lumen-shell.log 2>/dev/null | head -n 1); [ -n "$f" ] && tail -n 20000 "$f"; }' </dev/null > "$OUT/run.log"
sh $STATS --since $((T0 + 5)) --until $END "$OUT/run.log"         # END from the wait
grep 'GL vendor' "$OUT/run.log" | tail -n 1
```

`frame-stats.sh` exits 1 with "no frame statistics" if the app drew nothing in
the window. Then read the tail of `run.log` to see why and don't report
numbers. Possible causes are a crash, a launch that never started, a shell
without focus or keys (look for `key down` and the focus lines), or a shell
that settled before the owner moved. Check `pidof lumen-shell` before calling
it a crash: on 2026-10-21 a B2 that looked crashed was alive, only without
focus. Add `--miss-ms` only if the milestone sets another line; the default
25 ms is a frame and a half at 60 Hz.

A build with the timing lines gets their sections from the same `frame-stats.sh`
run, printed after the frame statistics. They are printed even when there are
no frame statistics, and the exit status is then still 1. `--since` drops every
line before it, and the `start:` line is logged a second or two after the
launch, so a `--since` after the launch loses it. To include it, make a second
pass over the whole run, with a `--since` from before the launch (`LAUNCH0`,
read before `tv.sh launch`, not the `T0` read once keys work). Report the frame
numbers from the first pass and the start-up from the second.

What the log holds, for reading the output:

* `fps … avg … ms max … ms (…)`: one line per window of about a second, from
  hello-gles and the shell. A window has an average and a worst frame.
* `drew N frames in … ms (avg … ms, worst … ms, … quads); first …, …, … ms;
  idle`: one line per burst of motion, from the shell. `first` gives the times
  of the burst's frames 2, 3 and 4 (frame 1 follows an idle wait and has
  nothing to be timed from); a short burst gives fewer, a 1-frame burst none.
  The output summarises them on a "first frames" line.
* `slow frame n: … ms = loop … + batch … + upload … + submit … + swap … ms; K
  uploads, X KB, G glyphs; faults … minor, … major`: a frame over 25 ms, `n`
  its place in the burst, timed from the previous frame's swap to its own.
  The five parts add up to its time; [look-and-feel.md, "Log
  lines"](../../../docs/specs/look-and-feel.md#log-lines) says what each
  covers. `faults` are the main thread's page faults since the previous frame;
  a major one read a page back from swap. A burst logs at most 4, and `slow
  frames: K more not logged`, just before its `drew` line, counts the rest.
  The output summarises them in a "slow frames" section.
* `SDL repeat K of scancode S: … ms after the press, … ms after the last`: the
  remote's key repeats (B3 builds on), summarised after the frame statistics.
* `key S: shown after … ms`, `key S: not shown after … ms: <why>` and `keys: K
  more presses not timed`: the time from a press to the frame that shows it.
  The output gives p50, p95 and max, how many were within 33 ms (target 2), and
  the presses not shown by reason.
* `start: …`, `foreground: …` and `launch <id>: …`: the start-up split into its
  phases, the wake-ups from the background, and how long luna took to answer
  each launch (target 6, 100 ms).
* `window focus lost: …`: how long since the last key. The output counts the
  focus losses with no key pressed.
* The key-diagnostics lines (written on `claude/shell-timing`, K1–K5, not yet
  run on the TV; formats in [look-and-feel.md, "Key
  diagnostics"](../../../docs/specs/look-and-feel.md#key-diagnostics)).
  "Input" is any key, 484 and 485 included, and any mouse event.
  * `pointer shown` and `pointer hidden`: scancodes 484 and 485, the Magic
    Remote's pointer appearing and going.
  * `mouse button <b> down at <x>,<y>: taken as OK`: a left click while the
    pointer shows. It is a launch, timed as `key 40`, so step C's "no OK"
    covers clicks too. `…: ignored` is any other button; `mouse button <b>
    up` is the release.
  * `pointer: SDL_webOSCursorVisibility found …` or `… not in LG's SDL …`:
    once at start-up, whether arrows hide a shown pointer.
  * `power <state>: last input … s ago`: LG's power state (`Active`,
    `Screen Saver`).
  * `front app <id>: last input … s ago`: what came in front. A line naming
    `com.webos.app.screensaver` or another app explains a focus loss.
  * `screensaver request: allowed; last input … s ago`: LG asked to start
    the screensaver, and the shell let it.
  * `luna watch <what>: <error text>`: one of the three subscriptions was
    refused; it logs nothing more until it answers again. `luna watch
    thread:` lines mean the watch itself could not start.
  * `no input for <N> s; focused …; pointer …; power …`: every 30 s with no
    input, up to 10 min, then every 10 min. These lines bound a gap without
    a `T0` read: the owner can say "I was pressing at that line".
  * `frame-stats.sh` will summarise them in an "input" section (another
    session is writing it).
* `key down scancode …` and `luna <method>: N ms` are not timing lines; the
  script leaves them out.
* The log times only a burst's first and slow frames, not every frame. The
  percentiles are over windows or bursts, nearest rank, and the output says
  so; keep that sentence when you pass the numbers on. The key, launch and
  foreground figures are over single events.

## 5. Step D: close and restore (ask first)

```sh
sh tools/tv.sh close $APP
```

Then, by what step A found:

* **Not installed before:** `sh tools/tv.sh remove $APP`
* **Installed before:** `sh tools/tv.sh install "$OUT"/restore/<the .ipk>`

Verify with step A's second command: the first line must say `NOT-INSTALLED`
(or the app's directory, for a restore) and the second `NOT-RUNNING`. For a
restore, also compare the binary with the backup:

```sh
sh tools/tv.sh run 'sha256sum /media/developer/apps/usr/palm/applications/'$APP'/lumen-shell' </dev/null
tar -xOf "$OUT/previous.tar" $APP/lumen-shell | sha256sum
```

The two sums must match. A shell build also logs `clean exit` when it closes.
The app's log stays where it is (a native app's
private `/tmp`, or `/tmp`); it is the app's own file and goes away with it.

## 6. Report

Give the owner this, for the milestone to record. Quote the output of
`frame-stats.sh` as it printed. Don't round a miss away or call a window better
than it was.

```
TV check: <APP> <version> from <branch> @ <short sha> (clean | uncommitted changes)
TV: <modelName>, firmware <firmwareVersion>; GL <renderer line>
Run: <UTC date>; 30 s of statistics after 5 s of warm-up;
     <what was on screen and what the owner did>; also running: <for example a DNS capture, or nothing>
<frame-stats.sh output>
Afterwards: <removed | previous build reinstalled, binary SHA-256 matches the backup>; app not running
Raw log: <OUT>/run.log
```

For comparison, the B9 (OLED55B9PUA, firmware 05.50.00) has measured:

| Build | Result |
|---|---|
| hello-gles (0932965, 2026-10-20), about 3,000 quads | a steady 60.0 fps, 16.67 ms average, 17–19 ms worst frame per second ([b9.md](../../../docs/device/b9.md)) |
| the home grid, B1 (3572bf3, 2026-10-20), moving | about 520 frames in 13 bursts averaging 16.2–16.7 ms; worst frame at or under 17.8 ms in 11 bursts, 19.6 ms once, and one 33.3 ms miss; 856–1,631 quads (`docs/specs/shell-mvp.md` on the shell branch) |
| hello-gles, about 3,000 quads, this skill's first run (2026-10-21; 30 s after 5 s of warm-up; Phase 1's DNS capture running) | 60.0 fps, 16.68 ms average; window worst frame p50 17.55 ms, p95 24.57 ms, longest 24.89 ms (5 of 30 windows above 19 ms); no frame over 25 ms |
| the home grid, B2 (3543c41, mock tiles), moving for about 4.4 s only (2026-10-21; DNS capture running) | about 270 frames in 6 bursts, 16.68 ms average (60.0 fps); 3 frames of 25.41, 26.17 and 31.84 ms; 26–51 quads. Too short to judge B2's shader cost |
| the home grid, c500289, 09711d7 and df2a4d0, moving (2026-10-22; reports in `tools/out/archive/tv-check-20261022T*`) | averages 16.80–16.84 ms, but a frame over 25 ms in most bursts (37 of 50, 16 of 20, 11 of 14); [look-and-feel.md, target 1](../../../docs/specs/look-and-feel.md#snappy) |

A reading of "holds 60 fps" means an average near 16.67 ms (up to about
16.8 ms) with the missed vsyncs counted and named, not hidden. A frame of
about 33 ms is one missed vsync.

## If something goes wrong

* **SSH fails or the TV stops answering:** stop, tell the owner, and follow the
  escalation in [docs/safety.md](../../../docs/safety.md). Don't power-cycle or
  retry in a loop.
* **The owner needs the TV:** close and restore at once (step D), then report
  what was measured so far, labelled partial.
* **The restore does not match:** say so, with the two sums, and leave the TV as
  it is until the owner decides.
