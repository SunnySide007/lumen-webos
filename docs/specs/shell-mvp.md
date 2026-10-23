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
* **App grid:** rows of five tiles in launcher order. Each tile is its app's
  colour (webOS's `iconColor`) or, without one, a colour from an 8-colour
  palette by its position in the list (so no two neighbours match). The
  app's icon sits in the middle; an app without one, or whose icon will not
  load, shows the first letter of its title instead.
* **Focus:** exactly one tile is focused. It is scaled up, gets a soft
  shadow, draws above its neighbours, and its title appears below it, as on
  tvOS. Unfocused tiles show no title.
* **Status line:** a short message ("Opening Netflix") in a dark pill at the
  top right, over the Top Shelf while focus is on row 0, shown for 2 s.
* **No apps:** "No apps" centred where row 0 would be; arrows do nothing.
* **Text:** Inter SemiBold (4.1, OFL), built into the binary. It is not
  kerned yet: fontdue reads kerning only from a `kern` table, and Inter keeps
  its kerning in GPOS.

## Keys

| Action | Simulator (PC keyboard) | B9 remote (measured) |
|---|---|---|
| Up, Down, Left, Right | arrow keys: 82, 81, 80, 79 | the same: 82, 81, 80, 79 |
| Select | Return (40), keypad Enter (88) | OK: 40; while the pointer shows, OK arrives as a click of pointer button 1 instead, which the shell takes as Select ("Input diagnostics") |
| Back | Escape (41) | BACK: 482 (AC Back, 270, is accepted too) |

* 484 and 485 are the Magic Remote's pointer appearing and going away
  (webosbrew's SDL names them `SDL_SCANCODE_WEBOS_CURSOR_SHOW` and
  `_HIDE`; the B9 sends 484 as the remote moves and 485 about 5 s after it
  stops). They are key-downs with no key-up; the shell notes them and acts
  on neither. Everything else is ignored.
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
* The first 8 key events and the first event of each other kind (up to 24
  kinds) are logged with their first 48 bytes in hex, so the next TV run can
  identify the unknown field and check where window events keep their data.
* SDL's own repeats are timed with each event's timestamp (byte 4 in both
  layouts) for the first 8 keys held long enough to repeat: the log gets
  each of a key's first 5 repeats, then, at its release, how many came and
  their average spacing. Only the key pressed last is timed.
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
| Shadow | one shader-drawn shadow in the tile's shape, offset 18 px down at full lift, its edge fading over 24 px either side, opacity 0.4 |
| Icon | longer side 120 px at rest, centred, growing with the tile |
| Letter (no icon) | the title's first letter, 96 px at rest, centred, growing with the tile |
| Title | 30 px, below the focused tile only: capitals 14 px under its lifted edge, centred, fading in with the lift; cut to the lifted width (368 px), ending in `…` |
| Top Shelf title | 72 px, left at x 80, baseline 60 px above the shelf's lower edge; the caption "TOP SHELF" (24 px, dim) above it |
| Status pill | top right of the safe area, 28 px |
| No apps | 48 px |

Checks the numbers pass (each becomes a unit test):

* A lifted tile grows 24 px each side, leaving 16 px to its neighbour, and
  about 14 px top and bottom, leaving 74 px to the next row; its shadow ends
  about 56 px below the tile's resting edge, inside the 88 px gap, and so does
  its title, whose line ends at most about 58 px below that edge. Lifted
  edge tiles reach 24 px into the side margins, which is allowed: the safe
  area bounds the resting layout. Icons and letters fit inside a resting
  tile.
* **Scrolling:** with row 0 focused the scroll is 0. With row r of 1 or more
  focused, the content scrolls so that row sits at screen y 340: scroll =
  360 + 280r. At row 1 that is 640, which lifts the Top Shelf exactly off
  screen and puts row 0 at y 60, the safe top. One row above the focus stays
  visible and the next row peeks in below.
* Rows entirely off screen are not drawn.

### Draw cost

On the B9, hello-gles held a steady 60.0 fps (average 16.67 ms, worst
17–19 ms) drawing about 3,000 flat quads in one batched call, on a Mali-T820
MP2 reporting OpenGL ES 3.2 at 1920x1080 with vsync, and the B1 grid (856 to
1,631 quads, rounded corners made of strips, a pixel font) held 60 fps too.
B2 keeps one draw call a frame with far fewer quads:

* Every rounded rectangle and every shadow is one quad, cut out by the
  fragment shader (the distance to a rounded box, with a one-pixel soft
  edge; a shadow's edge is 24 px wide either side).
* Icons and glyphs come from one 2048x1024 RGBA atlas (8 MB of graphics
  memory), one quad each. The start screen is 45 quads in the simulator.
* The cost moves from vertices to pixels: the shape shader runs over every
  pixel of every tile, pill and shadow, in high precision where the GPU has
  it. On the B9 the grid averages 16.8 ms a frame but misses a vsync in most
  bursts of motion, which B1 did not ("Measured on the B9", 6).
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
pub struct App {
    pub id: AppId,
    pub title: String,
    pub kind: AppKind,              // App or Input
    pub icon: Option<PathBuf>,      // a PNG (webOS's icon or largeIcon)
    pub color: Option<[u8; 3]>,     // webOS's iconColor
}

pub trait Launcher {
    fn list(&mut self) -> Result<Vec<App>, LaunchError>;
    fn launch(&mut self, id: &AppId) -> Result<(), LaunchError>;
}
```

* Launch order is the launcher's order. On the TV that is LG's, with
  plx-native pinned first (B3, below).
* `MockLauncher` serves the simulator and tests: a fixed list of 12 entries
  modelled on the B9's apps and HDMI inputs (rows of 5, 5 and 2, so the short
  last row gets exercised). It records every launch, and a test can make one
  ID fail. `with_icons_in(dir)` gives its apps the placeholder pictograms in
  `shell/assets/mock-icons` (play, note, tv, input, globe, gear; drawn by
  `tools/mock-icons.py`), leaving Plex and Disney+ as letter tiles. The
  simulator finds them through `LUMEN_MOCK_ICONS`, which `tools/sim.ps1`
  sets. The TV build has no mock icons; `LUMEN_LAUNCHER=mock` gives it the
  mock list with letter tiles.
* On the TV the icon paths come from other apps' appinfo, so an icon is
  read only from a regular file of at most 1 MB, opened without blocking (a
  FIFO or a device cannot hang the shell), and decoded only if it is at most
  512 px a side, within 4 MB of decoder memory. One larger than 160 px is
  halved until it fits, since a tile draws it at most 138 px; that keeps a
  frame's icons well inside the atlas and about 100 KB of memory each. A
  failure is logged once, with the path escaped, and the tile shows its
  letter.
* Icons load on a thread of their own, in the order tiles first ask for
  them. Until its icon arrives a tile is drawn plain, neither icon nor
  letter, so a letter never flashes up first; each arrival draws one frame.
  While any is on its way, a settled screen looks for them every 16 ms.
  With `--fixed-dt` they load as they are first drawn, so the frames stay
  the same from run to run.
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
  launcher calls; `draw.rs` turns `home`'s view into a `render::Batch`. The
  hello-gles scene goes; the GL details it showed stay in the log.
* B2 adds `atlas.rs` (the shelf packer and pending uploads), `text.rs`
  (Inter through fontdue, a glyph cache, fitting with an ellipsis) and
  `icons.rs` (PNG decoding with limits, premultiplied); `render.rs` gains the
  shape shader and the atlas texture. The 5x7 font goes.
* No new crates.io dependencies in B1. B2 adds `png` 0.18 and `fontdue` 0.9
  (`std` only), both approved by the owner.
* B3 adds `luna.rs` (the TV's `Launcher`) and `serde_json` 1.0 (`std` only,
  its `Value` API, no derive), which the owner chose on 2026-10-20 over a
  parser of our own: luna's replies carry other apps' data. `lumen-core`
  stays std only and gains `clean_title` and a `Launcher` impl for `Box`.
  `luna.rs` loads libluna-service2 and glib through `libloading`, as
  `sdl.rs` loads SDL, so the cross-compile still links no TV library.
* **The TV build uses the FPU.** The target's default is software float:
  every float operation is a call into libgcc's integer code. The B9's
  Cortex-A53 has VFPv4 and NEON, so `shell/.cargo/config.toml` builds the
  TV target with `-Ctarget-feature=-soft-float,+vfp4,+neon`. Floats still
  pass in core registers (softfp), so LG's libraries and the prebuilt `std`
  link as they are. rustc warns that the three features are unstable; the
  warnings are not lints, so `make check` passes. On the B9 this cut the
  font set-up from about 930 ms to about 170 ms ("Measured on the B9", 16).

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

### B1: the home grid in the simulator (done)

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

### B2: icons and text (done; measured on the B9 on 2026-10-22)

Textured quads in the renderer, app icons, real text for titles,
shader-drawn rounded corners and soft shadows. Of the two ways to decode
images and draw text, the owner chose the Rust crates (`png`, `fontdue`) over
LG's SDL2_image and SDL2_ttf on 2026-10-20: the simulator then shows exactly
what the TV draws, and nothing depends on LG's `SDL_Surface` layout. Titles
show under the focused tile only, as on tvOS (also the owner's choice).

**Done when:** `make check` is green; the reviews leave no critical or high
findings; and simulator screenshots show the start screen, focus moved along
row 0, row 1 with the Top Shelf scrolled off, a launch's status line, and
letter tiles for apps without icons. Then the TV run: the frame time while
moving, now that the cost is in the fragment shader. That run, with B3's
apps, is in "Measured on the B9" (6).

### B3: the TV launcher (this branch)

A `Launcher` backed by luna: list apps and inputs, and launch them, through
`libluna-service2.so.3` (3.21.2 on the B9), loaded at run time.
`MockLauncher` stays for the simulator. The first run on the TV is by hand,
as an ordinary app, with no takeover (docs/safety.md, rule 3).

**Why the library and not `luna-send`.** B3 started with `luna-send`, which
needs no FFI. Its first TV run (2026-10-21, OLED55B9PUA, firmware 05.50.00,
started with `tools/tv.sh launch`) answered the question this section had
left open: from inside the app's jail, `/usr/bin/luna-send` cannot be
executed. All three calls failed with `Permission denied (os error 13)`
within 2 ms, and the shell listed 0 apps. (Over SSH, as root, `luna-send`
works, which is how the replies below were taken; why the jail refuses it,
file mode or mount, was not checked.) The routes weighed:

| Route | Verdict |
|---|---|
| libluna-service2 through `dlopen`, as an anonymous client | **Taken.** The jail mounts `/usr/lib`, which holds the library (the shell's SDL, EGL and GLES load from it), and the hub's socket directory, `/var/run/luna-service2`. plx-native, a jailed native app, measured on webOS 4.10.2 (2026-09-04, `rust-modules/platform/src/webos.rs`, module `ls2`) that the hub accepts `LSRegister(NULL)` from the jail and answers that client's `applicationManager/getForegroundAppInfo` and `launch` calls, and that it refuses `LSRegisterApplicationService(NULL, appId)` (`Can not find service "" permissions`). moonlight-tv launches LG's Home from its jail too, through LG's libhelpers, which wraps the same library. Needs no root and writes nothing on the TV. |
| `luna-send` | Not executable in the jail (above). |
| Homebrew Channel's `luna://org.webosbrew.hbchannel.service/exec`, running `luna-send` | Would answer everything, but runs the command as root, and the shell would still need the bus to reach it. Not needed: the hub answers the library route (below). Anything through it needs the owner's OK first (docs/safety.md). |
| `applicationManager/dev/listApps`, the developer-mode call Homebrew Channel uses without root | Lists developer-mode apps only, not LG's Home bar or its order. |
| `requiredACG` in appinfo.json | Not needed here. `ares-package` warns that it is missing for any `tv` profile, but LG's ACG guide says webOS TV 25 and older do not support ACG; it is testable on 26 and required from 27. The B9 runs webOS 4.5, so appinfo.json stays as it is. (The guide puts `launch` in the `application.launcher` group and lists none of the three list calls.) |

What a jailed client may call is the hub's decision: by default a
non-system app reaches only public methods (webosbrew's "Luna service bus"
page), and plx-native's evidence covers `launch` only. A refusal would come
back as a reply with `returnValue: false`, whose `errorText` the log records.

**The library build on the B9** (2026-10-21, OLED55B9PUA, firmware
05.50.00; branch `claude/shell-ls2`, started with `tools/tv.sh launch` with
the owner at the TV): the hub answered all three calls from the jail,
`getAllInputStatus` in 9 ms, `listLaunchPoints` in 44 ms and `listApps` in
50 ms, run at once, so the tiles were ready 55 ms after the calls began.
That gave 30 tiles: LG's 26 launch points other than Lumen, then the 4
connected inputs (AV, unconnected, left out). The owner confirmed that the
grid matched LG's Home bar, with plx-native first. Launching a tile was not
tried that night; the first launches were timed on 2026-10-22 ("Measured on
the B9", 10).

**Only 4 of the 30 tiles had icons.** The app's jail mounts `/media/developer`
but not `/usr/palm/applications`, `/media/cryptofs/apps` or
`/usr/palm/plugins/inputapps` (its `/proc/mounts`, read on the same run).
Apart from Lumen, the B9 has exactly 4 developer-mode apps (plx-native,
Homebrew Channel, a webOS terminal and `com.open.hidden.menu`), which
accounts for the 4. The other icon paths don't resolve inside the jail, so
confinement drops them and those tiles show their letters, inputs included.

**What the B9 answers** (queried over SSH as root on 2026-10-20; trimmed
copies with generic input names are the tests' fixtures, in
`shell/crates/lumen-shell/fixtures/luna/`):

| Call | Gives |
|---|---|
| `applicationManager/listLaunchPoints` | LG's Home bar: 27 launch points in its order, each with `title`, `iconColor`, absolute `icon` (80 px) and `largeIcon` (130 px, sometimes empty), `lptype` (all `default` here) and `params`. No inputs; Lumen itself is one. |
| `applicationManager/listApps` | 99 apps (27 visible), each with its `folderPath`. Apps live under `/usr/palm/applications`, `/media/cryptofs/apps`, `/media/developer/apps`, `/media/system/apps` and `/mnt/otncabi`. |
| `service.eim/getAllInputStatus` | AV and HDMI 1–4, each with its app (`com.webos.app.hdmi1`…), the `label` the owner gave it, `connected`, and an icon under `/usr/palm/plugins/inputapps/assets/small/`. |

**The list**, owner's choice on 2026-10-20 (LG's launcher list, not every
visible app):

1. plx-native (`com.beb.plxnative`) first.
2. LG's launch points in LG's order, so rearranging LG's Home bar
   rearranges Lumen. Left out: Lumen itself, any launch point that is not
   an app's own (`lptype` other than `default`: bookmarks, channels), and
   repeats.
3. The inputs with something connected, by port, titled with the owner's
   names (`Game console`), unless LG's bar already has them.

The three calls run at once, each on a thread of its own and through an
anonymous client of its own (`LSRegister` with no name), attached to a glib
context of its own, so no handle or context is shared between threads: the
one-process-a-call shape `luna-send` had. A call is `LSCallOneReply`; its
thread dispatches the context without blocking, sleeping 1 ms whenever
nothing came in, until the reply is in or 5 s have passed, and a call still
open then is cancelled. The client is then unregistered and its context
freed. If the cancel or the unregister is refused, what the library may
still use (the reply's slot, the context) is leaked rather than freed under
it. A reply over 4 MB is refused. Each call is timed in the log, and a
refusal from the library logs its own message. If the library cannot be
loaded, every call fails with the reason.
Without launch points the shell shows its empty state and the error.
Without `listApps` no icon can be confined, so every tile shows its letter.
Without the inputs there are only apps. Launching calls
`applicationManager/launch` with the app's ID and its launch point's
`params`; a refusal's `errorText` goes to the log, and the status line says
the app could not open.

Feeding other apps' data into the shell, B3 also:

* Confines each icon path to its owner's folder: an app's icon to the
  `folderPath` `listApps` gives it, an input's to
  `/usr/palm/plugins/inputapps`. The path is resolved against that folder,
  canonicalised, and refused unless it is still inside, so neither `..` nor
  a link can take it elsewhere. The 130 px icon is tried first, then the
  80 px one. An app can change its folder after the list, and a tile may
  first ask for its icon minutes later, so the loader opens an icon only if
  its path still resolves to itself (on Linux; the shell's other checks
  then apply).
* Takes `iconColor` only as `#rrggbb`; titles through `clean_title`:
  trimmed of spaces and control characters, those inside as spaces, at most
  64 characters; and IDs only as printable ASCII without spaces, at most
  255 bytes (a folder name's limit), since they reach the log and `launch`
  as they are.
* Loads icons on a thread of their own ("Apps and launching").
* Drops the mock icons from the TV build: the IPK's `mock-icons` folder, and
  the binary's search for one.

**Done when:** `make check` is green and the reviews leave no critical or
high findings. Then the TV run, by hand: the list matches LG's Home bar with
plx-native first, every tile launches, the inputs switch, and the log shows
how long the three calls and each launch take. On 2026-10-21 the first run
showed that `luna-send` may not run inside the app's jail, and the second,
through the library, that the list works and matches LG's Home bar (above).
On 2026-10-22 HDMI 1 and Homebrew Channel launched from their tiles
("Measured on the B9", 10). Still owed: the other tiles, the other inputs,
and icons for apps outside `/media/developer`.

### Frame time: finding the missed vsyncs (plan, approved 2026-10-22)

Most bursts of motion miss a vsync on the B9 ("Measured on the B9", 6),
which B1 did not. It is looked into before any Phase 4 item spends frame
time (look-and-feel.md, "Order").

**What changed since B1.** B1 drew every pixel with a flat colour in
mediump. B2 draws every pixel, the full-screen background and the 1920x640
shelf included, through one shader in highp with three branches (flat,
rounded-box distance, atlas texture) and blending on. B2 also rasterises
glyphs on the main thread the first time they are drawn and uploads them
with `glTexSubImage2D` into the atlas that the same frame reads. A frame's
time runs from one swap's return to the next; the first frame after an idle
wait is not timed; and the log keeps only each burst's worst frame, so which
frame was slow, and where its time went, is not known.

| | Cause | Fits because | Told apart by |
|---|---|---|---|
| H1 | Waking from idle: the GPU clock has dropped, or the buffer queue starts out of phase | A single tap's burst starts from idle and almost always misses; hello-gles drew without stopping and never missed | The slow frame is the burst's first or second; drawing without a stop removes it |
| H2 | The pixel cost of the highp shader over about 4 Mpx a frame | Down's scroll misses; the cost moved from vertices to pixels | Slow frames mid-burst, their time in the swap; a flat or mediump shader removes them |
| H3 | Main-thread work: rasterising glyphs, and uploading into an atlas the GPU still reads (on Mali, a copy of the 8 MB texture or a wait) | Each tap shows a new title, at 30 px under the tile and 72 px on the shelf | The slow frame has an upload and a high CPU time; glyphs made ahead remove it |
| H4 | The measurement: the first swap returns at once and the second waits a whole vsync, with nothing lost on screen | A tap's worst frame is 28–34 ms, about a vsync plus the render | Always the burst's second frame; the slow-motion video shows no stutter |
| H5 | Paging from swap (39.7 MB resident, the B9 already swapping) | Weakly | Page faults jump in the slow frames |

Two held Ups that started from idle stayed at 17.9 and 18.3 ms, against a
pure H1 or H4; that is two samples.

**Stage 1: which frame, and where its time goes.** No change on screen.

* Each burst's line gains its first three frame times.
* Each frame over 25 ms gets a line of its own: its place in the burst, its
  CPU time split into building the batch, uploading (bytes and ms) and
  submitting, the time in the swap, the glyphs rasterised, and the minor and
  major page faults since the last frame (`getrusage`). At most a few such
  lines a burst.
* `tools/frame-stats.sh` summarises them.
* On the TV: a `lumen-tv-check` run, and 10 single taps filmed in slow
  motion to check H4 against the screen. Separately, asked first and
  read-only: whether the B9 shows the Mali clock in `/sys` (devfreq), for H1.

**Stage 2: switches, only if stage 1 leaves it open.** One build reads a
flags file from the app's own folder, so a reinstall or the restore removes
it and nothing outlives the check: `finish` (`glFinish` before the swap, for
the GPU time), `keepalive` (draw while settled; H1, H4), `flat` (B1's flat
shader; H2), `mediump` (shapes in mediump, atlas coordinates in highp; H2)
and `warm` (every title's glyphs made right after the first frame; H3). One
switch a run, the same moves each time.

**Stage 3: the fix for the cause found,** measured against target 1 with
`lumen-tv-check`. Candidates: mediump distances with highp texture
coordinates, separate programs for flat, shape and texture, and the
background drawn without blending (H2); glyphs made ahead off the frame
path, and no upload into an atlas the frame reads (H3); a short warm tail
after a burst (H1); or, for H4, a burst's first frame time left out, said so
here, and judged by the video.

Not in this plan: Phase 4 items, the held-key speed-up and the start-up
time (its 923 ms in `draw::Assets` may share a fix with H3; it is a target of
its own).

**Stage 1 ran on 2026-10-22** (df2a4d0; "Measured on the B9", 14). Read
against the table: on the B9 the wait shows in `submit`, the GL draw call,
not in the swap, so "time in the swap" above reads "time in submit" there.
H4 holds, refined: the slow frame is the third of a burst from idle, not the
second, with frames 2 and 3 together taking two vsyncs (the queue filling),
so most "misses" probably lose nothing on screen but step the springs 9 then
about 31 ms. H3 is confirmed: every slow frame with an upload spent 9–25 ms
in it. H5 is out for the motion. H1 is against: one frame, not a run of
them, and the next frame is back at 16.7 ms. H2 stays open for the few real
mid-burst misses, with nothing uploaded. The slow-motion video of the taps
has not been read yet. Stage 2 waits; stage 3's design (springs stepped by
whole vsyncs, uploads kept out of the frame that reads the atlas, and
whether that also cuts start-up's assets time) is being worked out on the PC
first.

## Input diagnostics

Keys go missing for minutes at a time on the B9 ("Measured on the B9", 13
and 14), and the log could not say why: whether the presses reached the TV,
whether something else was in front, or whether the shell had the focus at
all. A reading of the three 2026-10-22 logs (research note, 2026-10-21):
every focus loss with no key came 112–120 s after the last input the shell
saw, as `FOCUS_LOST` and `LEAVE` in the same millisecond with no hidden or
background event, which is LG's screensaver taking the screen after about
two minutes without "user action". And an OK pressed while the pointer
shows arrives as a click (0x401/0x402, pointer button 1, at upstream SDL's
offsets), not as key 40, which the shell ignored: that is the launch that
"never arrived" in run 14. So the shell gains the lines below (approved
2026-10-22; formats also in look-and-feel.md, "Log lines", which
`tools/frame-stats.sh` summarises in an "input" section).

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

* **Input** is any key, 484 and 485 included, and any pointer event (a
  button, a move or the wheel). The key-up SDL makes for 484 or 485 when the
  window loses focus is not input (LG never sends one), though it still
  counts as one of the "other events" in the focus lines, as before. Since
  LG sends no key-up for 484 and 485, SDL may flag a later one as a repeat;
  the shell reads the state from the code, not the flag.
* **The pointer** is `shown` from a 484 until a 485, `hidden` before the
  first. `sdl.rs` decodes pointer buttons and moves at upstream's offsets,
  which the B9's own events confirm (the four 0x401/0x402 pairs in the
  2026-10-22 logs are the tests' fixtures). Their `which` field at bytes
  12–15 reads 2, the same value LG's key events carry there, so that field
  is an input-device id ("Still open", 3).
* **A click is OK.** Pointer button 1 down goes through Select's own path:
  it is timed as `key 40: shown after N ms`, counts as a key for the focus
  lines, shows the press pulse first and launches after that frame. Its
  release releases the key. Other buttons are logged and ignored. The
  position is logged, not used: nothing is pointed at.
* **Arrows hide the pointer** when LG's SDL exports
  `SDL_webOSCursorVisibility` (webosbrew's fork has it; `dlsym` at start-up
  says whether LG's 2.0.5 does, in the `pointer:` line), as moonlight-tv
  does, so the OK that follows comes as a key. Each arrow while the pointer
  shows asks once; the shell then takes the pointer as hidden, and LG's
  485, if it sends one, says so again.
* **The luna watch** (`watch.rs`) runs on a thread of its own, with an
  anonymous client and a glib context of its own, as the launcher's calls
  do, and holds three subscriptions (`LSCall`, `subscribe: true`) for the
  life of the shell: `tvpower/power/getPowerState` (`Active`, `Screen
  Saver`, `Screen Off`), `applicationManager/getForegroundAppInfo` (which
  names what takes the focus), and `tvpower/power/registerScreenSaverRequest`
  as `<appId>.wakelock`. A change of state or of app logs one line; a
  screensaver request is answered on `responseScreenSaverRequest` with
  `{clientName, ack: true, timestamp}`, so **the screensaver is allowed and
  only logged** (the panel is an OLED). A measurement run that wants it off
  can later answer `false` behind a flag; nothing persists, the
  subscriptions die with the process. A subscription the hub refuses, or a
  message that cannot be read or is over 64 KB, logs one `luna watch` line
  for that subscription and nothing more until it answers again; `luna
  watch thread:` lines say when the thread itself could not start, load the
  library or register (`tools/frame-stats.sh` lists the three
  subscriptions' failures, not these). Other services' strings reach the
  log trimmed, with control characters as spaces and cut to 200 characters.
  The thread dispatches its context blocking, is woken at exit, and is
  given 2 s to unregister before the exit goes on without it. It starts
  before the first frame, after the apps are listed, so the next run's
  `start:` line says whether it costs anything there.
* **The heartbeat** comes 30 s after the last input, then every 30 s until
  10 minutes without input, then every 10 minutes, on the loop's clock (so
  not under `--fixed-dt`, where it only comes if the keys or frames carry
  the clock past it). `focused` follows SDL's focus events. It bounds a
  gap without a clock read: the owner says "I was pressing at that line",
  and the next lines say whether the TV saw the press.
* **What the next run decides.** With the owner's OK the run also records
  the remote's evdev nodes read-only to `/tmp` (`cat /dev/input/event1`,
  `event3`, `event4`; 16-byte records on this kernel) and takes two read-only
  looks (`luna-send getPowerState`; `grep -a` of LG's SDL for its `SDL_webOS`
  exports). Presses in the recording but not in the shell's log mean the
  loss is in LG's input path; none means the remote or its service, which
  fresh batteries or a re-pair would answer. A `front app` or `power` line
  in the same second as a focus loss with no key names the screensaver.

Not in this: pointing at tiles with the pointer, and turning the screensaver
off.

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

The B2/B3 builds ran on the same set on 2026-10-22, opened by hand from LG's
app list with the owner on the remote: commit c500289 (B3 through the
library) with Phase 1's DNS capture running, then 09711d7 (branch
`claude/shell-timing`: the timing lines in look-and-feel.md, and the launch
called after the pulse frame) with nothing else of ours running. Each run's
window runs from the log's own first to last arrow. The log keeps one average
and one worst frame per burst, so the counts are over bursts, not frames.

6. **Moving, most bursts miss a vsync.** c500289: 2,611 frames in 50
   bursts, 16.83 ms on average (59.4 fps); 37 bursts had a frame over 25 ms,
   the worst 50.6 ms. 09711d7: 1,399 frames in 20 bursts, 16.82 ms on
   average; 16 bursts over 25 ms, the worst 35.1 ms. So the capture is not
   the cause. The slow frames come mostly in a single tap's burst (its worst
   28–34 ms) and while Down scrolls (33–35 ms); holding Up stayed at
   17.8–18.3 ms. Frames drew 24–104 quads, against B1's 856–1,631, so the
   cost is in the pixels, not the vertices. Which frame of a burst is the slow
   one, the log doesn't say. (The once-a-second `fps` line keeps its worst
   frame across a hide; the burst lines don't.)
7. **The remote repeats** a held key first after about 505 ms (491–522),
   then every 50 ms (26–68), over 16 holds. The shell's own repeat, 400 ms
   then 120 ms, is what sets the pace.
8. **Key to frame** (09711d7): all 72 presses shown were on screen within
   20 ms of SDL's stamp on the event: 1–7 ms from a settled screen, 16–20 ms
   when the press came mid-burst. One more came as the window hid and was
   not shown. This assumes LG's SDL stamps an event when it arrives.
9. **Start-up** (09711d7): the first frame came 1,395 ms after `main()`:
   SDL 163 ms, the GL context 138, `Renderer::new` 59, `draw::Assets` 923
   (the font and the atlas), apps from luna 63, the frame 49. c500289 took
   1,197 ms.
10. **Launching.** c500289 called `launch` inside the key handler: HDMI 1
    answered in 320 ms and no frame of the press pulse was drawn. 09711d7
    draws the pulse frame first, 1 ms after OK, then calls: HDMI 1 answered
    in 18 ms and Homebrew Channel in 573 ms. Launching HDMI 1 sent `window
    minimized`, two app-background events and a focus loss, all within 1 ms
    and 180 ms after OK; the swap before them took 154 ms. HOME, by
    contrast, sends only a focus loss (2).
11. **Coming back to the front:** picking the running shell from LG's app
    list sent `window restored`, then the did-enter-foreground event, then
    `focus gained` 350 ms later; the first frame came 170 ms after the
    restore. On two other returns it came 13 ms after a focus gain and 2 ms
    after a restore.
12. **Memory:** 39.7 MB resident (c500289, about 10 minutes after its
    start), against B1's 18 MB.
13. **Keys still go missing when opened by hand.** c500289: in two of its
    three processes keys stopped arriving, and the shell lost focus without a
    key several times; at times another build was in front when it looked
    like this one. 09711d7: no focus loss without a key (0 of 3), but in its
    second round, which the owner spent moving, the log has no arrow for
    20 s and then 28 s; whether presses were lost in those gaps is not yet
    confirmed.

Stage 1 of the frame-time plan (df2a4d0, the slow-frame lines) ran on the
same set on 2026-10-22, 04:55–05:23 UTC, opened by hand with nothing else
of ours running (report: `tools/out/archive/tv-check-20261022T044903Z` on
main):

14. **Where a slow frame's time goes.** 35 s of motion: 14 bursts, 1,308
    frames, 16.80 ms on average; 11 bursts had a frame over 25 ms, the worst
    34.82 ms; 17 slow-frame lines, one more not logged. Their time is in
    `submit` (p50 29.16 ms, max 33.76), not the swap (max 1.80 ms): on this
    driver the draw call blocks until a buffer is free. 8 of the 17 are
    frame 3 of a burst from idle, and nearly every burst from idle reads
    `first 9, 30, 17 ms`: frames 2 and 3 take two vsyncs together, the queue
    filling. The 5 with uploads spent 9.02–24.53 ms in `upload` for 2–5
    uploads of 3–10 KB (once a 128 KB icon). 5 more missed mid-burst (25–35
    ms in submit, nothing uploaded; 41.44 ms once after a return to the
    front). No major page fault in any slow frame of the motion; 2 in one
    frame after 47 s without focus, with 5.8 MB of the shell's 35.6 MB
    resident in swap. Key to frame: 73 of 74 within 33 ms, the one at 35 ms
    in a slow frame. Start-up 1,594 ms (sdl 165, gl 167, renderer 112, assets
    983, apps 104, first frame 63). Then no key for 118 s with no focus
    loss, a focus loss 118.3 s after the last key, and an OK that came as a
    pointer click (0x401 at 1792645177.3) while the pointer showed, which
    the build ignored. The read in "Input diagnostics" follows from this.

The key diagnostics (6e28bb9) ran on the same set from 2026-10-22 07:52 UTC,
opened by hand, with the remote's evdev nodes recorded read-only, and stayed
open until 2026-10-23 01:18 UTC (report:
`tools/out/archive/tv-check-20261022T074700Z` on main). The hardware-float
build ran on 2026-10-23 at 01:42 UTC (report:
`tools/out/archive/tv-check-20261023T013938Z`).

15. **No press is lost between the kernel and the shell.** The Magic
    Remote's node (`event3`) recorded 95 presses of an arrow, OK or a click.
    The 75 that came while the shell had the focus all reached it: from the
    kernel's stamp to the shell's `key down` line, 7 ms on average from a
    settled screen and 20 ms mid-burst (max 37 ms), where the press waits
    for the loop's next pass. The other 20 came while LG's launcher, HDMI 1
    or the screensaver was in front. The gaps with no key while the shell
    had the focus have no press in the kernel either. Every focus loss with
    no key (6 of 9) was LG's screensaver: `screensaver request` 109.7–115.6 s
    after the last input, then `power Screen Saver` and the focus loss
    within 0.8 s. The press that wakes the TV from it is LG's; the shell
    never gets it, and has the focus back 0.15–0.23 s later. A click while
    the pointer showed was taken as OK and launched HDMI 1 (answered in
    18 ms); an arrow hid the pointer (LG's 485 29 ms later). LG's SDL exports
    `SDL_webOSCursorVisibility`, `SDL_webOSGetRefreshRate`,
    `SDL_webOSGetPanelResolution`, `SDL_webOSGetFlashInfo` and
    `SDL_webOSGetRamInfo`. The luna watch answered from the jail for 17
    hours. `event1` carries only POWER. On `event3` the pointer showing and
    hiding are codes 1198 and 1199 (beyond Linux's KEY_MAX, so LG's own),
    and 773, held 150–200 ms just before each trip through LG's launcher,
    is probably the Home key; 139 (KEY_MENU) comes with the pointer
    appearing and is held under 1 ms, so it isn't a press of a button.
    Motion as in 14: 17 bursts, 16.72 ms on average, 10 with a
    frame over 25 ms (worst 35.82 ms); 7 of the 14 slow frames were frame 3
    of a burst from idle. The frame after a launch waits while LG switches
    to the input (93 and 51 ms) and counts as a normal frame.
16. **Hardware float** (9456516 with only the build flags above): start-up
    688 and 524 ms, against 1,312–1,594 ms with software float; `assets`
    (fontdue outlining Inter's 2,937 glyphs) 163 and 176 ms, against
    922–983 ms; the heaviest `batch` 2.13 ms against 9.82 ms for the same
    work (5 glyphs and a 128 KB icon). The missed vsyncs are as before: 10
    of 16 bursts had a frame over 25 ms, and 9 of the 11 slow frames were
    frame 3. The owner saw no difference on screen.

Still open:

1. ~~How fast the remote repeats.~~ Answered on 2026-10-22 (7).
2. ~~Which event brings the shell back to the front.~~ Answered on
   2026-10-22 (11): from LG's app list, `window restored` comes first.
3. What the u32 at bytes 12–15 means. Another input (a USB keyboard, LG's
   phone app) may show a different value. The B9's pointer events carry 2
   in their `which` (input-device id) field at the same bytes, so a device
   id is the likely answer ("Input diagnostics").
4. Where the shell gets icons for apps whose folders the jail doesn't mount:
   LG's own apps, store apps and the inputs (B3, 2026-10-21). Only
   developer-mode apps' icons resolve inside the jail. Reaching the others
   needs root, for example a copy made through Homebrew Channel's `exec`,
   so it needs the owner's OK first.
5. ~~Why keys stopped reaching the shell.~~ Answered on 2026-10-23 (15) for
   a shell opened by hand: no press is lost between the kernel and the
   shell. The focus losses with no key are LG's screensaver, the lost OK was
   a click, and the presses missed in the gaps never reached the kernel.
   Opening with `tools/tv.sh launch` (2026-10-21: no key until the pointer
   was woken) was not tried again.
6. Why most bursts miss a vsync (6). Answered in part on 2026-10-22 (14):
   it is frame 3 of a burst from idle, waiting in the draw call, and uploads
   into the atlas; the fix is stage 3 of the frame-time plan. Hardware float
   (16) made the CPU's part faster and left the misses as they were.

## Not in this spec

The Home key filter (`takeover/`), pointing at tiles with the Magic Remote's
pointer (its state and a click as OK are in "Input diagnostics"), glass and
blur, Top Shelf content, the Control Center, parallax, app editing and
reordering (Phase 4), and sound.
