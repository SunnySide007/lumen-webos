# Shell MVP (Phase 3 core)

The first real home screen: a tvOS-style grid of app tiles that the remote
moves through, with a lifted focus, a placeholder Top Shelf, and launching
through a `Launcher` the TV can implement later. This spec covers the work
that does not need the TV (pure logic, drawing, simulator tooling). The Home
key takeover (`takeover/`), glass, and real Top Shelf content are not part of
it.

Numbers here are design values in the fixed 1920x1080 space. They are tuned
by eye in the simulator; anything about speed is checked on the B9 before it
is claimed (CONTRIBUTING.md, rule 3).

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
  colour picked from an 8-colour palette by a hash of the app's ID, with its
  title centred on it in the 5x7 font. Icons and Inter text arrive in B2.
* **Focus:** exactly one tile is focused. It is scaled up, gets a soft
  shadow, and draws above its neighbours.
* **Status line:** a short message ("OPENING NETFLIX") in a dark pill at the
  bottom right, shown for 2 s.
* **No apps:** "NO APPS" centred where row 0 would be; arrows do nothing.

## Keys

| Action | Simulator (PC keyboard) | TV remote (B9) |
|---|---|---|
| Up, Down, Left, Right | arrow keys (scancodes 82, 81, 80, 79) | to be confirmed on the B9 (expected the same) |
| Select | Return (40), keypad Enter (88) | OK: to be confirmed (expected 40) |
| Back | Escape (41) | BACK: 482 (webOS), 270 (AC Back) |
| Home | Home (74) | HOME: 384 (webOS) |
| LongHome | hold Home for 800 ms | hold HOME for 800 ms |

The B9's codes come from the main session's TV run. Until then the table
above (SDL's standard codes plus the webOS ones already in `sdl.rs`) is used.

### Timing

| Key | Fires | While held |
|---|---|---|
| Arrows | on press | repeats after 400 ms, then every 120 ms |
| Select, Back | on press | nothing |
| Home | on release, if held less than 800 ms | at 800 ms fires LongHome once; the release is then ignored |

* SDL's own key repeats are ignored. The shell makes its own repeats, so the
  PC and the TV behave the same whatever their repeat settings.
* A key-down for a key that is already down is ignored (remotes may resend).
  A key-up with no key-down is ignored.
* When the window loses focus, held keys are dropped without firing.
* 800 ms must match the threshold the Home key filter uses in `takeover/`.

## Navigation

| Action | Effect |
|---|---|
| Left, Right | Move along the row. At either end the focused tile bumps; focus does not wrap. |
| Up, Down | Move to the same column in the next row, or to that row's last tile if it is shorter. Up on row 0 and Down on the last row bump. |
| Select | Launch the focused app. The tile dips (a press pulse) and the status line says what opened, or that it could not. |
| Back | Below row 0: focus row 0, same column, and scroll to the top. On row 0: leave the shell (see below). |
| Home | Focus the first tile and scroll to the top. |
| LongHome | Open LG's Home through the launcher: the escape hatch in docs/safety.md. |

* Moves follow tvOS's geometry: there is no remembered column, so Down into a
  short row and Up again lands on the tile above, not where you started.
* A bump is a 14 px nudge toward the blocked direction that springs back. A
  key repeat that reaches an edge stops there silently; only a fresh press
  bumps.
* **Back on row 0 leaves the shell.** Until the takeover makes the shell the
  Home, it is an ordinary webOS app, and BACK at the top is how those close.
  This is a policy flag in the core (`BackAtTop::Exit` now, `Stay` once the
  shell owns Home), so the change later is one line.
* **HOME reaching the shell at all is unconfirmed on the TV:** LG's
  compositor normally takes it. In the takeover design the key filter handles
  short and long presses itself. The shell's own handling covers the
  simulator, and the TV if LG's SDL turns out to deliver HOME.

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
| Status pill | bottom right of the safe area, 5x7 font at 3 px |

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
about 2% of one core on the same hardware class; Lumen's own figure is taken
on the B9 at Phase 3's TV check.

* A frame is drawn only when something changed (a key's effect, a window or
  app-lifecycle event, the status line expiring, the first frame) or a spring
  is still moving.
* After the last frame of a burst the shell blocks in `SDL_WaitEventTimeout`
  until an event or the next deadline: a key repeat, the long-press
  threshold, the status line's expiry, a scripted key, or `--exit-after`.
  With no deadline it waits without a timeout.
* Each burst ends with one log line: frames drawn, how long the burst took,
  average and worst frame time, then "idle". Frame statistics never include
  the idle wait.

## Apps and launching

```rust
pub struct App { pub id: AppId, pub title: String, pub kind: AppKind } // App or Input

pub trait Launcher {
    fn list(&mut self) -> Result<Vec<App>, LaunchError>;
    fn launch(&mut self, id: &AppId) -> Result<(), LaunchError>;
    fn open_system_home(&mut self) -> Result<(), LaunchError>; // LongHome
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
  | `input` | scancode → key map; the press, release, repeat and long-press state machine; `next_deadline()` |
  | `focus` | grid focus: move by direction → moved or blocked |
  | `layout` | the numbers above: tile rects, lift, scroll per row, safe area |
  | `anim` | the critically damped spring |
  | `apps` | `App`, `Launcher`, `MockLauncher` |
  | `home` | the home screen's state: takes input, returns effects (launch, LG Home, exit), steps the springs, answers `is_settled()`, and lists what to draw |
  | `script` | the `--keys` parser and timeline |

* **`lumen-shell`:** `sdl.rs` decodes key-up, window and app-lifecycle events
  and binds `SDL_WaitEventTimeout`; `app.rs` runs the loop above and turns
  effects into launcher calls; a new drawing module turns `home`'s view into
  a `render::Batch` (which gains a rounded rectangle made of horizontal
  strips). The hello-gles scene goes; the GL details it showed stay in the log.
* No new crates.io dependencies in B1.

## Simulator automation

So the UI can be checked without a person at the keyboard. All flags also
work on the TV, which makes them usable for smoke tests there later.

| Flag | Does |
|---|---|
| `--keys "R,R,D,OK"` | Plays keys: `U` `D` `L` `R` `OK` `BACK` `HOME`, and `LHOME` (Home held 1 s); case-insensitive. The first key goes down at 0.5 s, each is held 60 ms, and the next follows 300 ms after the previous release. They enter as synthetic scancodes, through the same input path as real keys. An unknown token is an error at start-up. |
| `--fixed-dt <s>` | A deterministic clock: each frame advances it by exactly `s`; when the screen is settled it jumps straight to the next deadline instead of sleeping. The same keys and step give the same frames. |
| `--screenshot-when-settled <png>` | Once the last scripted key is released and the screen settles, saves that frame and exits. |

* The existing `--screenshot`, `--exit-after`, `--windowed` and `--gles`
  keep working. `--exit-after` counts real time, so it is also a watchdog.
* `tools/sim.ps1` gains `-Keys`, `-FixedDt` and `-ScreenshotWhenSettled`; with
  `-ScreenshotWhenSettled` and no `-ExitAfter` it sets a 30 s watchdog.

## Milestones

### B1: the home grid in the simulator (this branch)

1. `lumen-core` with the modules above, each unit-tested.
2. `lumen-shell` draws the grid from `home`, decodes key-up, and stops
   repainting when settled.
3. The simulator automation flags.

**Done when:** `tools/dev.sh make check` is green (fmt, clippy for the PC and
armv7 with `-D warnings`, all tests); a Rust review and the orchestrated
review leave no critical or high findings; and simulator screenshots show the
start screen, focus moved along row 0, row 1 with the Top Shelf scrolled off,
and a launch's status line, with the log showing "idle" after each settle.

### B2: icons and text (when the owner says so)

Textured quads in the renderer, app icons, the Inter typeface for titles,
shader-drawn rounded corners and soft shadows. Needs dependencies the owner
approves first: PNG decoding and a font rasteriser. Inter's licence (OFL)
goes into THIRD-PARTY.md.

### B3: the TV launcher (after the TV recon)

A `Launcher` backed by luna: list apps and inputs, launch them, and open LG's
Home on LongHome; whether through `luna-send` or by loading
`libluna-service2` is decided from the recon report. `MockLauncher` stays for
the simulator. The first run on the TV is by hand, as an ordinary app, with
no takeover (docs/safety.md, rule 3).

## Settled on the TV (by the main session)

1. The B9 remote's scancodes for the arrows, OK, BACK and HOME.
2. Whether HOME reaches an app at all without the key filter.
3. How held keys arrive (SDL repeat flag, or repeated down and up pairs),
   which decides whether the shell's own repeat stays as specified.
4. Whether `SDL_WaitEventTimeout` on LG's SDL really sleeps.
5. The grid's frame time while moving, and the CPU use when idle.

## Not in this spec

The Home key filter (`takeover/`), glass and blur, Top Shelf content, the
Control Center, parallax, app editing and reordering (Phase 4), and sound.
