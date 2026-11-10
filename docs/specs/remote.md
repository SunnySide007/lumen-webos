# Remote: the TV from the PC, the phone and Google Home

Proposed on 2026-10-28 as [decision 9](../decisions.md) and approved by the
owner on 2026-10-31 as drafted, with their answers D1 to D13. It runs beside
the phases, like Play. The research, with sources, is in
[research.md, "Remote"](../research.md#remote-control-channels-google-home-and-the-desktop-app).
It leans on [privacy.md](privacy.md) for the TV's listening ports.

## What the owner asked for

Answered on 2026-10-28:

* **Control:**
  * power, volume and inputs
  * a full remote: arrows, OK, Back, Home, playback, and typing from the
    PC's keyboard
  * apps, Lumen's home and screensaver, and the privacy state
  * picture and sound settings
* **From where:** the Windows 11 PC and the owner's Pixel. No other family
  devices.
* **The PC app:** a tray popup and a normal window, with bold web animations:
  liquid glass, morphing controls, ambient colour, depth and tilt.
* **The phone:** a web UI the TV serves, opened in the phone's browser.
* **Google Home:** power, volume and mute, inputs and apps, and media
  controls, by voice and in the app.
* **Away from home:** everything, through a VPN into the house. No port is
  ever opened to the TV.
* **Google devices:** a Nest Wifi Pro, and a Google TV Streamer (4K) on one
  of the B9's HDMI inputs. No Nest speaker or display.
* **Ports 3000 and 3001** (privacy P10): nobody at home uses LG ThinQ, a phone
  remote app or Web Video Caster.
* **Where a bridge would run:** not decided; see D9.

## Where it starts

Measured on the B9 (OLED55B9PUA, webOS 4.10.2, firmware 05.50.00) by the
privacy surveys of 2026-10-28:

* **LG's control door is open, and its record is poor.** tcp 3000 and 3001
  are LG's second-screen gateway (SSAP).
  * **2024:** its PIN could be skipped on this generation (CVE-2023-6317,
    confirmed on webOS 4.9.7), and root could then be reached.
  * **This month:** a chain to root through 3001, with no PIN, was published
    for webOS 7.5.
  * **The B9's status:** whether firmware 05.50.00 has LG's 2024 fix is
    unknown. Updates stay blocked, so the B9 won't get any later fix.
* **Waking it.** `wolwowlOnOff` is on, so a Wake-on-LAN packet may turn the
  TV on over Wi-Fi; untested. The Streamer can turn it on over HDMI-CEC.
* **What already works:** SSH on port 22, by key only, with luna as root
  behind it. The shell has made luna calls from its jail on the B9 since
  2026-10-23: launch points, launching, and a foreground watch.
* **When the TV is off,** its services stop, including any server Lumen
  adds. Only Wake-on-LAN and CEC reach a TV that's off.

## How it fits together

```
PC: Lumen Remote (Tauri) ─ SSH, forced command ─▶ TV: lumen-ctl (root) ─ luna ─▶ LG's services
   └─ Wake-on-LAN ─▶ the TV's Wi-Fi                      ▲
Pixel: browser ─ HTTPS, paired ─▶ TV: lumen-remote (unprivileged) ─┘
Google Home ─ Google's cloud ─▶ TV Streamer ─ HDMI-CEC ─▶ TV: on, off, volume
```

* **`lumen-ctl`** is the only part with root. It's a Rust binary in the shell
  workspace, built with the same armv7 toolchain.
  * **Its language:** requests, replies and events, one JSON object per
    line.
  * **What it accepts:** only the verbs below. It runs no shell, takes no
    paths, and checks every argument's type and range.
  * **What it pushes:** LG's state (power, volume, the app in front, the
    inputs), through luna subscriptions, so every client sees a change
    whoever made it.
* **The PC's door is SSH.** The app's own key sits in `authorized_keys` with
  `command="…/lumen-ctl serve"` and `restrict`, or the separate `no-…`
  options if the TV's dropbear predates `restrict`. The key runs `lumen-ctl`
  and nothing else: no shell, no terminal, no forwarding. One connection stays
  open while the popup is in use, and no port is added.
* **The phone's door is `lumen-remote`,** a small HTTPS and WebSocket server
  on one new port.
  * It drops root before it reads anything from the network.
  * It serves the web UI's files and checks each device's token.
  * It passes requests to `lumen-ctl` over a Unix socket.
  * It's off by default and answers only paired devices.
* **One web front-end** (TypeScript, bundled, no CDN) serves both the PC app
  and the phone, so they look and move the same.
* **Power on:** the PC app sends Wake-on-LAN. The phone uses Google Home
  (CEC through the Streamer), because the TV's own server is off whenever the
  TV is.
* **Google Home** goes through the Streamer over CEC (R1). Home Assistant
  comes in only if that isn't enough (R5).

### Verbs

| Group | Verbs | Through |
|---|---|---|
| Power | `power.off`, `screen.off`, `screen.on`. Power on is Wake-on-LAN or CEC, from outside the TV | LG's `tvpower` service |
| Sound | `volume.up`, `volume.down`, `volume.set` (0–100), `mute.set` | LG's audio service |
| Inputs and apps | `apps.list`, `launch <id>` for apps and HDMI inputs | `applicationManager`, as the shell does |
| Remote | `key <name>` from a fixed list (arrows, OK, Back, Home, play, pause …); `text.insert`, `text.enter`, `text.delete` | Keys: the route R0 finds. Text: LG's input method service |
| Lumen | `lumen.open`, `ambient.start`, `privacy.state` (the boot hook's log, the 11111 rule, the stopped jobs, and Guard's counts, mode and upstream route, with when a fallback in clear began, once Guard exists) | `applicationManager`, Lumen's own files |
| Picture and sound | `settings.get`, and `settings.set` for a fixed list of keys and values (picture mode, OLED light, sound mode …) | LG's `settingsservice`, as the Control Center plans |
| On the TV | `toast <text>` (pairing codes, "Volume 20") | LG's notification service |

No method name is written down as fact until R0 has read it on the TV.

## Rules for every milestone

* **One pull request each,** on its own branch, ending with a check on the B9
  that's recorded with model and firmware (CLAUDE.md).
* **The TV:** [safety.md](../safety.md) applies. Ask before any `tools/tv.sh`
  command. Anything persistent is run by hand first, the owner confirms it,
  and `tools/backup.sh` runs before it.
* **Privacy:** no telemetry, analytics or crash uploads, and no updater
  calling home. Fonts and images are bundled (Inter), never fetched. The PC
  app talks only to the TV. A listener on the LAN is off by default and needs
  pairing.
* **Security:** anything on the TV that reads network input gets a security
  review, on top of `/ecc:rust-review` and `/ecc:astra-review`, before it
  runs on the TV.
* **Workflow:** milestones run with `/ecc:orch-add-feature` and stop for TV
  steps and failures.
* **Numbers:** a speed claim names the hardware it was measured on: the B9
  for the TV side, the owner's PC for the app.

## Milestones

| | Milestone | Needs | Can run beside |
|---|---|---|---|
| R0 | A read-only look at the B9 | — | R1 |
| R1 | Google Home through the Streamer (the owner, no code) | — | R0, R2 |
| R2 | `lumen-ctl` on the TV, driven from the PC over SSH | R0 | R1 |
| R3 | Lumen Remote for Windows | R2 | — |
| R4 | The phone: the TV serves the web UI, paired | R2, R3's front-end | — |
| R5 | Google Home beyond CEC (only if R1 falls short) | R1, a bridge box (D9) | R6 |
| R6 | Away from home through a VPN | a bridge box (D9) | R5 |

R2 and R4 need the strongest model: R2 is root code that reads requests, and
R4 is a server on the LAN. R3 is the main design work. The rest run on the
default model.

### R0: a read-only look at the B9

**Brief.** With the owner's OK for each command, read without changing
anything:

1. **SSH:** dropbear's version, and from it whether `command=`, `restrict`,
   `no-pty` and `no-port-forwarding` are honoured.
2. **Methods:** the luna method behind each verb, with read-only calls only
   (volume, power state, the app in front, the picture and sound settings).
3. **Keys:** whether `/dev/uinput` exists and the kernel has
   `CONFIG_INPUT_UINPUT`, and which of LG's input services answer. No key is
   sent.
4. **Firmware date:** the build date of 05.50.00, to place it before or after
   LG's fix of 2024-03-22.
5. **Wake-on-LAN,** with the owner at the TV: the PC sends a magic packet
   after the TV has been off for 1 minute, 30 minutes and a night. Did it
   wake each time?

**Done when:** a "Remote" section in b9.md records dropbear's version, each
verb's method (or "none"), the key route, the firmware date and the
Wake-on-LAN results.

**Undo:** nothing changes.

**Run on 2026-10-31** (`tools/recon.sh --remote`, report `docs/device/remote-20261031-060938.txt`): the answers are in [b9.md, "Remote"](../device/b9.md#remote). Dropbear 2022.83 honours `restrict` and `command=`; every verb has a luna method, and LG's `com.webos.service.tv.keymanager/createKeyEvent` is a key route to try before `uinput`; the firmware is from August 2025, after LG's 2024 SSAP fix. Wake-on-LAN (item 5) is still the owner's test.

### R1: Google Home through the Streamer

**Brief.** No code. The owner, in LG's settings, the Streamer's settings and
the Google Home app:

1. Turn SimpLink (HDMI-CEC) on, if it's off.
2. Set the Streamer's volume control to the TV over CEC.
3. From the Pixel's voice and from the Google Home app, try: on, off, volume
   up, down and to a number, and mute. "Open Netflix" opens it on the
   Streamer, not in LG's app, and pause and resume act on the Streamer's
   playback.
4. Watch whether the Streamer turns the TV back on after it's been turned
   off.

**Done when:** b9.md has a table of what worked, by voice and in the app, and
D8 is decided from it.

**Undo:** SimpLink off, and the Streamer's volume setting back.

**Risks:**
* Google describes TV power through CEC for its speakers and displays; the
  Pixel may not offer it.
* CEC volume works unevenly across owners' reports.
* LG's apps, and HDMI inputs other than the Streamer's, are out of reach this
  way.

### R2: `lumen-ctl`, driven from the PC

**Brief.**
* **The binary:** a new one in the shell workspace. `lumen-ctl serve` speaks
  the JSON lines above and opens with a `hello` that carries a protocol
  version. Its tests use a fake luna, like the shell's fixtures.
* **`tools/remote.sh install | status | remove`:**
  * pushes the binary to `/var/lib/lumen/remote/` and compares its SHA-256,
    like `tools/privacy-hook.sh`;
  * adds the PC's key line with the forced command, after a backup of
    `authorized_keys`;
  * `remove` takes both away.
* **Nothing starts at boot:** `lumen-ctl` runs only while an SSH session runs
  it.

**Done when:** `make check` is green, and on the B9:
* a script on the PC drives every verb over SSH;
* an event arrives when the volume changes on the Magic Remote;
* an unknown verb and an out-of-range value are refused;
* with the forced key, asking for `sh` gets `lumen-ctl`, and a port forward
  is refused.

Each verb's time from the PC to the TV acting is recorded.

**Undo:** `tools/remote.sh remove`.

**Risks:**
* It's root code that reads requests (risk 4).
* Keys may have no clean route (risk 1).

**Built on 2026-10-31** (`shell/crates/lumen-ctl`, `tools/remote.sh`,
`profiles/remote/`, `tools/remote/ctl.py`); run on the TV the same day
(below).
* **The binary:** `lumen-ctl serve [--bus library|command] [--keys
  none|keymanager] [--log FILE]`, plus `probe` and `probe-keys` for the first
  TV step. The forced command passes no flags, so
  `/var/lib/lumen/remote/ctl.conf` decides (`tools/remote.sh config`), else
  `command` and `none`.
* **Two buses:**
  * `command`, the default, runs `/usr/bin/luna-send -n 1 -f`. It polls
    power, volume, the app in front and the inputs every 2 s, and the
    picture and sound settings every 10 s.
  * `library` loads libluna-service2 and subscribes wherever LG's first
    reply says `"subscribed": true`, polling the rest.
  * The hello's `polling` lists what is polled. `probe` reports which bus
    the B9 allows, since LG's access control may refuse an unknown root
    client on the library route.
* **Keys:** off until `probe-keys` shows what LG's `createKeyEvent` wants.
  * `keymanager` builds each call from a template in
    `/var/lib/lumen/remote/keys.json` (`tools/remote.sh keys`), and `uinput`
    can come in behind the same seam.
  * The hello leaves `key` out while there is no route, and leaves out
    `ambient.start`, which answers `unavailable` for now.
* **On the TV:**
  * One line at the end of `/home/root/.ssh/authorized_keys`:
    `restrict,command="/var/lib/lumen/remote/lumen-ctl serve" ssh-ed25519 …
    lumen-remote`.
  * In `/var/lib/lumen/remote/` (mode 0700): `lumen-ctl`, `ctl.conf`,
    `keys.json`, `authorized_keys.before` (the file as it was before the
    first install), and `lumen-ctl.log`. The log holds each request and
    reply, never the text of `text.insert` or `toast`, and is moved to `.1`
    at 1 MiB.
* **On the PC:** the key in `~/.lumen/remote/` (`tools/remote.sh keygen`).
  The copy of `authorized_keys` and the probes' output go in
  `tools/out/remote/`.
* **Undo:** `tools/remote.sh remove` takes that line out and leaves every
  other. It refuses if no key would be left, then deletes the folder.
  `tools/uninstall-all.sh` runs it.

**On the B9, 2026-10-31** (OLED55B9PUA, webOS 4.10.2-31, firmware
05.50.00; b9.md, "Remote", item 5; raw output in `tools/out/remote/`):
* **Installed** at 18:01 UTC after a backup
  (`Lumen-backups/20261031T175935Z`): `lumen-ctl` `ae8cc9e5…`, and the
  key's line second in a two-line `authorized_keys`.
* **The door:** all four of `ctl.py check-door` pass. `sh` gets `lumen-ctl`,
  the client's `LD_PRELOAD` stays out, and a local forward is refused. A
  remote forward is never granted: dropbear doesn't answer it, and no port
  listened on the TV meanwhile. `check-door` reads `ssh -v` since then
  (5f0851d): a granted forward fails it, silence or a refusal passes it.
* **The bus:** `command`. The library route is refused to root over SSH
  ("Invalid permissions", -1027), so it is not offered.
* **The verbs**, each with its round trip from the PC (SSH both ways):

  | Verb | Round trip | On the TV |
  |---|---|---|
  | `apps.list` | 78 ms | 32 apps |
  | `privacy.state` | 94 ms | Guard's counts, S1's hook log and jobs, port 11111 |
  | `settings.get` | 63 ms | the four settings |
  | `volume.set` (3, as it was) | 63 ms | no change, so no event |
  | `volume.up`, `volume.down` | 31, 78 ms | 4, then 3, each as an event |
  | `mute.set` true, false | 32, 47 ms | each as an event |
  | `toast` | 16 ms | shown over Netflix (the owner) |
  | `screen.off`, `screen.on` | 94, 93 ms | LG's `Screen Off`, then `Active`; seen by the owner |
  | `settings.set` OLED light 80, 100 | 63, 47 ms | dimmed and back (the owner); 80 read back by the next session |
  | `lumen.open`, `launch` Netflix | 62, 110 ms | Lumen in front, then Netflix where it was (the owner) |

  Every change came back as an event right after its reply.
* **Left for "done when":** `text.insert`, `text.delete` and `text.enter`
  (with a text field open), `power.off` (and the `reason` LG wants), a
  volume change on the Magic Remote arriving as an event, and an unknown
  verb and an out-of-range value refused on the TV. `probe` cuts LG's
  replies at 300 characters, so OLED light's range is still unread.

### R3: Lumen Remote for Windows

**Brief.**
* **App:** Tauri 2 (D5), with the shared web front-end.
* **Tray:** a left click opens the popup above the tray icon, and clicking
  away hides it. A right click opens a menu: Open window, Pair a phone, Quit.
  The same view opens as a normal window that can stay open.
* **Look (D6):**
  * Liquid glass: Acrylic behind a transparent window, with blur and
    refraction of the panel's own layers in CSS and SVG, and a rim light
    that follows the pointer.
  * Morphing controls: View Transitions and springs.
  * Ambient colour: taken from the app or input in front.
  * Depth and tilt: tiles tilt toward the pointer.

  Reduced motion is honoured. When nothing changes, nothing moves
  (look-and-feel.md, principle 4).
* **Reaching the TV:** SSH to `lumen-ctl`, with the app's own ed25519 key in
  the user's profile, readable only by the user. Windows' own OpenSSH client
  or a Rust SSH crate, picked in the build.
* **Power on:** Wake-on-LAN to the TV's MAC.
* **WebView2:** SmartScreen off, and crash dumps kept on the PC. A content
  security policy loads only the app's own files.
* **Install:** for the user only, no admin rights. Uninstalling removes
  everything. No updater: new versions come from GitHub releases by hand
  (D13).

**Done when:**
* Each control works on the B9 from the owner's PC.
* *Proposed:* a warm popup opens in ≤ 150 ms, measured by the app's own
  timing line.
* Hidden, the app uses no measurable CPU.
* A Windows packet capture (`pktmon`) during a session shows the app talking
  only to the TV and the Wake-on-LAN broadcast. It also records anything
  WebView2 sends on its own.
* The build and checks run in `make check`. How the Windows build fits the
  container is settled in the build.

**Undo:** uninstall the app, and `tools/remote.sh remove` takes its key off
the TV.

**Built on 2026-11-08** (`remote/app`, `shell/crates/lumen-remote-link`,
`remote/ui/src/host.ts`; remote/README.md, "The PC app"), against
`lumen-ctl serve --bus fake` only. Not yet run against the B9; its check list
went to the builder session the same day. `lumen-ctl` is unchanged.
* **App:** Tauri 2.12.3. The tray, the popup (420 × 720, above the icon,
  hidden when it loses the focus) and the window are as the brief says.
  Closing the window leaves the app in the tray. Pair a phone opens the
  Pairing screen, where pairing waits for R4. A first run with no TV set
  opens the window on "This device" (address, SSH port, MAC).
* **Reaching the TV:** Windows' own OpenSSH (`System32\OpenSSH\ssh.exe`,
  9.5p2 on the owner's PC), picked over a Rust SSH crate.
  * **Why:** the app holds no SSH or crypto code, and it runs the options
    `ctl.py` proved on the B9: `-F none`, only the app's key, `BatchMode`,
    no password, no terminal, `ClearAllForwardings`, `accept-new`.
  * **The session:** one per shown window, and a hidden window lets its
    session go after 60 s. Lines pass through both ways (64 KiB at most),
    and the page's `RemoteClient` checks the hello and every reply, as over
    the mock's WebSocket.
  * **Errors:** ssh's complaints reach the status chip in plain words, such
    as a refused key, a changed host key, no answer, or a key other accounts
    can read.
* **Settings:** `~/.lumen/remote/tv.conf` (host, port, MAC, optionally
  another ssh), beside the key and `app.log`. The app makes no key.
* **Power on:** Wake-on-LAN, sent twice to 255.255.255.255:9 and to the TV's
  /24 broadcast. "Turn on" shows on the Remote screen while the link is down
  or the TV is in standby.
* **Keys:** the pad, Back, Home, Menu and playback stay hidden until a hello
  offers `key`, and stay hidden through reconnects. A notice says why.
* **Look:** Acrylic behind both windows, with the page's backdrop
  translucent over it, and the shared front-end's motion. The popup uses the
  phone layout.
* **WebView2:**
  * **Content security policy** (`tauri.conf.json`): the app's own files and
    Tauri's IPC, no WebSockets.
  * **The one capability** grants event listening and the app's eight
    commands, nothing else of Tauri's.
  * **SmartScreen** is off: `msSmartScreenProtection` is in the
    `--disable-features` list, pinned in the config.
  * **Crash dumps:** not done. Keeping them on the PC is WebView2's
    `IsCustomCrashReportingEnabled`, an environment option Tauri 2.12 doesn't
    expose.
* **Install:** NSIS, per user, `webviewInstallMode: skip` (nothing
  downloaded), no updater. The installer itself isn't built yet: the first
  bundling run downloads Tauri's NSIS tools, so the builds so far are
  `--no-bundle` (`lumen-remote.exe`, 3.3 MB).
* **Build and checks:** the Windows build runs on the host, since the
  container has no WebView2 or Windows target.
  * `make check` builds, lints (PC and TV targets) and tests
    `lumen-remote-link`, `tests/fake_bus.rs` included: the app's session
    through a fake ssh to `lumen-ctl --bus fake`.
  * `remote/ui` adds `test/host.test.ts`, over lines captured from
    `lumen-ctl --bus fake`.
  * `npm run e2e:pc` drives the built app over WebView2's debugging port.
* **On the owner's PC** (Windows 11 Pro 10.0.26300, 3440 × 1440), against
  the fake bus through a stand-in ssh in the build container:
  * **Popup:** painted 20 and 21 ms after a tray click, by `app.log`'s
    timing line (a debug build). That is short of "done when", which wants
    a warm popup in a release build against the B9.
  * **Hidden:** after the link had gone, 31 ms of CPU in 30 s across the app
    and its seven WebView2 processes (0.10 % of one core).
  * **End to end:** `e2e:pc` passed against the debug build, and 4 of 5
    runs against the release build. The failure's output was overwritten
    before it was read.
* **Open:**
  * **Licences:** `tools/licenses.py` over the app's `Cargo.lock` finds
    MPL-2.0 in Tauri's tree. It comes from cssparser, selectors and
    dtoa-short (tauri-utils' build-time HTML parsing) and from option-ext
    (`dirs`), and three crates spell their licences the old `A/B` way.
    MPL-2.0 can be met under GPL-3.0-or-later (MPL 2.0 §3.3), but it isn't
    on the list, so that is the owner's decision. Until it is made, the
    app's tree isn't in `make check`.
  * **The key's permissions:** Windows' ssh refuses the key in
    `~/.lumen/remote/` as it is. It inherits read access for a sandbox-users
    group, and `ssh-keygen -y` says "UNPROTECTED PRIVATE KEY FILE". The app
    shows the `icacls` line that fixes it.
  * **Not done yet:** the `pktmon` capture, a single-instance guard, and
    D12.

### R4: the phone: the TV serves the web UI

**Brief.**
* **`lumen-remote`** serves HTTPS and WebSocket on one port on the LAN. It
  opens its port and its socket to `lumen-ctl`, then drops to an unprivileged
  user before it reads any request.
* **HTTPS:** D4.
* **Pairing:**
  * The owner opens pairing from the PC app, and the TV shows a six-digit
    code for two minutes.
  * The phone types it in and gets a long random token, which the TV stores
    hashed.
  * The PC app lists paired devices and revokes them.
  * Five wrong codes close pairing.
  * **Roles (decision 12, 2026-11-05):** a paired device starts
    **remote-only**: power, volume, inputs, apps, keys and picture presets.
    **Owner** rights (privacy, learning windows, Lumen's settings, editing
    the grid) are granted only from the PC app, over its SSH key, and the
    PC can take them back. `lumen-remote` checks the role on every request,
    and `lumen-ctl` checks it again for owner verbs.
* **The rest of the web:** an unpaired browser gets only the pairing page.
  Requests from other origins are refused, and the cookie is `Secure`,
  `HttpOnly` and `SameSite=Strict`.
* **The port:** an `INPUT` rule accepts it only from the home network's
  addresses. It is never forwarded on the router.
* **Starting it:** by hand first; a boot hook comes only after the owner's
  tests (safety rules 3–5), and checks the kill switch.
* **The privacy plan:** privacy.md's home-network table gets the new port.

**Done when:**
* The Pixel pairs and controls the B9, and adds the page to its home screen.
* An unpaired browser gets nothing but the pairing page, and a revoked token
  stops working at once.
* After the TV has been off and on again, the server answers.
* The security review leaves no CRITICAL or HIGH finding open.

**Undo:** stop it, and `tools/remote.sh remove` takes away the hook, the
files and the tokens. Remove Lumen's certificate authority from the phone.

**Built on 2026-11-08** (`shell/crates/lumen-remote`, `tools/remote/ca.sh`,
`tools/remote.sh web`, `profiles/remote/web-*.sh`, the Pairing screen in
`remote/ui`; remote/README.md, "The phone"), against `lumen-ctl serve --bus
fake` only. Not yet run on the B9; its by-hand steps and the phone's checks
went to the builder session the same day. There is no boot hook.
* **The server:** one thread around `poll`, as root only until it drops.
  * **Before any request:** it reads the certificate, the key and every
    file of `remote/ui/dist` into memory (each must be root's and no wider
    than 0755, the key 0600), binds the port, and starts `lumen-ctl serve
    --link web` as its child over a socket pair, with a cleared environment.
  * **The drop:** `chroot` into an empty folder only root can write
    (`/var/lib/lumen/remote/empty`), `setgroups(0)`, then uid and gid 61101,
    which no file on the TV has. Then `PR_SET_NO_NEW_PRIVS` and
    `PR_SET_DUMPABLE` 0, and a check that `setuid(0)` now fails. Without
    root it refuses to run unless told `--no-drop` (tests only), and as root
    it refuses `--no-drop`.
  * **Crates:** rustls 0.23.46 on ring 0.17.14, TLS 1.3 only, ALPN
    `http/1.1`, plus libc and serde_json. HTTP/1.1 (8 KiB heads, 2 KiB
    bodies), the WebSocket framing and base64 are hand-written. The TV's
    release binary is about 1.3 MB.
* **Certificates (D4):** `tools/remote/ca.sh` on the PC, with OpenSSL.
  * P-256 keys. The authority carries critical name constraints with
    exactly the TV's address and DNS name, if one is given; with an address
    alone, DNS names are bound to `lumen.invalid`. Constraints bind only the
    kinds of name they list, so without that line its key could sign any
    DNS name. The tests sign rogue certificates with the authority's own key
    and OpenSSL refuses them.
  * The TV's certificate lasts 397 days (`ca.sh issue` renews it, nothing
    new on the phone). The authority's key never leaves the PC.
* **Pairing and roles, in `lumen-ctl`:** the brief's design, with
  `lumen-ctl` holding the devices.
  * **The store** (`/var/lib/lumen/remote/devices.json`, root's): each
    device's id, name, role, when it paired, and the SHA-256 of its token
    (256 random bits from `/dev/urandom`), never the token. At most 16
    devices. The pairing window is in `pairing.json`: its code, its end
    (120 s) and its tries left (5). Changes take a lock and land by rename.
  * **Pairing:** `pair.open` toasts the code on the TV and returns it to the
    PC, which shows it with its countdown. The fifth wrong code closes the
    window; a right code closes it too.
  * **Roles:** a remote-only device gets power, volume, inputs, apps, keys,
    typing and `settings.set` on `picture.mode` and `sound.mode`.
    Everything else (`privacy.state`, `toast`, every other setting, and
    later the owner's verbs) is the owner's. `lumen-remote` checks each request
    against the role's verbs from `lumen-ctl`'s hello, and `lumen-ctl`
    checks the token and the role again on every request.
  * **Revocation:** `lumen-ctl` pushes a `devices` event when the store
    changes. `lumen-remote` then asks again for every open WebSocket's
    device: a revoked one is closed with 4401, and the page goes back to
    the pairing page; a changed role is closed with 4001, and the page
    reconnects with its new verbs.
* **What changed in `lumen-ctl`, for the roles:** `--link ssh|web`
  (`ssh` is the default, the PC's forced command) and `--state DIR`.
  * **The SSH link** (the PC app) gets five new verbs: `pair.open`,
    `pair.close`, `devices.list`, `devices.revoke` and `devices.role`.
  * **The web link** (lumen-remote only) takes `pair.complete` and
    `device.auth`, plus every phone request with its token; it refuses the
    PC's five.
  * **Also new:** the `forbidden` error code and the `devices` event.
    `src/devices.rs` holds the store, and `src/sha256.rs` is a hand-written
    SHA-256 with NIST's test vectors.
* **The web rules** (`src/server.rs` has the table):
  * **Unpaired:** an unpaired browser gets the pairing page (no script; its
    CSP allows its one style, by hash) and nothing else: the WebSocket and
    `/whoami` answer 401, and any other path goes back to `/`.
  * **Host and origin:** every request must name the TV as its Host (421
    otherwise), and the form's POST and the WebSocket must come from
    `https://` that host (403 otherwise).
  * **The cookie:** `__Host-lumen`, `Secure; HttpOnly; SameSite=Strict`,
    400 days. A revoked phone's is cleared when it is next served the
    pairing page.
  * **The network:** peers outside `--allow` are closed unread, behind the
    `INPUT` rule.
  * **Referrer-Policy is `same-origin`,** not `no-referrer`: under
    `no-referrer` Chromium sends `Origin: null` with the form's POST, which
    the origin check refused. The phone tests found it.
* **On the TV:** `tools/remote.sh web install --allow NET/BITS` copies
  `lumen-ctl`, `lumen-remote`, the page and the TV's certificate and key,
  each checked by SHA-256 through the stage, and writes `web.conf`.
  * **`web start`** (`profiles/remote/web-start.sh`) checks the kill
    switch, that uid and gid 61101 are nobody's and that nothing listens on
    8443. Then it adds `INPUT -p tcp --dport 8443 ! -s NET -j REJECT`,
    unless that very rule is there (another rule on 8443, such as an
    ACCEPT, doesn't count), and starts `lumen-remote` with `setsid`. If it
    doesn't come up, the rule comes out again.
  * **`web stop`** stops it and takes out every REJECT on 8443, leaving any
    other rule on the port. `remove` does
    both before it deletes the folder, paired devices included. Nothing
    lasts past a reboot.
* **The front-end:**
  * **On the PC app,** the Pairing screen opens a window: six code cells,
    the countdown, tries left and Close pairing. It lists the paired devices
    with Make owner / Make remote only and a two-tap Revoke.
  * **On the phone,** the screen shows its name and role. Owner-only
    settings are marked and disabled for a remote-only phone, and the Lumen
    screen says privacy is the owner's.
  * **The mock** (`remote/mock-ctl`) pairs a phone from its console
    (`phone NAME`).
* **Tests:**
  * **lumen-remote:** 23 unit tests, and 7 end-to-end tests that run it as
    root in the container and check the drop through `/proc`.
  * **lumen-ctl:** the store, roles and links.
  * **bats:** `remote-web.bats` and `remote-ca.bats`.
  * **`npm run e2e:phone`** (Playwright, 5 tests) runs `lumen-remote` and
    the fake `lumen-ctl` in the build container (`e2e/phone-tv.sh`), with
    the PC's side as a second `lumen-ctl --link ssh` over `docker exec`. It
    covers pairing, the cookie's flags, five wrong codes, a remote-only
    phone refused the owner's verbs until the PC grants them, and
    revocation.
* **Not done yet:** the boot hook (after the owner's tests), "done when" on
  the B9 and the Pixel, and the security review of the running server on
  the TV. The PC app's binary is also called `lumen-remote` (its Cargo
  package, in its own workspace); the two never meet on one machine.

### R5: Google Home beyond CEC (only if R1 falls short)

**Brief.** Home Assistant on a bridge box (D9), with:
* **The link to the TV:** a small Home Assistant integration, or
  `rest_command` with Home Assistant's universal media player, talking to
  `lumen-remote` with its own paired token. Google gets a TV with on and off
  (by Wake-on-LAN), volume, inputs and LG's apps as sources, and playback.
* **The internet side:** Nabu Casa or a self-hosted project (D10).

Two routes are left out:
* the Matter bridge, since Google shows a TV only as a Speaker, which R1
  already gives;
* Home Assistant's webOS TV integration, which needs ports 3000 and 3001.

**Done when:** each voice command in the owner's list works from the Pixel,
and the bridge's token can be revoked.

**Undo:** unlink it in Google Home, revoke the token, and remove Home
Assistant.

### R6: away from home through a VPN

**Brief.**
* **The endpoint:** on the bridge box (D9, D11). Either WireGuard with one
  UDP port forwarded on the Nest Wifi Pro, or Tailscale with none.
* **Using it:** the PC app and the phone reach the TV through it exactly as
  they do at home.
* **What stays closed:** nothing on the TV is forwarded. The VPN reaches
  only `lumen-remote`'s port and SSH.

**Done when:** from the phone's mobile data, the web UI and the PC app work,
and a scan of the home's public address finds only the VPN's port, or none.

**Undo:** remove the forward and stop the VPN.

## Risks

1. **Remote keys may have no clean route on webOS 4,** since LG gives apps no
   way to send them. If R0 finds neither `uinput` nor a usable LG input
   service, the full remote keeps apps, inputs, text and playback, and loses
   the arrows.
2. **Wake-on-LAN over Wi-Fi may fail after a deep standby;** Home Assistant's
   docs expect Ethernet. The fallbacks are CEC through the Streamer and an
   Ethernet cable.
3. **A new listener on a TV that can't be patched (R4).** It's off by
   default, paired, unprivileged, written in Rust with a fixed verb list,
   reachable only from the LAN, and reviewed. With 3000 and 3001 closed (D2),
   the home network sees fewer of the TV's doors than today. If a bridge box
   arrives, the web UI can move there (D3).
4. **A root process that reads requests (R2).** It has fixed verbs, typed and
   range-checked arguments, no shell and no paths, tests against a fake luna,
   and a review before it runs on the TV.
5. **The PC's key.** A stolen key runs `lumen-ctl`'s verbs (volume, apps,
   picture settings), not a shell, and `tools/remote.sh` revokes it.
6. **Google hears the voice** on every Google Home route, and in R5 Nabu
   Casa relays the commands too.
7. **The Streamer's CEC quirks:** it may turn the TV back on after it's been
   turned off, and its volume control varies. R1 tries both.
8. **WebView2 sends Windows' required diagnostic data** whatever the app
   does. Lumen Remote sends nothing itself, and R3's capture records what the
   Windows component sends.
9. **Picture and sound writes (D7)** can leave the picture wrong. They come
   from a fixed list, the app reads and shows the old value first, and it
   offers an undo.
10. **Bold animations cost battery on the phone.** Motion stops when the page
    settles, and reduced motion is honoured.
11. **Beside Lumen's shell and Glasshouse:** a launch from the phone goes
    through the same `applicationManager` as the shell. `ambient.start` waits
    for Phase 2's one-owner rule for the screensaver.
12. **Dependencies** are a supply-chain risk. The front-end stays small, and
    every version is locked and audited.

## Decisions for the owner

| | Question | Recommendation |
|---|---|---|
| D1 | How the PC reaches the TV: luna over SSH with a forced-command key, SSAP (LG's API on 3000/3001), or the phone's web API | SSH with a forced command: no new port, a door that's already key-only, and every verb. SSAP is ruled out by its security record |
| D2 | Ports 3000 and 3001 (privacy P10). Nobody uses them, and Remote doesn't need them | Reject them from the LAN in the privacy boot hook, checked with the checklist: DIAL casting must still work. The privacy track applies it |
| D3 | Where the phone's web UI is served: the TV (your first answer) or a bridge box | The TV, as asked, with R4's safeguards. If a box arrives for R5 or R6, move it there: the box can be patched, the TV can't, and the box can wake the TV |
| D4 | HTTPS for the phone: Lumen's own certificate authority, limited to the TV's name and installed on the Pixel; a self-signed certificate accepted in the browser; or plain HTTP | Lumen's own authority, limited to the TV: no warnings, a page the phone can add to its home screen, and a stolen authority key can't vouch for any other site |
| D5 | The desktop stack: Tauri 2, Electron or WinUI 3 | Tauri 2: Rust like the shell, small, with tray and glass built in. Its cost is WebView2's Windows data (risk 8) |
| D6 | The look: bold motion (all four directions) on Lumen's colours and Inter from one token file, or a palette of its own | Lumen's tokens, generated from the shell's `style` values, with Remote's bolder motion on top |
| D7 | Picture and sound settings from Remote | Yes: from a fixed list, through LG's settings service, only when you ask (never automatically), with an undo |
| D8 | Google Home: the Streamer over CEC first, Home Assistant only if needed | R1 first; decide R5 from its table |
| D9 | Where R5 and R6 run | Nowhere until R1's results are in. If R5 or R6 goes ahead, a small always-on box (a Home Assistant Green or a Raspberry Pi): not the PC, which sleeps, and not the TV, which is weak, off in standby and can't be patched |
| D10 | R5's internet side: Nabu Casa (a subscription, and its relay sees the commands) or self-hosted (free, but Home Assistant is reachable from the internet and a Google project needs keeping alive) | Nabu Casa, if R5 happens: no open port at home |
| D11 | R6's VPN: WireGuard (one UDP port forwarded, needs a public address) or Tailscale (no port, but its coordination server knows the devices) | WireGuard if the home line has a public IPv4 address; Tailscale otherwise |
| D12 | A global hotkey, and a banner on the TV for changes made from the PC | Not now (not picked on 2026-10-28); easy to add to R3 later |
| D13 | Updates of the PC app | By hand from GitHub releases; no updater calling home |

**Answered on 2026-10-28:** the owner took every recommendation, D1–D13.
D2 is applied by the privacy track. D8, D9, D10 and D11 are settled again by
what R1 finds and by the open questions below.

## Changes of 2026-11-05

From the design session's decisions 10 to 14:

* **Roles** for paired devices (R4, above): remote-only by default, owner
  rights granted from the PC.
* **Privacy is changed only from owner devices.** The TV's Privacy panel is
  read-only. Remote gains owner-only verbs for Guard: allow or drop a name,
  start a learning window, show the denied list.
* **The grid is edited from Remote,** not on the TV: which apps sit in the
  first row, their order, and what goes into "Other apps".
* **Software comes from the PC:** Remote's PC app fetches and hash-checks
  IPKs before they are pushed; the TV fetches none itself.
* **D2 stands, and is extended** by privacy.md's port rules: DLNA, AirPlay
  and HomeKit close too.
* **Only Guard reaches the internet** from the TV; `lumen-remote` stays the
  one LAN listener. Glasshouse, mentioned in risk 11, is never installed.

## Open questions

1. Does the home line have a public IPv4 address, or is it behind the ISP's
   NAT (D11)?
2. Could an Ethernet cable reach the TV, if Wake-on-LAN over Wi-Fi fails
   (risk 2)?

## Not in this spec

Code. ThinQ, Alexa and HomeKit. Casting: DIAL and Netflix's own listener stay
as they are. A phone app, since the web UI is the phone's app.
