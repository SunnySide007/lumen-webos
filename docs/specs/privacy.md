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
| 3 | Names | telemetry, ads, AI, ThinQ, with suffix rules | Lumen Guard, below | proposed |
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
workspace and built with the same armv7 toolchain.

- **Loopback only:** it listens on 127.0.0.2:53, UDP and TCP.
- **Every lookup reaches it:** one nat rule sends every UDP and TCP packet
  to port 53 that isn't for loopback to 127.0.0.2:53. That covers connman's
  forwards and any program that asks a public resolver itself. Guard marks
  its own upstream packets with a DSCP value the rule skips, since there is
  no owner match. To be proved by hand first: DNAT to loopback for the
  TV's own packets.
- **Rules:** `allowlist.txt`, `required-blocks.txt` and the block list, with
  the suffix and `=exact` rules `store-safe-check.sh` and `summarize.py`
  already use. The allowlist wins. A blocked name gets 0.0.0.0 and ::, as
  from the hosts file.
- **Log:** one line per lookup (time, name, type, verdict) under
  `/var/lib/lumen/guard/`, two files of at most 4 MB. Names only: after
  connman's proxy Guard can't tell which program asked. It never leaves the
  TV unless fetched over SSH. Started at every boot, it outlives a
  power-off, which is what the capture lacked. tcpdump stays for addresses
  used without a lookup.
- **Fails open:** the boot hook's loop adds the nat rule once Guard
  answers and deletes it when Guard exits, so lookups go straight to the
  router as today. A reboot clears every rule anyway (P3).
- **Started** by a boot hook in `/var/lib/webosbrew/init.d` that checks the
  kill switch, runs in the background and is time-boxed (safety rules 4 and
  5), installed only after it has run by hand (rule 3).
- **Counts for the shell:** a small status file, so a Privacy panel can say
  what was blocked today (Phase 4's Control Center).
- **Small surface:** it reads only a query's question and passes answers
  through untouched; no recursion and no cache to start with.

| | Glasshouse hosts | dnsmasq, static | Lumen Guard |
|---|---|---|---|
| Suffix rules | no | yes | yes |
| Record of blocked names | no | in syslog | its own log |
| Catches programs with their own resolver | no | with the same nat rule | with the nat rule |
| Encrypted upstream | no | no | yes (G3) |
| Counts for the shell | no | by parsing logs | yes |
| Reads `allowlist.txt` as it is | through a converter | through a converter | yes |
| New code running as root | none | none, but another C parser of network input | about 1,000 lines of Rust |

## Stages

Each is undone by a reboot until it becomes a boot hook, and each ends with
`tools/store-safe-check.sh` and the manual checklist in
[aggressive.md](../../profiles/glasshouse/aggressive.md).

| Stage | Delivers | Done when |
|---|---|---|
| S0 | `tools/recon.sh --privacy`, the read-only survey | done: two runs on 2026-10-28 (above) |
| S1 | Quick fixes. Owner, in LG Settings: IPv6 off. By hand over SSH, undone by a reboot: `initctl stop` for `uploadd`, `rdxd`, `crashreportd` and `remotelogger`; an `INPUT` rule rejecting tcp 11111 from the LAN; P9 and P10's answers. Then one boot hook for whatever passed. `nsu.lge.com` waits for Guard (G2) | each checked; the checklist passes, also after a cold boot with the hook. Applied by hand 2026-10-28 06:30 UTC with `profiles/privacy/s1-apply.sh` (undo: `s1-undo.sh` or a reboot): the four jobs stayed stopped, tcp 11111 no longer answers from the LAN, DIAL still does; the owner's checklist passed the same day. A full backup (`tools/backup.sh`) was taken at 06:52 UTC, before any boot hook. The boot hook (`profiles/privacy/lumen-privacy`, SHA-256 `35747be3…`) was installed with `tools/privacy-hook.sh install` at 07:00 UTC, the first persistent change. After a reboot at 07:03:49 UTC it added the rule and stopped all four jobs at 07:04:23, about 37 s into the boot; tcp 11111 again didn't answer from the LAN, and DIAL did. Still owed: the owner's quick check (Netflix, casting) after that boot |
| G1 | Guard, log only: forwards everything | a week of names across power-offs, summarised like the capture |
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
