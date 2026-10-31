#!/bin/sh
# Read-only survey of a rooted LG webOS TV for the Remote spec's R0
# (docs/specs/remote.md). tools/recon.sh --remote pipes this to the TV over SSH
# and saves what it prints. What each section answers:
#
#   ssh            question 1: dropbear's version and the authorized_keys
#                  options its binary knows
#   luna-methods   question 2: the luna methods behind the verbs
#   keys           question 3: whether remote keys can be sent (uinput, LG's
#                  input services)
#   firmware-date  question 4: whether firmware 05.50.00 predates LG's
#                  2024-03-22 fix
#   home           the Home look in docs/specs/depth.md
#
# Question 5, Wake-on-LAN, is a test at the TV with the owner, not a survey.
#
# Nothing here writes to the TV: it reads /proc, /sys, /etc, /usr and /var,
# lists devices and upstart jobs, and runs four programs that could do more
# than read, each only one way:
#   luna-send     get* and list* methods only (luna() refuses anything else)
#   ls-monitor    only -l and -i SERVICE (lsmon() refuses anything else: with
#                 no option it would ask the hub to route every client's
#                 messages to it, and -s and -m call other services' private
#                 methods)
#   dropbear      only -V, which prints the version and exits
#   initctl       only list
# No key is printed. An authorized_keys line comes out as its option names and
# its key type; no private key, host key, password hash or process environment
# is read. Input devices' Phys and Uniq values, which can hold Bluetooth
# addresses, are withheld here, and redact.py masks them again.
# It runs under the TV's busybox sh, so it sticks to POSIX shell.
#
# The two variables below exist so tests/recon.bats can point the survey at
# fixtures. On the TV they are unset and the defaults are the real paths.

# Older busybox (webOS 4) spells it `timeout -t SECS`, newer `timeout SECS`.
if timeout 1 true >/dev/null 2>&1; then
    T="timeout 10"
elif timeout -t 1 true >/dev/null 2>&1; then
    T="timeout -t 10"
else
    T=""
fi

KEYFILES=${REMOTE_SURVEY_KEYFILES:-"/home/root/.ssh/authorized_keys /var/lib/webosbrew/sshd/authorized_keys"}
INPUT_DEVICES=${REMOTE_SURVEY_INPUT:-/proc/bus/input/devices}

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

# The printable runs in a binary, one per line, which any grep can search.
# The range, not [:print:]: busybox's tr -c gets the class backwards.
bin_strings() {
    tr -c ' -~' '\n' < "$1"
}

# One line per key in an authorized_keys file: the names of its options, with
# their values hidden, and its key type. Nothing else from a line is printed,
# so no key can reach the output. Each key line is split at its first blank
# outside quotes: that word is the options, unless it is the key type itself.
key_options() {
    awk '
    # Only these words ever reach the output, so a malformed line shows as ?.
    function label(name, hasval,    known, tail) {
        known = " restrict command from permitopen permitlisten environment principals cert-authority tunnel expiry-time"
        known = known " no-pty pty no-port-forwarding port-forwarding no-agent-forwarding agent-forwarding"
        known = known " no-x11-forwarding x11-forwarding no-user-rc user-rc no-touch-required verify-required "
        if (index(known, " " tolower(name) " ") == 0) return "?"
        tail = hasval ? "=…" : ""
        return name tail
    }
    function names(s,    i, c, inq, cur, hasval, out, sep) {
        inq = 0; cur = ""; hasval = 0; out = ""
        s = s ","
        for (i = 1; i <= length(s); i++) {
            c = substr(s, i, 1)
            if (inq) {
                if (c == "\\") { i++ } else if (c == "\"") { inq = 0 }
            } else if (c == "\"") {
                inq = 1
            } else if (c == "=") {
                hasval = 1
            } else if (c == ",") {
                sep = (out == "") ? "" : ","
                out = out sep label(cur, hasval)
                cur = ""; hasval = 0
            } else if (!hasval) {
                cur = cur c
            }
        }
        return out
    }
    { gsub("\r", "") }
    /^[ \t]*(#|$)/ { next }
    {
        n++
        line = $0
        sub(/^[ \t]+/, "", line)
        inq = 0; sp = 0
        for (i = 1; i <= length(line); i++) {
            c = substr(line, i, 1)
            if (inq) {
                if (c == "\\") { i++ } else if (c == "\"") { inq = 0 }
            } else if (c == "\"") {
                inq = 1
            } else if (c == " " || c == "\t") {
                sp = i
                break
            }
        }
        if (sp == 0) { first = line; rest = "" } else { first = substr(line, 1, sp - 1); rest = substr(line, sp + 1) }
        sub(/^[ \t]+/, "", rest)
        if (first ~ /^(ssh-|ecdsa-|sk-)/) {
            opts = ""; typ = first
        } else {
            opts = first
            split(rest, w, /[ \t]+/)
            typ = w[1]
        }
        if (typ !~ /^(ssh-(rsa|dss|ed25519)|ecdsa-sha2-nistp(256|384|521)|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com)(-cert-v01@openssh\.com)?$/) typ = "(unrecognised)"
        shown = (opts == "") ? "none" : names(opts)
        printf "  key %d: options: %s; type: %s\n", n, shown, typ
    }
    ' "$1"
}

section identity
show date -u
show id
show uname -r

# Question 1: which dropbear serves SSH, its version, and which key options
# its binary knows. `dropbear -V` prints to stderr.
section ssh
printf '$ dropbear processes: pid, binary, arguments\n'
bins=""
found=""
for d in /proc/[0-9]*; do
    [ "$(cat "$d/comm" 2>/dev/null)" = dropbear ] || continue
    found=1
    exe=$(readlink "$d/exe" 2>/dev/null)
    printf '%s: %s: %s\n' "${d#/proc/}" "${exe:-(binary unreadable)}" \
        "$(tr '\0' ' ' < "$d/cmdline" 2>/dev/null | cut -c1-300)"
    case $exe in
        '' | *[!A-Za-z0-9._/-]*) ;;
        /*)
            case " $bins " in
                *" $exe "*) ;;
                *) bins="$bins $exe" ;;
            esac
            ;;
    esac
done
[ -n "$found" ] || echo '(no dropbear process)'
path=$(command -v dropbear 2>/dev/null)
printf 'command -v dropbear: %s\n' "${path:-(not on PATH)}"
case $path in
    '' | *[!A-Za-z0-9._/-]*) ;;
    /*)
        case " $bins " in
            *" $path "*) ;;
            *) bins="$bins $path" ;;
        esac
        ;;
esac
# shellcheck disable=SC2086
for bin in $bins; do
    printf -- '--- %s\n' "$bin"
    if [ "${bin##*/}" != dropbear ] || [ ! -x "$bin" ]; then
        echo '(skipped: not an executable named dropbear)'
        continue
    fi
    printf '$ %s -V\n' "$bin"
    $T "$bin" -V 2>&1 || printf '(exit %s)\n' "$?"
    printf '$ authorized_keys option names found in the binary (up to 3 strings each)\n'
    for opt in 'command=' restrict no-pty no-port-forwarding no-agent-forwarding no-X11-forwarding; do
        hits=$(bin_strings "$bin" | grep -F -- "$opt" | cut -c1-100 | head -n 3)
        if [ -n "$hits" ]; then
            printf '%s: found\n%s\n' "$opt" "$(printf '%s\n' "$hits" | sed 's/^/    /')"
        else
            printf '%s: not found\n' "$opt"
        fi
    done
done
printf '$ installed keys: how many, then the option names and key type of each\n'
# shellcheck disable=SC2086
for f in $KEYFILES; do
    printf -- '--- %s\n' "$f"
    if [ -r "$f" ]; then
        ls -l "$f"
        printf '%s key line(s)\n' "$(grep -c -v -E '^[[:space:]]*(#|$)' "$f")"
        key_options "$f"
    else
        echo '(absent)'
    fi
done
show ls -ld /home/root /home/root/.ssh

# Question 2: the methods behind the verbs. ls-monitor -l lists what is
# registered with the hub, ls-monitor -i what one service offers (a build of
# the hub without introspection prints nothing). Then read-only calls.
section luna-methods
registered=""
if command -v ls-monitor >/dev/null 2>&1; then
    registered=$(lsmon -l)
    printf '%s\n' "$registered"
    for svc in com.webos.service.tvpower com.webos.audio com.webos.service.audio \
        com.webos.applicationManager com.webos.settingsservice com.webos.service.ime \
        com.webos.service.eim com.webos.service.tv.systemproperty \
        com.webos.notification com.webos.service.config \
        com.webos.service.connectionmanager; do
        lsmon -i "$svc"
    done
else
    echo 'ls-monitor: absent on this TV, so no list of services or methods (the calls below still run)'
fi
# Dotted names in the list, which is a table of pid, name, binary and type.
input_services=$(printf '%s\n' "$registered" \
    | grep -o -E '(^|[[:space:]])[A-Za-z][A-Za-z0-9_-]*(\.[A-Za-z0-9_-]+){2,}' \
    | sed 's/^[[:space:]]*//' | sort -u | grep -i -E 'input|rcu|key|remote' | head -n 20)
printf '\n-- registered services with input, rcu, key or remote in the name\n'
if [ -n "$input_services" ]; then
    # shellcheck disable=SC2086
    for svc in $input_services; do
        lsmon -i "$svc"
    done
else
    echo '(none)'
fi
printf '\n-- read-only calls (get and list methods only)\n'
luna com.webos.service.tv.systemproperty/getSystemInfo \
    '{"keys":["modelName","firmwareVersion","sdkVersion"]}'
luna com.webos.service.tvpower/power/getPowerState '{}'
luna com.webos.audio/getStatus '{}'
luna com.webos.audio/getVolume '{}'
luna com.webos.service.audio/master/getVolume '{}'
luna com.webos.applicationManager/getForegroundAppInfo '{}'
luna com.webos.service.eim/getAllInputStatus '{}'
luna com.webos.service.connectionmanager/getstatus '{}'
for category in picture sound option network; do
    luna com.webos.settingsservice/getSystemSettings "{\"category\":\"$category\"}"
done

# Question 3: can keys be sent without LG's remote? uinput would do it from
# the TV; nothing here opens a device, it only reads modes and the kernel's
# configuration. Names only for the service files.
section keys
for node in /dev/uinput /dev/input/uinput /dev/uhid; do
    if [ -e "$node" ]; then show ls -l "$node"; else echo "$node: (absent)"; fi
done
show ls -l /dev/input
if [ -r /proc/config.gz ]; then
    printf '$ kernel config (uinput, uhid)\n'
    zcat /proc/config.gz | grep -E '^(# )?CONFIG_(INPUT_UINPUT|UHID)[ =]' \
        || echo '(none matched)'
else
    echo '/proc/config.gz: (absent)'
fi
printf -- '--- %s (Phys and Uniq withheld, they can hold Bluetooth addresses)\n' "$INPUT_DEVICES"
if [ -r "$INPUT_DEVICES" ]; then
    sed -e 's/^P: Phys=..*/P: Phys=(withheld)/' -e 's/^U: Uniq=..*/U: Uniq=(withheld)/' \
        "$INPUT_DEVICES"
else
    echo '(absent)'
fi
printf '$ luna service files with input, rcu, key, remote or hid in the name\n'
found=""
for f in /usr/share/luna-service2/services.d/*; do
    [ -r "$f" ] || continue
    case $(printf '%s' "${f##*/}" | tr '[:upper:]' '[:lower:]') in
        *input* | *rcu* | *key* | *remote* | *hid*)
            found=1
            grep -H -E '^(Name|Exec)=' "$f" || echo "${f##*/}: (no Name or Exec line)"
            ;;
    esac
done
[ -n "$found" ] || echo '(none matched)'
printf '$ registered input-related services (from luna-methods above)\n'
printf '%s\n' "${input_services:-(none)}"

# Question 4: the firmware's build date. Squashfs files often carry one
# timestamp, so the release files and the kernel's build line come first.
section firmware-date
showfile /etc/starfish-release /etc/buildinfo /etc/build-info /etc/lsb-release /proc/version
for f in /etc/starfish-release /etc/buildinfo /etc/build-info /etc/lsb-release \
    /usr/bin/com.webos.app.home /usr/lib/libSDL2-2.0.so.0; do
    [ -e "$f" ] || continue
    show ls -l "$f"
    [ -L "$f" ] && show ls -lL "$f"
    if command -v stat >/dev/null 2>&1; then show stat -L "$f"; fi
done

# The Home look in docs/specs/depth.md: how LG's Home is declared, launched and
# run, so the route for Lumen as Home can be chosen.
section home
showfile /usr/palm/applications/com.webos.app.home/appinfo.json \
    /usr/palm/applications/com.webos.app.homeconnect/appinfo.json
showfile /usr/lib/qt5/qml/KeyFilters/appLaunch.js /usr/lib/qt5/qml/KeyFilters/systemUi.js
printf '$ initctl list (sam, bootd, surface, home)\n'
if command -v initctl >/dev/null 2>&1; then
    initctl list 2>&1 | grep -E 'sam|bootd|surface|home' || echo '(none matched)'
else
    echo 'initctl: absent'
fi
printf '$ upstart job files for sam, bootd, surface and home\n'
found=""
for f in /etc/init/*sam* /etc/init/*bootd* /etc/init/*surface* /etc/init/*home*; do
    [ -r "$f" ] || continue
    found=1
    showfile "$f"
done
[ -n "$found" ] || echo '(none)'
printf '$ jail files and folders\n'
found=""
for f in /etc/jail* /etc/palm/jail* /var/palm/jail*; do
    [ -e "$f" ] || continue
    found=1
    show ls -ld "$f"
    [ -d "$f" ] && show ls "$f"
done
[ -n "$found" ] || echo '(none)'
printf '$ jail*.conf contents\n'
found=""
for f in /etc/jail*.conf /etc/palm/jail*.conf /etc/palm/jail*/*.conf /var/palm/jail*.conf; do
    [ -f "$f" ] || continue
    found=1
    showfile "$f"
done
[ -n "$found" ] || echo '(none)'
# ps has no uniform -o on busybox, so /proc says who runs each process, which
# root it sees (a jail shows as a path other than /) and whether its mount
# namespace is init's.
printf '$ processes: com.webos.app.home, sam, surface-manager, bootd\n'
init_ns=$(readlink /proc/1/ns/mnt 2>/dev/null)
found=""
for d in /proc/[0-9]*; do
    arg0=$(tr '\0' '\n' < "$d/cmdline" 2>/dev/null | head -n 1)
    case ${arg0##*/} in
        com.webos.app.home | sam | surface-manager | bootd) ;;
        *)
            case $(cat "$d/comm" 2>/dev/null) in
                sam | surface-manager | bootd) ;;
                *) continue ;;
            esac
            ;;
    esac
    found=1
    uid=$(awk '/^Uid:/ { print $2 }' "$d/status" 2>/dev/null)
    user=$(awk -F: -v u="$uid" '$3 == u { print $1; exit }' /etc/passwd 2>/dev/null)
    printf 'pid %s: uid %s (%s), gid %s, groups %s, capabilities %s\n' "${d#/proc/}" \
        "${uid:-?}" "${user:-no name}" \
        "$(awk '/^Gid:/ { print $2 }' "$d/status" 2>/dev/null)" \
        "$(sed -n 's/^Groups:[[:space:]]*//p' "$d/status" 2>/dev/null)" \
        "$(sed -n 's/^CapEff:[[:space:]]*//p' "$d/status" 2>/dev/null)"
    ns=$(readlink "$d/ns/mnt" 2>/dev/null)
    if [ -n "$ns" ] && [ "$ns" = "$init_ns" ]; then ns_note='the same as init'; else ns_note='different from init, or unreadable'; fi
    printf '  binary %s, root %s, mnt namespace %s\n' "$(readlink "$d/exe" 2>/dev/null)" \
        "$(readlink "$d/root" 2>/dev/null)" "$ns_note"
    printf '  args: %s\n' "$(tr '\0' ' ' < "$d/cmdline" 2>/dev/null | cut -c1-300)"
done
[ -n "$found" ] || echo '(none running)'
for category in general option; do
    luna com.webos.settingsservice/getSystemSettings \
        "{\"category\":\"$category\",\"keys\":[\"homeAutoLaunch\",\"lastInput\"]}"
done
printf '$ bootd files and the preferences folder (names only)\n'
found=""
for f in /etc/bootd*; do
    [ -e "$f" ] || continue
    found=1
    show ls -ld "$f"
    [ -d "$f" ] && show ls "$f"
done
[ -n "$found" ] || echo '(no /etc/bootd*)'
show ls /var/luna/preferences/

section end
echo "remote survey complete"
