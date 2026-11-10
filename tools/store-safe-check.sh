#!/bin/sh
# Proves the TV's blocking is store-safe: nothing on the allowlist is
# sinkholed, and everything that must be blocked is. Run it after every
# privacy change (profiles/glasshouse/aggressive.md, docs/specs/privacy.md).
#
#   tools/store-safe-check.sh                     the TV's /etc/hosts, plus live
#                                                 lookups on the TV (tools/tv.sh)
#   tools/store-safe-check.sh --hosts-file FILE   a hosts file here, offline
#
# Options:
#   --stage N        what should be blocked by now (aggressive.md's stages):
#                    1 or 2: nothing required yet, only the allowlist is checked
#                    3: also the firmware hosts (required-blocks.txt)
#                    4: also b9-aggressive.hosts' active entries (the default)
#                    5: Lumen Guard blocks (G2): its rules must keep every name
#                       it has to resolve and block the firmware hosts
#                       (tools/guard/rules.py --check, with the owner's file),
#                       and live, Guard's answers must sinkhole the firmware
#                       hosts and a name no rule allows; the hosts file need
#                       not list them
#   --profile DIR    rules to check against (default profiles/guard)
#   --owner FILE     the owner's Guard rules, for stage 5 (default
#                    private/guard/owner.txt, when it exists)
#
# A name is sinkholed when a hosts line maps it to 0.0.0.0, 127.x, :: or ::1.
# It fails if any line sinkholes an allowlisted name, and counts a block only
# when every line for the name is a sinkhole. Live, an allowlisted name with no
# answer fails too. Exit status 0 means PASS; 1 means a FAIL line was printed.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV_SH=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
PROFILE=$ROOT/profiles/guard
STAGE=4
HOSTS_FILE=
OWNER=${LUMEN_GUARD_OWNER:-$ROOT/private/guard/owner.txt}
OWNER_SET=

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "store-safe-check.sh: $*" >&2
    exit 2
}

while [ $# -gt 0 ]; do
    case $1 in
        --hosts-file) [ $# -ge 2 ] || die "--hosts-file needs a file"; HOSTS_FILE=$2; shift 2 ;;
        --stage) [ $# -ge 2 ] || die "--stage needs 1 to 4"; STAGE=$2; shift 2 ;;
        --profile) [ $# -ge 2 ] || die "--profile needs a directory"; PROFILE=$2; shift 2 ;;
        --owner) [ $# -ge 2 ] || die "--owner needs a file"; OWNER=$2; OWNER_SET=1; shift 2 ;;
        -h | --help | help) usage; exit 0 ;;
        *) usage >&2; die "unknown argument: $1" ;;
    esac
done

case $STAGE in 1 | 2 | 3 | 4 | 5) ;; *) die "--stage must be 1 to 5, not $STAGE" ;; esac
ALLOW=$PROFILE/allowlist.txt
REQUIRED=$PROFILE/required-blocks.txt
for f in "$ALLOW" "$REQUIRED"; do
    [ -f "$f" ] || die "no $f"
done
# Glasshouse's custom list, if this profile has one (profiles/guard has not:
# Guard blocks by default instead).
CUSTOM=$PROFILE/b9-aggressive.hosts
[ -f "$CUSTOM" ] || CUSTOM=/dev/null
if [ -n "$OWNER_SET" ] && [ ! -e "$OWNER" ]; then
    die "no such owner file: $OWNER"
fi

# A Python 3 that runs: on Windows `python3` may be the Store's placeholder.
python_cmd() {
    for p in ${LUMEN_PYTHON:-} python3 python; do
        if "$p" -c 'import sys; sys.exit(sys.version_info < (3, 8))' >/dev/null 2>&1; then
            echo "$p"
            return 0
        fi
    done
    return 1
}

status=0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Stage 5: Guard's own rules, as Guard reads them (tools/guard/rules.py is
# held to rules.rs by tools/tests/test_guard_rules.py).
if [ "$STAGE" -ge 5 ]; then
    py=$(python_cmd) || die "stage 5 needs Python 3 (tools/guard/rules.py)"
    set -- --check --rules "$PROFILE"
    [ -f "$OWNER" ] && set -- "$@" --owner "$OWNER"
    if "$py" -I "$ROOT/tools/guard/rules.py" "$@"; then
        if [ -f "$OWNER" ]; then
            echo "Guard's rules keep the store, DRM and the paid apps, and block the firmware hosts (with $OWNER)"
        else
            echo "Guard's rules keep the store, DRM and the paid apps, and block the firmware hosts (no owner file)"
        fi
    else
        status=1
    fi
fi

if [ -n "$HOSTS_FILE" ]; then
    [ -e "$HOSTS_FILE" ] || die "no such hosts file: $HOSTS_FILE"
    cp "$HOSTS_FILE" "$tmp/hosts"
    source="$HOSTS_FILE"
else
    "$TV_SH" run 'cat /etc/hosts' </dev/null >"$tmp/hosts" || die "could not read the TV's /etc/hosts"
    source="the TV's /etc/hosts"
fi

# Shared by both checks: rule loading and allowlist matching.
AWK_LIB='
function lc(s) { return tolower(s) }
function strip(s) { gsub(/\r/, "", s); sub(/#.*/, "", s); gsub(/^[ \t]+|[ \t]+$/, "", s); return lc(s) }
function sinkhole(a) { return a == "0.0.0.0" || a ~ /^127\./ || a == "::" || a == "::0" || a == "::1" || a == "0:0:0:0:0:0:0:0" || a == "0:0:0:0:0:0:0:1" }
# The allowlist rule that covers name, or "".
function allowed(name,   s, i) {
    if (name in exact) return "=" name
    s = name
    while (1) {
        if (s in allow) return s
        i = index(s, ".")
        if (i == 0) return ""
        s = substr(s, i + 1)
    }
}
function load_rule(line,   r) {
    r = strip(line)
    if (r == "") return
    if (substr(r, 1, 1) == "=") exact[substr(r, 2)] = 1
    else allow[r] = 1
}
# Active (uncommented) names of a hosts-format line, into arr.
function hosts_names(line, arr,   n, f, i) {
    gsub(/\r/, "", line)
    sub(/#.*/, "", line)
    n = split(line, f, /[ \t]+/)
    i = 1
    if (f[1] == "") i = 2
    if (n < i + 1) return
    if (!sinkhole(f[i])) return
    for (i = i + 1; i <= n; i++) if (f[i] != "") arr[lc(f[i])] = 1
}
'

# shellcheck disable=SC2016 # awk program, not shell
awk -v stage="$STAGE" -v source="$source" "$AWK_LIB"'
phase == "allow" { load_rule($0); next }
phase == "required" { n = strip($0); if (n != "") required[n] = 1; next }
phase == "custom" { hosts_names($0, custom); next }
phase == "hosts" {
    line = $0
    gsub(/\r/, "", line)
    sub(/#.*/, "", line)
    k = split(line, f, /[ \t]+/)
    i = (f[1] == "") ? 2 : 1
    if (k < i + 1) next
    for (j = i + 1; j <= k; j++) {
        if (f[j] == "") continue
        name = lc(f[j])
        if (sinkhole(f[i])) sunk[name] = 1; else real[name] = 1
    }
    next
}
END {
    fails = 0
    for (n in required) if ((r = allowed(n)) != "") {
        printf "FAIL profile: %s is in required-blocks.txt and allowlisted by %s\n", n, r; fails++
    }
    for (n in custom) if ((r = allowed(n)) != "") {
        printf "FAIL profile: %s is in b9-aggressive.hosts and allowlisted by %s\n", n, r; fails++
    }
    nsunk = 0; nbad = 0
    for (n in sunk) {
        nsunk++
        if ((r = allowed(n)) != "") {
            printf "FAIL allowlisted name is sinkholed: %s (rule %s)\n", n, r; fails++; nbad++
        }
    }
    nreq = 0; okreq = 0
    if (stage == 3 || stage == 4) for (n in required) {
        nreq++
        if ((n in sunk) && !(n in real)) okreq++
        else { printf "FAIL firmware host not sinkholed: %s\n", n; fails++ }
    }
    ncus = 0; okcus = 0
    if (stage == 4) for (n in custom) {
        ncus++
        if ((n in sunk) && !(n in real)) okcus++
        else { printf "FAIL custom-list host not sinkholed: %s\n", n; fails++ }
    }
    printf "%s: %d sinkholed names, %d of them allowlisted\n", source, nsunk, nbad
    if (stage >= 5) print "firmware hosts: Guard blocks them (checked live)"
    else if (stage >= 3) printf "firmware hosts sinkholed: %d of %d\n", okreq, nreq
    else print "firmware hosts: not checked before stage 3"
    if (stage >= 5) print "custom-list hosts: none at stage 5 (Guard blocks by default)"
    else if (stage >= 4) printf "custom-list hosts sinkholed: %d of %d\n", okcus, ncus
    else print "custom-list hosts: not checked before stage 4"
    exit (fails > 0 ? 1 : 0)
}
' phase=allow "$ALLOW" phase=required "$REQUIRED" phase=custom "$CUSTOM" phase=hosts "$tmp/hosts" ||
    status=1

# Live lookups on the TV: what apps actually get back, through /etc/hosts
# and the resolver. getent uses the same path as apps; busybox nslookup is
# the fallback (newer busybox versions skip /etc/hosts, so a FAIL from
# nslookup alone is worth a second look).
if [ -z "$HOSTS_FILE" ]; then
    {
        # Not the apex lgtvsdp.com: it has no address record anywhere
        # (checked 2026-11-10), so "no answer" there says nothing.
        for n in us.lgtvsdp.com us.lgeapi.com www.netflix.com \
            www.amazon.com atv-ps.amazon.com plex.tv www.googleapis.com \
            www.youtube.com spclient.wg.spotify.com; do
            echo "allow $n"
        done
        if [ "$STAGE" -ge 3 ]; then
            awk '{ sub(/#.*/, ""); if ($1 != "") print "block " tolower($1) }' "$REQUIRED"
        fi
        if [ "$STAGE" -ge 5 ]; then
            # A name no rule allows (RFC 2606's .example, so a leak gets
            # NXDOMAIN), and Lumen's exact carve-outs: only Guard sinkholes
            # them.
            echo "block lumen-default-deny.example"
            if [ -f "$PROFILE/carve-outs.txt" ]; then
                awk '{ sub(/#.*/, "") } $1 ~ /^=/ { print "block " tolower(substr($1, 2)) }' \
                    "$PROFILE/carve-outs.txt" | head -n 2
            fi
        elif [ "$STAGE" -ge 4 ]; then
            awk '!/^[ \t]*#/ && ($1 == "0.0.0.0" || $1 == "::") { print "block " tolower($2) }' "$CUSTOM" |
                sort -u | head -n 3
        fi
    } >"$tmp/names"
    # Only plain host names go into the script that runs on the TV.
    if grep -v '^\(allow\|block\) [a-z0-9.-]*$' "$tmp/names" >/dev/null; then
        die "unexpected characters in a profile name"
    fi
    cat >"$tmp/lookup.sh" <<'LOOKUP'
if command -v getent >/dev/null 2>&1; then tool=getent; else tool=nslookup; fi
echo "tool $tool"
lookup() {
    if [ "$tool" = getent ]; then
        getent hosts "$1" | awk '{ print $1 }'
    else
        nslookup "$1" 2>/dev/null | awk '/^Name:/ { f = 1; next }
            f && /^Address/ { for (i = 2; i <= NF; i++)
                if ($i ~ /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/ || $i ~ /:.*:/) { print $i; break } }'
    fi
}
LOOKUP
    while read -r kind name; do
        # shellcheck disable=SC2016 # $a and $(lookup) expand on the TV
        printf 'for a in $(lookup %s); do echo "%s %s $a"; done; echo "%s %s end"\n' \
            "$name" "$kind" "$name" "$kind" "$name"
    done <"$tmp/names" >>"$tmp/lookup.sh"
    "$TV_SH" script "$tmp/lookup.sh" >"$tmp/lookups" || die "live lookups on the TV failed"
    # shellcheck disable=SC2016 # awk program, not shell
    awk -v stage="$STAGE" '
    function sinkhole(a) { return a == "0.0.0.0" || a ~ /^127\./ || a == "::" || a == "::0" || a == "::1" || a == "0:0:0:0:0:0:0:0" || a == "0:0:0:0:0:0:0:1" }
    $1 == "tool" { tool = $2; printf "live lookups on the TV (%s):\n", $2; next }
    NF == 3 && $3 == "end" {
        key = $1 " " $2
        names++
        # No answer for a name that must work is a failure: DNS down, or an
        # upstream blocker, breaks the store just as a sinkhole does. At
        # stage 5 Guard answers every name it blocks, so silence means it
        # is not blocking.
        if (!(key in got) && $1 == "allow") { printf "FAIL allow %s: no answer\n", $2; fails++ }
        else if (!(key in got) && stage >= 5) { printf "FAIL block %s: no answer (is Guard blocking?)\n", $2; fails++ }
        else if (!(key in got)) { printf "WARN %s %s: no answer (not in /etc/hosts?)\n", $1, $2 }
        else if ($1 == "allow" && bad[key]) { printf "FAIL %s resolves to a sinkhole (%s)\n", $2, addrs[key]; fails++ }
        else if ($1 == "block" && !sunkall[key]) { printf "FAIL %s resolves to %s, not a sinkhole\n", $2, addrs[key]; fails++ }
        else printf "ok   %s %s -> %s\n", $1, $2, addrs[key]
        next
    }
    NF == 3 {
        key = $1 " " $2
        if (!(key in got)) sunkall[key] = 1
        got[key] = 1
        addrs[key] = (addrs[key] == "") ? $3 : addrs[key] " " $3
        if (sinkhole($3)) bad[key] = 1; else sunkall[key] = 0
    }
    END {
        if (tool == "" || names == 0) { print "FAIL live lookups did not run on the TV"; fails++ }
        exit (fails > 0 ? 1 : 0)
    }
    ' "$tmp/lookups" || status=1
fi

if [ "$status" -eq 0 ]; then
    echo "PASS (stage $STAGE)"
else
    echo "FAIL (stage $STAGE)"
fi
exit "$status"
