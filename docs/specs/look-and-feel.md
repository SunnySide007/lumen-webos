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
`claude/shell-ls2` until the shell merges). This spec doesn't repeat them; it
sets the bar they are judged against.

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
| 1 | Frame time while moving | B1 grid, 13 bursts (~520 frames): averages 16.2–16.7 ms, worst ≤ 17.8 ms in 11, one 19.6 ms, one missed vsync (33.3 ms). B2/B3 (shape shader, atlas), 2026-10-22: c500289 with Phase 1's capture running, 50 bursts (2,611 frames): average 16.83 ms, a frame over 25 ms in 37, worst 50.6 ms; 09711d7 with no capture, 20 bursts (1,399 frames, 23 s drawn): average 16.82 ms, a frame over 25 ms in 16, worst 35.1 ms. The misses come mostly in a single tap's burst and in Down's scroll (shell-mvp.md, "Measured on the B9" 6) | *Proposed:* each burst averages ≤ 16.8 ms; worst ≤ 20 ms in ≥ 95% of bursts; at most 1 missed vsync (> 25 ms) per 10 s of motion | `lumen-tv-check`: 30 s of navigation after 5 s skipped, summarised by `tools/frame-stats.sh` | Phase 3, then again with each Phase 4 feature on screen |
| 2 | Key press to the first frame that shows it, inside the shell | 09711d7, 2026-10-22: all 72 presses shown were within 20 ms: 1–7 ms from a settled screen, 16–20 ms when the press lands mid-burst; one more came as the window hid and wasn't shown. Measured from SDL's stamp on the event, assuming LG's SDL stamps it on arrival | *Proposed:* ≤ 2 frames (≤ 33 ms) for 95% of presses | New log line (below) | Phase 3 |
| 3 | Remote to screen, end to end | Not measured | *Proposed:* no slower than LG's Home bar on the same remote, within 1 frame | Slow-motion video on the owner's phone (240 fps, about 4 ms a frame), 10 presses on Lumen and 10 on LG's Home | Phase 3 |
| 4 | Cold start to first frame | B1: 444 ms (352 ms of it is SDL's window and GL context). B3: 1,504 ms: GL ready at 380 ms, then 1,023 ms in `Renderer::new` and `draw::Assets` (shader compile and font/atlas set-up), then 50 ms of luna. c500289: 1,197 ms. 09711d7, split by the `start:` line: 1,395 ms: SDL 163, GL 138, renderer 59, assets 923, apps 63, first frame 49 | *Proposed:* ≤ 600 ms to a first frame; icons and titles may fill in after | New start-up log line (below) | Phase 3 |
| 5 | Home key to the shell, warm | Not possible yet (no takeover). The shell's half, 09711d7: its first frame 170 ms after `window restored` when picked from LG's app list; 13 ms after a focus gain and 2 ms after a restore on two other returns | < 150 ms ([roadmap](../roadmap.md)) | Slow-motion video, and the foreground log line (below) | Phase 3 |
| 6 | Launch | c500289 called luna inside the key handler: HDMI 1 answered in 320 ms and no pulse frame was drawn. 09711d7 draws the pulse frame first (1 ms after OK), then calls: HDMI 1 answered in 18 ms, Homebrew Channel in 573 ms | *Proposed:* pulse on the next frame; the launch call answered in ≤ 100 ms. The app's own start is reported next to LG's Home launching the same app, not targeted | Log line, slow-motion video | Phase 3 |
| 7 | Held key | The shell repeats after 400 ms, then every 120 ms. The remote (LG's SDL repeats, 16 holds on 2026-10-22): first after about 505 ms (491–522), then every 50 ms | Owner's call, 2026-10-22: keep it. Speeding up after 1.5 s held (every 80 ms), as tvOS does, was tried in the simulator and not kept | `SDL repeat` log lines; the owner's feel | Phase 3 |
| 8 | Settled CPU | 0.2% of a core (B1: 20 ms of CPU in 10 s) | *Proposed:* ≤ 1% | Clock ticks in `/proc/<pid>/stat` over 10 s | Phase 3; again in Phase 4, since glass must not add idle drawing |
| 9 | Memory | 18 MB resident (B1); 39.7 MB (c500289, about 10 minutes after its start). The B9 has about 330 MB free and is already using swap | *Proposed:* ≤ 64 MB resident; GPU textures ≤ 24 MB | `VmRSS` in `/proc/<pid>/status`; texture sizes from the code | Phase 3 and 4 |

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

### What blocks measuring now

* **Keys:** after `tools/tv.sh launch`, keys didn't reach the shell
  (shell-mvp.md, "Still open" 5). B1, opened by hand from LG's app list, got
  them. The 2026-10-22 runs opened the shell by hand, and keys still went
  missing at times (shell-mvp.md, "Measured on the B9" 13). So a run's window
  comes from the log's own first and last key, not from a clock read, and the
  log, not the screen, says whether the shell is in front.

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
   looked into before any pretty item.
2. The log lines above, then targets 1–9 measured. The lines ran on the B9 on
   2026-10-22 (targets 2, 4, 6 and the shell's half of 5); 3 and 5 still need
   the slow-motion video.
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
