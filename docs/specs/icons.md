# LG's icons through a root copy

The shell runs as an ordinary webOS app in a jail. The jail mounts
`/media/developer` but not `/usr/palm/applications`, `/media/cryptofs/apps`,
`/mnt/otncabi` or `/usr/palm/plugins/inputapps` (its `/proc/mounts`,
OLED55B9PUA, firmware 05.50.00, 2026-10-21). So only developer-mode apps'
icons resolve: 4 of the 30 tiles on the B9 (shell-mvp.md, B3). The other 26
show their letter.

This spec gives those 26 tiles their icons. A script that ships in the IPK,
run as root through Homebrew Channel's `exec`, copies LG's app and input
icons into Lumen's own app folder, which the jail can read. The owner allowed
this route on 2026-10-22 (`docs/specs/look-and-feel.md` on main, owner's
decision 2: "LG's app icons copied through Homebrew Channel's exec,
reversible, with every TV step asked first") and approved this track at
Gate 1 for the four side sessions.

## What Homebrew Channel's `exec` is

From the source (webosbrew/webos-homebrew-channel at 1a2b969, 2026-08-03,
`services/service.ts` and `services/elevate-service.ts`; version 0.7.3):

* **Call:** `luna://org.webosbrew.hbchannel.service/exec` with
  `{"command": "<string>"}`. A missing or non-string `command` is refused.
* **What it runs:** Node's `child_process.exec(command)`, which is
  `/bin/sh -c command`, as root once Homebrew Channel's service is elevated
  (its `checkRoot` method says whether it is). No timeout and no working
  directory are set. Node's default output cap applies (`maxBuffer`, 200 KB
  in Node before 12; more kills the command), so the script prints one line.
* **Reply:** one reply, when the command ends:
  `{"returnValue": true, "stdoutString", "stdoutBytes", "stderrString",
  "stderrBytes"}` (the `Bytes` are base64). A non-zero exit gives
  `returnValue: false` and `errorText` is Node's message
  (`Command failed: <command>` and the stderr), with the same four fields.
* **Who may call it:** `elevate-service` puts
  `org.webosbrew.hbchannel.service/*` in luna's `public` group and adds
  `*` to the service's `allowedNames` and `inbound`. So any client on the bus
  that may call public methods may call `exec`. That is Homebrew Channel's
  design ("root execution service that can be easily used by webOS homebrew
  developers", its README); Lumen adds no new way in.
* **How others use it:** plx-native, a jailed native Rust app like Lumen,
  calls `exec` through the same anonymous `LSRegister(NULL)` client the shell
  uses, with a 10 s budget, a fixed command, and exact markers on stdout
  (`rust-modules/platform/src/webos/jail_repair.rs`). lg-luma, a native
  launcher, runs `/usr/bin/luna-send -n 1 -w 4000 <uri> '{}'` through it to
  read the app lists as root (`src/platform.rs`). AltHome runs its own
  `elevate-service` through it. The source does not say whether plx-native's
  call was measured on a TV, so whether the hub answers Lumen's jailed client
  is the first thing the TV check list finds out.

## The design

### Where the copy goes

`/media/developer/apps/usr/palm/applications/io.github.sunnyside007.lumen.shell/lg-icons/`,
inside Lumen's own app folder:

* the jail mounts `/media/developer`, so the shell reads it at the same path;
* docs/safety.md, rule 1 lets Lumen write in its app directories;
* uninstalling the app removes the folder, and with it the copy.

The copy mirrors each icon's absolute path: LG's
`/media/cryptofs/apps/usr/palm/applications/netflix/LARGE_APP_ICON.png` is
copied to `lg-icons/media/cryptofs/apps/usr/palm/applications/netflix/LARGE_APP_ICON.png`.
So the shell finds a copy from the path luna gives, with no table. Every
folder name in the copy is either one of the fixed roots below, a validated
app ID, or a validated part of a path.

Next to it, while a run is going, are `lg-icons.lock` (the lock: a link to
the run's PID), `lg-icons.work/` (the run's scratch files), `lg-icons.new/`
(the copy being made) and `lg-icons.old/` (the one it replaces, for a
moment). A run removes all four when it ends, but a run that finds another
one going leaves that one's lock alone.

### The root command

Fixed, a constant in `luna.rs`, with no argument at all:

```
/bin/sh /media/developer/apps/usr/palm/applications/io.github.sunnyside007.lumen.shell/lumen-icons.sh
```

The script ships in the IPK (`packaging/shell/lumen-icons.sh`, mode 0755,
owner 0:0 as `ares-package` writes it). Starting it with `/bin/sh` means it
runs even if a file system drops its execute bit. Everything the script
copies, it finds itself: the shell passes it nothing, so no byte of another
app's data reaches the command line.

### What the script does

POSIX sh for the TV's busybox, shellcheck-clean, `set -eu`, umask 022.

1. **Refuses** unless it runs as root (`id -u` is 0), `sha256sum` is there
   (the B9's BusyBox 1.24.1 has no `cksum`), and Lumen's app folder exists.
   It checks the folders our installs set: the three shared ones,
   `/media/developer/apps/usr`, `usr/palm` and `usr/palm/applications`, and
   Lumen's own. Each must be a real folder (not a link), owned by the
   script's user and writable by no one else, or a non-root writer could
   swap one and steer root's writes. Their modes come from the folder modes
   in the data.tar of the last IPK installed, which `tools/ipk/fix_modes.py`
   makes 0755. `/media/developer` and `/media/developer/apps` are not
   checked: LG's developer mode makes them 0777 (B9, 2026-10-23) and no IPK
   sets them ("Known limits").
   It then pins Lumen's folder before writing anything: it enters it, checks
   `.` again, and refuses unless `pwd -P` is the expected path, so no link
   anywhere on the way moved it. From then on it uses relative names only.
2. **Locks** with `ln -s <its PID> lg-icons.lock`, which is atomic. A
   second run while the first is going prints `lumen-icons: busy` and exits
   0. A lock is stale unless `/proc/<PID>/cmdline` names `lumen-icons.sh`:
   a power cut can leave a PID that another process holds after the reboot.
   A stale lock is taken over. Two runs that both take over the same stale
   lock at once can both go on; the worse case is that one fails and the
   copy there was stays.
3. **Puts back** a copy that a run cut off mid-swap left as
   `lg-icons.old`.
4. **Asks luna** as root, as lg-luma does:
   `/usr/bin/luna-send -n 1 -w 4000 luna://com.webos.applicationManager/listLaunchPoints '{}'`,
   into `lg-icons.work`. A reply over 4 MB, or without
   `"returnValue":true`, fails the run and leaves the copy as it was.
5. **Plans the apps' icons.** It takes every `"icon"` and `"largeIcon"`
   string out of the reply (`grep -oE`, so no JSON parser), and keeps a path
   only if all of these hold:
   * it ends in `.png` or `.PNG`, has only `A–Z a–z 0–9 . _ / -`, no empty,
     `.` or `..` part, and is at most 255 bytes past the app's folder;
   * it starts with one of four fixed roots, the folders `listApps` gives
     LG's and the store's apps on the B9: `/usr/palm/applications`,
     `/media/cryptofs/apps/usr/palm/applications`,
     `/media/system/apps/usr/palm/applications` and
     `/mnt/otncabi/usr/palm/applications`. `/media/developer` is left out:
     the jail reads those apps directly, and they are the easiest to plant;
   * the next part, the app's folder, is a valid app ID: `A–Z a–z 0–9 . _ -`,
     not starting with a dot, at most 255 bytes;
   * `readlink -f` of the file is still inside `readlink -f` of the app's
     folder, so no link leads out, has only the same characters, and is a
     regular file.

   At most 128 app icons are planned; paths that fail a check do not count
   against that.
6. **Plans the input icons:** each regular `.png` in
   `/usr/palm/plugins/inputapps/assets/small`, LG's own folder on the
   read-only system image (where `getAllInputStatus` points every input on
   the B9), with the same link and file checks, at most 64.
7. **Skips** the work if nothing changed: the `sha256sum` of the script itself,
   the plan, and each planned file's `ls -lnL` line (size and time) is kept
   in `lg-icons/.sum`. The same sum prints `lumen-icons: unchanged` and
   exits 0. So an app added or removed, an icon replaced where it was, or a
   new script redoes the copy.
8. **Copies** each planned file: it reads at most 1 MB + 1 byte of it
   (`head -c`) into a scratch file, and keeps that only if it is at most
   1 MB (the shell's own limit, `icons.rs`), starts with the 8-byte PNG
   signature (`cmp` against a copy of it, byte for byte), and the copy stays
   within 8 MB in all. Only then does it make the icon's folders and move
   the file in. Checking the bytes it copied, not the file it opened, means a
   file swapped after the plan still cannot put anything but a small PNG
   into Lumen's folder.
9. **Swaps** the new copy in, after a `sync`: `lg-icons` becomes
   `lg-icons.old`, `lg-icons.new` becomes `lg-icons`, and `lg-icons.old` is
   removed. A tile that asks for its icon in that moment shows its letter.
   A run that fails between the two renames keeps `lg-icons.old`, which the
   next run puts back (step 3).
10. **Reports** one line on stdout,
    `lumen-icons: changed, <n> copied, <m> refused`, and the same line in
    `lg-icons/status`. A failure exits 1 with its reason on stderr.

Files are 0644 and folders 0755, owned by root: the shell reads them, and
only root writes them.

Time: luna-send waits at most 4 s. The copy is bounded by the counts above.
Homebrew Channel sets no timeout, and a cancelled call does not stop the
script, so a run ends on its own and the lock keeps a second one out. A file
swapped for a FIFO between the plan and the copy would hang `head` as root
with the lock held; that needs write access to LG's root-owned folders.

### How the shell finds the copies

`luna.rs` confines every icon to its owner's folder (shell-mvp.md, B3). It
now tries two places, in order:

1. the icon in its folder, as before (developer-mode apps resolve here);
2. the same icon and folder under `lg-icons/`, mirrored, confined the same
   way: canonicalised and refused unless still inside the mirrored folder,
   which must itself still be inside `lg-icons/` (a `folderPath` with `..`
   in it cannot lead out).

Inputs are confined to `/usr/palm/plugins/inputapps`, then to its mirror.
`icons.rs` is unchanged: a copy's path resolves to itself, and the loader's
limits (a regular file, at most 1 MB, 512 px a side, opened without
blocking) still apply. No Rust code reads anything the script wrote except
through that confinement and loader.

### What triggers it

The first copy is made by hand, over SSH as root, with the owner's OK (the
TV check list below). That is docs/safety.md, rule 3: applied by hand first,
checked, then automatic.

After that the shell keeps it fresh. On its first `list`, if `lg-icons/`
exists (or only `lg-icons.old/`, which the script puts back), the shell
calls `exec` with the fixed command on a thread of its own and does not
wait for it: the tiles use the copy already there, so start-up gains no
time. That call waits up to 30 s for its reply, not the 5 s of the other
calls, since a first copy reads every icon; how long it takes on the B9 is
not yet measured. It is timed in the log like every luna call
(`luna exec: <ms> ms`, or `luna exec failed after <ms> ms: <errorText>`),
the format the log already has. Without either folder, the shell never
calls `exec`.

### Refreshing when apps change

Each start runs the script, which redoes the copy only when the sum changed
(step 7): an app installed or removed, an icon replaced where it was, or a
new script. The tiles pick a new copy up on the next start, since the shell
lists once. An app installed while the shell runs shows its letter until
then.

### The undo

* Remove the copy: `rm -rf <app folder>/lg-icons <app folder>/lg-icons.new
  <app folder>/lg-icons.old <app folder>/lg-icons.work
  <app folder>/lg-icons.lock`. The shell then stops calling `exec`, and the
  26 tiles show their letters again.
* Uninstall Lumen: the app folder goes, copy and script with it.
* Nothing else on the TV changes: the script writes only inside Lumen's
  folder, makes no link but its lock, starts nothing that outlives it, and
  touches no setting.

## Files

| File | What |
|---|---|
| `packaging/shell/lumen-icons.sh` | the script |
| `tools/tests/lumen-icons.bats` | its tests, against a fake file system and a fake `luna-send` |
| `shell/crates/lumen-shell/src/luna.rs` | the copy's root, the fixed command, the refresh thread, confinement into the copy |
| `Makefile` | stages the script in the IPK; shellcheck covers `packaging/shell/*.sh` |
| `tools/tests/test_ipk_modes.py` | a staged script keeps 0755 through `fix_modes.py` |

The script reads one variable, `LUMEN_ICONS_PREFIX`, for its tests: a fake
file system root put in front of every path, `luna-send` included. On the
TV it is unset. It gives a caller nothing new: whoever can set the
environment of a root command through `exec` can already run any command.

## What the TV must still show

Each is a step in the check list.

1. Whether the hub lets the jailed shell's anonymous client call `exec`.
2. That busybox on the B9 has what the script uses. Answered in part on
   2026-10-23 (BusyBox v1.24.1): `readlink -f`, `head -c`, `grep -oE`,
   `cmp`, `tr` and the rest are there, `cksum` is not, so the script uses
   `sha256sum`, which the B9 has (Lumen's TV tools run it there). Still to
   see: `sha256sum` reading stdin, `ls -lnL`, `ln -sn` and `pwd -P`. The tests ran under dash with GNU tools, and under
   busybox 1.36.1.
3. That the four checked folders are real, root's and 0755 once a
   `fix_modes.py`'d IPK is installed. On 2026-10-23 all six folders down to
   Lumen's were owner 0 and real, and all 0777: LG's two by developer mode,
   the other four from an IPK packaged without `fix_modes.py`. An earlier
   install that night of an IPK with fixed modes had set them 0755, so an
   install does reset an existing folder's mode.

   And that `pwd -P` in Lumen's folder prints
   `/media/developer/apps/usr/palm/applications/io.github.sunnyside007.lumen.shell`.
   If `/media/developer` were a link or reached through another path, every
   run would refuse with "is not where it should be (<the real path>)". The
   first run of this version on the TV is by hand, so this is seen there
   first (docs/safety.md, rule 3).
4. That a path the jail does not mount fails with "no such file", not
   "permission denied": the shell tries the copy only when the original
   cannot be resolved.
5. Whether installing a new IPK over the old keeps the folder's `lg-icons/`
   or removes it. If it removes it, the copy is made by hand again after
   each install, until the owner chooses to make it automatic.
6. How long a run takes, changed and unchanged.

## Known limits

* `/media/developer` and `/media/developer/apps` are 0777, LG's developer
  mode's choice, and the script does not check them: no IPK sets them, and
  refusing them would refuse every developer-mode TV. So a non-root process
  that can write `/media/developer` could, at the moment of a run, move
  another root-owned folder into Lumen's place, or root-owned folders into
  the places of `usr`, `palm` or `applications` above it (a swap the folder
  checks and `pwd -P` would not see, since every folder it then finds is
  real, root's and at the right path). The script would put its
  `lg-icons*` entries there, and remove or replace any `lg-icons*` entries
  that folder already had: PNG copies of at most 1 MB, 8 MB in all, a
  status line and a lock link; nothing executable, and nothing outside
  `/media/developer`.
* The three shared folders are every developer-mode app's. Installing
  another IPK packaged without `fix_modes.py` (`ares-package` writes 0777
  folders) can make them 0777 again. The script then refuses, naming the
  folder, and the copy stops being refreshed until an IPK with sane modes
  is installed. It fails closed; the icons already copied stay.

* An icon path may name a PNG in another app's folder under the four
  roots; that PNG is then copied for it. These are app resources, not
  secrets.
* The script runs its applets through the `PATH` Homebrew Channel's service
  has. Fixing it in the script would also stop the tests from faking `id`.
* A refresh thread still waiting on its reply when the shell exits is not
  joined.

## Proposed, for the owner

* **A log line** with the script's summary,
  `icons: changed, 52 copied, 0 refused` (or `unchanged`), from the reply's
  `stdoutString`. Not added: new log-line formats are the owner's call.
  Until then the line is in `lg-icons/status`.
* **Re-listing after a change,** so a first copy or a new app's icon shows
  without a restart. It needs a hook in `app.rs`: the refresh thread sets a
  flag when the script prints `changed`, and the main loop, when idle,
  calls `list` again and redraws.
* **Making the first copy automatic** once the hand test passes: ship an
  empty `lg-icons/` in the IPK, which opens the shell's gate on the first
  start.
