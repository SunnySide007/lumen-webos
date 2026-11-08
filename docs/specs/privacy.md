# Privacy: a quiet B9 (proposed)

Proposed on 2026-10-27, for the owner's approval before anything is built
beyond the read-only survey. It keeps decision 5 (aggressive, store-safe) and
its costs (voice search, ThinQ, LG Channels), and changes how Phase 1 gets
there: a lookup log that survives the TV being turned off replaces the 24 h
capture, and the work goes below `/etc/hosts`, to the senders, the addresses
and the home network.

**Changed on 2026-11-05** (decisions 11 to 13, from research.md's
"Research of 2026-11-05"):

* G2 blocks **by default**: only allowlisted names resolve, with suffix
  carve-outs, a denied list in the Privacy panel, and learning windows the
  owner starts from the phone or the PC.
* G3's upstream is **Quad9** over DNS over TLS (P4), with a logged fallback
  in clear only while the clock is wrong or Quad9 is down.
* Ports 3000/3001, DLNA, AirPlay and HomeKit close (P9, P10).
* S1 grows by ACR's consent flags and `acr2`, and UEI's network scanner.
* Glasshouse is never installed (P2).
* The Privacy panel is read-only, and only the owner's devices loosen
  anything.

The sections below are updated where they state these.

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
| `aic.cdpbeacon2.lgtvcommon.com` | a usage beacon | hagezi's LG list since 2026-10-30 (by its whole domain); not on the others |
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
| tcp 8443 (Lumen, R4) | `lumen-remote`, started by hand (`tools/remote.sh web start`), as uid 61101 in an empty chroot | the owner's paired phones: HTTPS with Lumen's own certificate authority (remote.md, D4) | an `INPUT` rule rejects it from outside the home network; never forwarded on the router; not running until started, and gone after a reboot |
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
| 3 | Names | everything not allowlisted (decision 11), with suffix carve-outs | Lumen Guard, below | G1 (log only) built; rebuilt around `resolv.conf` after its first by-hand run (2026-10-31); the new hook passed by hand the same day and runs as a boot hook since 16:10 UTC. G2 (default deny) built 2026-11-08, off until tried on the TV and the owner's yes |
| 4 | Senders | LG jobs that report | stop them at run time, one per check, from the list above; never `tvdataexchanger` or `alwaysready` | measured; four proposed for S1 |
| 5 | Addresses | hard-coded endpoints, public resolvers | iptables `OUTPUT` rules; LG-owned addresses only, never AWS or CDN ranges (the store is on AWS: five us-west-2 addresses) | possible: netfilter measured. Decided: one reject for LG's 156.147.0.0/16 (`LG-NET-KR`) once a capture shows the store doesn't need it |
| 6 | Identifiers | the MAC inside the TV's IPv6 address | IPv6 off in LG's settings | done 2026-10-28: `ipv6Enable` false, no IPv6 address on `wlan0` |
| 7 | The home network | other devices reaching the TV's services | iptables `INPUT` rules for ports nobody uses; SSH by key only | port 11111 closed by S1. Decided: 3000/3001, DLNA (1103, 1537, 1551, 1830, 1833, 1973), AirPlay 7000 and HomeKit, one at a time with a casting check; `--syn` for ports inside the ephemeral range |
| 8 | The ISP | lookups readable on the wire | Guard forwards to Quad9 over DNS over TLS (G3) | decided (P4). G3 built 2026-11-08 (lane K), off until tried on the TV and the owner's yes. Until then, the Nest's DNS points at Quad9 for the whole house |

Layer 7 matters more on a rooted set. Updates stay blocked (safety rule 8),
so the TV never gets LG's security fixes. In October 2026 a chain giving
root from the LAN without a PIN was
[published for webOS 7.5](https://thehackernews.com/2026/10/lg-smart-tvs-vulnerable-to-pre-auth.html),
through port 3001, which listens on this TV too; whether webOS 4.5 is
affected is not known. Older bugs sit on the same ports: Bitdefender
confirmed a pairing bypass and a root command injection there on a 2019
webOS 4.9.7 set in 2024 (CVE-2023-6317, CVE-2023-6319), and no fixed
webOS 4.5 firmware has been seen. A TV that can't be patched should offer
the LAN as little as possible.

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
            [--mode log|block] [--mode-file FILE] [--owner FILE] [--learn FILE]
            [--dscp N] [--max-log-bytes N]
            [--tls on|off] [--tls-file FILE] [--tls-server ADDR:PORT]...
            [--tls-name NAME] [--tls-ca FILE] [--clock-floor UNIX]
lumen-guard --probe ADDR:PORT [--name NAME]
```

**G3 was built on 2026-11-08** (lane K) and is off on the TV until the
owner's yes: Guard forwards to Quad9 over DNS over TLS, with a logged
fallback to the router in clear. The "Upstream" bullet below says how; its
trial plan is under "G3 on the TV".

**G2 was built on 2026-11-08** (lane H) and is off on the TV until the
owner's yes: blocking by default, the rules in `profiles/guard/`, the
owner's own file, learning windows, and the switch, all below. Its trial
plan is under "G2 on the TV".

- **Loopback only:** it listens on 127.0.0.2:53, UDP and TCP. TCP takes one
  query per connection (RFC 1035's two-byte length), 3 s to send it, at
  most 32 connections at once.
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
  (compression pointers followed, loops refused), type and class. Each UDP
  query it forwards goes upstream unchanged from a socket of its own, and
  the first answer with its ID comes back unchanged; messages up to 4,096
  bytes, TCP answers streamed through in pieces of that size. No cache, no
  recursion. With no answer within 3 s nothing is sent back, and the asker
  retries.
- **Upstream (G3, P4):** with the owner's TLS switch off, as above: the
  router, in clear. With it on, what Guard forwards goes to Quad9 over DNS
  over TLS (RFC 7858): `9.9.9.9:853`, then `149.112.112.112:853`, checked as
  `dns.quad9.net` against Mozilla's roots built into the binary
  (webpki-roots), TLS 1.3 only, with rustls and ring on plain blocking
  sockets (`dot.rs`). Three worker threads take the lookups from a queue of
  at most 64; each keeps one connection open and reuses it, one query at a
  time, and closes it after 15 s unused, so at most three connections are
  open. TLS 1.3 session tickets make a new connection after the first a
  resumption. A UDP asker gets a TLS answer longer than it can take (512
  bytes, or its EDNS size) truncated, so it asks again over TCP. A query
  goes to a server only once its certificate has passed.
  - **The fallback**, in clear through the router, the same way G2
    forwards, in two cases only. While the TV's clock reads before
    2026-11-01 (`--clock-floor`; the B9 boots at 2019-01-01 until LG's
    time server answers), at once, without trying TLS, so the boot's first
    lookups, the time server's among them, never wait on it; a certificate
    refused as expired or not yet valid counts the same. And when Quad9
    can't be reached or won't complete a checked handshake within 1.5 s:
    that lookup goes to the router at once, the next tries the other
    address, and once both have failed, lookups go to the router for 10 s,
    doubling after each further failure up to 5 min, then TLS is tried
    again.
  - **Logged and shown:** Guard's log (the hook's,
    `/var/lib/lumen/log/lumen-guard.log`) has a line for every failure
    (`tls: …; this lookup goes in clear through the router`), for the start
    of each fallback (`FALLBACK: …`, with the reason) and for its end
    (`upstream tls again after N s of fallback, in which M lookups went in
    clear`). The status file adds `tls` (the switch), `route` (`tls`,
    `clear`, `fallback-clock` or `fallback-down`), `tls_server`,
    `tls_name`, `tls_open`, `tls_answers`, `tls_handshakes`, `tls_errors`,
    `fallback`, `fallback_today` (lookups sent in clear while the switch was
    on) and `fallback_since`.
  - **Only allowed names leave in clear** in block mode, since nothing else
    is forwarded at all (decision 11). Log mode forwards everything, so its
    fallback does too: log mode keeps G1's promise that nothing it does
    changes what resolves.
  - **The switch** (decision 12): `/var/lib/lumen/guard/tls`, read as the
    mode file is: `tls` (`guard.sh tls on`, across reboots, the family's
    yes), `tls <boot id>` (`guard.sh tls trial`, until the next boot), no
    file or `clear` (`guard.sh tls off`, the undo, within a second).
  - **Not covered:** what reaches connman rather than Guard (above: its own
    lookups, its online check, and programs started before the mount)
    still goes to the router in clear, unlogged, as in G1. No EDNS padding
    (RFC 8467) is added, since Guard passes queries through unchanged.
  - **Tested** in `cargo test` against a local rustls server with a test
    certificate (`tests/fixtures/dot`), never the real Quad9: answers over
    TLS and connection reuse, the truncated UDP answer and its TCP retry,
    block mode keeping blocked names off TLS, the fallback for the clock and
    for a server that is down, no query sent to a server whose certificate
    has the wrong name or an unknown issuer, the switch file, and the
    memory below. By hand from the PC on 2026-11-08: four lookups through
    the real Quad9 (`dns.quad9.net`, Mozilla's roots) answered in 21 to
    75 ms, with no fallback.
- **Rules (G2, decision 11):** a name resolves only when a rule allows it.
  [profiles/guard/](../../profiles/guard/README.md) holds `allowlist.txt`
  (the generic list: the store, DRM, the paid apps, casting and Lumen's
  probes, each with its reason; moved from `profiles/glasshouse`, with G1's
  measured names added), `carve-outs.txt` (Lumen's names under an allowed
  suffix that stay blocked), `imported/` (hagezi's LG list, GPL-3.0;
  Perflyst's SmartTV list, MIT; Glasshouse's ad and telemetry names, MIT;
  each in its own file with source, commit and license) and
  `required-blocks.txt` (the firmware hosts, now with `snu-dev.lge.com`).
  The owner's own file (decision 12) is not in the repository. The
  firmware hosts are blocked whatever; then the owner's rules decide; then
  the most specific rule of the lists (an exact rule over a suffix, a
  longer suffix over a shorter one, an allow over a block of the same
  rule); a name no rule covers is blocked. `b9-aggressive.hosts` is no
  longer read: its candidates are blocked by default.
- **Blocked names** get Guard's own answer, as from a hosts file: 0.0.0.0
  for A, `::` for AAAA, no record (NOERROR) for any other type, with a TTL
  of 60 s, so a name the owner allows resolves within a minute. Nothing
  goes upstream for them. In block mode only a standard query with one
  readable question goes upstream at all: another opcode gets REFUSED, and
  two questions or an unreadable one FORMERR. In log mode everything goes
  upstream, as in G1.
- **Never the store:** Guard refuses to block while its rules, with the
  owner's file, would block a name it must resolve: the store
  (`us.lgtvsdp.com`, `lgtvsdp.com`, `us.lgeapi.com`), DRM
  (`www.googleapis.com`), Netflix, Prime Video, Plex, YouTube and Spotify
  as `store-safe-check.sh` asks for them, and the hook's
  `heartbeat.lumen.invalid` (`MUST_RESOLVE` in `rules.rs`). It keeps to
  logging and says why (`block_refused` in its status), and an owner file
  that would do it is refused whole, the earlier one kept.
- **The owner's controls** (decision 12): three files in
  `/var/lib/lumen/guard`, read within a second of a change, and only when
  root owns them and no one else can write them, so nothing but the
  owner's SSH key loosens the TV. `tools/guard.sh` writes them.
  - `mode`: `block` blocks across reboots (`guard.sh block on`, the
    family's yes); `block <boot id>` blocks until the next boot only
    (`guard.sh block trial`, so a reboot undoes it); no file, or `log`, only
    logs (`guard.sh block off`, the undo, within a second).
  - `owner.txt`: the owner's rules, the allowlist's syntax plus `!name` and
    `!=name` to block. Kept in `private/guard/owner.txt` on the PC, which
    git ignores, and in the owner's backups with the rest of `/var`.
  - `learn`: a learning window (decision 11), `<start> <end>` by the TV's
    clock, an hour at most, one at a time. While it is open, names no rule
    covers resolve and are logged as `learn`; carve-outs, the imported
    lists and the firmware hosts stay blocked. A window from before a reboot
    or a clock jump is inert by its times. `guard.sh learn 15` opens one,
    `learned` lists what it learned, and `keep NAME` (into the owner's file,
    pushed and checked) or `drop NAME` decide. At most 512 names a window.
- **Log:** `/var/lib/lumen/guard/lookups.log`, one line per lookup,
  `<unix seconds> <type> <name> <verdict>` with verdict `allow`, `learn`,
  `block` in block mode, or `would-block` in log mode (the rules say no, and
  the lookup goes upstream all the same), moved to `lookups.log.1` at 4 MB
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
  lines rewritten whole every 10 s and when SIGTERM ends Guard: lookups,
  would-blocks, blocks and learned lookups since start and since local
  midnight, the mode and why blocking was refused, the open window's end and
  the names it learned, the owner's rule count, the start time, upstream,
  DSCP and error counts. For `tools/guard.sh status` and a Privacy panel
  that says what was blocked today (G5, Phase 4's Control Center).
- **Started** by `profiles/privacy/lumen-guard` in
  `/var/lib/webosbrew/init.d`. It does nothing while the kill switch exists,
  returns at once and logs to `/var/lib/lumen/log/lumen-guard.log`, keeping
  the run before as `lumen-guard.log.1`; never to `/tmp`, which the app jails
  share. When that folder or `/var/lib/lumen` is a link, or writable or owned
  by anyone but root, the hook starts Guard all the same, unlogged, and
  `tools/guard.sh status` says "no log". In the background it
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
- **Small surface:** about 3,500 lines of Rust besides its tests, doc
  comments included (G1 was about 1,400, G2 about 2,500): `std`, and the
  `libc` crate in one module (`sys.rs`: the DSCP mark, `poll`, SIGTERM, the
  local date, the effective uid and the `getauxval` shim), with `unsafe`
  denied everywhere else; for G3, rustls (with ring, rustls-webpki,
  rustls-pki-types, untrusted, subtle, zeroize, getrandom, once_cell and
  cfg-if under it) and webpki-roots in one module (`dot.rs`), no async
  runtime. rustls and ring are the same versions and features as
  `lumen-remote`'s (R4), one copy of each in `Cargo.lock`. Five threads: UDP, TCP and three TLS workers. Under 4 MB
  resident was the target, and the B9 measured 940 kB for G1 on
  2026-10-31. G2's rules add about 1,000 names in hash tables; G2's and
  G3's memory on the TV are not measured yet.
- **Size, measured 2026-11-08** (not on the TV): the armv7 binary
  (`tools/dev.sh make guard`, stripped) is 1,297,584 bytes, against
  453,800 for G2; rustls, ring and Mozilla's roots are the difference. It
  needs nothing newer than glibc 2.12, and ring's `getrandom` is a weak
  symbol, so on the B9's glibc 2.24 it falls back to the system call. On
  the PC (x86_64, release builds, the shipped rules, 100 lookups): G2
  2,536 kB resident; G3 with TLS off 3,412 kB, nearly all of it pages of
  the binary and the C library, with 96 kB its own (anonymous). With TLS on
  (`memory_with_tls_stays_small`, 200 lookups in bursts over three
  connections): 3,844 kB resident, 332 kB of it anonymous, 52 kB more than
  at start with one connection. The B9's figure comes from the trial.
- **The same rules on the PC:** `tools/guard/rules.py` decides as
  `rules.rs` does (`tools/tests/test_guard_rules.py` holds both to the
  cases in `tools/tests/fixtures/guard-g2`). `tools/guard.sh log` shows
  each logged name with what these rules decide now and which rule, and
  `tools/store-safe-check.sh --stage 5` checks the rules, with the owner's
  file, before anything changes on the TV, and then Guard's live answers.

| | Glasshouse hosts | dnsmasq, static | Lumen Guard |
|---|---|---|---|
| Suffix rules | no | yes | yes |
| Record of blocked names | no | in syslog | its own log |
| Catches programs with their own resolver | no | with the same nat rule | with the nat rule |
| Encrypted upstream | no | no | yes (G3), with a logged fallback |
| Counts for the shell | no | by parsing logs | yes |
| Reads `allowlist.txt` as it is | through a converter | through a converter | yes |
| New code running as root | none | none, but another C parser of network input | about 3,500 lines of Rust, and rustls with ring |

### G2 on the TV

Built and tested off the TV on 2026-11-08 (lane H): `cargo test` (the
rules, the owner's files, block mode end to end over UDP and TCP on
loopback), `tools/tests/test_guard_rules.py`, `guard.bats` and
`store-safe-check.bats`. Nothing of it has run on the TV. What G1's week
says the shipped rules do with its 94 names: 67 allowed (the store,
Netflix with its logging, Prime Video's API, YouTube), 27 blocked, 1,588
of its 8,211 lookups (LG Home's beacon, content and promotion calls, LG's
ad and AI names, UEI, Amazon's device metrics and Prime Video's and
YouTube's telemetry, doubleclick, Apple's media CDN, and reverse lookups).
`tools/guard.sh log` shows that table for any log.

The trial, in the order it runs, each step with its undo (safety.md's
tiers):

1. **PC only.** `tools/dev.sh make guard`, then
   `tools/store-safe-check.sh --hosts-file /dev/null --stage 5` passes.
2. **Tier A.** `tools/backup.sh`.
3. **Tier B, lab window.** `tools/guard.sh push`: the new binary, the
   rules folder and the hook's inert copy. The running Guard is untouched.
   Undo: push from a checkout without G2.
4. **Tier B.** Restart Guard on it in log mode: `guard-undo.sh`, then
   `guard-apply.sh`; `tools/guard.sh status` shows `mode log` and
   `block_refused none`. The installed G1 hook also starts the new binary
   after a reboot, in log mode (its flags are a subset). Undo:
   `guard-undo.sh`, or a reboot.
5. **Tier A, days.** The log-mode preview: the family uses the TV as
   usual, including Prime Video and Plex playback, Spotify and casting,
   which G1's week did not show. `tools/guard.sh log` then lists every
   name the rules would block; any a paid app needs goes into
   `allowlist.txt` (with its reason) before step 6.
6. **Tier B, lab window, with the undo armed.** `tools/guard.sh block
   trial`: blocking until the next boot. `tools/store-safe-check.sh
   --stage 5` live, and the Netflix, YouTube, Plex and casting checks. The
   run ends with `tools/guard.sh block off`, and a real remote press
   yields to it. Undo: `block off` (within a second), or a reboot.
7. **Tier B, same window.** The learning window: `tools/guard.sh learn 5`,
   look up a name no rule covers (`example.com`), `learned` lists it,
   `keep example.com` makes it resolve, `drop example.com` blocks it again.
8. **Tier B.** After a backup, `tools/guard.sh install` (the hook with
   the owner's files), a reboot over SSH, then a cold boot: the hook
   starts Guard with its control files, the trial's mode file is spent,
   and step 6's checks pass again under `block trial`. Guard's resident
   memory is measured. Undo: `tools/guard.sh remove`, then install from a
   checkout without G2.
9. **Tier C: the owner's one yes.** `tools/guard.sh block on`, with steps
   5 to 8's results in front of the owner. Undo: `tools/guard.sh block off`.

Not built in G2 yet: the `OUTPUT` reject for 156.147.0.0/16, which waits
for a capture that shows the store doesn't need it.

### G3 on the TV

Built and tested off the TV on 2026-11-08 (lane K, "Upstream" above).
Nothing of it has run on the TV. It runs after G2's trial: G3's binary is
G2's plus TLS, and with no TLS file it forwards as G2 does. Each step with
its undo; the TV steps are tier B, in the night window (nothing lights the
panel), and the family's switch is tier C.

1. **PC only.** `tools/dev.sh make check`, then `tools/dev.sh make guard`
   (`dist/guard/`, with webpki-roots' licence beside the binary).
2. **Tier A.** `tools/backup.sh`.
3. **Tier B.** `tools/guard.sh push`: the G3 binary, the rules and the
   hook's inert copy (which now passes `--tls-file`). The running Guard is
   untouched. Undo: push from a checkout without G3.
4. **Tier B.** Restart Guard on it: `guard-undo.sh`, then `guard-apply.sh`.
   `tools/guard.sh status`: `TLS file: none`, `tls off`, `route clear`,
   and the mode G2's trial left. Guard's `VmRSS` (`/proc/<pid>/status`).
   Undo: `guard-undo.sh`, or a reboot (the installed hook then starts the
   binary as before, without the TLS file).
5. **Tier B.** `tools/guard.sh tls trial` (TLS until the next boot). Within
   a second: `route tls`; then `tools/tv.sh run 'getent hosts
   us.lgtvsdp.com'` resolves, `tls_answers` grows, `fallback 0`, and the
   log says `upstream tls`. Time a lookup on the TV (`time
   /var/lib/lumen/guard/lumen-guard --probe 127.0.0.2:53 --name
   www.netflix.com`) against the same with TLS off. After 10 minutes of the
   TV's own lookups, `VmRSS` again, and `tls_open` at most 3. Undo:
   `tools/guard.sh tls off` (within a second), or a reboot.
6. **Tier B, same window.** Quad9 unreachable: `tools/tv.sh run 'iptables
   -I OUTPUT -p tcp --dport 853 -j REJECT'`. A lookup still resolves
   (through the router), `route fallback-down`, the log has `tls: connect
   9.9.9.9:853`, then `149.112.112.112:853`, then `FALLBACK: … for 10 s`.
   Then `tools/tv.sh run 'iptables -D OUTPUT -p tcp --dport 853 -j
   REJECT'`: within the backoff, `route tls` and `upstream tls again after
   N s of fallback, in which M lookups went in clear`. Undo: the `-D`
   command, or a reboot.
7. **Tier B, same window.** The clock, without touching the TV's: a second
   Guard on a spare loopback port that believes the clock is wrong,
   `tools/tv.sh run 'd=/var/lib/lumen/guard/g3-clock; mkdir -p $d &&
   (/var/lib/lumen/guard/lumen-guard --listen 127.0.0.2:5354 --upstream
   <router>:53 --rules /var/lib/lumen/guard/rules --log $d --status $d/status
   --tls on --clock-floor 4102444800 2>$d/err & echo $! >$d/pid); sleep 1;
   /var/lib/lumen/guard/lumen-guard --probe 127.0.0.2:5354; kill $(cat
   $d/pid); sleep 1; cat $d/err; grep -E "^(route|fallback)" $d/status'`:
   an answer, `route fallback-clock`, and a `FALLBACK` line naming the
   clock. Undo: `tools/tv.sh run 'rm -r /var/lib/lumen/guard/g3-clock'`.
8. **Tier A, same window.** What leaves the TV: `tools/capture.sh start
   1`, the TV's lookups for a few minutes, `stop`, `fetch`, and
   `tools/capture/summarize.py`. Port 853 goes only to Quad9; port 53 goes
   to the router only from what reaches connman rather than Guard
   ("Upstream", not covered), and from Guard only during step 6's
   fallback. `tools/capture.sh clean` afterwards.
9. **End of the window.** `tools/guard.sh tls off` before 06:00, so the
   family's daytime runs as G2 until the owner's yes; a real remote press
   does the same when the run yields.
10. **Tier B, after a backup.** `tools/guard.sh install` (the G3 hook,
    passing `--tls-file`; with no TLS file it runs as G2), a reboot over
    SSH, then a cold boot: the hook starts Guard with the TLS file absent,
    `route clear`. Undo: `tools/guard.sh remove`, then install from a
    checkout without G3.
11. **Tier C: the owner's one yes.** `tools/guard.sh tls on`, with steps 4
    to 8's results (the lookup times, the memory, the fallback's log lines,
    the capture). After the next cold boot, the log shows the clock
    fallback (`FALLBACK: … the clock (1546…) reads before 1793491200`) and
    its end (`upstream tls again after N s …`) once LG's time server has
    answered; Netflix, YouTube, Plex and casting work. Undo: `tools/guard.sh
    tls off`.

## Stages

Each is undone by a reboot until it becomes a boot hook, and each ends with
`tools/store-safe-check.sh` and the manual checklist in
[aggressive.md](../../profiles/glasshouse/aggressive.md).

| Stage | Delivers | Done when |
|---|---|---|
| S0 | `tools/recon.sh --privacy`, the read-only survey | done: two runs on 2026-10-28 (above) |
| S1 | Quick fixes. Owner, in LG Settings: IPv6 off. By hand over SSH, undone by a reboot: `initctl stop` for `uploadd`, `rdxd`, `crashreportd` and `remotelogger`; an `INPUT` rule rejecting tcp 11111 from the LAN; P9 and P10's answers. Then one boot hook for whatever passed. `nsu.lge.com` waits for Guard (G2) | each checked; the checklist passes, also after a cold boot with the hook. Applied by hand 2026-10-28 06:30 UTC with `profiles/privacy/s1-apply.sh` (undo: `s1-undo.sh` or a reboot): the four jobs stayed stopped, tcp 11111 no longer answers from the LAN, DIAL still does; the owner's checklist passed the same day. A full backup (`tools/backup.sh`) was taken at 06:52 UTC, before any boot hook. The boot hook (`profiles/privacy/lumen-privacy`, SHA-256 `35747be3…`) was installed with `tools/privacy-hook.sh install` at 07:00 UTC, the first persistent change. After a reboot at 07:03:49 UTC it added the rule and stopped all four jobs at 07:04:23, about 37 s into the boot; tcp 11111 again didn't answer from the LAN, and DIAL did. The owner's quick check came after the reboot of 2026-10-31 16:10 UTC, with S1's and Guard's hooks both running: Netflix, YouTube and casting all work. **Reinstalled** with its log in `/var/lib/lumen/log` rather than `/tmp`, after a backup (`Lumen-backups/20261031T170258Z`): `tools/privacy-hook.sh install` at 17:05 UTC (SHA-256 `b1d9b4c8…`), staged in `/var/lib/lumen/stage` and checked there. After a reboot at 17:09:56 UTC it added the rule and stopped all four jobs at 17:10:29, logging only to `/var/lib/lumen/log/lumen-privacy.log` with the install run kept as `.log.1`; the owner's check passed again |
| G1 | Guard, log only: forwards everything | a week of names across power-offs, summarised like the capture. Built 2026-10-30 (`shell/crates/lumen-guard`, `profiles/privacy/lumen-guard`, `tools/guard.sh`). The redirect design was tried by hand on the B9 on 2026-10-31 and failed open as designed. The hook was rebuilt around `resolv.conf`, a design proven by hand the same day. The new hook passed its by-hand run on 2026-10-31 15:25 to 15:54 UTC: one read-only mount, both rules, 940 kB resident, Netflix's own resolver (8.8.8.8) caught by the rules, the kill switch failing open in 5 s, a Wi-Fi drop survived, a hung Guard failing open in 2 min 51 s, nothing left after `guard-undo.sh`. P11 decided and tried by hand the same day; the hook now names connman second. **Installed** after a backup (`Lumen-backups/20261031T160857Z`): `tools/guard.sh install` at 16:10 UTC (hook SHA-256 `75d2ae6d…`), then a reboot over SSH at 16:10:51. The hook started 37 s into the boot and Guard was answering a second later: both nameserver lines, the read-only mount, both rules, 104 lookups in its first 90 s with no upstream error or timeout; S1's hook ran alongside as before. After that boot the owner checked Netflix, YouTube and casting: all work. The week of log runs from 2026-10-31 16:11 UTC. **Reinstalled** with its log in `/var/lib/lumen/log` rather than `/tmp`, after the same backup as S1's (`20261031T170258Z`): `tools/guard.sh install` at 17:07 UTC (hook SHA-256 `3ca099ec…`, binary `bcb81e80…`); its one run found Guard running and did nothing. After a reboot at 17:09:56 UTC, Guard started from the new binary at 17:10:29 with both nameserver lines, the mount and both rules, and answered 43 lookups in its first minute with no upstream error or timeout. Its log is in a `drwx------` folder of root's, and nothing of Lumen's was left in `/tmp`. The owner's check passed again, and the week of log goes on (the lookup log was kept) |
| S2 | S1's additions (decision 11), each after a read-only look at its values: LG's ACR consent flags off (`acrOnAllowed`, `acrAdAllowed`, `additional1Allowed`) and `acr2` held down from boot; UEI's `iconnectivity` held down by a do-nothing stand-in mounted over it in RAM; the `INPUT` rules for 3000/3001, the DLNA ports, 7000 and HomeKit, one at a time | each checked with casting, Netflix and the store; then folded into `lumend` (decision 14) |
| G2 | Guard blocks by default (decision 11): only the allowlist resolves; suffix carve-outs from Lumen's measured names and the imported lists (hagezi, Perflyst, Glasshouse's names, each in its own file with its license); learning windows started from the owner's devices; the owner's own additions kept off the public repository; `OUTPUT` reject for 156.147.0.0/16 once a capture clears it | the store-safe check and the checklist pass, also after a cold boot; a learning window adds and drops a name. **Built 2026-11-08** off the TV (lane H): block mode, `profiles/guard/`, the owner's file, learning windows and the mode switch, `store-safe-check.sh --stage 5`; tested in `cargo test` and the tool tests, not yet on the TV ("G2 on the TV"). The address rule is not built |
| G3 | Encrypted upstream (P4): Quad9 over DNS over TLS (`dns.quad9.net`), rustls with ring and webpki-roots on a plain socket. Clear through the router only while the clock is implausible after a power cut or Quad9 is unreachable, logged and shown | lookups leave the TV only to Quad9, except the logged fallback. **Built 2026-11-08** off the TV (lane K): the TLS upstream, its fallback and the owner's switch (`tools/guard.sh tls`); tested in `cargo test` against a local TLS server and by hand against Quad9 from the PC; armv7 binary 1,297,584 bytes; not yet on the TV ("G3 on the TV"). What reaches connman rather than Guard still goes to the router in clear |
| G4 | Senders stopped, one job per check | each job's effect is recorded; the checklist passes |
| G5 | Privacy panel in the shell, read-only (decision 12): what is blocked, what was denied, each layer's state, the fallback if used | the owner sees today's blocked and denied names and counts on the TV, and nothing on the TV can loosen them |

G1 replaces the Stage C capture of the runbook; G2 replaces Stages C7–D.

## Decisions for the owner

| | Question | Recommendation |
|---|---|---|
| P1 | Run the survey | done 2026-10-28 |
| P2 | Lumen Guard rather than Glasshouse's hosts blocker | yes, G1 first. Decided 2026-11-05: Glasshouse is never installed (decision 13) |
| P3 | Fail open (blocks lapse while Guard is down) or closed (lookups stop) | open: the store and the family's apps keep working |
| P4 | Upstream: the router as today, or DNS-over-TLS to a resolver the owner picks. The ISP stops seeing names; the resolver sees them instead | decided 2026-11-05: Quad9 only, over DNS over TLS (decision 11). Mullvad's public DNS closed on 2026-11-02 and dns0.eu in 2025 |
| P5 | Try blocking Netflix's logging names | not by default; one trial with the checklist, if wanted |
| P6 | IPv6: privacy addresses, or off | off: done by the owner in LG's settings, 2026-10-28 |
| P7 | `lgtvonline.lge.com`, connman's online check | keep allowed until G1 shows how often it's fetched; then a trial block with the checklist |
| P8 | Run the second survey (listening programs, reporting jobs' start-up) | done 2026-10-28 |
| P9 | Does anyone at home use AirPlay or HomeKit (an iPhone, iPad or Mac)? | decided 2026-11-05: reject tcp 7000 and HomeKit's port from the LAN (its port, 35254 on 2026-10-28, is checked after a reboot) |
| P10 | Ports 3000 and 3001: does the family use LG ThinQ, a phone remote app or Web Video Caster? | decided 2026-11-05: reject both |
| P11 | After a fail-open, an app launched while Guard ran keeps asking 127.0.0.2 (its jail's copy of `resolv.conf`) and can't resolve until it is launched again. Accept that, or have Guard's file name connman's proxy second (`nameserver 127.0.0.2`, then `nameserver 127.0.0.1`), so glibc moves on to connman when nothing listens on 127.0.0.2 | decided 2026-10-31: name connman second. Tried by hand first: with both lines mounted and nothing on 127.0.0.2, lookups by new processes took 0.10 and 0.11 s against 0.05 and 0.08 s with connman alone (b9.md, "Resolver") |
