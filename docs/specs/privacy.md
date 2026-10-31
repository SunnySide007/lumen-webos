# Privacy: a quiet B9 (proposed)

Proposed on 2026-10-27, for the owner's approval before anything is built
beyond the read-only survey. It keeps decision 5 (aggressive, store-safe) and
its costs (voice search, ThinQ, LG Channels), and changes how Phase 1 gets
there: a lookup log that survives the TV being turned off replaces the 24 h
capture, and the work goes below `/etc/hosts`, to the senders, the addresses
and the home network.

## Where it starts

Measured on the B9 (OLED55B9PUA, firmware 05.50.00, webOS 4.10.2, kernel
4.4.84). The privacy surveys of 2026-10-28 are
[privacy-20261028-061205.txt](../device/privacy-20261028-061205.txt) and
[privacy-20261028-061746.txt](../device/privacy-20261028-061746.txt) (the
second adds the listening programs and the jobs' start-up).

**LG's own switches are already off:** `livePlus` (viewing data),
`customizedAd`, `personalRecommend`, `homePromotion`, `aiNudge` and
`screenSaverAd` are off, `lmt` (limit ad tracking) is on, and IP control is
off. Toggling them again gains nothing.

**The TV still calls LG with them off.** The first capture (4 hours on
2026-10-20, 18:56–23:02 PDT, mostly Netflix and Plex), summarised on
2026-10-27:

| Name | Probably | On a list |
|---|---|---|
| `aic.cdplauncher.lgtvcommon.com` | LG Home's content | none |
| `aic.cdpbeacon2.lgtvcommon.com` | a usage beacon | none (public lists have `cdpbeacon`, not `cdpbeacon2`) |
| `aic.cdpsvc.lgtvcommon.com` | content discovery | none |
| `aic.homeprv.lgtvcommon.com` | home promotions | candidate list has only `us.homeprv…` |
| `aic.netflixvoice.lgtvcommon.com` | voice in Netflix | none |
| `ngfts.lge.com` | LG's AI and feature service | candidate |
| `lgtvonline.lge.com` | connman's online check (below) | candidate, to be reconsidered |
| `us.lgtvsdp.com` | the Content Store | allow (keep) |

"Probably" comes from the names and the
[Level1Techs list](https://forum.level1techs.com/t/lg-tv-block-mini-how-to/255178),
not from LG. No ACR host (`alphonso.tv`) showed in those 4 hours, but LG's
ACR service is installed (`com.webos.service.acr`, started on demand), so
the longer log has to cover the HDMI inputs. Netflix logs as well
(`nrdp.logs.netflix.com`, `logs.netflix.com`, `customerevents.netflix.com`,
`ichnaea.netflix.com`), all under the `netflix.com` allow rule (P5).

**The kernel has an IPv4 firewall.** Unlike the newer sets Level1Techs
describes, the B9's kernel has netfilter built in (no modules needed):
`iptables` with the filter, nat and mangle tables; targets DNAT, SNAT,
REJECT, MASQUERADE, TOS and DSCP besides the built-in ACCEPT and DROP; and
the tcp, udp, icmp, tos and dscp matches. There is no connection-state,
owner, multiport or address-range match, so rules are stateless and one
address or port each. Every chain is empty, and rules disappear at reboot.

**IPv6 has no firewall and carries the MAC.** The TV has a global IPv6
address built from its MAC address (EUI-64), privacy addresses are off
(`use_tempaddr` 0), and the kernel has no IPv6 netfilter. Every server
reached over IPv6 sees an identifier that follows the TV across networks,
and IPv6 traffic would pass any IPv4 rule. LG's own setting is
`ipv6Enable: true`.

**Lookups:** `/etc/resolv.conf` links to connman's file, which names its
proxy on 127.0.0.1:53; connman forwards to the router. `/etc/hosts` is
Homebrew Channel's bind mount, the same for init and for SSH. connman's
online check fetches `http://lgtvonline.lge.com/`
(`/etc/connman/main.conf`), so blocking that name may make the TV think it
is offline.

**Firmware updates are mostly blocked already.** Homebrew Channel lists
`snu`, `su`, `su-ssl` and `su-dev.lge.com` at 127.0.0.1 and ::1;
`nsu.lge.com` is missing. The `update` job runs. Level1Techs reports a
hard-coded fallback, `156.147.69.32:8080`, from newer sets; unchecked on the
B9.

**Senders.** Running jobs that report, in their own `/etc/init`
descriptions:

| Job | LG's description | Restarts if it dies | Proposal |
|---|---|---|---|
| `uploadd` | "Remote diagnostics upload daemon" | yes | stop (S1) |
| `rdxd` | "Remote diagnostics daemon" | yes | stop (S1) |
| `crashreportd` | "Crash report daemon for filing a bug to Jira on a crash"; starts with `rdxd` | yes | stop (S1) |
| `remotelogger` | none | yes | stop (S1) |
| `crashd` | crash report generation | yes | keep: it handles crashes locally |
| `contentminer`, `nudge`, `user-context-manager`, `event-monitor`, `qosm`, `update` | none or generic | most | one at a time (G4) |

`initctl stop` holds a job down until the next boot even when it restarts
on its own ("respawn" only revives a job meant to be running). Several of
them already stop whenever LG starts a firmware update (`stop on started
start_update`). On-demand luna services that look like reporting: `acr`,
`admanager`, `sdx`, `statisticsmanager`, `aiservice`, `bugreport` and the
ThinQ AI adapter. No crash reports are queued; `/tmp/var/log` holds 808 KB.

**The home network.** Every listening socket, with its program:

| Port | Program | What uses it | Proposal |
|---|---|---|---|
| tcp 22 (and IPv6) | Homebrew Channel's dropbear | the owner's SSH key | keep |
| tcp 11111 | `crashreportd` | nothing in the house | close (S1) |
| tcp 3000, 3001 | `xinetd` → second-screen gateway | ThinQ, phone remote apps, Web Video Caster's control | owner (P10) |
| tcp 36866, 18181 | `xinetd` → DIAL | casting YouTube and Netflix from the phone | keep |
| tcp 9080, udp 1900 | Netflix | casting to Netflix | keep |
| tcp 7000, tcp6 35254 | AirPlay, HomeKit | Apple devices | owner (P9) |
| tcp 1103, 1537, 1551, 1830, 1833, 1973, udp 1900 | `upnpd` (DLNA) | DLNA casting; the Plex server probes it | keep unless nobody casts by DLNA |
| udp 5353 and others | `mdnsd`, `avahi-daemon` | finding the TV (Spotify Connect, casting) | keep |
| 53 on loopback | connman | the TV's own lookups | keep |

SSH is key-only in effect: the root password is locked and one key is
installed. A rule for IPv4 also covers the IPv4 side of the IPv6 sockets
(22, 35254), and switching IPv6 off removes their IPv6 side.

**Why both 24 h captures stopped is not settled.** `quickStartMode` is off,
but a quick off and on with the remote (2026-10-28, about 10 s off) kept
the system running: uptime went on, the kernel logged no suspend, and
nothing restarted. Both captures ended after hours in standby, so a deeper
standby later, or a reboot, ended them. Either way the measurement has to
outlast both, which a log started at every boot does.

**What `/etc/hosts` can't do:** it matches exact names only, while LG's
names already include ones no public list has; it keeps no record, so a
blocked name drops out of view; it can't stop an address; and a program
with its own resolver skips it.

## The layers

From the setting down to the wire; each catches what the one above misses.

| | Layer | Stops | How | State |
|---|---|---|---|---|
| 1 | LG's switches | ACR, ad profiles, recommendations | LG Settings | done |
| 2 | LG Home out of the way | launcher, beacon and promotion calls at their source | the shell as home (Phase 3) | measure with layer 3's log once the shell owns Home |
| 3 | Names | telemetry, ads, AI, ThinQ, with suffix rules | Lumen Guard, below | G1 (log only) built; rebuilt around `resolv.conf` after its first by-hand run (2026-10-31); the new hook passed by hand the same day and runs as a boot hook since 16:10 UTC |
| 4 | Senders | LG jobs that report | stop them at run time, one per check, from the list above; never `tvdataexchanger` or `alwaysready` | measured; four proposed for S1 |
| 5 | Addresses | hard-coded endpoints, public resolvers | iptables `OUTPUT` rules; LG-owned addresses only, never AWS or CDN ranges (the store is on AWS: five us-west-2 addresses) | possible: netfilter measured |
| 6 | Identifiers | the MAC inside the TV's IPv6 address | IPv6 off in LG's settings | done 2026-10-28: `ipv6Enable` false, no IPv6 address on `wlan0` |
| 7 | The home network | other devices reaching the TV's services | iptables `INPUT` rules for ports nobody uses; SSH by key only | measured; port 11111 proposed for S1 |
| 8 | The ISP | lookups readable on the wire | Guard forwards over DNS-over-TLS | owner's choice (P4) |

Layer 7 matters more on a rooted set. Updates stay blocked (safety rule 8),
so the TV never gets LG's security fixes. In October 2026 a chain giving
root from the LAN without a PIN was
[published for webOS 7.5](https://thehackernews.com/2026/10/lg-smart-tvs-vulnerable-to-pre-auth.html),
through port 3001, which listens on this TV too; whether webOS 4.5 is
affected is not known. A TV that can't be patched should offer the LAN as
little as possible.

## Lumen Guard

A small DNS forwarder on the TV, written in Rust as a new binary in the shell
workspace (`shell/crates/lumen-guard`) and built with the same armv7
toolchain (`tools/dev.sh make guard`, into `dist/guard/`). Stage G1, which
only logs, was built on 2026-10-30. Its first hook sent lookups to Guard
with a nat redirect. On the B9 on 2026-10-31 that redirect failed by hand and
the hook failed open as designed. The hook was rebuilt around a
`resolv.conf` mount, which had been proven by hand the same day, and the
new hook passed its by-hand run later that day (b9.md, "Resolver"): the
start, the lookups, the kill switch, a Wi-Fi drop and a hung Guard.

```text
lumen-guard --listen ADDR:PORT --upstream ADDR:PORT --rules DIR --log DIR --status FILE
            [--mode log] [--dscp N] [--max-log-bytes N]
lumen-guard --probe ADDR:PORT [--name NAME]
```

- **Loopback only:** it listens on 127.0.0.2:53, UDP and TCP. TCP takes one
  query per connection (RFC 1035's two-byte length), 3 s to send it, at
  most 32 connections at once. `--mode block` is G2 and refused for now.
- **What reaches it**, as measured on the B9 on 2026-10-31 (b9.md,
  "Resolver"):
  - Most lookups, through `resolv.conf`. The hook bind-mounts a file
    naming 127.0.0.2 over connman's `/var/lib/misc/resolv.conf`, which
    `/etc/resolv.conf` links to. Programs that start after the mount ask
    Guard directly. That includes every app launched after it, because the
    app jails copy `/etc/resolv.conf` at launch. Done by hand, `ping` and
    `wget` resolved through Guard in under a second. Homebrew's hosts-file
    sinkholes still applied first. The jail's copy stays when Guard stops,
    so an app launched while Guard ran keeps asking 127.0.0.2 after a
    fail-open; Guard's file therefore names connman second (P11), and glibc
    moves on to it at once when 127.0.0.2 refuses.
  - Not everything. glibc 2.24 reads `resolv.conf` when a process first
    resolves, not when the file changes. So processes started before the
    mount keep asking connman, and connman forwards to the router unlogged.
    connman's own lookups, its online check among them, also go to the
    router unlogged.
  - Why not a redirect, as first built. connman's forwarding sockets are
    bound to `wlan0`, so a nat rule that redirects its forwards can't work
    in either direction. Aimed at 127.0.0.2, the packets never reached the
    local stack. Aimed at the TV's own `wlan0` address, every forward
    reached a forwarder and got the router's answer, but connman never
    accepted the replies, which arrive on `lo`. The rule also sees only the
    first packet of connman's persistent flow.
  - Programs with a hard-coded resolver, through two nat rules, one per
    protocol. They send packets for port 53 to 127.0.0.2:53, except those
    going out on `lo` and those to the router (`<router>`, the upstream).
    connman's forwards to the router therefore pass untouched. iptables
    takes one `-d` a rule, so loopback is left out by its interface:

    ```text
    iptables -t nat -A OUTPUT -p udp --dport 53 ! -o lo ! -d <router> -m dscp ! --dscp 7 -j DNAT --to-destination 127.0.0.2:53
    iptables -t nat -A OUTPUT -p tcp --dport 53 ! -o lo ! -d <router> -m dscp ! --dscp 7 -j DNAT --to-destination 127.0.0.2:53
    ```

    Netflix is such a program: `netflix.bin` sends its own lookups to
    8.8.8.8, and on 2026-10-31 these rules brought them to Guard, which
    answered; DNAT to loopback works both ways for a socket not bound to
    `wlan0` (b9.md, "Resolver"). There is no owner match, so Guard marks its own upstream packets with
    DSCP 7 (`IP_TOS`, set before the first packet, the TCP SYN included),
    and the rules skip them. 7 comes from RFC 2474's pool for local use and
    is in the same priority class as unmarked packets. A fixed source port
    exempted with `! --sport` was weighed and dropped. It would need no
    `libc`, but `std` can't bind a TCP socket before connecting, so Guard's
    TCP lookups would loop back to it. One shared UDP port would also
    serialise queries or rewrite their IDs.
- **Passes bytes through:** it reads only the first question's name
  (compression pointers followed, loops refused) and type. Each UDP query
  goes upstream unchanged from a socket of its own, and the first answer
  with its ID comes back unchanged; messages up to 4,096 bytes, TCP answers
  streamed through in pieces of that size. No cache, no recursion. With no
  answer within 3 s nothing is sent back, and the asker retries.
- **Rules:** `allowlist.txt`, `required-blocks.txt` and `b9-aggressive.hosts`
  as they are, read as `store-safe-check.sh` and `summarize.py` read them:
  an allowlist `name` covers the name and every name under it, `=name` that
  name only; required blocks and uncommented hosts lines with a sinkhole
  address are exact blocks; commented hosts lines are candidates, not
  blocks; the allowlist wins. G1 only logs the verdict; in G2 a blocked name
  gets 0.0.0.0 and ::, as from the hosts file.
- **Log:** `/var/lib/lumen/guard/lookups.log`, one line per lookup,
  `<unix seconds> <type> <name> <verdict>` with verdict `allow` or
  `would-block` (`block` in G2), moved to `lookups.log.1` at 4 MB
  (`--max-log-bytes`), so two files at most. Names come off the wire in
  lower case, any octet but a letter, digit, `-` or `_` written `\DDD`, so
  a line can't be forged; a query whose question can't be read is logged
  as `- -`. Names only, no addresses or answers: every asker is on
  loopback, so Guard can't tell which program asked. The hook's heartbeat
  adds one `heartbeat.lumen.invalid` lookup a minute, which `tools/guard.sh
  log` leaves out of its summary. A line that can't be written
  (a full disk) is dropped and counted. It never leaves the TV unless
  fetched over SSH (`tools/guard.sh log`, which also summarises it). Started
  at every boot, it outlives a power-off, which is what the capture lacked.
  tcpdump stays for addresses used without a lookup.
- **Counts for the shell:** `/var/lib/lumen/guard/status`, plain `key value`
  lines rewritten whole every 10 s and when SIGTERM ends Guard: lookups and
  would-blocks since start and since local midnight, the start time,
  upstream, mode, DSCP and error counts. For `tools/guard.sh status` and a
  Privacy panel that says what was blocked today (G5, Phase 4's Control
  Center).
- **Started** by `profiles/privacy/lumen-guard` in
  `/var/lib/webosbrew/init.d`. It does nothing while the kill switch exists,
  returns at once and logs to `/tmp/lumen-guard.log`. In the background it
  finds the upstream (connman's newest `Nameservers=` line in
  `/var/lib/connman/*/settings`, else an address the owner wrote to
  `/var/lib/lumen/guard/upstream`, never loopback). It starts Guard, keeps
  its pid in `/var/lib/lumen/guard/lumen-guard.pid`, and waits up to about
  five minutes for `lumen-guard --probe 127.0.0.2:53` to get an answer.
  Guard sends nothing back when the router is silent, so an answer proves
  Guard and the router behind it. Then the hook:
  1. writes `/var/lib/lumen/guard/resolv.conf`: `nameserver 127.0.0.2`,
     then connman's `nameserver 127.0.0.1` (P11), so a program that kept a
     copy still resolves through connman once nothing answers on 127.0.0.2.
     A lookup Guard leaves unanswered for 5 s also goes on to connman,
     unlogged;
  2. bind-mounts that file over `/var/lib/misc/resolv.conf`;
  3. checks that `/etc/resolv.conf` now names 127.0.0.2, makes the mount
     read-only if the kernel lets it (so connman's in-place rewrite of its
     file fails instead of landing in Guard's), and checks that
     `lumen-guard --probe 127.0.0.2:53 --name us.lgtvsdp.com` answers;
  4. adds the two rules.
- **Fails open:** if any of those steps fails, the hook unmounts every copy
  of the mount, removes the rules and stops Guard, in that order, so
  lookups go through connman to the router as before. Once the steps pass,
  the shell stays as Guard's watchdog:
  - every 5 s it checks that Guard is alive, the kill switch is absent, the
    mount is still there and `/etc/resolv.conf` still names 127.0.0.2,
    rewriting Guard's file if connman wrote its own over it;
  - every 60 s it asks Guard for `heartbeat.lumen.invalid` (2 s at most):
    the router answers NXDOMAIN, an answer all the same.
  It fails open the same way, and logs why, when Guard exits, when the kill
  switch appears, when the mount is gone, or when three heartbeats in a row
  get no answer. Guard
  isn't restarted. A reboot clears the mount and the rules anyway (P3).
  That is how the hook keeps safety rules 4 and 5: the start-up is
  time-boxed, and the watchdog afterwards only sleeps, reads `/proc` and
  probes, and it acts on the kill switch within 5 s rather than at the
  next boot.
- **By hand first** (rule 3): `tools/guard.sh push` puts the binary, the
  rules and an inert copy of the hook in `/var/lib/lumen/guard/`.
  `tools/tv.sh script profiles/privacy/guard-apply.sh` runs that copy and
  reports how the start went, with `/etc/resolv.conf` and the mount.
  `guard-undo.sh` (or a reboot) unmounts, removes the rules and stops Guard,
  then prints connman's file. After that, `tools/guard.sh install` adds the
  hook and `remove` takes it away. `tools/uninstall-all.sh` runs
  `guard-undo.sh` before it deletes `/var/lib/lumen`.
- **Small surface:** about 1,400 lines of Rust besides its tests: `std`, and
  the `libc` crate in one module (`sys.rs`: the DSCP mark, `poll`, SIGTERM,
  the local date and the `getauxval` shim), with `unsafe` denied everywhere
  else. Two threads, UDP and TCP; under 4 MB resident was the target, and
  the B9 measured 940 kB on 2026-10-31.

| | Glasshouse hosts | dnsmasq, static | Lumen Guard |
|---|---|---|---|
| Suffix rules | no | yes | yes |
| Record of blocked names | no | in syslog | its own log |
| Catches programs with their own resolver | no | with the same nat rule | with the nat rule |
| Encrypted upstream | no | no | yes (G3) |
| Counts for the shell | no | by parsing logs | yes |
| Reads `allowlist.txt` as it is | through a converter | through a converter | yes |
| New code running as root | none | none, but another C parser of network input | about 1,400 lines of Rust |

## Stages

Each is undone by a reboot until it becomes a boot hook, and each ends with
`tools/store-safe-check.sh` and the manual checklist in
[aggressive.md](../../profiles/glasshouse/aggressive.md).

| Stage | Delivers | Done when |
|---|---|---|
| S0 | `tools/recon.sh --privacy`, the read-only survey | done: two runs on 2026-10-28 (above) |
| S1 | Quick fixes. Owner, in LG Settings: IPv6 off. By hand over SSH, undone by a reboot: `initctl stop` for `uploadd`, `rdxd`, `crashreportd` and `remotelogger`; an `INPUT` rule rejecting tcp 11111 from the LAN; P9 and P10's answers. Then one boot hook for whatever passed. `nsu.lge.com` waits for Guard (G2) | each checked; the checklist passes, also after a cold boot with the hook. Applied by hand 2026-10-28 06:30 UTC with `profiles/privacy/s1-apply.sh` (undo: `s1-undo.sh` or a reboot): the four jobs stayed stopped, tcp 11111 no longer answers from the LAN, DIAL still does; the owner's checklist passed the same day. A full backup (`tools/backup.sh`) was taken at 06:52 UTC, before any boot hook. The boot hook (`profiles/privacy/lumen-privacy`, SHA-256 `35747be3…`) was installed with `tools/privacy-hook.sh install` at 07:00 UTC, the first persistent change. After a reboot at 07:03:49 UTC it added the rule and stopped all four jobs at 07:04:23, about 37 s into the boot; tcp 11111 again didn't answer from the LAN, and DIAL did. The owner's quick check came after the reboot of 2026-10-31 16:10 UTC, with S1's and Guard's hooks both running: Netflix, YouTube and casting all work |
| G1 | Guard, log only: forwards everything | a week of names across power-offs, summarised like the capture. Built 2026-10-30 (`shell/crates/lumen-guard`, `profiles/privacy/lumen-guard`, `tools/guard.sh`). The redirect design was tried by hand on the B9 on 2026-10-31 and failed open as designed. The hook was rebuilt around `resolv.conf`, a design proven by hand the same day. The new hook passed its by-hand run on 2026-10-31 15:25 to 15:54 UTC: one read-only mount, both rules, 940 kB resident, Netflix's own resolver (8.8.8.8) caught by the rules, the kill switch failing open in 5 s, a Wi-Fi drop survived, a hung Guard failing open in 2 min 51 s, nothing left after `guard-undo.sh`. P11 decided and tried by hand the same day; the hook now names connman second. **Installed** after a backup (`Lumen-backups/20261031T160857Z`): `tools/guard.sh install` at 16:10 UTC (hook SHA-256 `75d2ae6d…`), then a reboot over SSH at 16:10:51. The hook started 37 s into the boot and Guard was answering a second later: both nameserver lines, the read-only mount, both rules, 104 lookups in its first 90 s with no upstream error or timeout; S1's hook ran alongside as before. After that boot the owner checked Netflix, YouTube and casting: all work. The week of log runs from 2026-10-31 16:11 UTC |
| G2 | Guard blocks: required hosts and the confirmed list | the store-safe check and the checklist pass, also after a cold boot |
| G3 | Encrypted upstream (P4) | lookups leave the TV only to the chosen resolver |
| G4 | Senders stopped, one job per check | each job's effect is recorded; the checklist passes |
| G5 | Privacy panel in the shell | the owner sees today's blocked names and counts on the TV |

G1 replaces the Stage C capture of the runbook; G2 replaces Stages C7–D.

## Decisions for the owner

| | Question | Recommendation |
|---|---|---|
| P1 | Run the survey | done 2026-10-28 |
| P2 | Lumen Guard rather than Glasshouse's hosts blocker | yes, G1 first; decide on Glasshouse after a week of G1's log |
| P3 | Fail open (blocks lapse while Guard is down) or closed (lookups stop) | open: the store and the family's apps keep working |
| P4 | Upstream: the router as today, or DNS-over-TLS to a resolver the owner picks. The ISP stops seeing names; the resolver sees them instead | the owner's call |
| P5 | Try blocking Netflix's logging names | not by default; one trial with the checklist, if wanted |
| P6 | IPv6: privacy addresses, or off | off: done by the owner in LG's settings, 2026-10-28 |
| P7 | `lgtvonline.lge.com`, connman's online check | keep allowed until G1 shows how often it's fetched; then a trial block with the checklist |
| P8 | Run the second survey (listening programs, reporting jobs' start-up) | done 2026-10-28 |
| P9 | Does anyone at home use AirPlay or HomeKit (an iPhone, iPad or Mac)? | if not, reject tcp 7000 and 35254 from the LAN |
| P10 | Ports 3000 and 3001: does the family use LG ThinQ, a phone remote app or Web Video Caster? | if not, reject them; if so, keep them open, since the router hands out addresses and a rule per phone would need fixed ones |
| P11 | After a fail-open, an app launched while Guard ran keeps asking 127.0.0.2 (its jail's copy of `resolv.conf`) and can't resolve until it is launched again. Accept that, or have Guard's file name connman's proxy second (`nameserver 127.0.0.2`, then `nameserver 127.0.0.1`), so glibc moves on to connman when nothing listens on 127.0.0.2 | decided 2026-10-31: name connman second. Tried by hand first: with both lines mounted and nothing on 127.0.0.2, lookups by new processes took 0.10 and 0.11 s against 0.05 and 0.08 s with connman alone (b9.md, "Resolver") |
