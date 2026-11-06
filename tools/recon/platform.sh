#!/bin/sh
# Read-only survey of the B9 for the design of 2026-11-05: the facts listed
# in docs/specs/depth.md, "Facts only the TV can answer". tools/recon.sh
# --platform pipes this to the TV over SSH and saves what it prints. What
# each section answers, by fact number:
#
#   lumen          1, 5, 9, 12: the shell's processes, its jail and
#                  namespaces, its libraries, and its files' modes and times
#   bus            1, 2, 4: the hub's view, LG's SDL's strings, role files
#   app-manager    7: keepAliveApps, SAM's configuration, Lumen's app info
#   screensaver    8: who owns the idle timer
#   memory         11: the memory manager, and whether it closed Lumen
#   acr            13: LG's consent flags and acr2
#   guard-log      14, 15: names of interest and reverse-lookup sweeps in
#                  Guard's log, as counts and times only; UEI's iconnectivity
#   lan-chain      16: the October 2026 chain's parts
#   ports          17, 18, 21, 22: who listens where, a USB drive, the
#                  ephemeral range
#   lg-addresses   19: 156.147.0.0/16 in the update program and in
#                  connections now
#   clock          20: the clock's source and the hardware clock
#
# Fact 3, 6, 10, and the trials in 7, 8 and 9, are TV steps with the owner,
# not this survey. Facts 1, 5 and 9 need the shell running: open Lumen first.
#
# Nothing here writes to the TV: it reads /proc, /sys, /etc, /usr and /var,
# lists upstart jobs, and runs two programs that could do more than read,
# each only one way:
#   luna-send     get* and list* methods only (luna() refuses anything else)
#   ls-monitor    only -l and -i SERVICE (lsmon() refuses anything else)
# A process's environment is read for four names only (APPID,
# WAYLAND_DISPLAY, XDG_RUNTIME_DIR, DISPLAY_ID). Guard's log comes out as
# counts and times, never the names a reverse lookup asked for. The clock's
# answers lose their time zone, city and country.
# It runs under the TV's busybox sh, so it sticks to POSIX shell.
#
# The variables below exist so tests/recon.bats can point the survey at
# fixtures. On the TV they are unset and the defaults are the real paths.

# Every time in UTC: a local time from ls or stat beside `date -u` gives the
# TV's time zone away.
TZ=UTC0
export TZ

# Older busybox (webOS 4) spells it `timeout -t SECS`, newer `timeout SECS`.
# KILL, not the default TERM, so no call that ignores TERM can hold the
# survey up.
if timeout -s KILL 1 true >/dev/null 2>&1; then
    T="timeout -s KILL 10"
elif timeout -t 1 -s KILL true >/dev/null 2>&1; then
    T="timeout -t 10 -s KILL"
else
    T=""
fi

APP_ID=${PLATFORM_SURVEY_APP_ID:-io.github.sunnyside007.lumen.shell}
APP_DIR=${PLATFORM_SURVEY_APP_DIR:-/media/developer/apps/usr/palm/applications/$APP_ID}
JAIL=${PLATFORM_SURVEY_JAIL:-/var/palm/jail/$APP_ID}
LUMEN_LOGS=${PLATFORM_SURVEY_LUMEN_LOGS:-/var/lib/lumen/log}
GUARD_LOG=${PLATFORM_SURVEY_GUARD_LOG:-/var/lib/lumen/guard/lookups.log}
LIB=${PLATFORM_SURVEY_LIB:-/usr/lib}
BINDIRS=${PLATFORM_SURVEY_BINDIRS:-"/usr/sbin /usr/bin /sbin /bin"}
LS2=${PLATFORM_SURVEY_LS2:-/var/luna-service2-dev}
LS2_LEGACY=${PLATFORM_SURVEY_LS2_LEGACY:-/var/palm/ls2-dev}
APPS=${PLATFORM_SURVEY_APPS:-/usr/palm/applications}

section() {
    printf '\n===== %s =====\n' "$1"
}

show() {
    printf '$ %s\n' "$*"
    "$@" 2>&1 || printf '(exit %s)\n' "$?"
}

showfile() {
    for f in "$@"; do
        printf -- '--- %s\n' "$f"
        if [ -r "$f" ]; then cat "$f"; else echo '(absent)'; fi
    done
}

# Prints what comes in, or "(none matched)" when nothing does.
or_none() {
    got=$(cat)
    printf '%s\n' "${got:-(none matched)}"
}

# The paths given that exist (unmatched globs drop out), or "(none matched)".
existing() {
    for f in "$@"; do
        if [ -e "$f" ] || [ -L "$f" ]; then echo "$f"; fi
    done | or_none
}

luna() {
    case $1 in
        */*) ;;
        *) echo "luna $1: refused, not a service and a method"; return ;;
    esac
    case ${1##*/} in
        get* | list*) ;;
        *) echo "luna $1: refused, not a get or list method"; return ;;
    esac
    printf '$ luna %s %s\n' "$1" "$2"
    # shellcheck disable=SC2086
    $T luna-send -n 1 -f "luna://$1" "$2" </dev/null 2>&1 || echo '(no answer)'
}

lsmon() {
    ok=""
    case "${1:-}" in
        -l) [ $# -eq 1 ] && ok=1 ;;
        -i)
            if [ $# -eq 2 ]; then
                case $2 in
                    '' | *[!A-Za-z0-9._-]*) ;;
                    *) ok=1 ;;
                esac
            fi
            ;;
    esac
    if [ -z "$ok" ]; then
        echo "ls-monitor $*: refused, only -l and -i SERVICE are allowed"
        return
    fi
    printf '$ ls-monitor %s\n' "$*"
    # shellcheck disable=SC2086
    $T ls-monitor "$@" </dev/null 2>&1 || echo '(no answer)'
}

# Of a clock answer, the lines about its source, the time and errors; never a
# time zone, city or country.
clock_lines() {
    grep -E '^\$ luna |^\(no answer\)|refused|[Ss]ource|"utc"|returnValue|[Ee]rror|[Vv]alid' \
        | grep -v -i -E 'zone|city|country|"tz"' | or_none
}

# The printable runs in a binary, one per line, which any grep can search.
# The range, not [:print:]: busybox's tr -c gets the class backwards.
bin_strings() {
    tr -c ' -~' '\n' < "$1"
}

# The first program of that name in BINDIRS, or nothing.
find_bin() {
    for dir in $BINDIRS; do
        if [ -f "$dir/$1" ]; then
            echo "$dir/$1"
            return
        fi
    done
}

# The pids whose program, or whose comm, has that name.
pids_of() {
    for d in /proc/[0-9]*; do
        # 2>/dev/null first, so a process that ended mid-scan says nothing.
        arg0=$(tr '\0' '\n' 2>/dev/null < "$d/cmdline" | head -n 1)
        if [ "${arg0##*/}" = "$1" ] || [ "$(cat "$d/comm" 2>/dev/null)" = "$1" ]; then
            echo "${d#/proc/}"
        fi
    done
}

init_mnt=$(readlink /proc/1/ns/mnt 2>/dev/null)
init_net=$(readlink /proc/1/ns/net 2>/dev/null)

# "the same as init" when a namespace link equals init's.
like_init() {
    if [ -n "$1" ] && [ "$1" = "$2" ]; then
        echo 'the same as init'
    else
        echo 'different from init, or unreadable'
    fi
}

# Who runs a process, from which binary, in which root and namespaces.
describe() {
    d=/proc/$1
    printf 'pid %s: uid %s, binary %s, root %s\n' "$1" \
        "$(awk '/^Uid:/ { print $2 }' "$d/status" 2>/dev/null)" \
        "$(readlink "$d/exe" 2>/dev/null)" "$(readlink "$d/root" 2>/dev/null)"
    printf '  mnt namespace %s; net namespace %s\n' \
        "$(like_init "$(readlink "$d/ns/mnt" 2>/dev/null)" "$init_mnt")" \
        "$(like_init "$(readlink "$d/ns/net" 2>/dev/null)" "$init_net")"
}

sockets_of() {
    n=0
    for fd in /proc/"$1"/fd/*; do
        case $(readlink "$fd" 2>/dev/null) in
            socket:*) n=$((n + 1)) ;;
        esac
    done
    echo "$n"
}

# Each named program: where it is, and its processes.
programs() {
    for name in "$@"; do
        path=$(find_bin "$name")
        printf '%s: %s\n' "$name" "${path:-(not found)}"
        for pid in $(pids_of "$name"); do
            describe "$pid"
        done
    done
}

jobs_matching() {
    printf '$ initctl list, jobs matching %s\n' "$1"
    if command -v initctl >/dev/null 2>&1; then
        initctl list 2>&1 | grep -i -E "$1" | or_none
    else
        echo 'initctl: absent'
    fi
}

section identity
show date -u
show uname -r
printf 'uptime, seconds: %s\n' "$(cut -d ' ' -f 1 /proc/uptime 2>/dev/null)"
printf 'boot time, unix seconds: %s\n' "$(awk '$1 == "btime" { print $2 }' /proc/stat 2>/dev/null)"
luna com.webos.service.tv.systemproperty/getSystemInfo \
    '{"keys":["modelName","firmwareVersion","sdkVersion"]}'

# Facts 1, 5, 9 and 12.
section lumen
lumen_pids=$(pids_of lumen-shell)
if [ -z "$lumen_pids" ]; then
    echo '(the shell is not running: facts 1, 5 and 9 need it; open Lumen and run the survey again)'
fi
for pid in $lumen_pids; do
    describe "$pid"
    printf '  environment, four names only:\n'
    vars=$(tr '\0' '\n' 2>/dev/null < "/proc/$pid/environ" \
        | grep -E '^(APPID|WAYLAND_DISPLAY|XDG_RUNTIME_DIR|DISPLAY_ID)=')
    printf '%s\n' "${vars:-(none, or unreadable)}" | sed 's/^/    /'
    printf '  libraries loaded: wayland, EGL, GLES, SDL, luna, mali, webos\n'
    libs=$(grep -o -E '/[^ ]+\.so[^ ]*' "/proc/$pid/maps" 2>/dev/null \
        | grep -i -E 'wayland|egl|gles|sdl|luna|mali|webos' | sort -u)
    printf '%s\n' "${libs:-(none, or unreadable)}" | sed 's/^/    /'
    printf '  sockets held: %s\n' "$(sockets_of "$pid")"
done
printf -- '--- the jail, %s\n' "$JAIL"
for p in "$JAIL" "$JAIL/var" "$JAIL/var/run" "$JAIL/tmp"; do
    if [ -e "$p" ]; then show ls -ld "$p"; else echo "$p: (absent)"; fi
done
if [ -d "$JAIL/var/run" ]; then show ls -la "$JAIL/var/run"; fi
printf '$ mounts under the jail as init sees them: root, target, options, then type and source\n'
awk -v j="$JAIL" 'index($5, j) == 1 { s = $0; sub(/.* - /, "", s); print $4, $5, $6, "-", s }' \
    /proc/1/mountinfo 2>/dev/null | head -n 60 | or_none
printf -- '--- the app files: modes, owners, and their change times\n'
if [ -d "$APP_DIR" ]; then
    show ls -la "$APP_DIR"
    if command -v stat >/dev/null 2>&1; then
        for f in "$APP_DIR" "$APP_DIR/appinfo.json" "$APP_DIR/lumen-shell" \
            "$APP_DIR/lumen-icons.sh" "$APP_DIR/lg-icons"; do
            if [ -e "$f" ]; then show stat "$f"; fi
        done
    else
        echo 'stat: absent'
    fi
else
    echo "$APP_DIR: (absent)"
fi
printf -- '--- the hook logs, for their times\n'
if [ -d "$LUMEN_LOGS" ]; then show ls -la "$LUMEN_LOGS"; else echo "$LUMEN_LOGS: (absent)"; fi

# Facts 1, 2 and 4.
section bus
if command -v ls-monitor >/dev/null 2>&1; then
    registered=$(lsmon -l)
    printf '$ ls-monitor -l lines for Lumen, the app manager, the compositor, power, memory and Homebrew Channel\n'
    printf '%s\n' "$registered" \
        | grep -i -E "$APP_ID|applicationmanager|surface|tvpower|memory|hbchannel|webosbrew" | or_none
    for svc in com.webos.applicationManager com.webos.service.tvpower com.webos.memorymanager \
        com.webos.service.memorymanager com.webos.pmlogd com.webos.service.ics \
        com.webos.service.acr; do
        lsmon -i "$svc"
    done
else
    echo 'ls-monitor: absent on this TV'
fi
printf -- '--- LG SDL under %s: the files, and the strings about key policy and the life cycle\n' "$LIB"
found=""
for f in "$LIB"/libSDL2*.so*; do
    [ -e "$f" ] || continue
    found=1
    show ls -l "$f"
    if [ -f "$f" ] && [ ! -L "$f" ]; then
        # The names alone (hints, luna URIs), not LG's lines around them.
        bin_strings "$f" \
            | grep -o -E '[A-Za-z0-9_./:]*(ACCESS_POLICY|registerNativeApp|registerApp|nativeLifeCycle|relaunch|applicationManager|WEBOS)[A-Za-z0-9_./:]*' \
            | sort -u | head -n 60 | or_none
    fi
done
[ -n "$found" ] || echo '(no libSDL2)'
printf -- '--- bus role and permission files; the contents of those for Lumen and Homebrew Channel\n'
for dir in "$LS2/roles.d" "$LS2/services.d" "$LS2/client-permissions.d" \
    "$LS2/api-permissions.d" "$LS2/manifests.d" "$LS2/groups.d" \
    "$LS2_LEGACY/roles/pub" "$LS2_LEGACY/roles/prv" \
    "$LS2_LEGACY/services/pub" "$LS2_LEGACY/services/prv"; do
    printf -- '--- %s\n' "$dir"
    if [ -d "$dir" ]; then
        existing "$dir"/* | sed 's|.*/||'
        for f in "$dir"/*; do
            case ${f##*/} in
                *"$APP_ID"* | *hbchannel*) showfile "$f" ;;
            esac
        done
    else
        echo '(absent)'
    fi
done

# Fact 7, read only: the change and its check are TV steps.
section app-manager
luna com.webos.service.config/getConfigs \
    '{"configNames":["com.webos.applicationManager.keepAliveApps","com.webos.service.nop.screenSaverTimeout"]}'
luna com.webos.applicationManager/getAppInfo "{\"id\":\"$APP_ID\"}"
showfile "$APP_DIR/appinfo.json"
printf '$ files named sam under /etc and /usr/share\n'
sam_files=$(find /etc /usr/share -maxdepth 3 -name 'sam*' 2>/dev/null)
printf '%s\n' "${sam_files:-(none)}"
for f in $sam_files; do
    case $f in
        *.conf | *.json)
            printf -- '--- %s: lines about keep-alive, preload, the life cycle and memory\n' "$f"
            grep -n -i -E 'keepalive|preload|lifecycle|memory|nativeapp|devmode' "$f" | head -n 40 | or_none
            ;;
    esac
done

# Fact 8, read only: whether Lumen's ack:false holds the saver off is a trial.
section screensaver
programs screenmanager screensaver nopd
jobs_matching 'screen|saver|nop'
luna com.webos.service.tvpower/power/getPowerState '{}'
luna com.webos.settingsservice/getSystemSettings \
    '{"category":"option","keys":["screenSaverTimer","screenSaver","autoPowerOff","standbyMode"]}'

# Fact 11.
section memory
grep -E '^(MemTotal|MemFree|MemAvailable|Cached|SwapTotal|SwapFree):' /proc/meminfo
programs memorymanager memmanager lmkd
jobs_matching 'mem|lmk'
printf '$ /etc files with mem in the name\n'
existing /etc/*mem* /etc/palm/*mem*
printf '$ the system log: lines naming Lumen with memory, reclaim, close, pause or oom (the last 40)\n'
for f in /var/log/messages.1 /var/log/messages.0 /var/log/messages; do
    if [ -r "$f" ]; then cat "$f"; fi
done | grep -F "$APP_ID" | grep -i -E 'memory|reclaim|close|pause|oom|lmk' | tail -n 40 | or_none
printf '$ the kernel log: low memory\n'
dmesg 2>/dev/null | grep -i -E 'lowmem|oom|lmk' | tail -n 20 | or_none

# Fact 13.
section acr
luna com.webos.settingsservice/getSystemSettings \
    '{"category":"general","keys":["acrOnAllowed","acrAdAllowed","additional1Allowed","acrAllowed","generalTermsAllowed","doNotSellMyPersonalInformation"]}'
luna com.webos.settingsservice/getSystemSettings \
    '{"category":"option","keys":["livePlus","acrOnAllowed","acrAdAllowed","additional1Allowed"]}'
acr=$(find_bin acr2)
printf 'acr2: %s\n' "${acr:-(not found)}"
if [ -n "$acr" ]; then
    show ls -l "$acr"
    printf '$ the consent names in its strings\n'
    bin_strings "$acr" | grep -o -E '[A-Za-z0-9_]*(Allowed|livePlus|doNotSell)[A-Za-z0-9_]*' \
        | sort -u | head -n 40 | or_none
fi
acr_pids=$(pids_of acr2)
for pid in $acr_pids; do
    describe "$pid"
    printf '  sockets held: %s\n' "$(sockets_of "$pid")"
done
[ -n "$acr_pids" ] || echo 'acr2: not running'
jobs_matching 'acr'

# Facts 14 and 15. Counts and times only: no name a reverse lookup asked
# for leaves the TV.
section guard-log
logs=""
for f in "$GUARD_LOG.1" "$GUARD_LOG"; do
    if [ -r "$f" ]; then logs="$logs $f"; fi
done
if [ -z "$logs" ]; then
    echo "$GUARD_LOG: (absent)"
else
    # shellcheck disable=SC2086
    cat $logs | awk -v doms='alphonso.tv lgsmartad.com lgads.tv elastic.lgwebostv.com ueiwsp.com' '
    BEGIN { k = split(doms, dom, " ") }
    4 <= NF {
        n++
        if (first == "") first = $1
        last = $1
        name = $3
        for (i = 1; i <= k; i++) {
            d = dom[i]
            if (name == d || substr(name, length(name) - length(d)) == "." d) {
                c[d]++
                if (f[d] == "") f[d] = $1
                l[d] = $1
                if (shown[d] < 20) { t[d] = t[d] " " $1; shown[d]++ }
            }
        }
        if ($2 == "PTR" && name ~ /\.in-addr\.arpa$/) {
            if (name ~ /\.168\.192\.in-addr\.arpa$/ || name ~ /\.10\.in-addr\.arpa$/ \
                || name ~ /\.(1[6-9]|2[0-9]|3[01])\.172\.in-addr\.arpa$/) {
                ptr++
                p[int($1 / 86400)]++
            } else {
                other++
            }
        }
    }
    END {
        printf "log: %d lines, first %s, last %s (unix seconds)\n", n, first, last
        for (i = 1; i <= k; i++) {
            d = dom[i]
            if (c[d] == 0) { printf "%s: lookups 0\n", d; continue }
            printf "%s: lookups %d, first %s, last %s; the first times:%s\n", d, c[d], f[d], l[d], t[d]
        }
        printf "reverse lookups of private addresses: %d; other reverse lookups: %d\n", ptr, other
        lo = ""
        for (day in p) {
            if (lo == "" || day + 0 < lo + 0) lo = day
            if (hi == "" || hi + 0 < day + 0) hi = day
        }
        if (lo != "") for (day = lo + 0; day <= hi + 0; day++) if (day in p) printf "  unix day %d: %d\n", day, p[day]
    }'
fi
printf -- "--- UEI's program, which the sweeps are put down to\n"
programs iconnectivity
for pid in $(pids_of iconnectivity); do
    printf '  pid %s holds %s sockets\n' "$pid" "$(sockets_of "$pid")"
done
jobs_matching 'iconnect|^ics|uei'

# Fact 16.
section lan-chain
printf '$ apps named like sync-demo or cmp-client under %s\n' "$APPS"
existing "$APPS"/*sync* "$APPS"/*cmp*
pmlogd=$(find_bin pmlogd)
printf 'pmlogd: %s\n' "${pmlogd:-(not found)}"
if [ -n "$pmlogd" ]; then
    if bin_strings "$pmlogd" | grep -q -F collectAllLog; then
        echo 'collectAllLog: found in pmlogd'
    else
        echo 'collectAllLog: not found in pmlogd'
    fi
fi

# Facts 17, 18, 21 and 22. busybox netstat can't name the programs, so:
# /proc/net/* gives each listening socket's inode, and every process's fd
# links name the inodes it holds.
section ports
links=$(for fd in /proc/[0-9]*/fd/*; do
    link=$(readlink "$fd" 2>/dev/null) || continue
    case $link in
        socket:*) pid=${fd#/proc/}; echo "${link#socket:} ${pid%%/*}" ;;
    esac
done)
printf 'table bound-to:port program\n'
for table in tcp tcp6 udp udp6; do
    [ -r "/proc/net/$table" ] || continue
    awk -v t="$table" 'NR != 1 && (($4 == "0A" && t ~ /tcp/) || ($4 == "07" && t ~ /udp/)) {
        split($2, a, ":"); print t, a[1], a[2], $10 }' "/proc/net/$table"
done | while read -r table addr port inode; do
    case $addr in
        00000000 | 00000000000000000000000000000000) where=any ;;
        0100007F | 00000000000000000000000001000000) where=loopback ;;
        *) where=one-address ;;
    esac
    pid=$(printf '%s\n' "$links" | awk -v i="[$inode]" '$1 == i { print $2; exit }')
    prog='(unknown)'
    if [ -n "$pid" ]; then
        prog=$(tr '\0' '\n' 2>/dev/null < "/proc/$pid/cmdline" | head -n 1)
        [ -n "$prog" ] || prog=$(cat "/proc/$pid/comm" 2>/dev/null)
    fi
    printf '%s %s:%s %s\n' "$table" "$where" "$((0x$port))" "$prog"
done | sort -u
printf 'ip_local_port_range: %s\n' "$(cat /proc/sys/net/ipv4/ip_local_port_range 2>/dev/null || echo unreadable)"
printf 'USB drives mounted now: %s\n' "$(grep -c -E ' /tmp/usb| /media/usb|^/dev/sd[a-z]' /proc/mounts 2>/dev/null)"

# Fact 19, read only: which addresses the TV reaches without a lookup needs a
# capture.
section lg-addresses
update=$(find_bin update)
printf 'update: %s\n' "${update:-(not found)}"
if [ -n "$update" ]; then
    printf '$ 156.147. addresses in its strings\n'
    bin_strings "$update" | grep -o -E '156\.147\.[0-9]+\.[0-9]+(:[0-9]+)?' | sort -u | or_none
fi
printf '$ files under /etc naming 156.147.\n'
grep -r -l -E '156\.147\.[0-9]+\.[0-9]+' /etc 2>/dev/null | head -n 20 | or_none
printf '$ TCP connections now to 156.147.0.0/16: address, port, state\n'
if [ -r /proc/net/tcp ]; then
    # Addresses are little-endian hex: 156.147.x.y ends in 939C.
    awk 'NR != 1 { split($3, a, ":"); if (substr(a[1], 5, 4) == "939C") print a[1], a[2], $4 }' /proc/net/tcp \
        | while read -r hex port state; do
            third=$(echo "$hex" | cut -c 3-4)
            fourth=$(echo "$hex" | cut -c 1-2)
            printf '156.147.%d.%d port %d state %s\n' "$((0x$third))" "$((0x$fourth))" \
                "$((0x$port))" "$state"
        done | or_none
else
    echo '/proc/net/tcp: (unreadable)'
fi

# Fact 20. Only the lines about the clock's source and the time stay: the
# time zone, city and country say where the TV is.
section clock
luna com.palm.systemservice/clock/getTime '{}' | clock_lines
luna com.palm.systemservice/time/getSystemTime '{}' | clock_lines
showfile /sys/class/rtc/rtc0/name /sys/class/rtc/rtc0/date /sys/class/rtc/rtc0/time \
    /sys/class/rtc/rtc0/since_epoch
programs ntpd sntp chronyd timesyncd
printf '$ /etc files for time keeping\n'
existing /etc/ntp* /etc/chrony*

section end
echo "platform survey complete"
