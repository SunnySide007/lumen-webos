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
| 1 | Frame time while moving | B1 grid, 13 bursts (~520 frames): averages 16.2–16.7 ms, worst ≤ 17.8 ms in 11, one 19.6 ms, one missed vsync (33.3 ms). B2/B3 (shape shader, atlas) not yet measured moving | *Proposed:* each burst averages ≤ 16.8 ms; worst ≤ 20 ms in ≥ 95% of bursts; at most 1 missed vsync (> 25 ms) per 10 s of motion | `lumen-tv-check`: 30 s of navigation after 5 s skipped, summarised by `tools/frame-stats.sh` | Phase 3, then again with each Phase 4 feature on screen |
| 2 | Key press to the first frame that shows it, inside the shell | Not logged | *Proposed:* ≤ 2 frames (≤ 33 ms) for 95% of presses | New log line (below) | Phase 3 |
| 3 | Remote to screen, end to end | Not measured | *Proposed:* no slower than LG's Home bar on the same remote, within 1 frame | Slow-motion video on the owner's phone (240 fps, about 4 ms a frame), 10 presses on Lumen and 10 on LG's Home | Phase 3 |
| 4 | Cold start to first frame | B1: 444 ms (352 ms of it is SDL's window and GL context). B3: 1,504 ms: GL ready at 380 ms, then 1,023 ms in `Renderer::new` and `draw::Assets` (shader compile and font/atlas set-up, not yet split), then 50 ms of luna | *Proposed:* ≤ 600 ms to a first frame; icons and titles may fill in after | New start-up log line (below) | Phase 3 |
| 5 | Home key to the shell, warm | Not possible yet (no takeover) | < 150 ms ([roadmap](../roadmap.md)) | Slow-motion video, and the foreground log line (below) | Phase 3 |
| 6 | Launch | The press pulse is designed to start on the next frame; no launch has been timed (B3 listed apps only) | *Proposed:* pulse on the next frame; the launch call answered in ≤ 100 ms. The app's own start is reported next to LG's Home launching the same app, not targeted | Log line, slow-motion video | Phase 3 |
| 7 | Held key | The shell repeats after 400 ms, then every 120 ms. The remote's own repeat rate is unmeasured (B2 logs it) | Owner's call: keep it, or speed up after 1.5 s held (every 80 ms), as tvOS does, tried in the simulator first | `SDL repeat` log lines; the owner's feel | Phase 3 |
| 8 | Settled CPU | 0.2% of a core (B1: 20 ms of CPU in 10 s) | *Proposed:* ≤ 1% | Clock ticks in `/proc/<pid>/stat` over 10 s | Phase 3; again in Phase 4, since glass must not add idle drawing |
| 9 | Memory | 18 MB resident (B1). The B9 has about 330 MB free and is already using swap | *Proposed:* ≤ 64 MB resident; GPU textures ≤ 24 MB | `VmRSS` in `/proc/<pid>/status`; texture sizes from the code | Phase 3 and 4 |

Memory is on the list because a page-in from swap is a stall no frame budget
survives.

### Log lines to add (proposed)

Built with `/ecc:orch-add-feature` once this spec is approved, and summarised
by `tools/frame-stats.sh`:

* `key <scancode>: shown after N ms`: from the SDL event's timestamp to the end
  of the swap of the first frame that drew its effect.
* `start: sdl N, gl N, renderer N, assets N, apps N, first frame N ms`: each
  phase, from the process's start.
* `foreground: first frame after N ms`: from focus gained, or
  did-enter-foreground, to the first frame shown.
* `launch <id>: answered in N ms`: luna's reply to `applicationManager/launch`.

### What blocks measuring now

* **Keys:** after `tools/tv.sh launch`, keys didn't reach the shell
  (shell-mvp.md, "Still open" 5). B1, opened by hand from LG's app list, got
  them. The next TV run opens the shell by hand.
* **Tools:** `lumen-tv-check` and `tools/frame-stats.sh` are on branch
  `claude/play-p0-tv-check-38065c`, not on main yet.

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
2. The log lines above, then targets 1–9 measured.
3. The cheap fixes: the `style` module, dither, kerning; icons once the owner
   decides about root.
4. Phase 4, in this order: focus light and tilt, Top Shelf, glass (the chosen
   option), Control Center, editing.

## Owner's decisions

1. Glass: A (live) or B (frosted still).
2. Icons: allow a root copy of LG's app icons through Homebrew Channel, or keep
   letters for those apps.
3. Held keys: speed up after 1.5 s, or keep the steady 120 ms.
4. Sound: tvOS ticks on focus moves. shell-mvp.md leaves sound out; in or out
   for Phase 4?
5. Top Shelf art: the owner's per-app art, stills from Lumen's loops, or both.

## Not in this spec

Code changes. The Home key takeover itself (`takeover/`), screensavers (Phase
2), and any content service or API.
