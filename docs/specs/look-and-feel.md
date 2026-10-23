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

Today's layout and motion values are in `docs/specs/shell-mvp.md` (on branch
`claude/shell-timing` until the shell merges). This spec doesn't repeat them;
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
| 3 | Remote to screen, end to end | Not measured | *Proposed:* no slower than LG's Home bar on the same remote, within 1 frame | Slow-motion video on the owner's phone (240 fps, about 4 ms a frame), 10 presses on Lumen and 10 on LG's Home | Phase 3 |
| 4 | Cold start to first frame | B1 (3572bf3): 444 ms (352 ms of it is SDL's window and GL context). B3: 1,504 ms: GL ready at 380 ms, then 1,023 ms in `Renderer::new` and `draw::Assets` (shader compile and font/atlas set-up), then 50 ms of luna. c500289, 2026-10-22: 1,197 ms. Split by the `start:` line, 2026-10-22: 09711d7, 1,395 ms: SDL 163, GL 138, renderer 59, assets 923, apps 63, first frame 49; df2a4d0, 1,594 ms: SDL 165, GL 167, renderer 112, assets 983, apps 104, first frame 63. 6e28bb9: 1,422 and 1,312 ms (assets 932 and 922). Hardware float (2026-10-23): 688 and 524 ms: SDL 199 and 139, GL 145 and 63, renderer 86 and 48, assets 163 and 176, apps 59 and 62, first frame 36 and 36 | *Proposed:* ≤ 600 ms to a first frame; icons and titles may fill in after | New start-up log line (below) | Phase 3 |
| 5 | Home key to the shell, warm | Not possible yet (no takeover). The shell's half, 09711d7: its first frame 170 ms after `window restored` when picked from LG's app list; 13 ms after a focus gain and 2 ms after a restore on two other returns. df2a4d0, 2026-10-22: 10 and 2 ms after a focus gain. 6e28bb9: 7 returns, p50 11 ms, max 19 ms | < 150 ms ([roadmap](../roadmap.md)) | Slow-motion video, and the foreground log line (below) | Phase 3 |
| 6 | Launch | c500289 called luna inside the key handler: HDMI 1 answered in 320 ms and no pulse frame was drawn. 09711d7 draws the pulse frame first (1 ms after OK), then calls: HDMI 1 answered in 18 ms, Homebrew Channel in 573 ms. df2a4d0: none; the OK on HDMI 1 came as a click, which that build ignored. 6e28bb9: HDMI 1 answered in 17, 16 and 18 ms, the third from a click. The pulse frame is drawn with no time step, so it shows no dip yet (the branch's audit; being fixed) | *Proposed:* pulse on the next frame; the launch call answered in ≤ 100 ms. The app's own start is reported next to LG's Home launching the same app, not targeted | Log line, slow-motion video | Phase 3 |
| 7 | Held key | The shell repeats after 400 ms, then every 120 ms. The remote (LG's SDL repeats, 16 holds in the c500289 and 09711d7 runs, 2026-10-22): first after about 505 ms (491–522), then every 50 ms. df2a4d0's 5 holds agree (p50 508 ms, 497–519) | Owner's call, 2026-10-22: keep it. Speeding up after 1.5 s held (every 80 ms), as tvOS does, was tried in the simulator and not kept | `SDL repeat` log lines; the owner's feel | Phase 3 |
| 8 | Settled CPU | 0.2% of a core (B1, 3572bf3, 2026-10-20: 20 ms of CPU in 10 s) | *Proposed:* ≤ 1% | Clock ticks in `/proc/<pid>/stat` over 10 s | Phase 3; again in Phase 4, since glass must not add idle drawing |
| 9 | Memory | 18 MB resident (B1, 3572bf3); 39.7 MB (c500289, 2026-10-22, about 10 minutes after its start); 35.6 MB resident and 5.8 MB swapped out (df2a4d0, 2026-10-22, about 10 minutes after its start). The B9 had about 335 MB available and 188 MB of swap in use at the 2026-10-20 survey ([b9.md](../device/b9.md#memory)) | *Proposed:* ≤ 64 MB resident; GPU textures ≤ 24 MB | `VmRSS` in `/proc/<pid>/status`; texture sizes from the code | Phase 3 and 4 |

Memory is on the list because a page-in from swap is a stall no frame budget
survives.

### Log lines

Decided; the owner approved them on 2026-10-22. The shell writes them (branch
`claude/shell-timing`) and `tools/frame-stats.sh` summarises them. Each line
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
window <event>: last key <S.s> s ago, <M> other events since
window <event>: no key yet, <M> other events since
app <background|foreground>: last key <S.s> s ago, <M> other events since
app <background|foreground>: no key yet, <M> other events since
in front again (<cause>): drawing resumes
drew <N> frames in <ms> ms (avg <ms> ms, worst <ms> ms, <Q> quads); first <a>, <b>, <c> ms; idle
slow frame <n>: <T> ms = loop <L> + batch <B> + upload <U> + submit <S> + swap <W> ms; <K> uploads, <X> KB, <G> glyphs; faults <minor> minor, <major> major
slow frame <n>: … ; faults -
slow frames: <K> more not logged
```

* `key` (target 2): from the SDL event's timestamp to the end of the swap of the
  first frame that drew its effect. `(<B> ms in launch)` says how much of that
  went on a launch. `keys:` counts the presses that got no line of their own.
* `start` (target 4): running totals in ms from the process's `main()`;
  `frame-stats.sh` turns them into the time each phase took.
* `foreground` (target 5): from focus gained, or did-enter-foreground, to the
  first frame shown.
* `launch` (target 6): luna's reply to `applicationManager/launch`.
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
* Unlike the lines above, the frame lines give times to hundredths of a ms;
  only `drew`'s total is whole.
* The frame lines are stage 1 of "Frame time: finding the missed vsyncs"
  (shell-mvp.md, on `claude/shell-timing`): which frame of a burst is slow,
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
   idle). The slow-motion video decides whether it shows.
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
* **The slow-motion video** is still owed. It decides target 3, the rest of
  target 5, H4, and whether stage 3's pacer evens the start of a tap.

## Pretty

### Today (B2, in the simulator)

A fixed dark gradient, a placeholder Top Shelf, and five-across 320 × 192
tiles in each app's colour. The focused tile scales by 1.15, gains a shadow and
shows its title; resting tiles are dimmed to 80%. Everything moves on springs.

### Tokens

Every look value moves into one `style` module, so Phase 4 tunes the look in
one place and reference screenshots change with it.

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
   (shell-mvp.md, "Still open" 4).
2. **Banding.** The UI plane is 8-bit. The background falls from `#0E1222` to
   black over 1,080 px, so blue steps about every 32 px and red about every
   77 px. On an OLED in a dark room that can show as bands; it hasn't been
   looked at on the TV yet. Fix: an ordered dither of half a step in the
   fragment shader, a few instructions per pixel, measured with target 1.
3. **Kerning.** Inter isn't kerned: fontdue reads kerning only from a `kern`
   table, and Inter keeps it in GPOS. The 72 px shelf title shows it most. Fix:
   pull the pairs out of GPOS when the shell is built, so nothing is added at
   run time.

### Phase 4 features

| Feature | Looks like | Cost per frame | Fallback |
|---|---|---|---|
| Focus light and tilt | The newly focused tile leans a few degrees toward the move and springs back; a soft highlight sweeps across it once, then stops so the screen settles | 1–3 quads (estimate) | Light only |
| Layered parallax | Only for art Lumen owns (the Top Shelf, Lumen's own tiles): 2–3 layers shift with the tilt. LG's icons are flat images, so they tilt as one piece | One quad per layer | Flat art |
| Top Shelf hero | Full-bleed 1920 × 640 art for the focused row-0 app, crossfading on the scroll spring, with the title over a dark scrim at the bottom | 1–2 full-width quads; about 5 MB of texture each, 10 MB during a crossfade | Half-size art (960 × 320) scaled up, about 1.2 MB |
| Glass | Control Center platter and dock (options below) | See below | Flat tinted panels, black at 75% |
| Control Center | The roadmap's toggles that match LG Settings, on a glass platter | Its glass | |
| App editing | The picked tile wiggles; arrows move it | Moving while in edit mode is fine; it settles when editing ends | |

* **Top Shelf art:** the owner's own per app, stills from Lumen's ambient
  loops, or both. Nothing is fetched from the internet (no telemetry, no
  scraping).
* **No video in the Top Shelf:** video is on a hardware plane that GL can't
  read. Whether GL could leave a clear hole over it is untested, so it isn't
  planned.

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
   2026-10-22 (targets 2, 4, 6 and the shell's half of 5); 3 and 5 still
   need the slow-motion video.
3. The cheap fixes: the `style` module, dither, kerning; icons through the
   root copy.
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

## Not in this spec

Code changes. The Home key takeover itself (`takeover/`), screensavers (Phase
2), and any content service or API.
