# Shell MVP (Phase 3 core)

The first real home screen: a tvOS-style grid of app tiles that the remote
moves through, with a lifted focus, a placeholder Top Shelf, and launching
through a `Launcher` the TV can implement later. This spec covers the work
that does not need the TV (pure logic, drawing, simulator tooling). The Home
key takeover (`takeover/`), glass, and real Top Shelf content are not part of
it.

Layout numbers are design values in the fixed 1920x1080 space, tuned by eye
in the simulator. Facts about the B9 come from the main session's hello-gles
run on an LG OLED55B9PUA, webOS 4.10.2-31, firmware 05.50.00 (hello-gles at
commit 0932965) and from this spec's B1 build on the same set ("Measured on
the B9"); anything else about speed is checked on the TV before it is
claimed (CONTRIBUTING.md, rule 3).

## The screen

```
y=0     ┌──────────────────────────── Top Shelf (placeholder) ───────────────────────────┐
        │                                                                                │
        │  NETFLIX                         title of the focused app, large               │
y=640   └────────────────────────────────────────────────────────────────────────────────┘
y=700      ┏━━━━━━━━┓ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐     row 0, focus lifted
           ┗━━━━━━━━┛ └────────┘ └────────┘ └────────┘ └────────┘
y=980      ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐     row 1 peeks in
x=         80         440        800        1160       1520 (to 1840)
```

* **Background:** a dark vertical gradient, fixed (it does not scroll).
* **Top Shelf:** a full-width placeholder band, 1920x640 at the top of the
  content. It shows a gradient and, bottom left, the title of the focused
  app on row 0. It is not focusable in this phase.
* **App grid:** rows of five tiles in launcher order. Each tile is a solid
  colour from an 8-colour palette, by its position in the list (so no two
  neighbours match), with its title centred on it in the 5x7 font. Icons and
  real text arrive in B2.
* **Focus:** exactly one tile is focused. It is scaled up, gets a soft
  shadow, and draws above its neighbours.
* **Status line:** a short message ("OPENING NETFLIX") in a dark pill at the
  top right, over the Top Shelf while focus is on row 0, shown for 2 s.
* **No apps:** "NO APPS" centred where row 0 would be; arrows do nothing.

## Keys

| Action | Simulator (PC keyboard) | B9 remote (measured) |
|---|---|---|
| Up, Down, Left, Right | arrow keys: 82, 81, 80, 79 | the same: 82, 81, 80, 79 |
| Select | Return (40), keypad Enter (88) | OK: 40 |
| Back | Escape (41) | BACK: 482 (AC Back, 270, is accepted too) |

* Everything else is ignored, including 484 and 485, which arrive around
  Magic Remote pointer movement (probably pointer shown and hidden).
* **HOME never reaches the shell.** On the B9, LG's compositor takes it and
  opens LG's Home, which is also today's escape hatch. Short and long Home
  presses belong to the key filter in `takeover/`, so the shell has no Home
  action and nothing in it depends on one.

### Timing

| Key | Fires | While held |
|---|---|---|
| Arrows | on press | repeats after 400 ms, then every 120 ms |
| Select, Back | on press | nothing |

* SDL's own repeat events never fire actions. The shell makes its own
  repeats, so the PC and the TV move at the same rate whatever their repeat
  settings. SDL's repeats only show that a key is still held.
* A key-down for a key that is already down is ignored (remotes may resend).
  A key-up with no key-down is ignored.
* When the window loses focus or the app goes to the background, held keys
  are dropped without firing.
* **A lost release cannot scroll forever:** a held key that has had neither
  an SDL repeat nor a release for 1.5 s counts as released. Both the B9 and
  a PC send repeats while a key is held, so this only trips when a key-up
  goes missing.

### Key events on LG's SDL

LG's SDL on the B9 (`/usr/lib/libSDL2-2.0.so.0`, reporting 2.0.5) lays out
`SDL_KeyboardEvent` 4 bytes later than upstream SDL. This is why hello-gles's
BACK did nothing on the TV: it read the state byte as the scancode.

| Field | Upstream SDL (PC) | LG's SDL (B9) |
|---|---|---|
| state | byte 12 | byte 16 |
| repeat | 13 | 17 |
| keysym.scancode | 16 | 20 |
| keysym.sym | 20 | 24 |
| bytes 12–15 | (state, repeat, padding) | an unidentified u32, always 2 so far |

* **The layout is chosen when SDL loads:** LG's layout for the system
  library on the TV (32-bit ARM Linux, loaded by its bare name), upstream's
  for `LUMEN_SDL2`, a copy bundled beside the binary, and every PC.
  `LUMEN_SDL_KEY_LAYOUT=upstream` or `=lg` overrides the choice.
* A key-down whose state byte is not "pressed" at the chosen offsets is
  logged once as a likely layout mismatch.
* The first 8 key events and the first 8 other events are logged with their
  first 48 bytes in hex, so the next TV run can identify the unknown field
  and check where window events keep their data.
* Window events are read at upstream offsets, which the B9 run confirmed.
  App-lifecycle events carry no data beyond their type, so they need no
  offsets.
* The raw event buffer grows from 64 to 128 bytes. LG's structs differ from
  upstream's, and a larger `SDL_Event` must not overrun it.
* Regression tests decode the B9's byte pattern: state 1 at 16, repeat at
  17, scancode at 20.

## Navigation

| Action | Effect |
|---|---|
| Left, Right | Move along the row. At either end the focused tile bumps; focus does not wrap. |
| Up, Down | Move to the same column in the next row, or to that row's last tile if it is shorter. Up on row 0 and Down on the last row bump. |
| Select | Launch the focused app. The tile dips (a press pulse) and the status line says what opened, or that it could not. |
| Back | Below row 0: focus row 0, same column, and scroll to the top. On row 0: leave the shell (see below). |

* Moves follow tvOS's geometry: there is no remembered column, so Down into a
  short row and Up again lands on the tile above, not where you started.
* A bump is a 14 px nudge toward the blocked direction that springs back. A
  key repeat that reaches an edge stops there silently; only a fresh press
  bumps.
* **Back on row 0 leaves the shell.** Until the takeover makes the shell the
  Home, it is an ordinary webOS app, and BACK at the top is how those close.
  This is a policy flag in the core (`BackAtTop::Exit` now, `Stay` once the
  shell owns Home), so the change later is one line.

## Layout

All in the 1920x1080 space, origin top left. "Content" coordinates scroll;
screen coordinates do not.

| Item | Value |
|---|---|
| Safe area | 80 px left and right, 60 px top and bottom (tvOS's guide): x 80–1840, y 60–1020 |
| Columns | 5 |
| Tile | 320 x 192 (5:3, tvOS's icon shape), corner radius 12 |
| Column gap | 40 (pitch 360): five tiles fill the safe width exactly |
| Top Shelf | content rect 0, 0, 1920 x 640 |
| Row 0 | content y 700 |
| Row pitch | 280 (row gap 88) |
| Focus scale | 1.15 about the tile's centre: 368 x 221 |
| Shadow | 4 layers stepping out 6 px each, offset 18 px down at full lift, total opacity about 0.4 |
| Title | 5x7 font at 3 px per font pixel, centred; cut to fit 16 px inside the tile, ending in `...` |
| Top Shelf title | 5x7 font at 8 px, left at x 80, bottom 60 px above the shelf's lower edge |
| Status pill | top right of the safe area, 5x7 font at 3 px |

Checks the numbers pass (each becomes a unit test):

* A lifted tile grows 24 px each side, leaving 16 px to its neighbour, and
  about 14 px top and bottom, leaving 74 px to the next row; its shadow ends
  about 56 px below the tile's resting edge, inside the 88 px gap. Lifted
  edge tiles reach 24 px into the side margins, which is allowed: the safe
  area bounds the resting layout.
* **Scrolling:** with row 0 focused the scroll is 0. With row r of 1 or more
  focused, the content scrolls so that row sits at screen y 340: scroll =
  360 + 280r. At row 1 that is 640, which lifts the Top Shelf exactly off
  screen and puts row 0 at y 60, the safe top. One row above the focus stays
  visible and the next row peeks in below.
* Rows entirely off screen are not drawn.

### Draw-list size

On the B9, hello-gles held a steady 60.0 fps (average 16.67 ms, worst
17–19 ms) drawing about 3,000 quads in one batched call, on a Mali-T820 MP2
reporting OpenGL ES 3.2 at 1920x1080 with vsync. B1 aims to stay at or below
that size, so that measurement remains the reference until the grid itself
is measured:

* Rounded corners use 6 strips per corner band (13 quads a tile).
* The 5x7 font costs one quad per lit pixel, about 130 quads for a typical
  title; with 12 visible tiles, the Top Shelf title and the status line, a
  frame comes to roughly 2,000–2,500 quads.
* The vertex list is reused from frame to frame, never reallocated. Memory
  is tight on the B9 (about 330 MB free of 1.6 GB, with swap in use).

## Motion

Every animation is a critically damped spring stepped by an injected time
step, using the spring's exact solution. The same motion comes out whether
frames are 16.7 ms, 33 ms or a 300 ms hitch: a slow frame shows as a jump,
never as slow motion.

| Spring | Value | Speed (ω) | Feel |
|---|---|---|---|
| Lift, per tile | 0 resting, 1 focused | 30 /s | 95% there in about 160 ms, settled in about 350 ms |
| Scroll | content offset, px | 20 /s | 95% in about 240 ms, settled in about 0.55 s |
| Bump, focused tile | x or y offset, px | 20 /s | kicked to peak at 14 px after 50 ms, settled in about 0.45 s |
| Press pulse | the lift spring, kicked down | 30 /s | dips to about 0.5 (scale about 1.08), then returns |

* Retargeting keeps the spring's velocity, so fast key presses chain smoothly
  rather than restarting.
* A spring is settled when it is within 0.25 px of its target and moving
  slower than 2 px/s (lift: within 0.001, slower than 0.01/s). It then snaps
  exactly onto the target.

## Repainting

A settled screen stops repainting. plx-native measured such a screen at
about 2% of one core on the same hardware class; Lumen's settled grid used
about 0.2% of one core on the B9 ("Measured on the B9").

* A frame is drawn only when something changed (a key's effect, the window
  coming back, the status line expiring, the first frame) or a spring is
  still moving.
* After the last frame of a burst the shell blocks in `SDL_WaitEventTimeout`
  until an event or the next deadline: a key repeat, the stuck-key timeout,
  the status line's expiry, a scripted key, or `--exit-after`. With no
  deadline it waits without a timeout.
* Each burst ends with one log line: frames drawn, how long the burst took,
  average and worst frame time, then "idle". Frame statistics never include
  the idle wait.

### In the background, nothing is drawn

On the B9, hello-gles kept drawing at 60 fps behind LG's Home. The shell
stops drawing, even mid-animation, when:

* the window is hidden or minimised (`SDL_WINDOWEVENT` hidden, minimised),
  on every platform;
* the app goes to the background (`SDL_APP_WILLENTERBACKGROUND`,
  `SDL_APP_DIDENTERBACKGROUND`), on every platform;
* the window loses focus, on the TV only. There it means another app is in
  front; on a PC an unfocused window is still on screen.

Springs then snap to their targets and held keys are dropped. Shown,
exposed, restored, focus gained (TV) and `SDL_APP_DIDENTERFOREGROUND` draw
one fresh frame. On the B9, HOME sends only a focus loss, so the TV-only
focus rule is what stops the drawing there.

## Apps and launching

```rust
pub struct App { pub id: AppId, pub title: String, pub kind: AppKind } // App or Input

pub trait Launcher {
    fn list(&mut self) -> Result<Vec<App>, LaunchError>;
    fn launch(&mut self, id: &AppId) -> Result<(), LaunchError>;
}
```

* Launch order is the launcher's order. Pinning (plx-native first) and
  hiding system apps come with the TV launcher in B3.
* `MockLauncher` serves the simulator and tests: a fixed list of 12 entries
  modelled on the B9's apps and HDMI inputs (rows of 5, 5 and 2, so the short
  last row gets exercised). It records every launch, and a test can make one
  ID fail.
* If `list` fails, the shell shows the empty state and puts the error in the
  status line and the log.

## Code

* **`shell/crates/lumen-core`** (new): pure logic, std only,
  `#![forbid(unsafe_code)]`, no SDL or GL, builds for
  `armv7-unknown-linux-gnueabi`. Lints move to `[workspace.lints]` in
  `shell/Cargo.toml` and both crates inherit them.

  | Module | Holds |
  |---|---|
  | `input` | scancode → key map; the press, release, repeat and stuck-key state machine; `next_deadline()` |
  | `focus` | grid focus: move by direction → moved or blocked |
  | `layout` | the numbers above: tile rects, lift, scroll per row, safe area |
  | `anim` | the critically damped spring |
  | `apps` | `App`, `Launcher`, `MockLauncher` |
  | `home` | the home screen's state: takes input, returns effects (launch, exit), steps the springs, answers `is_settled()`, and lists what to draw |
  | `script` | the `--keys` parser and timeline |

* **`lumen-shell`:** `sdl.rs` decodes key-down and key-up in both layouts,
  window and app-lifecycle events, logs the first raw events, and binds
  `SDL_WaitEventTimeout`; `app.rs` runs the loop above and turns effects into
  launcher calls; a new drawing module turns `home`'s view into a
  `render::Batch` (which gains a rounded rectangle made of horizontal
  strips). The hello-gles scene goes; the GL details it showed stay in the log.
* No new crates.io dependencies in B1.

## Simulator automation

So the UI can be checked without a person at the keyboard. All flags also
work on the TV, which makes them usable for smoke tests there later.

| Flag | Does |
|---|---|
| `--keys "R,R,D,OK"` | Plays keys: `U` `D` `L` `R` `OK` `BACK`; case-insensitive. The first key goes down at 0.5 s, each is held 60 ms, and the next follows 300 ms after the previous release. They enter as synthetic scancodes, through the same input path as real keys. An unknown token is an error at start-up. |
| `--fixed-dt <s>` | A deterministic clock: each frame advances it by exactly `s`; when the screen is settled it jumps straight to the next deadline instead of sleeping. The same keys and step give the same frames. |
| `--screenshot-when-settled <png>` | Once the last scripted key is released and the screen settles, saves that frame and exits. |

* The existing `--screenshot`, `--exit-after`, `--windowed` and `--gles`
  keep working. `--exit-after` counts real time, so it is also a watchdog.
* `tools/sim.ps1` gains `-Keys`, `-FixedDt` and `-ScreenshotWhenSettled`; with
  `-ScreenshotWhenSettled` and no `-ExitAfter` it sets a 30 s watchdog.

## Milestones

### B1: the home grid in the simulator (this branch)

1. `lumen-core` with the modules above, each unit-tested.
2. `lumen-shell` reads keys in both SDL layouts, draws the grid from `home`,
   stops repainting when settled, and stops drawing in the background.
3. The simulator automation flags.

**Done when:** `tools/dev.sh make check` is green (fmt, clippy for the PC and
armv7 with `-D warnings`, all tests); a Rust review and the orchestrated
review leave no critical or high findings; and simulator screenshots show the
start screen, focus moved along row 0, row 1 with the Top Shelf scrolled off,
and a launch's status line, with the log showing "idle" after each settle.
The TV build is then ready for the main session to run, which also captures
the raw event log. That run was on 2026-10-20 ("Measured on the B9").

### B2: icons and text (when the owner says so)

Textured quads in the renderer, app icons, real text for titles,
shader-drawn rounded corners and soft shadows. Two ways to decode images and
draw text; the owner picks one:

| | Rust crates (a PNG decoder and a font rasteriser, for example `png` and `fontdue`) | LG's SDL2_image and SDL2_ttf, loaded at run time like SDL2 |
|---|---|---|
| On the B9 | compiled in | already in `/usr/lib` (2.0.x) |
| Dependencies | new crates, for the owner to approve | none; the simulator downloads pinned Windows DLLs for both, as it does SDL2 |
| PC and TV match | same code, same pixels | different FreeType builds may draw text slightly differently |
| Risk | binary size | both hand back `SDL_Surface` structs, whose field offsets would need checking on the TV, since LG's SDL already differs from upstream |

Recommendation: the crates, because the simulator then shows exactly what
the TV draws, and nothing depends on LG's struct layouts. Inter, the planned
typeface, is OFL-licensed and goes into THIRD-PARTY.md either way.

### B3: the TV launcher (after the TV recon)

A `Launcher` backed by luna: list apps and inputs, and launch them. Both
routes exist on the B9: `luna-send` works, and `libluna-service2.so.3`
(3.21.2) can be loaded at run time. B3 starts with `luna-send`, which needs
no FFI, and moves to the library only if a launch time measured on the TV
calls for it. `MockLauncher` stays for the simulator. The first run on the TV
is by hand, as an ordinary app, with no takeover (docs/safety.md, rule 3).

## Measured on the B9

The main session's hello-gles run answered the remote's scancodes, that
HOME never reaches the app, that LG's SDL marks held keys with the repeat
flag, the key-event layout, and the GPU's headroom for one batched draw.

The B1 build (commit 3572bf3) ran on the same set on 2026-10-20, by hand as
an ordinary app, with the owner on the remote:

1. **Bytes 12–15 of LG's key events** read 2 in all 8 logged key events,
   presses and releases of Right, Down and Up. A constant, so probably an
   input-device id rather than data about the key. The shell ignores it.
2. **Window events are not shifted:** the event id is at byte 12, and data1
   and data2 at 16 and 20 (size changed 1920x1080, shown, focus gained), as
   upstream. Launching sends the will- and did-enter-foreground events.
   **HOME sends only a focus loss,** with a pointer enter, motion and leave
   as LG's Home shows the pointer: no hidden, minimised or background
   event (the log records the first event of each kind, and none came). The
   TV-only rule that focus loss means another app is in front is what stops
   the drawing. Coming back resumed drawing on a kind of event already seen
   at launch.
3. **Key-ups arrive on release,** 100–151 ms after the key-down of a tap.
   Holding Right fired the shell's own repeats, which stopped silently at
   the end of the row.
4. **Settled, the shell sleeps:** 2 clock ticks (20 ms of CPU) in 10 s, about
   0.2% of one core, at 18 MB resident.
5. **Moving, the grid holds 60 fps:** about 520 frames in 13 bursts averaged
   16.2–16.7 ms a burst, the worst frame at or under 17.8 ms in 11 of them.
   Down into the short last row took 19.6 ms once, and one frame took
   33.3 ms (a missed vsync), the only miss. Frames drew 856–1,631 quads.

BACK below row 0 scrolled to the top, and BACK on row 0 closed the shell
("clean exit" 180 ms later). The shell lost and regained focus a second time
during the run, with the same result.

Still open:

1. How fast the remote repeats. SDL's repeat events are not logged, so the
   next run needs a log line with the times of the first few.
2. Which event brings the shell back to the front (focus gained, or the
   foreground events): the log records only the first event of each kind.
3. What the u32 at bytes 12–15 means. Another input (a USB keyboard, LG's
   phone app) may show a different value.

## Not in this spec

The Home key filter (`takeover/`), the Magic Remote pointer, glass and blur,
Top Shelf content, the Control Center, parallax, app editing and reordering
(Phase 4), and sound.
