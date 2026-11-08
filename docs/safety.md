# Safety rules

Lumen runs as root on a television that people rely on and cannot easily
repair. These rules apply to every change that runs on the TV, and a pull
request that breaks one will not be merged.

## The rules

1. **Never write to a system partition.** webOS's system images are signed,
   read-only squashfs. Lumen changes behaviour only with:
   * bind mounts over individual files or directories, made at run time;
   * configd values it owns (for example its own key-filter entry);
   * boot scripts in `/var/lib/webosbrew/init.d`;
   * its own files under `/var/lib/lumen` and its app directories.
2. **Everything is reversible.** Each mod ships with its undo. Before each
   persistent change, `tools/backup.sh` copies the TV's settings and data to
   the owner's PC, outside the repository. Whatever Lumen
   overlays is backed up first, and `tools/uninstall-all.sh` (Phase 1) returns
   the TV to stock. Uninstalling and rebooting must leave no trace.
3. **Test before persisting.** A mod is applied first without persisting
   ("apply now"), then checked, by hand or by the bench (`tools/bench.sh`)
   with its automatic undo armed, and only then installed as a boot hook.
   The project owner confirmed before the first persistent change to their
   TV; from 2026-11-08 the tiers in "Working without asking" say which later
   changes need their yes.
4. **Boot hooks can never hang the TV.** They background their work, time-box
   it, and do nothing when the kill switch exists.
5. **There is a kill switch:** `/var/luna/preferences/lumen_disable`. Every Lumen
   boot hook checks it first. Create it over SSH (`touch`) and reboot to start
   the TV with Lumen off.
6. **Keys, not passwords.** All access is SSH with a key the owner installs. No
   script stores, types or sends a password.
7. **Never break what the owner paid for.** The LG Content Store, DRM, Netflix,
   Prime Video, Plex and HDMI-CEC stay working; `tools/store-safe-check.sh`
   (Phase 1) verifies the store and DRM hosts after every privacy change.
8. **Leave the firmware alone.** No flashing, ever. Keep updates blocked while
   rooted: an LG update can remove root and patch the exploit that gave it.

## Working without asking (from 2026-11-08)

The owner approved these tiers on 2026-11-08 ("lets follow your recs"), so
the builder can finish Lumen with a weekly check-in instead of a yes per
command. Every tier sits under the rules above: a tier never makes a
forbidden change allowed. When a step fits no tier, or it is unclear which,
ask.

* **Tier A, just do it.** Reversible, with nothing lasting outside Lumen's
  own app folder and log.
  * Back up (`tools/backup.sh`).
  * Read-only surveys that call only names the hub already lists
    (`ls-monitor -l` first: calling a dynamic service starts it).
  * Install, launch and close Lumen builds, and put back the last good one.
  * Robot keys, screenshots and log reads.
  * Hold the screensaver off during a run, then restore it.
* **Tier B, do it, then tell.** Lasting changes, but only with a tested
  undo, a backup first and the automatic undo armed, and only in the lab
  window. Each goes in the next report with its undo command.
  * Boot hooks and lumend parts, one at a time.
  * Route D (Lumen as Home) on or off.
  * Stopping one LG service at a time for the lean track, kept only if it
    measurably helps.
  * Reboots over SSH.
  * Lumen's own settings.
  * New `lumen-ctl` or Guard binaries.
* **Tier C, one yes per milestone.** Anything the family would notice in
  the daytime. Ask once per milestone, with the test results in front of
  the owner.
  * Guard blocking for real.
  * Closing ports that casting might use.
  * Installing other people's apps (YouTube AdFree, plx-native 0.9.0).
  * LG settings outside Lumen (picture, sound, network).
* **Never.** System partitions or firmware, a factory reset, anything that
  risks root, the owner's passwords or accounts. Rules 1 to 8 stand
  whatever the tiers say.

**The lab window.** Unattended TV work runs only in a lab window: 00:00 to
06:00 TV local time, plus any time the TV has sat on Lumen or LG's
screensaver for 20 minutes. A run never starts with another app or an HDMI
input in front. **The screen stays dark at night** (the owner, 2026-11-08):
from 00:00 to 06:00 a run does only what lights nothing new on the panel
(builds, backups, surveys, log reads, work with the screen already off).
Frame timings, screenshots and anything that puts Lumen in front run in
the idle windows outside those hours.

**Yield.** A real remote press stops the run: an event on the remote's own
evdev node, read without grabbing it, never one from the injector. The run
then puts back the last good build and settings and waits for the next
window.

**Reports.** At the end of each stage and at least weekly, with numbers
from the TV (model and firmware), screenshots, what could not be verified,
every tier B change with its undo, and any tier C ask.

## If something goes wrong

In order of escalation:

1. **The home screen misbehaves:** hold Home to reach LG's own Home (Phase 3
   provides this), or uninstall the app from LG's app list.
2. **A boot hook misbehaves:** SSH in, `touch /var/luna/preferences/lumen_disable`,
   reboot. Then fix or remove the hook in `/var/lib/webosbrew/init.d`.
3. **No SSH:** Homebrew Channel's failsafe mode. If the TV crashes during boot,
   Homebrew Channel starts only an emergency Telnet shell; remove the cause,
   then delete `/var/luna/preferences/webosbrew_failsafe`.
4. **`lumend` and the shell's keep-alive** (decisions 14 and 15, once
   built): the kill switch stops `lumend` at its door, and
   `tools/uninstall-all.sh` removes its luna role and Lumen's keep-alive
   value before it removes the app.

A factory reset removes root, Homebrew Channel and every Lumen change, but also
everything else the owner set up, so it is the last resort, not the plan.
