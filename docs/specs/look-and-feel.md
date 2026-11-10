# Look and feel: snappy and pretty

Lumen is meant to be a snappy, pretty home in the direction of tvOS 26
([README](../../README.md), [decision 1](../decisions.md)). This spec turns
those two words into checks. Each **snappy** target has a number, a way to
measure it on the B9 and the phase that checks it. Each **pretty** item has a
look, a GPU cost and a fallback.

Drafted 2026-10-22 for the owner's review. A number marked *proposed* is a
target, not a result, until a run on the B9 backs it (CONTRIBUTING.md, rule
3). "Measured" means the owner's OLED55B9PUA, webOS 4.10.2-31, firmware
05.50.00.

Today's layout and motion values are in `docs/specs/shell-mvp.md` (on `main`
since the shell merged on 2026-10-31). This spec doesn't repeat them;
it sets the bar they are judged against.

## Principles

1. **The remote is answered at once.** A press changes the picture on the next
   frame. Nothing on that path waits for the disk, the bus or the network.
2. **Focus is the only decoration.** One tile is lifted and lit; the rest step
   back. Depth comes from lift, shadow and light.
3. **Glass is scarce.** Only panels that float over content get it, within the
   GPU budget below.
4. **Still when nothing happens.** A settled screen stops drawing. No endless
   idle animation: it costs CPU, and a moving screen is no kinder to the OLED
   than a dim one.
5. **Readable from the couch.** Large type, high contrast, no thin weights.
6. **The direction, not the assets.** No Apple artwork, icons or fonts; Inter
   instead of SF Pro ([research.md](../research.md#design-reference)).

## Snappy

| # | Target | Measured so far | Target | How it's measured | Checked in |
|---|---|---|---|---|---|
| 1 | Frame time while moving | B1 grid (3572bf3, 2026-10-20), 13 bursts (~520 frames): averages 16.2–16.7 ms, worst ≤ 17.8 ms in 11, one 19.6 ms, one missed vsync (33.3 ms). B2/B3 (shape shader, atlas), 2026-10-22: c500289 with Phase 1's capture running, 50 bursts (2,611 frames): average 16.83 ms, a frame over 25 ms in 37 (and in 32 of 43 one-second windows: about 7 per 10 s), worst 50.6 ms; 09711d7 with no capture, 20 bursts (1,399 frames, 23 s drawn): average 16.82 ms, a frame over 25 ms in 16 (about 7 per 10 s), worst 35.1 ms. The misses come mostly in a single tap's burst and in Down's scroll (shell-mvp.md, "Measured on the B9" 6). Stage 1 (df2a4d0, 2026-10-22, no capture), 14 bursts (1,308 frames, 21.7 s drawn): average 16.80 ms, a frame over 25 ms in 11 (about 5 per 10 s), worst 34.82 ms; where the time goes is under "What stage 1 found" below. 6e28bb9 (2026-10-22, 17 bursts, 26.1 s drawn): average 16.72 ms, a frame over 25 ms in 10 (about 4 per 10 s), worst 35.82 ms. Hardware float (2026-10-23, 16 bursts, 10.4 s drawn): 16.81 ms, 10 of 16, worst 37.21 ms, so the misses aren't CPU time | *Proposed:* each burst averages ≤ 16.8 ms; worst ≤ 20 ms in ≥ 95% of bursts; at most 1 missed vsync (> 25 ms) per 10 s of motion | `lumen-tv-check`: 30 s of navigation after 5 s skipped, summarised by `tools/frame-stats.sh` | Phase 3, then again with each Phase 4 feature on screen |
| 2 | Key press to the first frame that shows it, inside the shell | 09711d7, 2026-10-22: all 72 presses shown were within 20 ms: 1–7 ms from a settled screen, 16–20 ms when the press lands mid-burst; one more came as the window hid and wasn't shown. df2a4d0, 2026-10-22: 73 of 74 presses in the motion window within 33 ms (p50 17 ms), the other 35 ms, during a slow frame. Measured from SDL's stamp on the event, assuming LG's SDL stamps it on arrival. 6e28bb9, 2026-10-22: 71 of 71 within 33 ms (p50 17). The remote's evdev recording in the same run shows what these leave out: from the kernel's stamp to the shell reading the press, 7 ms on average from a settled screen and 20 ms mid-burst (max 37 ms), which fits SDL stamping an event when the loop reads it | *Proposed:* ≤ 2 frames (≤ 33 ms) for 95% of presses | New log line (below) | Phase 3 |
| 3 | Remote to screen, end to end | Not measured. **Dropped on 2026-11-08** (owner's call), with the slow-motion video it needed: the remote's radio and the panel are the same for Lumen and LG's Home bar, so what Lumen adds is target 2, and its frame pacing is target 1's vsync count | ~~No slower than LG's Home bar on the same remote, within 1 frame~~ Dropped | None (was: a slow-motion video on the owner's phone, 10 presses on Lumen and 10 on LG's Home) | — |
| 4 | Cold start to first frame | B1 (3572bf3): 444 ms (352 ms of it is SDL's window and GL context). B3: 1,504 ms: GL ready at 380 ms, then 1,023 ms in `Renderer::new` and `draw::Assets` (shader compile and font/atlas set-up), then 50 ms of luna. c500289, 2026-10-22: 1,197 ms. Split by the `start:` line, 2026-10-22: 09711d7, 1,395 ms: SDL 163, GL 138, renderer 59, assets 923, apps 63, first frame 49; df2a4d0, 1,594 ms: SDL 165, GL 167, renderer 112, assets 983, apps 104, first frame 63. 6e28bb9: 1,422 and 1,312 ms (assets 932 and 922). Hardware float (2026-10-23): 688 and 524 ms: SDL 199 and 139, GL 145 and 63, renderer 86 and 48, assets 163 and 176, apps 59 and 62, first frame 36 and 36 | *Proposed:* ≤ 600 ms to a first frame; icons and titles may fill in after | New start-up log line (below) | Phase 3 |
| 5 | Home key to the shell, warm | Not possible yet (no takeover). The shell's half, 09711d7: its first frame 170 ms after `window restored` when picked from LG's app list; 13 ms after a focus gain and 2 ms after a restore on two other returns. df2a4d0, 2026-10-22: 10 and 2 ms after a focus gain. 6e28bb9: 7 returns, p50 11 ms, max 19 ms | < 150 ms ([roadmap](../roadmap.md)) | The TV's own timing, no video (owner's call, 2026-11-08): from the Home press's kernel stamp (the remote's evdev node, read as root, as in the 2026-10-22 runs) to the swap of the shell's first frame after it, on one clock; the `foreground` and `relaunch` lines give the shell's half. The panel's queue after the swap (up to two refreshes) is stated, not measured | Phase 3 |
| 6 | Launch | c500289 called luna inside the key handler: HDMI 1 answered in 320 ms and no pulse frame was drawn. 09711d7 draws the pulse frame first (1 ms after OK), then calls: HDMI 1 answered in 18 ms, Homebrew Channel in 573 ms. df2a4d0: none; the OK on HDMI 1 came as a click, which that build ignored. 6e28bb9: HDMI 1 answered in 17, 16 and 18 ms, the third from a click. That build's pulse frame was drawn with no time step, so it showed no dip; 5726aa3 holds the launch until the dip is deepest (33 ms), not yet run on the TV | *Proposed:* pulse on the next frame; the launch call answered in ≤ 100 ms. The app's own start is reported next to LG's Home launching the same app, not targeted | Log line | Phase 3 |
| 7 | Held key | The shell repeats after 400 ms, then every 120 ms. The remote (LG's SDL repeats, 16 holds in the c500289 and 09711d7 runs, 2026-10-22): first after about 505 ms (491–522), then every 50 ms. df2a4d0's 5 holds agree (p50 508 ms, 497–519) | Owner's call, 2026-10-22: keep it. Speeding up after 1.5 s held (every 80 ms), as tvOS does, was tried in the simulator and not kept | `SDL repeat` log lines; the owner's feel | Phase 3 |
| 8 | Settled CPU | 0.2% of a core (B1, 3572bf3, 2026-10-20: 20 ms of CPU in 10 s) | *Proposed:* ≤ 1% | Clock ticks in `/proc/<pid>/stat` over 10 s | Phase 3; again in Phase 4, since glass must not add idle drawing |
| 9 | Memory | 18 MB resident (B1, 3572bf3); 39.7 MB (c500289, 2026-10-22, about 10 minutes after its start); 35.6 MB resident and 5.8 MB swapped out (df2a4d0, 2026-10-22, about 10 minutes after its start). The B9 had about 335 MB available and 188 MB of swap in use at the 2026-10-20 survey ([b9.md](../device/b9.md#memory)) | *Proposed:* ≤ 64 MB resident; GPU textures ≤ 24 MB | `VmRSS` in `/proc/<pid>/status`; texture sizes from the code | Phase 3 and 4 |

Memory is on the list because a page-in from swap is a stall no frame budget
survives.

### Log lines

Decided; the owner approved them on 2026-10-22. The shell writes them (built
on `claude/shell-timing`, on `main` since 2026-10-31) and
`tools/frame-stats.sh` summarises them. Each line
starts with `[<unix seconds>] ` like the others. Times are whole milliseconds.
`<why>` is one of `the window lost focus`, `the window was hidden`, `the app
went to the background`, `nothing to draw` or `the shell is leaving`.

```text
key <sc>: shown after <N> ms
key <sc>: shown after <N> ms (<B> ms in launch)
key <sc>: not shown after <N> ms: <why>
key <sc>: not shown after <N> ms (<B> ms in launch): <why>
keys: <K> more presses not timed
start: sdl <N>, gl <N>, renderer <N>, assets <N>, apps <N>, first frame <N> ms
start: sdl <N>, gl <N>, renderer <N>, assets <N>, apps <N> ms; no first frame: <why>
foreground: first frame after <N> ms
foreground: not shown after <N> ms: <why>
launch <id>: answered in <N> ms
launch <id>: failed after <N> ms: <error text>
launch <id>: dropped: <why>
window <event>: last key <S.s> s ago, <M> other events since
window <event>: no key yet, <M> other events since
app <background|foreground>: last key <S.s> s ago, <M> other events since
app <background|foreground>: no key yet, <M> other events since
in front again (<cause>): drawing resumes
drew <N> frames in <ms> ms (avg <ms> ms, worst <ms> ms, <Q> quads); first <a>, <b>, <c> ms; idle
drew <N> frames in <ms> ms (avg <ms> ms, worst <ms> ms, <Q> quads); first <a>, <b>, <c> ms; <K> vsyncs skipped; idle
slow frame <n>: <T> ms = loop <L> + batch <B> + upload <U> + submit <S> + swap <W> ms; <K> uploads, <X> KB, <G> glyphs; faults <minor> minor, <major> major
slow frame <n>: … ; faults -
slow frames: <K> more not logged
motion paced: <P> ms a frame, from <source>; SDL_webOSGetRefreshRate <rate>, display mode <rate>; read in <ms> ms
warm: <G> glyphs, <U> uploads, <KB> KB in <ms> ms
```

The platform layer's Wayland path (lane L, decision 15; chosen per run, SDL
stays the default) writes the same lines, with three changes and two more:

```text
start: wayland <N>, gl <N>, renderer <N>, assets <N>, apps <N>, first frame <N> ms
start: wayland <N>, … ms; no first frame: started hidden
relaunch: first frame after <N> ms
relaunch: not shown after <N> ms: <why>
hidden: Top Shelf art freed, <U> in textures (<KB> KB) and <D> decoded (<KB> KB); the atlas stays (<KB> KB)
```

* `start: wayland`: the first phase is the compositor connected and the
  window made, where SDL's is SDL loaded and initialised. `started hidden`
  is a preload or `"hidden": true`; the relaunch that shows it gives a
  `foreground` line.
* `relaunch` (Home with Lumen in front): from the app manager's relaunch to
  the first frame with focus back on the first tile. `relaunch (Home): last
  key …` before it is the event, like the `window` lines.
* `hidden` (both paths): staying resident, the Top Shelf art goes while the
  shell is out of sight and the icon atlas stays; back in front the hero
  loads again and fades in. Said only when something was freed.

The owner approved four more on 2026-10-23: the `dropped` launch (written by
`5726aa3`), and the skip count, `motion paced` and `warm` for stage 3 of the
frame-time plan (being written).

* `key` (target 2): from the SDL event's timestamp to the end of the swap of the
  first frame that drew its effect. `(<B> ms in launch)` says how much of that
  went on a launch. `keys:` counts the presses that got no line of their own.
* `start` (target 4): running totals in ms from the process's `main()`;
  `frame-stats.sh` turns them into the time each phase took.
* `foreground` (target 5): from focus gained, or did-enter-foreground, to the
  first frame shown.
* `launch` (target 6): luna's reply to `applicationManager/launch`. A launch
  waits until the press pulse's dip is deepest (33 ms) and a frame shows it.
  `dropped` is a launch asked for but never called, because the shell was
  hidden or left first; `<why>` is one of the reasons above.
* `window` and `app`: `<event>` is `focus lost`, `focus gained`, `hidden`,
  `shown`, `minimized` or `restored`. They say how long ago the last key came
  and how many other events came since, to tell a lost focus from no keys.
  `frame-stats.sh` counts the focus losses with no key pressed.
* `in front again` pairs with the existing `the window lost focus: drawing
  stops`.
* `drew` (target 1): one line per burst of motion, when the screen settles.
  `first` gives the times of frames 2, 3 and 4 of the burst; frame 1 follows
  an idle wait and has nothing to be timed from. A short burst gives fewer, a
  1-frame burst none.
* `slow frame` (target 1): a frame over 25 ms; `<n>` is its place in the
  burst. A frame's time runs from the previous frame's swap returning to its
  own. It splits exactly into `loop` (events, springs, icons arriving:
  everything not timed below; a launch ends the burst first, so it is never
  in a timed frame), `batch` (building the frame,
  `draw::home`, glyph rasterising included), `upload` (new atlas pixels into
  the texture, `glTexSubImage2D`), `submit` (handing the batch to GL) and
  `swap` (inside `SDL_GL_SwapWindow`). The swap can wait for GPU work an
  earlier frame handed over, so a slow swap isn't always its own frame's cost.
  On the B9 the wait for the next vsync shows in `submit`, not in `swap`
  (below).
* Uploads and KB are the frame's atlas uploads; glyphs, the glyphs it
  rasterised. `faults` are the main thread's page faults since the previous
  frame (`getrusage(RUSAGE_THREAD)`). A major fault read a page back from swap
  (target 9). `faults -` means they aren't counted (the PC simulator on
  Windows).
* A burst logs at most 4 slow frames. `slow frames:` counts the rest, just
  before that burst's `drew` line.
* `vsyncs skipped` (target 1): with stage 3's pacer (vsync on, the real
  clock), how many vsyncs of the burst showed the frame before again: the
  missed vsyncs that show on screen. Frame 3's 30 ms after an idle wait is the
  buffer queue filling, not a skip. Every paced burst has the field, 0
  included; without a pacer (`--fixed-dt`, or no vsync) no burst has it.
* `motion paced`: once at start-up, with the pacer: the period motion steps
  by and where it came from (`the display mode` or `the 60 Hz default`), what
  each of SDL's two calls answered (`<n> Hz`, `absent` or `failed`), and how
  long reading them took. LG's own answer is logged, not used: on the B9 it
  is likely the panel's 120 Hz (`tv.hw.supportFrc` is true), while GL draws
  at 60.
* `warm`: once, at the first idle after the first frame (real clock only):
  the glyphs made ahead for every app's titles and status lines, the atlas
  uploads that took, and their cost, so no burst rasterises or uploads.
* Unlike the lines above, the frame lines give times to hundredths of a ms;
  only `drew`'s total is whole.
* The frame lines are stage 1 of "Frame time: finding the missed vsyncs"
  (shell-mvp.md): which frame of a burst is slow,
  and where its time goes.

#### What stage 1 found

df2a4d0 on the B9, 2026-10-22 04:55–05:23 UTC, opened by hand, no capture
running (report `tools/out/archive/tv-check-20261022T044903Z`). In the 35 s
motion window: 14 bursts, 17 slow-frame lines and 1 more not logged.

1. **The vsync wait shows in `submit`, not `swap`.** On this driver the
   draw call blocks until a buffer is free: submit p50 29.16 ms in the slow
   frames, swap at most 1.80 ms. On the B9, read the plan's "time in the
   swap" as time in `submit`.
2. **Most misses are frame 3 of a burst after idle.** 8 of the 17 lines,
   and nearly every burst from idle has the same shape: frame 2 about 9 ms,
   frame 3 about 30 ms, frame 4 back to 16.7 ms (`first 9.22, 30.69,
   17.56`). Frames 2 and 3 together take about two vsyncs: the buffer queue
   filling after the idle wait. Probably no frame is lost on screen, but the
   springs step 9 ms, then about 31 ms, for frames shown 16.7 ms apart. It is
   one frame, not several, so it counts against H1 (a slow GPU clock after
   idle). A slow-motion video was to decide whether it shows; it was
   dropped on 2026-11-08 (target 3), and stage 3's pacer steps the springs
   by whole periods either way.
3. **Atlas uploads block 9–25 ms (H3 confirmed).** Every slow frame with an
   upload (5 of 17) spent 9.02–24.53 ms in `upload` for 2–5 uploads of
   3–10 KB: a wait for the GPU to finish with the 8 MB atlas, not a copy.
4. **A few real mid-burst misses.** Frames 44, 46, 51, 63 and 444 took
   25–35 ms in `submit` with nothing uploaded. After the shell came back to
   the front, four more missed the same way, the worst 41.44 ms.
5. **No major page faults during the motion (H5 out):** 82 minor, 0 major
   in the window's slow frames. One frame after 47 s without focus had 2
   major faults.

Frame 1 of a burst is never timed, so frame 2's or 3's wait may pay for
frame 1's work, and frame 1's uploads and glyphs are in no line.

#### Key diagnostics

Decided 2026-10-22; written by the shell on `claude/shell-timing` (K1–K5)
and run on the B9 the same day (6e28bb9, report
`tools/out/archive/tv-check-20261022T074700Z`). Each line starts with
`[<unix seconds>] ` like the others.

```text
pointer shown
pointer hidden
mouse button <b> down at <x>,<y>: taken as OK
mouse button <b> down at <x>,<y>: ignored
mouse button <b> up at <x>,<y>
pointer: SDL_webOSCursorVisibility found in LG's SDL; arrows hide the pointer
pointer: SDL_webOSCursorVisibility not in LG's SDL; the pointer stays as LG leaves it
pointer: wl_webos_input_manager.set_cursor_visibility found; arrows hide the pointer
pointer: wl_webos_input_manager.set_cursor_visibility not found; the pointer stays as LG leaves it
power <state>: last input <S.s> s ago
front app <id>: last input <S.s> s ago
screensaver request: allowed; last input <S.s> s ago
luna watch <what>: <error text>
no input for <N> s; focused <yes|no>; pointer <shown|hidden>; power <state|unknown>
```

* "Input" is any key, 484 and 485 included, and any mouse event (a button,
  a move or the wheel). The key-up SDL makes for 484 or 485 at a focus loss
  is not input.
* `pointer shown` and `pointer hidden`: scancodes 484 and 485, which
  SDL-webOS names `SDL_SCANCODE_WEBOS_CURSOR_SHOW` and
  `SDL_SCANCODE_WEBOS_CURSOR_HIDE`. The shell keeps the pointer's state from
  them.
* `mouse button`: every press and release, not only the first. A left-button
  (button 1) down while the pointer shows is taken as OK and timed as
  `key 40: shown after <N> ms`. Any other button is logged as ignored.
* `pointer:` once at start-up. If LG's SDL exports
  `SDL_webOSCursorVisibility`, an arrow hides a shown pointer, so the next OK
  arrives as key 40. If not, the pointer stays as LG leaves it.
* `power`: each change of state from `tvpower/power/getPowerState`
  (subscribed), for example `Active` or `Screen Saver`; the first reply's
  state counts as a change.
* `front app`: each change from `applicationManager/getForegroundAppInfo`
  (subscribed): what came in front, for example `com.webos.app.screensaver`.
* `screensaver request`: each request from `registerScreenSaverRequest`,
  answered `ack: true`. The screensaver is allowed and only logged, for the
  OLED's sake.
* The three subscriptions run on a thread of their own. A refused one logs
  one `luna watch <what>` line and nothing more until it answers again;
  `luna watch thread:` lines say when the thread itself could not start,
  load the library or register. Other services' strings reach the log
  trimmed, control characters as spaces, cut to 200 characters.
* `no input for`: a heartbeat every 30 s with no input, for up to 10 min,
  then every 10 min. It bounds a gap without a clock read.
* `tools/frame-stats.sh` summarises these lines in its "input" section.

Why: every focus loss with no key in the 2026-10-22 logs came 112–120 s after
the last input the shell saw, which matches LG's screensaver (research note
`missing-keys.md`, not in the repo). The run of 6e28bb9 confirmed it, and its
evdev recording of the remote answered the rest (below).

#### The Top Shelf hero's lines

Proposed by lane J (the look, part 1, `claude/look`), for the owner to
approve. Each line starts with `[<unix seconds>] ` like the others. KB are
1,024 bytes; times are milliseconds to a tenth. `<name>` is the hero's
(`hero-plex`, `hero-palette-3`, `hero-ambient`, `art-owner/<app id>`), or
`gradient` for the placeholder.

```text
art from <folder>, the owner's from <folder>
art-owner: art for <N> apps: <app id>, <app id>, …
art-owner/"<file>": <why>; not used
hero <name>: <L> layer(s), <KB> KB; decoded in <ms> ms, uploaded in <ms> ms
hero <name>: decoded in <ms> ms; dropped, focus moved on
hero <name>: <file>: <why>; the shelf shows its gradient
hero <name>: upload failed: <error>; the shelf shows its gradient
hero <name>: <KB> KB more would take the textures to <T> KB, over <B> KB; not uploaded
crossfade <name> to <name>: <N> frames; textures <KB> KB at most
textures: <T> KB in all (atlas <A> KB, heroes <H> KB); <P> KB at most so far
no hero thread (<error>): heroes load at once
the hero thread stopped: heroes load at once
```

* `art from`: once at start-up, where the heroes are read from.
* `art-owner`: once, when the owner's folder has been listed: the apps with
  art, then a line for each file left out (wrong size, not a PNG, not named
  for an app ID). No folder, or an empty one, gives neither.
* `hero` (target 9): each hero put into textures, with its decode time (on
  the loader thread) and its upload time (on the main thread, at an idle
  wait). `dropped` is a hero decoded after focus had moved on; it never
  reaches a texture. A hero that fails says so once and is never asked for
  again; the shelf keeps its gradient.
* `crossfade`: each crossfade, once settled: the frames it took, up to and
  including the one that showed it settled, and the most the textures held
  meanwhile (the atlas, the hero it left and the one it came to).
* `textures`: once, at the first idle wait with a hero on the shelf and
  nothing fading. The budget is 24,000,000 bytes, 23,437 KB.
* `tools/frame-stats.sh` summarises them in its "top shelf" section.

#### The look's lines

Proposed by lane J with the hero's lines. Each starts with
`[<unix seconds>] ` like the others.

```text
look: hero <on|off>, dither <on|off>, tilt <on|off>, light <on|off>, parallax <on|off>, shadow <on|off>, sheen <on|off>, fade <on|off>, slide <on|off>, dim <on|off>
look: "<word>" is not a feature; left out
```

* The first, once at start-up, before SDL starts: the Phase 4 features this
  run has. All but the dither are on (`Look::DEFAULT`, since 2026-11-08:
  the dither cost the B9 about 12.5 ms a frame) unless the launch
  parameters' `"look"` says otherwise, in words applied in order:
  `{"look":"plain"}` turns every one off (the home screen as before
  Phase 4), `{"look":"-tilt,-light"}` those two, `{"look":"plain,hero"}`
  leaves the hero alone on, `{"look":"dither"}` adds the dither, and `full`
  turns everything on, the dither too. Before the home (2026-11-08) the line ended
  `; art <full|half> size` and had no `shadow` or `sheen`; `half` is now an
  unknown word. Before the Control Center (2026-11-08) it ended at `sheen`.
  `tools/frame-stats.sh` reads all three forms.
  `tools/tv.sh launch <app id> '<json>'` passes them (close the shell
  first: a running app is not started again); a Home press passes LG's
  own, which have no `"look"`, so the default look. `tools/sim.ps1 -Look`
  does the same on the PC. `tools/bench/looks.sh` passes no `"look"` for
  its `full`, so on builds since the default changed its `full` runs are
  the default; `{"look":"full"}` is the look with the dither.
* The second, for each word that names no feature, or a `"look"` that is
  not text: it changes nothing. A typo must never keep the TV's Home from
  starting.
* `tools/frame-stats.sh` repeats the first in its "top shelf" section, so
  a report says which look it measured.

#### The Control Center's lines

```text
reduce motion: <none|<effect>, <effect> off>
reduce motion: <effect> off; <saved|not saved: <error>|not kept>
privacy: Lumen Guard <value>; Today <value>; LG's reporting <value>; Port 11111 <value>
```

* The first, once at start-up after the look lines: what the
  `reduce-motion` file turns off (look-and-feel.md, "The Control Center").
  The look line is the launch parameters' look; the effects listed here
  are off on top of it.
* The second, each time a switch in the Control Center is turned: what is
  off now, and whether the file took it.
* The third, each time the Privacy panel's read comes back: what the panel
  says, line by line (or `Lumen Guard Can't read: <why>`).

### What blocks measuring now

* **Keys are explained** (6e28bb9, 2026-10-22; shell-mvp.md, "Measured on
  the B9" 15). No press is lost between the kernel and the shell: all 75
  presses the remote's evdev node recorded while the shell had the focus
  reached it. LG's screensaver takes the focus 110–116 s after the last
  input, and the press that wakes the TV goes to LG. An OK with the pointer
  up is a click, which the shell now takes as OK. The presses missed in the
  gaps of the earlier runs never reached the kernel. A run's window still
  comes from the log's own first and last key, and the log, not the screen,
  says whether the shell is in front.
* **The slow-motion video was dropped** on 2026-11-08 (owner's call).
  Target 3 goes. Target 5 is timed on the TV's own clock, from the Home
  press's kernel stamp to the first frame's swap. H4, and whether stage 3's
  pacer evens the start of a tap, are judged from the frame lines and the
  owner's eye.

## Pretty

### Today (B2, in the simulator)

A fixed dark gradient, a placeholder Top Shelf, and five-across 320 × 192
tiles in each app's colour. The focused tile scales by 1.15, gains a shadow and
shows its title; resting tiles are dimmed to 80%. Everything moves on springs.

### Tokens

Every look value moves into one `style` module, so Phase 4 tunes the look in
one place and reference screenshots change with it. Done on 2026-10-23
(04d8de1, `lumen-core`'s `style`; the simulator's screenshots were
byte-identical before and after).

| Token | Today | Note |
|---|---|---|
| Background | `#0E1222` to `#000000`, top to bottom | Fixed; doesn't scroll |
| Shelf placeholder | `#2E3862` to `#0E1222` | Replaced by the Top Shelf art |
| Text, dim text | `#EBEEF5`, `#8C94A5` | |
| Pill | black at 75% | Also the flat-glass fallback |
| Tile | rest shade 0.8, top-edge sheen 0.14, radius 12 | 8-colour palette for apps without a colour |
| Type | Inter SemiBold at 24, 28, 30, 48, 72 and 96 px | *Proposed:* add Inter Regular for body text in the Control Center |
| Spacing | safe area 80 / 60; column gap 40; row gap 88 | |
| Panel radius | none yet | *Proposed:* 28 for panels; pills fully round |
| Focus | scale 1.15; shadow 18 px down, 24 px soft edge, 0.4 opacity | |
| Motion | lift ω 30/s, scroll and bump ω 20/s | Critically damped springs (shell-mvp.md, "Motion") |

### Cheap fixes first

These cost little or nothing per frame and do the most for the look:

1. **Icons (the biggest gap).** Inside the app's jail only developer-mode apps'
   icons resolve: 4 of 30 tiles on the B9. LG's own apps, store apps and the
   inputs show a letter. Reaching their icons needs root, for example a copy
   made through Homebrew Channel's `exec`, so it waits for the owner's OK
   (shell-mvp.md, "Still open" 4). Built on 2026-10-23 (4f3dfc5,
   docs/specs/icons.md), not yet run on the TV. The B9's developer folders
   are writable by anyone (`/media/developer` and its `apps` by LG's
   developer mode, the rest by unfixed IPKs), so the copy checks only the
   folders our installs set and pins its own before writing (the owner's
   call, 2026-10-23).
2. **Banding.** The UI plane is 8-bit. The background falls from `#0E1222` to
   black over 1,080 px, so blue steps about every 32 px and red about every
   77 px. On an OLED in a dark room that can show as bands; it hasn't been
   looked at on the TV yet. Fix: an ordered dither of half a step in the
   fragment shader, a few instructions per pixel, measured with target 1.
   Built by lane J (`claude/look`): a 4x4 Bayer pattern on flat and shape
   quads, not on images, `style::DITHER` peak to peak. In the simulator's
   settled start screen (1280x720) the background's longest run of one
   colour fell from 22 px to 5 px. Its cost on the TV is not measured yet;
   the `-dither` look switch turns it off.

   **Measured on the B9 (OLED55B9PUA, 05.50.00, 2026-11-07),** 45 s of
   arrows held around the grid, the window from the log's first to last
   key (`tools/out/tv-session-20261107/plan.md`, git-ignored):

   | Build | Average frame | Vsyncs skipped per 10 s |
   |---|---|---|
   | 3a89955, before lane J | 16.88 ms | 4.7 |
   | Lane J's build, `{"look":"plain"}` | 21.61 ms | 131.7 |
   | main e57dc5b, `{"look":"plain"}` | 21.53 ms | 128.4 |
   | main e57dc5b, the full look (short bursts only) | 27.95 ms | 231 |

   Plain still paid: the dither's varying (`v_pos`) and its branch on
   `u_dither` were in the one shader, run for every pixel at `u_dither` 0,
   the only change to what plain draws. Since then the dither is built
   only into a program of its own (`render.rs`, `DITHER`), made when it is
   turned on, so plain's fragment shader is again the one from before lane
   J byte for byte (a test holds it).

   **Measured on the B9 (OLED55B9PUA, 05.50.00, 2026-11-08)** with the
   bench (docs/device/bench.md): the robot's 45 s pattern, three runs a
   build, the median shown; RSS after 5 min settled. Reports in
   `tools/out/bench/` (git-ignored).

   | Build | Average frame | Vsyncs skipped per 10 s | Start-up | RSS |
   |---|---|---|---|---|
   | 3a89955, before lane J | 16.89 ms | 4.82 | 541 ms | 42,848 kB |
   | 86b3527, `{"look":"plain"}` | 16.94 ms | 7.53 | 514 ms | 42,144 kB |
   | 86b3527, `{"look":"-dither"}` | 17.07 ms | 7.26 | 536 ms | |
   | 86b3527, the full look | 29.62 ms | 256 | 528 ms | |

   Plain is back to 3a89955's frame time, and the speed gate passes it on
   every row, so the fix is merged. Hero, tilt, light and parallax
   together cost about 0.1 ms. **The dither costs about 12.5 ms a frame on
   its own**: with it on, the home draws every other vsync (about 34 fps).
   Its program runs on every flat and shape quad, so its cost grows with
   everything those quads cover; where the time goes is not measured yet.
   Until it is made cheap (a small dither texture in place of the
   arithmetic, or one pass over the finished frame), the default look
   leaves it off: `Look::DEFAULT`, on the home's branch since 2026-11-08.
   `{"look":"dither"}` or `full` turns it on, for a pair against the
   default.

   **The cheap dither** (the home's branch, 2026-11-08, not yet measured).
   The threshold is now one fetch from a 4x4 texture of the same 16 Bayer
   levels on unit 1 (repeating, nearest texel), in place of two Bayer steps,
   two floors and a divide for every pixel: the dithered program adds a
   fetch and one multiply-add to the plain one. The pattern is the same
   (a test checks every level against the old arithmetic). The dithered
   program covers about 0.4 screens of the full look's pixels in the
   simulator, so the 12.5 ms was likely the frame tipping past its vsync
   (17.1 ms without the dither, already just over 16.7) more than the
   arithmetic alone. The bench pairs the default against `dither` on it;
   the aim is within 0.3 ms.
3. **Kerning.** Inter wasn't kerned: fontdue reads kerning only from a `kern`
   table, and Inter keeps it in GPOS. The 72 px shelf title shows it most.
   Done on 2026-10-23 (e823515): the pairs are pulled out of GPOS ahead of
   time into a 19 KB table the shell builds in, so nothing is parsed at run
   time. "Yo" closes by 7.1 px at 72 px. Not yet seen on the TV.

### Phase 4 features

| Feature | Looks like | Cost per frame | Fallback |
|---|---|---|---|
| Focus light and tilt | The newly focused tile leans a few degrees toward the move and springs back; a soft highlight sweeps across it once, then stops so the screen settles | 1–3 quads (estimate) | Light only |
| Layered parallax | Only for art Lumen owns (the Top Shelf, Lumen's own tiles): 2–3 layers shift with the tilt. LG's icons are flat images, so they tilt as one piece | One quad per layer | Flat art |
| Top Shelf hero | Superseded by the home's full-screen art ("The home" below). Was: full-bleed 1920 × 640 art for the focused row-0 app, crossfading on the scroll spring, with the title over a dark scrim at the bottom | 1–3 full-screen quads; 3.6 MB of texture each at 1280 × 720 | The gradient |
| Glass | Control Center platter and dock (options below) | See below | Flat tinted panels, black at 75% |
| Control Center | The roadmap's toggles that match LG Settings, on a glass platter; the Privacy panel, read-only (decision 12); Reduce Motion, one switch for every moving effect (focus tilt, focus light, parallax, slides, crossfades) | Its glass | |
| App editing | Done from Lumen Remote, not on the TV (decision 10): the first row, the order, "Other apps" | Nothing on the TV | |

* **Top Shelf art:** the owner's own per app, stills from Lumen's ambient
  loops, or both. Nothing is fetched from the internet (no telemetry, no
  scraping).
* **No video in the Top Shelf:** video is on a hardware plane that GL can't
  read. Whether GL could leave a clear hole over it is untested, so it isn't
  planned.

### The home

Stage 3 of the plan the owner approved on 2026-11-08 (slide 8 of
`tools/out/plan-20261108/lumen-streamline.html`): the Top Shelf grid gives
way to one screen of art with one row on it. Built on 2026-11-08
(`claude/home-design`); the simulator shows the layout, and the frame times
are the bench's to give. Places are in the 1920 × 1080 space
(`lumen-core`'s `layout`), values in `style`.

**What is on it.** The focused app's art fills the screen behind
everything. Over it, from the top:

* Top right, the clock (Inter SemiBold 48, twelve hours, `9:41`) and under
  it what was playing (28 px, dim, after a green dot), from the TV's local
  time and the luna watch.
* The focused item's name (88 px, baseline 500) and a line under it (30 px,
  dim, baseline 556): `Playing now` for what was playing, `Opened 5 minutes
  ago` for an app opened this run, `6 more apps` on the Other apps tile.
* One row of up to five wide tiles (320 × 180, radius 14, their tops at
  y 612), in decision 10's order: the contract's apps (Netflix, Prime
  Video, YouTube, then Plex or the Homebrew Channel), then **Other apps**.
  With none of the contract's apps installed, the first four apps take the
  row.
* A slim row of chips (60 high, fully round, y 900, up to seven): the
  inputs, then Settings. What was playing has the green dot here too.
  Each chip is as wide as its name, its 24 px padding each side and room
  for the dot (`layout::chip_width`), from 160 px ("PC" is still a chip)
  to 360 (a longer name is cut), 24 px apart; seven at the widest still
  fit the safe area, and would be narrowed alike if they did not. Before
  the font has measured them they are 220 wide. Up and Down go to the
  nearest middle, as the chips lie.
* The status pill, when there is one, top left.

**Other apps** is a page of its own: Select on its tile turns to it (the
home fades over the first 60% of a spring turn, the page over the last 60%,
each sliding 48 px), a heading and a grid of five columns that scrolls a row
at a time. Every tile that shows an icon has its name under it (30 px,
cut to the tile's width), dim at rest and brightening as the tile lifts,
so the page reads without moving focus over it; the first build named only
the focused one. A tile that shows its name across itself (no icon, or one
that failed to load) has none under it, which would only repeat it.
Back turns back to the Other apps tile.

**One tile style.** Every tile, "Other apps" included, is one rounded tile
of a colour, a soft rest shadow, and on it an icon, a mark or a name;
the first build had three looks (colour and name, an icon in a dark box,
and "Other apps"), which the build review (2026-11-09) asked to make one.
Its colour, in order:

1. Its icon's own edge colour, when the icon sits on a square of its own
   (at least half of its two outer rings of pixels opaque:
   `icons::edge_color`). The icon is then drawn 150 px (most of the tile's
   height), its square melting into the tile; the tile is flat under it.
   Focused, its light (below) goes over tile and icon together as white at
   each corner's lightness, which is the same as the colour taken toward
   white, coming with the lift; at rest it stays flat. A layer of light
   over every resting icon was most of what took "Other apps" over its
   budget with the B9's 30 icons (2026-11-10, 0a537bf: 17.92 ms a frame,
   0.78 screens of the fill's pixels and 0.62 of the main program's).
2. Else webOS's `iconColor`, unless it is near white or a light grey (less
   than 24 in 255 between its channels, and not dark): LG gives Netflix and
   YouTube white, which as a whole tile beside dark ones pulls the eye from
   focus, and shows only when their icons failed to copy.
3. Else the palette's colour for its place in the list.

An icon on a clear background is drawn 104 px on that colour; with no icon
the name is drawn across the tile, dark on a light colour. Icons are drawn
by the art's program (`Batch::picture`: a texture times a colour), not the
main one. "Other apps"
has a mark of its own drawn in code, four rounded squares two by two, on
its slate; its name is the title above the row when it has focus. Every
colour comes from the TV at run time, so no app's artwork is in the repo.
Focus lifts a tile (scale 1.15, a deeper shadow, and a 3 px rim of white
at 90% just outside it, coming with the lift), leans it toward the move
(tilt), sweeps a light across it once, and adds a sheen: the top-left
corner 26% of the way to white, fading across to the bottom right, drawn
as the tile's own corner colours (or the light over an icon's tile), so
it costs no quad on most tiles and one on an icon's. Resting tiles are at
65% brightness, their icons too when the tile takes the icon's colour.
The build review (2026-11-09) found focus too quiet from a couch at 1.12,
no rim and 80%; the rim is drawn under the tile round the part the tile
hides (`Batch::shadow_around`), so only its edge costs pixels. Chips light up white with
dark text and lift 1.08; they never lean or take the light. The page and
the art drift with focus (parallax) only on the home page.

**Moves.** Left and Right along a line; Up and Down to the nearest item by
centre in the line above or below; Home brings focus back to the first tile.
Back from a chip goes to the row; from the Other apps page, to its tile; on
the row, it goes back to what was playing (a launch), and with nothing
playing it does nothing: the shell owns Home now (`BackAtTop::Stay`), so
Back no longer closes it.

**What was playing** is the last app in front that was not Lumen, LG's
screensaver, LG's Home or nothing, as `getForegroundAppInfo` says (the
luna watch), read when the shell comes back to the front; an app the shell
launched counts at once. It is not checked that the app still runs: Back
after the app itself was closed starts it again.

**The clock** reads the local time with `localtime_r` after `tzset` (so a
time zone set in LG's settings shows within the minute) when the shell
comes to the front and then as each minute turns: one frame a minute while
settled, none while hidden. Before 2026 by the TV's clock (not set yet
after a boot) it shows nothing. `--clock 9:41` fixes it for screenshots; a
fixed clock (`--fixed-dt`) shows none without it. `--playing <app id>` (and
`tools/sim.ps1 -Clock`, `-Playing`) stands in for the luna watch on the PC.

**The art** is Lumen's own (`tools/art`, CC BY-SA 4.0), redrawn for the
home:

* At 1280 × 720, scaled up 1.5 times on the TV. The art is soft light and
  haze, and at this size a crossfade out of the layered ambient hero holds
  the texture budget: the atlas (8,192 KB), the ambient hero's three layers
  (7,085 KB) and the hero it fades to (3,600 KB) make 18,877 KB of the
  23,437 KB. At 1920 × 1080 the same moment would need about 32,000 KB.
  The owner's art is 1280 × 720 too, for the same reason: two full-size
  pictures in one crossfade would not fit with the atlas.
* With the home's scrim in the art (`artkit.home_scrim`): clear over the
  top third, darker under the name and the row, darkest under the chips,
  and softly behind the clock. No full-screen scrim is drawn over Lumen's
  art; the owner's art gets one quad of scrim from 30% of the height down.
  The name reads at 10.9:1 or better over 99% of its box on every hero.
* Drawn by a program of its own (`render.rs`, the art program): texture
  times colour, nothing else. The main program's per-pixel branches, the
  shapes and the dither stay off the screen's biggest quads.

**J2 to J5** (the plan's look items):

* J2, the seams: the ambient hero is drawn from frame 160 of the aurora
  loop, where no curtain fades out part way across (at frame 0 one did, and
  read as a hard edge). The input heroes' glass face drew a line: the
  lines' halos stopped at the face, the glints' glows (12 px) joined down
  it, and the ticks too. The halos now blend across a band, the glows are
  4 px, the ticks a third as tall.
* J3, the textures freed while hidden: the shell frees every hero when it
  goes out of sight and keeps the atlas (decision 15; the
  `hidden: Top Shelf art freed …` line), and loads the wanted one again
  when it comes back.
* J4, the half-size art: gone from the IPK and the repository, with the
  `half` look word. The full-size heroes are 8.6 MB of PNGs.
* J5, the amounts: tilt, light and parallax keep their tokens
  (`TILT_*`, `SWEEP_*`, `PARALLAX_PX`) until the bench and the owner's weekly
  look set them.

**Every effect has a switch** in the launch parameters' `"look"`
(`Look::parse`): `hero`, `dither`, `tilt`, `light`, `parallax`, `shadow`
(the resting tiles' shadows: one quad each), `sheen` (free, but a switch
like the rest), `fade` (the art's crossfade: off, the art cuts), `slide`
(the pages', and the Control Center's, slide as they turn: off, they only
fade) and `dim` (the dim behind "Other apps" and the Control Center: no
quad, the colours under it darkened as they are drawn). The look line says
them all (below).

**Cost.** The simulator's frames hold 80 to 95 quads; a test holds every
frame of the mock home under 600. The art is one to three full-screen quads
(three for the ambient hero's layers), which is the frame's fill; the
bench measures the rest on the B9. The Control Center, below, completes
Stage 3.

### The Control Center

Built 2026-11-08 (Stage 3 of the plan the owner approved that day, slide 8),
on the home's branch; the bench has not measured it yet.

**The gear.** A round button at the top right, left of the clock
(`layout::GEAR`), drawn like a chip with a cog of ten circles (eight teeth,
a disc and its hole), so nothing in it has to turn. Up from the row reaches
it, and Up from the chips when there is no row; Down, or Back, goes to the
row's tile nearest below it. On the gear the title reads "Control Center"
over "Privacy and motion", and the art stays the last app's. OK opens the
Control Center.

**The panel.** It comes in from the right over the home, 640 px wide and as
tall as the safe area (`layout::PANEL`): near-black at 94%, the flat
fallback for glass. With the look's `slide` it slides in 96 px as it fades
(`PAGE`'s spring); with `dim` the home behind it dims (no quad: see Cost).
Back closes it and leaves focus on the gear; the Home key closes it
and goes to the first tile, as it does from anywhere.

**Privacy, which only reads** (decision 12; privacy.md, G5's first part).
Four lines, each a label and a value, the value amber when it wants a look:

| Line | Values |
|------|--------|
| Lumen Guard | Blocking, Logging only, Not running (its status file is over 60 s old), Not installed |
| Today | "512 of 6,789 lookups blocked", or "… would be blocked" while Guard only logs |
| LG's reporting | All stopped, "1 of 4 jobs running", Unknown |
| Port 11111 | Closed, Open, Unknown |

Under them: "Read only: change these from your phone or PC". Nothing on the
TV loosens anything. The lines are judged as `lumen-ctl`'s `privacy.state`
judges them: a `REJECT` or `DROP` of tcp `dpt:11111` in `INPUT`, and a job
running only if `initctl status` says ` start/`.

**Where the panel's data comes from.** Each time the panel opens, the shell
asks Homebrew Channel's `exec`, on a thread of its own, to run
`lumen-privacy-read.sh`, which the IPK ships beside `lumen-icons.sh` and
which the shell names by its fixed path, with no arguments. As root, it
prints Guard's status file (`/var/lib/lumen/guard/status`), a `--- rules`
line, `iptables -L INPUT -n`, a `--- jobs` line, and `initctl status` for
crashreportd, rdxd, uploadd and remotelogger. It writes nothing, starts and
stops nothing, and leaves any part it cannot read empty; `lumen-core`'s
`privacy.rs` parses the output. The panel says "Reading…" meanwhile, and
"Can't read: no answer" if `exec` fails or takes over 10 s. This is the
simplest source that needs no new root daemon part: the jailed shell can't
read `/var/lib/lumen` itself, `lumend` isn't installed on the B9, and its
`status` request carries no Guard counts (adding them would be a
lumend-side change). `lumen-ctl privacy.state` reads the same things for
the phone and the PC.

**Motion** (Reduce Motion, decision 7). One switch, "Reduce Motion", green
while it is on, with a note under it: "Stops tilt, light sweep, parallax,
slides and fades". OK turns every moving effect off at once, and on again
(`Reduced::toggled`). It drives the look's own switches (`tilt`, `light`,
`parallax`, `slide`, `fade`), which stay separate in the launch
parameters' `"look"` for the bench, so the home and the bench see one set
of switches. The first build had a switch per effect; the build review
(2026-11-09) asked for the one a viewer expects, as tvOS has. It is on
when every effect the launch look runs is off; a file with only some off
(an older one) reads as off, and OK then turns the rest off. Reduce Motion
can only take effects away: what the launch look turns off stays off, and
with nothing moving in it the switch is drawn faint and ignores OK. An
effect turned off stops where it is: tiles stand upright, the art stops
drifting, a light sweep under way ends.

**Where Reduce Motion is kept.** In `reduce-motion`, a file beside the
executable in Lumen's folder, as `platform` is: the words of the effects
turned off, on one line (`tilt slide`). It is read at start-up and written
on each change; a missing, empty or unreadable file turns nothing off, and
a word that names no effect is passed over. Reinstalling the IPK removes
it, so a bench run, which installs, always starts with nothing turned off.
Under a fixed clock (`--fixed-dt`) it is neither read nor written, so
screenshots never depend on an earlier run. Whether the jailed shell may
write Lumen's folder on the B9 has not been tried: the `reduce motion: …;
saved` or `…; not saved: <error>` line says so the first time a switch is
turned. If it may not, the change lasts until the shell closes, and a
lasting store (lumend's, or the phone's through Remote) would be a
lumend-side change.

**Cost.** Open, the frame holds about 305 quads in the simulator (the home
under it, the panel's text a quad a glyph); a test holds it under 600. The
panel's glyphs are made with the rest at warm-up (`warm.rs`), so opening it
uploads nothing in its burst.

The first build drew the dim as a black quad over the whole screen and the
panel as one shape quad, about 1.3 screens of the main program's pixels
over the art, and "Other apps" its own dim, one screen more. On the B9
(OLED55B9PUA, firmware 05.50.00, 2026-11-09, 9d5a988) both ran at about
28 ms a frame in their bursts where the home ran at 16.9, which puts a
screen of the main program (a shape, a kind and a texture place for every
pixel) at about 11 ms. So neither dim is a quad now. Both are black, and
colours blend premultiplied, so a black quad of alpha `a` over what is
drawn is the same as every colour under it times `1 - a` (`Batch::set_dim`): the
art is darkened by both dims, the home by the Control Center's, and the
panel is drawn after, undimmed. The panel's middle is drawn by a third
program that carries the colour alone (`Batch::fill4`, mediump), and only
the bands holding its corners and a two-pixel strip down each side are
shape quads (`Batch::rounded_fill`). A test holds both screens to the
home's main-program pixels plus a tenth of a screen; the simulator's
frames match the old ones to within 2 in 255 (all but 7 pixels within 1).

That fixed the Control Center (2026-11-10, bc053bd: 16.84 ms a frame)
but not "Other apps" while focus moves in it (about 25 ms): its grid of
some 19 tiles, each with its resting shadow, was 1.03 screens of the
main program's pixels. So every tile's body is a `Batch::rounded_fill4`
too (the fill's program for its middle, gradient and sheen included, and
shape quads only along its edges), and a resting shadow under a level,
opaque tile leaves out the part the tile hides, all but its rounded
corners (`Batch::shadow_around`, four quads round the hole). With the
B9's 30 apps the grid is now about 0.36 screens of the main program and
0.36 of the fill's, the home 0.16 and 0.12. A test holds every screen,
still and moving, to 0.4 screens of each (`MAIN_PIXEL_BUDGET`,
`FILL_PIXEL_BUDGET`). While the dither is on the fills go back to the
main program, so their gradients are dithered with the rest.

### Glass: two options (owner picks)

The budget, from plx-native on a 49SM9000 (webOS 4.5, the same Mali-T820 MP2)
([research.md](../research.md#graphics-limits-from-plx-natives-measurements)):
blur holds 60 fps only under about 300,000 px of blurred area; full-screen blur
runs at about 24 fps; pieces far apart cost as much as one blur over the area
between them; nothing can be glass over video.

**A. Live glass.** Blur what's behind every frame, bend it at the edge, add a
rim light. It stays right while things move behind the panel (a Top Shelf
crossfade, a scroll).

* Cost: the panel's area counts against the 300,000 px. That's a Control
  Center of at most about 480 × 600 (288,000 px), or a dock of 1760 × 140
  (246,400 px), never both at once.
* Risk: medium. The 300,000 px figure is plx-native's, with its own blur, on a
  different model, and the cost of Lumen's grid moving is still unmeasured
  (target 1).

**B. Frosted still (recommended).** Lumen's background barely moves: a fixed
gradient and a still Top Shelf. So blur it once when a panel opens, keep the
result as a texture, and blur again only after what's behind it changes and
settles. Same edge, tint and rim light.

* Cost: about one textured quad a frame, at any panel size. The one-off blur
  may take a frame or two, hidden in the panel's slide-in.
* Difference from A: only while something moves behind an open panel, where
  the frosting lags until it settles.

Both fall back to flat tinted panels if they miss target 1.

## How "pretty" is checked

1. **Reference screens.** Each screen has a simulator screenshot made with
   `tools/sim.ps1 --keys … --screenshot-when-settled`: home on row 0, scrolled
   to row 1, a launch pulse, the Control Center open, and edit mode. They are
   kept in `docs/look/` (*proposed*) and remade whenever a token changes. The
   simulator matches the TV's framebuffer colours, not its speed.
2. **The owner's review on the TV,** from the couch, in a dark room and in
   daylight: no letter tiles for installed apps, no visible banding, kerned
   titles, focus obvious from across the room.
3. **The frame budget with the feature on screen:** every Phase 4 feature
   passes target 1 while visible and moving.

The roadmap's Phase 4 is done when all three pass.

## Order

1. The owed B2/B3 run on the TV, opened by hand: the frame time of the moving
   grid with the shape shader. Every pretty item spends against that number.
   Run on 2026-10-22: most bursts miss a vsync (target 1), so the misses are
   looked into before any pretty item. Stage 1 of that look ran the same day
   (df2a4d0, "What stage 1 found"). Stage 3 began on 2026-10-23 with
   hardware float, which brought start-up to 524–688 ms and left the misses
   as they were.
2. The log lines above, then targets 1–9 measured. The lines ran on the B9 on
   2026-10-22 (targets 2, 4, 6 and the shell's half of 5). On 2026-11-08
   target 3 was dropped and 5 moved to the TV's own timing, so no
   slow-motion video is needed.
3. The cheap fixes: the `style` module, dither, kerning; icons through the
   root copy. The style module and kerning were done on 2026-10-23 and the
   icon copy built; dither waits for the frame-time work, since it costs
   pixels.
4. Phase 4, in this order: focus light and tilt, Top Shelf, glass (B, frosted
   still), Control Center, editing.

## Owner's decisions

Answered on 2026-10-22:

1. Glass: **B, frosted still.**
2. Icons: **a root copy is allowed:** LG's app icons copied through Homebrew
   Channel's `exec`, reversible, with every TV step asked first
   ([docs/safety.md](../safety.md)).
3. Held keys: **keep the steady 120 ms** (after 400 ms). The owner tried it
   against a speed-up after 1.5 s held (every 80 ms) in the simulator and kept
   the steady curve. The remote repeats every 50 ms after about 505 ms
   (target 7), so the shell's own 120 ms sets the pace.
4. Sound: **in** for Phase 4: tvOS-style ticks on focus moves. The shell MVP
   stays silent.
5. Top Shelf art: **both:** the owner's art for an app where there is some,
   stills from Lumen's loops otherwise.

Answered on 2026-11-05 (the design session, decisions 10, 12 and 15):

6. **The grid shows the contract:** the contract's apps, then the inputs,
   then Settings; every other launch point in one "Other apps" tile.
   Edited from Lumen Remote, so on-TV editing is not built.
7. **Reduce Motion** in the Control Center, editable on the TV. It drives
   the same switches as the launch parameters' `"look"`: one switch for
   them all on the TV (the build review, 2026-11-09), each its own in the
   launch parameters.
8. **The hidden shell frees the heroes** (lane J's question 3) and keeps
   the icon atlas.
9. **Heroes move to ASTC 4x4** on an ES 3.0 context once a B9 measurement
   clears the gradients with the dither on; PNGs stay for the simulator.
10. **Partial redraw:** the damage comes from diffing draw lists, after
    one measured trial on the rows below the hero.

## Not in this spec

Code changes. The Home key takeover itself (`takeover/`), screensavers (Phase
2), and any content service or API.
