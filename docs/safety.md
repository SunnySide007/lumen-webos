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
   the TV to stock. Uninstalling and rebooting must leave no trace beyond what
   [Glasshouse documents](https://github.com/rorygallagher2024/glasshouse/blob/main/docs/TV-CHANGES.md)
   for its own changes.
3. **Test by hand before persisting.** A mod is applied manually first ("apply
   now"), checked, and only then installed as a boot hook. The project owner
   confirms before the first persistent change to their TV.
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

## If something goes wrong

In order of escalation:

1. **The home screen misbehaves:** hold Home to reach LG's own Home (Phase 3
   provides this), or uninstall the app from LG's app list.
2. **A boot hook misbehaves:** SSH in, `touch /var/luna/preferences/lumen_disable`,
   reboot. Then fix or remove the hook in `/var/lib/webosbrew/init.d`.
3. **No SSH:** Homebrew Channel's failsafe mode. If the TV crashes during boot,
   Homebrew Channel starts only an emergency Telnet shell; remove the cause,
   then delete `/var/luna/preferences/webosbrew_failsafe`.
4. **Glasshouse's parts:** `tvwebctl restore-glasshouse-home`, `restore-keys`
   and the uninstall in its Homebrew Channel entry undo its changes.

A factory reset removes root, Homebrew Channel and every Lumen change, but also
everything else the owner set up, so it is the last resort, not the plan.
