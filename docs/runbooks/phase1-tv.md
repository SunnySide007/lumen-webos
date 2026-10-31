# Phase 1 on the TV: runbook

The steps of Phase 1 that need the B9, in order, with what each should print
and how to undo it. Every command runs on the PC, in Git Bash, from the
repository root. `tools/tv.sh` reads the TV's address from `.env`.

Stages: **B** (day 1, about 20 minutes: plx-native, start the 24 h capture),
**C** (day 2: read the capture, install and configure Glasshouse), **D**
(day 3: a second capture with blocking on). The settings recipe and its
costs are in [profiles/glasshouse/aggressive.md](../../profiles/glasshouse/aggressive.md).

Ground rules ([docs/safety.md](../safety.md)): nothing here writes to a
system partition. The capture and plx-native leave no boot hook. Glasshouse
is the first persistent change, and **the owner confirms before it is
installed** (Stage C, step 5).

## Before starting (done on 2026-10-20, off the TV)

- `dist/capture/tcpdump` is built (`sh tools/dev.sh make capture-tools`):
  tcpdump 4.99.7 with libpcap 1.10.7 linked in, ARM EABI5, no glibc symbol
  newer than 2.7. `dist/capture/BUILD-INFO.txt` has its SHA-256.
- `dist/vendor/` holds both IPKs, checked against GitHub's published SHA-256
  ([PINNED.md](../../profiles/glasshouse/PINNED.md)).

If either is missing (a fresh clone), rebuild or re-download them first:

```bash
sh tools/dev.sh make capture-tools
```

and the `curl` lines in PINNED.md.

## Stage B: day 1

### B1. Check the connection

```bash
tools/tv.sh check
```

Expect `connected as root to …` and JSON with `"modelName": "OLED55B9PUA"`
and `"firmwareVersion": "05.50.00"`. If SSH times out, the TV may have
rebooted with SSH off: turn it on in Homebrew Channel and cold boot (unplug).

### B2. Check the IPKs

```bash
sha256sum dist/vendor/*.ipk
```

Expect exactly:

```
150737a0395276f10f283192f9dc97193bf8c5e61ed58745037eee2a7195d3e2 *dist/vendor/com.beb.plxnative_0.8.0_arm.ipk
287f1a5ca89061188aceb91a4d1c2e2e79ae6cb9a08bcb77fdf1bd741fdf8e2a *dist/vendor/io.github.rorygallagher2024.lg-webos-dashboard_0.118.0_all.ipk
```

Stop if either differs.

### B3. Install plx-native 0.8.0

```bash
tools/tv.sh install dist/vendor/com.beb.plxnative_0.8.0_arm.ipk
tools/tv.sh launch com.beb.plxnative
```

Expect `installed`, then the app opens on the TV. **Owner:** sign in with the
QR code (or plex.tv/link), pick the Plex server, play something for a minute.
If video fails, use plx-native's **Repair** button (for rooted TVs).

- Rollback: `tools/tv.sh remove com.beb.plxnative`.

### B4. Upload the capture tool

```bash
tools/capture.sh push
```

Expect two `pushed … bytes` lines, tcpdump's version (`tcpdump version
4.99.7`, `libpcap version 1.10.7`), and `tcpdump and the helper on the TV
match this checkout (…)`, whose first hash is the one in BUILD-INFO.txt
(63b86ba57be346ec7042acbb16c9bcfb7e2fb65f0ffdc66f1266c17924d63330; the build
is reproducible: two builds on 2026-10-20 gave it). Everything goes to
`/var/lib/lumen/capture/`, a folder only root may use (since 2026-10-31;
before, `/media/developer/temp/lumen-capture/`, which apps can reach). /var
is small on the B9, so the ring is 5 files of 20 MB, with 100 MB of /var kept
free.

If tcpdump fails to start on the TV (a missing library or symbol), stop
here: the capture needs a rebuild, and Stage B continues with B7 only.

### B5. Smoke test: two minutes

```bash
tools/capture.sh start 1
tools/capture.sh status
```

Expect `capturing: pid N, until 1 h from now, at most 10 files of 50 MB`,
then `running: pid N` and one `lumen-….pcap0` file. Open Netflix on the TV
for a minute, then:

```bash
tools/capture.sh stop
LUMEN_CAPTURE_OUT=tools/out/capture-smoke tools/capture.sh fetch
sh tools/dev.sh python3 -I tools/capture/summarize.py tools/out/capture-smoke/*.pcap*
```

Expect `stopped pid N` with tcpdump's packet counts, `fetched 1 file(s)`,
and a markdown table with Netflix names (`*.netflix.com`, `*.nflxvideo.net`)
marked `allow`. Link type 113 (or 276) in the header line.

Then clear the smoke files on the TV, so they do not mix with the real run:

```bash
tools/tv.sh run 'rm -f /var/lib/lumen/capture/pcap/lumen-*'
```

### B6. Start the 24-hour baseline capture

Stock settings, no Glasshouse.

```bash
tools/capture.sh start 24
tools/capture.sh status
```

Expect `capturing: …until 24 h from now…` and `running`. It stops by itself
after 24 hours, and at any reboot. It never uses more than 10 × 50 MB, and
refuses to start with less than 700 MB free.

- Rollback: `tools/capture.sh stop`, then `tools/capture.sh clean`.

### B7. During the 24 hours (owner)

Use the TV normally, and on purpose at least once each:

- Netflix, Prime Video and Plex (plx-native): play something.
- Spotify: play from the Pixel with Spotify Connect.
- Cast YouTube, and Netflix, from the Pixel.
- Web Video Caster: cast something.
- Open the Content Store and check for app updates.
- Try voice search and LG Channels once (so their hosts show up).
- Leave the TV in standby overnight; LG's telemetry runs then. Use the
  remote's power button (standby), do not unplug it: a power cut ends the
  capture.

Check now and then (any time, harmless):

```bash
tools/capture.sh status
```

If it says `not running` early, the TV rebooted: start again with
`tools/capture.sh start 24` (a new file set; both get fetched).

## Stage C: day 2

### C1. Stop and fetch

```bash
tools/capture.sh status
tools/capture.sh stop
tools/capture.sh fetch
```

After 24 hours `status` says `not running` and `stop` says `not running`,
which is fine. Expect `fetched N file(s) to tools/out/capture`. The pcaps
stay private: `tools/out/` is ignored by git.

### C2. Summarise

```bash
sh tools/dev.sh python3 -I tools/capture/summarize.py tools/out/capture/*.pcap* > tools/out/capture/summary.md
```

Read `tools/out/capture/summary.md`:

- **Hosts**: every name the TV looked up or sent in TLS, with counts and
  first and last seen, marked `allow`, `block` (required or active custom),
  `candidate` (a commented entry in b9-aggressive.hosts) or blank (unknown).
- **Addresses contacted without a DNS lookup**: anything here on the
  internet is worth a look (hard-coded addresses bypass /etc/hosts).
- **Connections to the TV**: the Pixel casting (DIAL, Cast), the Plex server.

### C3. Write the profile (with the owner)

1. In `profiles/glasshouse/b9-aggressive.hosts`, uncomment each candidate
   the capture shows (`candidate` rows), and add LG telemetry, ACR, ads,
   voice, ThinQ, AI and LG Channels names the capture found. Mark each
   `# capture 2026-10-2x`.
2. In `allowlist.txt`, add any name the store, DRM or the owner's apps used
   that no rule covers yet; narrow `nextlgsdp.com` to the names seen.
3. Check, and commit after the owner reads the diff:

```bash
tools/glasshouse-custom.sh
sh tools/dev.sh make check
```

Expect the dry run to list the names it would add, and check to pass. Any
`CONFLICT` row in the summary, or a store-safe failure in the dry run,
means a name is both allowed and blocked: resolve it before going on.

### C4. Baseline check, before Glasshouse

```bash
tools/store-safe-check.sh --stage 1
```

Expect `PASS (stage 1)`. Stock `/etc/hosts` has only localhost entries. The
live lookups print `ok   allow …` for the store and app names. Note which
lookup tool the TV has (`getent` or `nslookup`); if it is busybox
nslookup, a FAIL from the live part alone is worth a second look (newer
busybox nslookup skips /etc/hosts).

An allowlisted name with no answer is a FAIL (DNS down breaks the store as
surely as a sinkhole). If one fails here, before Glasshouse, the TV's DNS or
the probe name itself is the problem: fix the name in the list near the end
of `tools/store-safe-check.sh` (with the owner), not the TV, and re-run.

### C5. Install Glasshouse 0.118.0

**Owner confirms first: this is the first persistent change.** Glasshouse
adds boot hooks under `/var/lib/webosbrew/init.d/` and bind-mounts its
hosts file over `/etc/hosts` at every boot.

```bash
tools/tv.sh install dist/vendor/io.github.rorygallagher2024.lg-webos-dashboard_0.118.0_all.ipk
```

Expect `installed`. **Owner:** open Glasshouse from the app list on the TV
and run its setup wizard. During setup Glasshouse opens a "phone link" on
the LAN (even with a localhost dashboard); that ends with setup. Leave tile
hiding off.

- Rollback: `tools/uninstall-all.sh --glasshouse` (dry run; read it), then
  `tools/uninstall-all.sh --glasshouse --yes`, then cold boot.

### C6. Dashboard on localhost only

```bash
tools/tv.sh script profiles/glasshouse/dashboard-localhost.sh
```

Expect `"host": "127.0.0.1",`, Glasshouse restarting, and a listening
socket `127.0.0.1:8080` (not `0.0.0.0:8080` or `:::8080`). Then prove the
LAN cannot reach it (TV_HOST from `.env`):

```bash
. ./.env && curl -m 5 "http://$TV_HOST:8080/" ; echo "exit $?"
```

Expect a connection failure (`exit 7` or `exit 28`), not a page.

Open the tunnel in a **second terminal** and leave it running:

```bash
tools/tv.sh tunnel
```

Expect `Glasshouse dashboard: http://127.0.0.1:8080/`; open that address in
the PC's browser.

- Rollback: `tools/tv.sh run 'cp /var/lib/tvweb/config.json.lumen-bak /var/lib/tvweb/config.json && /var/lib/tvweb/tvwebctl restart'`.

### C7. Apply the four stages

For each stage of [aggressive.md](../../profiles/glasshouse/aggressive.md):
make the change in the dashboard (stage 4: the upload below), then run the
check for that stage and the manual checklist (end of aggressive.md).

| Stage | Change | Check | Expect |
|---|---|---|---|
| 1 | Blocker tier "ads & telemetry" | `tools/store-safe-check.sh --stage 1` | `PASS (stage 1)`, many sinkholed names, 0 allowlisted |
| 2 | Privacy toggles, LG services, unused preloaded apps | `tools/store-safe-check.sh --stage 2` | `PASS (stage 2)`; voice search now fails |
| 3 | "Block TV software updates" | `tools/store-safe-check.sh --stage 3` | `PASS (stage 3)`, `firmware hosts sinkholed: 5 of 5`, and `ok   block snu.lge.com -> 0.0.0.0` |
| 4 | Custom list (below) | `tools/store-safe-check.sh` | `PASS (stage 4)`, `custom-list hosts sinkholed: N of N` |

Stage 4's upload, through the tunnel:

```bash
tools/glasshouse-custom.sh
tools/glasshouse-custom.sh --yes
```

Expect the dry-run list, then Glasshouse's JSON reply with `"ok":true` and
`done: add N name(s)`. If it says `controls disabled`, set
`"allowControl": true` in `/var/lib/tvweb/config.json` (it is the default).

If a check FAILs: undo that stage (aggressive.md), re-run the check until
it passes, then work out which name broke it from the FAIL line.

- Rollback of stage 4 alone: `tools/glasshouse-custom.sh --clear --yes`.

### C8. Cold boot and re-check

Unplug the TV for 30 seconds, plug it in, wait two minutes, then:

```bash
tools/tv.sh check
tools/store-safe-check.sh
. ./.env && curl -m 5 "http://$TV_HOST:8080/" ; echo "exit $?"
```

Expect the check to `PASS (stage 4)` again (Glasshouse reapplied its hosts
at boot) and the dashboard still unreachable from the LAN. Then the manual
checklist once more, and the costs: voice search, ThinQ and LG Channels
fail; nothing else does.

## Stage D: day 3

### D1. The second capture, blocking on

```bash
tools/capture.sh status
tools/capture.sh start 24
```

(If `status` shows no tcpdump, run `tools/capture.sh push` first.) Use the
TV as in B7 for 24 hours. Then:

```bash
tools/capture.sh stop
LUMEN_CAPTURE_OUT=tools/out/capture-d tools/capture.sh fetch
sh tools/dev.sh python3 -I tools/capture/summarize.py tools/out/capture-d/*.pcap* > tools/out/capture-d/summary.md
```

Read it for:

- **Allowlisted names still looked up and connected to** (rows marked
  `allow`): the store, DRM and apps kept working.
- **Blocked names should be gone**: a sinkholed name is answered from
  /etc/hosts, so it sends no DNS query. A `block` row here means something
  resolves it another way.
- **LG telemetry left** (blank or `candidate` rows): add, re-upload, re-check.
- **Addresses without a lookup**: hard-coded endpoints /etc/hosts cannot stop.

### D2. Clean up the capture

```bash
tools/capture.sh clean
```

Expect `removed /var/lib/lumen/capture`.

Then the paperwork: decision 6 from proposed to accepted (with what
changed), the Phase 1 checklist in docs/roadmap.md, and the B9 notes.

## Rollback, all of it

```bash
tools/uninstall-all.sh --glasshouse --plx-native
tools/uninstall-all.sh --glasshouse --plx-native --yes
```

The first prints every command; read it. The second runs them, carrying on
past failures (an app that is not installed fails its step). Cold boot
afterwards. It never touches Homebrew Channel, root, the SSH key or the
`webosbrew_*` settings. If the TV misbehaves before SSH is up, see
"If something goes wrong" in [docs/safety.md](../safety.md).
