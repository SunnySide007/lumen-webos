# takeover: Home opens Lumen (Phase 3, route D)

Route D of [depth.md](../docs/specs/depth.md#home-three-routes): Lumen
becomes the B9's Home through LG's own switch, the setting LG's
`home_launcher.sh` uses to choose between its two homes. No file is
replaced, nothing is mounted, nothing restarts. Proven by hand on
2026-10-31 and kept on since 05:50 UTC that day
([b9.md, "Home"](../docs/device/b9.md#home)). Everything here runs on the
TV through `tools/tv.sh script`, from [tools/takeover.sh](../tools/takeover.sh);
[tools/uninstall-all.sh](../tools/uninstall-all.sh) runs the same undo files.

| Part | Files | State |
|---|---|---|
| The setting | `apply.sh`, `undo.sh`, `status.sh` | Ready; the TV already has it on, set by hand |
| The boot hook | `lumen-home`, `home-apply.sh` | Installed on the B9 on 2026-10-31 at 17:42 UTC, after its by-hand run and the owner's yes; it closed LG's Home at the next power-on ([b9.md, "Home"](../docs/device/b9.md#home)) |
| The LG Home tile | in the shell | Waits for `claude/shell-timing` on `main` |
| The long-Home filter | `filters/lumen-home.js`, `filter-*.sh` | Built and tested on the PC, delivered inert: the owner's decision |

## The setting

The compositor's Home handler (LG's `systemUi.js`) launches the app named by
`defaultApps.home` (`com.webos.settingsservice`, category `general`) on the
release of a short Home press, with `{"launchType": "homeKey"}`.

```sh
tools/takeover.sh status     # read-only: the home, the app, the save, the hook, the filter
tools/takeover.sh apply      # tools/backup.sh first
tools/takeover.sh undo
```

* `apply` refuses unless Lumen's app folder exists, or with the kill switch.
  It saves the value it replaces to `/var/lib/lumen/takeover/defaultApps.before`
  once, never over an earlier save, writes Lumen's id and fails unless the
  setting reads back as Lumen's. With Lumen already the home and nothing
  saved (the TV today), it saves nothing.
* `undo` writes the saved home back, or `com.webos.app.home` when nothing
  was saved, taking only a plain app id from the file; once the setting reads
  back, it deletes the save. It works with the kill switch present.

## The boot hook

With Lumen as Home, LG's Home still starts: SAM preloads it on snapshot
boots and keeps it alive, and it runs unseen, as root and outside any jail,
calling LG's servers (privacy.md's layer 2). `lumen-home`, installed as
`/var/lib/webosbrew/init.d/lumen-home`, closes it the way LG's own script
closes the home it doesn't use: `applicationManager/closeByAppId` with
`forceMode`.

* In the background it waits for SAM (at most a minute), then once a minute
  for about four minutes reads `defaultApps.home`. While that is Lumen's id,
  it closes LG's Home if it runs and isn't in front, and logs SAM's running
  list 10 s later, which shows whether keep-alive brings it back: by its
  pid, since on the B9 LG's Home can take more than 10 s to exit and is
  still listed meanwhile (seen on 2026-10-31).
* It never sets the home: with any other default it says so and stops. It
  does nothing while the kill switch exists, returns at once, writes only
  its log, and is over within five minutes, timed by `/proc/uptime` because
  the wall clock jumps when the TV sets it.
* Its log is `/var/lib/lumen/log/lumen-home.log`, with the run before in
  `lumen-home.log.1`, never `/tmp`. When that folder or `/var/lib/lumen` is a
  link, or writable or owned by anyone but root, it works unlogged and
  `hook status` says "no log".
* Every luna call it makes is time-boxed (`timeout -t 5`, busybox's, or
  `timeout 5`). With neither working it logs `no timeout command; nothing
  done` and stops: a boot hook that could hang is not run at all.
* Nothing it does outlasts a reboot: LG's Home is SAM's to start again.

```sh
tools/takeover.sh hook push                  # the inert copy, /var/lib/lumen/takeover/boot-hook
tools/tv.sh script takeover/home-apply.sh    # run it now, by hand; prints round 1
tools/takeover.sh hook status                # its later rounds
tools/takeover.sh hook install               # after that run and the owner's yes: from the next boot
tools/takeover.sh hook remove
```

**How a file reaches the TV**, for `hook push`, `hook install` and
`filter apply`. Nothing goes through `/tmp`, which every app's jail shares.
`tools/takeover.sh` writes a one-off script: the PC's SHA-256 of the file
in `WANT`, the file itself in a quoted here-document, then the TV-side
template (`hook-install.sh` or `filter-apply.sh`), and runs it with
`tools/tv.sh script`. On the TV the template writes the file to
`/var/lib/lumen/stage`, made with `umask 077` and refused unless it and the
folder above it are real folders (not links) owned by root and writable by
no one else. It checks the staged copy against `WANT` before anything uses
it; a copy that differs by a byte is deleted and nothing changes. Only then
is it copied into place (`boot-hook`, init.d, the filter's path), each copy
moved in whole. The staged copy is removed on every exit, and the PC checks
the SHA-256 the TV prints for each copy.

## The LG Home tile (once the shell is on `main`)

Lumen's tiles come from `listLaunchPoints`, which leaves LG's Home out (its
`appinfo.json` has `visible: false`). The shell will add `com.webos.app.home`
as "LG Home", last among the apps, launched like any other, with its icon
through the root icon copy. That is the escape hatch that needs neither SSH
nor the filter.

## The long-Home filter (inert)

With Lumen as the default home, holding Home does nothing: LG's handler
gives the long press a meaning only for its own two homes.
`filters/lumen-home.js` gives it one.

**How the chain works**, from LG's filter files (read on 2026-10-31 for the
contract only; none of their code is here). The configd value
`com.webos.surfacemanager.keyFilters` (`com.webos.service.config`) lists
`{"handler", "file"}` pairs in order, eleven on the B9, `handleSystemKeys`
fifth. Each handler is called as `handler(key, pressed, autoRepeat)` and
returns a `KeyPolicy`: `NextPolicy` hands the key to the next handler,
`Accepted` keeps it, `NotAccepted` gives it to the app in front. Home is
`Qt.Key_Super_L`. LG's Home handler times a press with the shared
`longPressTimer` for 1,000 ms, launches the default home on a short
release, and keeps the release after a long press. The filters share the
compositor's objects: `applicationManager`, `settingsService` (its
`getLaunchDefaultApp` is the setting above), `genericTimer` (independent
timers by id), `pmLog`, `KeyPolicy`, `Qt`.

**The handler**, listed just before `handleSystemKeys`. Its first line hands
every key but Home on. On a Home press, with Lumen as the default home and
none of LG's reasons to ignore Home (Home switched off, a modal alert, the
first-use wizard), it starts a `genericTimer` for 1,000 ms and hands the
press on, so LG's handler sees it as before. If the timer fires while Home
is down, it opens LG's Home as LG's handler does for its own
(`launch("com.webos.app.home", {"activateType": "recent"})`) and keeps the
release (`Accepted`), so LG's handler doesn't open Lumen on it. A short
release cancels the timer and goes on. Repeats change nothing; any error
hands the key on. It reads nothing from disk and logs one `pmLog` line per
action. The no-op build (`--noop`) does all of this but acts on nothing:
it logs `would open LG Home` and hands every key on.

**Applying it.** `tools/takeover.sh filter apply [--noop]` checks the file
on the PC (no syntax newer than ES5; `node --check`) and sends it as above.
On the TV, `filter-apply.sh` stages it and checks its SHA-256, compiles it
with the TV's node 0.12 (`new Function`, which runs nothing), saves the
chain without Lumen's entry to `/var/lib/lumen/takeover/keyFilters.before`
once, puts the file at `/var/lib/lumen/takeover/lumen-home.js`, writes the
chain with Lumen's entry and reads it back, writing the old chain back if
it differs. `filter undo` writes the current chain without Lumen's entry,
keeping anything else that changed since, says whether that is the saved
chain, and deletes the file and the save only once the chain reads back
without the entry. `filter check` is read-only. None restarts anything.

**Live or not.** LG's filter files don't say how the list is loaded; LG's
`/usr/lib/qt5/qml/YSM/StarfishKeyFilter.qml` does, and no look has read it
(`filter check` prints its relevant lines). Glasshouse's implementation
notes say the compositor subscribes to the value and rebuilds the whole
chain when it changes, with nothing restarted, and only when it changes,
rerunning every filter's `init()`; and that configd keeps a set value across
reboots. So expect a change to the list to be live at once, a new file
under an unchanged entry to load at the next reboot, and the entry to
survive every reboot. `initctl restart surface-manager` would restart the
display server under every app; nothing here uses it.

### Recovery, before anyone runs the trial

Neither a reboot, nor the kill switch (read by Lumen's boot hooks, not the
compositor), nor Homebrew Channel's failsafe (which skips boot hooks and
leaves configd alone) takes the entry out. What can break is narrow by
design: Lumen's code hands every key but Home on, and on any error Home
too. Outside its code, two things are unseen on webOS 4: how the loader
treats a file it fails to load (Glasshouse: that file's entry is lost, the
rest load), and what the compositor does with a handler that fails before
its `try`. The no-op trial answers both. The ways back, in order:

1. **SSH**, which doesn't need the remote: `tools/takeover.sh filter undo`.
   Live at once if the compositor follows the value; otherwise after a cold
   boot (the plug), which doesn't need the remote either.
2. **SSH down:** unplug the TV for 30 s; dropbear starts again at boot. Then 1.
3. **dropbear doesn't come back:** Homebrew Channel's failsafe root Telnet
   (safety.md, step 3), and there, by hand, the saved chain back:

   ```sh
   luna-send -n 1 luna://com.webos.service.config/setConfigs "{\"configs\":{\"com.webos.surfacemanager.keyFilters\":$(cat /var/lib/lumen/takeover/keyFilters.before)}}"
   ```

4. **No network at all:** if only Home is wrong, every other key still
   works, LG's Settings included; Settings, General, Reset to Initial
   Settings is the factory reset, safety.md's last resort. A remote that is
   dead entirely with no network has no way back but that, which is why the
   trial below never reboots with the filter on until every key has been
   checked live, with SSH open and step 1 typed in.

### The trial, step by step (the PM with the owner)

1. `tools/backup.sh`. Confirm SSH works (`tools/tv.sh check`) and keep a
   second terminal with `tools/takeover.sh filter undo` typed in.
2. `tools/takeover.sh filter check`: the chain is the eleven of 2026-10-31,
   Lumen's entry is absent, node is v0.12.2. Read the loader's lines it
   prints: they say whether the list is followed live.
3. `tools/takeover.sh filter apply --noop`. If live: every remote key now
   (arrows, OK, Back, Home short and held, Menu, Settings, Input, volume,
   mute, numbers, the Netflix and Amazon buttons, the pointer). Held Home
   does nothing new, and logs `would open LG Home` if filters' `pmLog`
   lines reach `/var/log/messages` (`grep LUMEN_HOME_FILTER`; Glasshouse
   saw them missing on a CX). Any key wrong: step 1's undo at once.
4. `tools/takeover.sh filter undo`, the keys again: the recovery path works
   before anything depends on it.
5. `tools/takeover.sh filter apply --noop` again, a reboot, every key again.
6. The real handler: `tools/takeover.sh filter apply`, then a reboot (the
   entry is unchanged, so the new file loads then). Held Home opens LG's
   Home; a short press opens Lumen; every other key as before.
7. The drill: `tools/takeover.sh filter undo` over SSH and a reboot; held
   Home does nothing again and every key works.

**A suggestion, not built:** a dead man's switch for steps 3 and 5,
`filter apply --revert-after 300`, would leave `filter-undo.sh` waiting on
the TV for five minutes until `filter keep` cancels it, so a TV with broken
keys and no SSH puts the chain back by itself. It would write configd from
the background as root, and it doesn't cover a reboot, which is why the
trial checks every key live first.

## Escape hatches

* `tools/takeover.sh undo` over SSH: a short Home press opens LG's Home again
  at once.
* The LG Home tile in Lumen, once the shell has it.
* Held Home, once the filter is on.
* `tools/uninstall-all.sh` takes the filter out of the chain, then sets the
  default home back, and removes Lumen's app only if that worked, so the TV
  is never left with a Home that isn't installed; then the boot hooks and
  `/var/lib/lumen`.
