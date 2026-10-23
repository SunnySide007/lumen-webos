#!/bin/sh
# Copies LG's app and input icons into Lumen's own app folder, where the
# shell's jail can read them (docs/specs/icons.md). It runs as root, started
# through Homebrew Channel's exec with this fixed command and no arguments:
#
#   /bin/sh /media/developer/apps/usr/palm/applications/io.github.sunnyside007.lumen.shell/lumen-icons.sh
#
# Everything it copies it finds itself, in luna's listLaunchPoints and LG's
# input icon folder, and every path is checked before it is read: one of four
# fixed roots, a valid app ID, no link out of the app's folder, a regular file
# that starts as a PNG, and caps on size and count. It writes only inside
# Lumen's folder. Undo: rm -rf <that folder>/lg-icons*; uninstalling Lumen
# removes it too.
#
# Prints one line: "lumen-icons: changed, N copied, M refused",
# "lumen-icons: unchanged" or "lumen-icons: busy". A failure exits 1 with its
# reason on stderr and leaves the copy there was.
set -eu
umask 022

# Tests only: a fake file system root in front of every path.
PREFIX=${LUMEN_ICONS_PREFIX:-}

APP=$PREFIX/media/developer/apps/usr/palm/applications/io.github.sunnyside007.lumen.shell
LUNA_SEND=$PREFIX/usr/bin/luna-send
LAUNCH_POINTS=luna://com.webos.applicationManager/listLaunchPoints
# Developer-mode apps: the jail reads their icons where they are.
DIRECT=/media/developer/
# Where getAllInputStatus points every input on the B9.
INPUTS=/usr/palm/plugins/inputapps/assets/small
# The shell's own limit (icons.rs).
MAX_FILE=1048576
MAX_TOTAL=8388608
MAX_REPLY=4194304
MAX_APP_ICONS=128
MAX_INPUT_ICONS=64
MAX_NAME=255
# Inside Lumen's folder, once the script works from it.
OUT=lg-icons
NEW=lg-icons.new
OLD=lg-icons.old
LOCK=lg-icons.lock
WORK=lg-icons.work

fail() {
    echo "lumen-icons: $*" >&2
    exit 1
}

case $0 in
    /*) SELF=$0 ;;
    *) SELF=$PWD/$0 ;;
esac
uid=$(id -u)
[ "$uid" = 0 ] || [ -n "$PREFIX" ] || fail "not root"
# The B9's busybox (1.24.1) has no cksum.
command -v sha256sum >/dev/null 2>&1 || fail "no sha256sum"
if [ ! -d "$APP" ] || [ -L "$APP" ]; then
    fail "no app folder at $APP"
fi

# safe_dir DIR [NAME]: a real folder that only this script's user may
# change, named NAME in a refusal. Root writes in Lumen's folder, so no one
# else may be able to swap it, or the folders our installs set above it, and
# steer those writes elsewhere.
safe_dir() {
    if [ ! -d "$1" ] || [ -L "$1" ]; then
        fail "${2:-$1} is not a folder"
    fi
    # ls -ln is POSIX, and stat is not: its mode and owner, split into words.
    set -f
    # shellcheck disable=SC2046,SC2012
    set -- "${2:-$1}" $(ls -ldn "$1")
    set +f
    case $2 in
        d????[!w]??[!w]*) ;;
        *) fail "$1 is writable by others ($2)" ;;
    esac
    [ "$4" = "$uid" ] || fail "$1 is owned by $4, not $uid"
}
# The three shared folders our installs set (the IPK's data.tar folder
# modes). /media/developer and /media/developer/apps above them are LG's
# developer mode's, 0777 on the B9, so they are not checked
# (docs/specs/icons.md, "Known limits").
SHARED=$PREFIX/media/developer/apps/usr
safe_dir "$SHARED"
safe_dir "$SHARED/palm"
safe_dir "$SHARED/palm/applications"
# Pinned before anything is written: from here on the script works in the
# folder it checked, by relative names, wherever its path leads later.
cd "$APP" || fail "cannot enter $APP"
safe_dir . "$APP"
if [ -n "$PREFIX" ]; then
    expected=$(CDPATH='' cd "$PREFIX" && pwd -P)${APP#"$PREFIX"}
else
    expected=$APP
fi
here=$(pwd -P)
if [ "$here" != "$expected" ]; then
    # Folder names there may be anyone's: shown only in plain characters.
    fail "$APP is not where it should be ($(printf '%s' "$here" | tr -c 'A-Za-z0-9._/-' '?'))"
fi
# The sum covers the script itself, so a script it cannot read is a failure,
# not a sum that never changes.
self_sum=$(sha256sum <"$SELF") || fail "cannot read $SELF"

# The lock is a link to the PID that holds it, which ln makes atomically. It
# is stale unless that PID is a run of this script: a power cut leaves a PID
# that another process may hold after the reboot. ln -n, and no folder in its
# place, or ln would make the link inside it and seem to succeed.
if [ -d "$LOCK" ] && [ ! -L "$LOCK" ]; then
    rm -rf "$LOCK"
fi
if ! ln -sn "$$" "$LOCK" 2>/dev/null; then
    pid=$(readlink "$LOCK" 2>/dev/null || true)
    case $pid in
        '' | *[!0-9]*) ;;
        # cmdline is NUL-separated, and busybox grep stops at a NUL.
        *) if { tr '\0' ' ' <"/proc/$pid/cmdline"; } 2>/dev/null | grep -qF lumen-icons.sh; then
            echo "lumen-icons: busy"
            exit 0
        fi ;;
    esac
    # Only the stale lock: another run may have just taken it over.
    if [ "$(readlink "$LOCK" 2>/dev/null || true)" = "$pid" ]; then
        rm -rf "$LOCK"
    fi
    if ! ln -sn "$$" "$LOCK" 2>/dev/null; then
        echo "lumen-icons: busy"
        exit 0
    fi
fi
cleanup() {
    rm -rf "$WORK" "$NEW"
    # Never the last copy: lg-icons.old is it while lg-icons is missing.
    if [ -d "$OUT" ]; then
        rm -rf "$OLD"
    fi
    if [ "$(readlink "$LOCK" 2>/dev/null)" = "$$" ]; then
        rm -f "$LOCK"
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
# A run cut off between the swap's two renames left the copy as lg-icons.old.
if [ ! -e "$OUT" ] && [ -d "$OLD" ]; then
    mv "$OLD" "$OUT" || fail "cannot restore $OLD"
fi
rm -rf "$WORK" "$NEW"
mkdir "$WORK" || fail "cannot make $WORK"

reply=$WORK/points.json
"$LUNA_SEND" -n 1 -w 4000 "$LAUNCH_POINTS" '{}' >"$reply" 2>/dev/null ||
    fail "luna-send failed"
size=$(wc -c <"$reply")
[ $((size)) -le "$MAX_REPLY" ] || fail "listLaunchPoints answered over $MAX_REPLY bytes"
grep -q '"returnValue" *: *true' "$reply" || fail "listLaunchPoints refused"

# name_ok NAME: an app ID or one file name: letters, digits, . _ -, and not
# . or ..
name_ok() {
    case $1 in
        '' | . | .. | *[!A-Za-z0-9._-]*) return 1 ;;
    esac
    [ ${#1} -le "$MAX_NAME" ]
}

# rel_ok PATH: a PNG's path inside an app's folder, in the same letters plus /,
# with no empty, . or .. part.
rel_ok() {
    [ ${#1} -le "$MAX_NAME" ] || return 1
    case $1 in
        '' | /* | */ | *//* | *[!A-Za-z0-9._/-]*) return 1 ;;
    esac
    case /$1/ in
        */./* | */../*) return 1 ;;
    esac
    case $1 in
        *.png | *.PNG) return 0 ;;
    esac
    return 1
}

# plan DIR REL DEST: adds "DEST/REL REAL" to the plan if DIR/REL stays inside
# DIR through any links and is a regular file. REAL is where it really is.
plan() {
    base=$(readlink -f "$1" 2>/dev/null) || return 1
    real=$(readlink -f "$1/$2" 2>/dev/null) || return 1
    case $real in
        "$base"/*) ;;
        *) return 1 ;;
    esac
    case $real in
        *[!A-Za-z0-9._/-]*) return 1 ;;
    esac
    [ -f "$real" ] || return 1
    printf '%s %s\n' "$3/$2" "$real" >>"$WORK/plan"
}

# app_icon PATH: plans one icon luna named, if it is under a fixed root, in a
# folder named as an app ID.
app_icon() {
    for root in /usr/palm/applications /media/cryptofs/apps/usr/palm/applications \
        /media/system/apps/usr/palm/applications /mnt/otncabi/usr/palm/applications; do
        case $1 in
            "$root"/*) ;;
            *) continue ;;
        esac
        rest=${1#"$root"/}
        id=${rest%%/*}
        rel=${rest#*/}
        [ "$rel" != "$rest" ] || return 1
        case $id in
            .*) return 1 ;;
        esac
        if ! name_ok "$id" || ! rel_ok "$rel"; then
            return 1
        fi
        plan "$PREFIX$root/$id" "$rel" "$NEW$root/$id"
        return
    done
    return 1
}

copied=0
refused=0
: >"$WORK/plan"

# Every "icon" and "largeIcon" string in the reply. A value with an escape in
# it ends at the backslash, which no path may hold.
grep -oE '"(icon|largeIcon)" *: *"[^"]*"' "$reply" |
    sed -e 's/^[^:]*: *"//' -e 's/"$//' | sort -u >"$WORK/paths"
n=0
while IFS= read -r path; do
    case $path in
        '' | "$DIRECT"*) continue ;;
    esac
    if [ "$n" -lt "$MAX_APP_ICONS" ] && app_icon "$path"; then
        n=$((n + 1))
    else
        refused=$((refused + 1))
    fi
done <"$WORK/paths"

n=0
for icon in "$PREFIX$INPUTS"/*; do
    [ -e "$icon" ] || [ -L "$icon" ] || continue
    name=${icon##*/}
    case $name in
        *.png | *.PNG) ;;
        *) continue ;;
    esac
    if [ "$n" -lt "$MAX_INPUT_ICONS" ] && name_ok "$name" &&
        plan "$PREFIX$INPUTS" "$name" "$NEW$INPUTS"; then
        n=$((n + 1))
    else
        refused=$((refused + 1))
    fi
done

# The copy is redone only if the script, the plan or any planned file
# changed: an app added or removed, or an icon replaced where it was.
sum=$({
    echo "$self_sum"
    cat "$WORK/plan"
    while read -r _ real; do
        ls -lnL "$real" 2>/dev/null || true
    done <"$WORK/plan"
} | sha256sum)
if [ -f "$OUT/.sum" ] && [ "$(cat "$OUT/.sum")" = "$sum" ]; then
    echo "lumen-icons: unchanged"
    exit 0
fi

# copy_one DEST REAL: copies REAL to DEST if what was read is a PNG within the
# caps. The bytes are checked after the copy, so a file swapped after the
# plan still cannot put anything else into Lumen's folder.
total=0
copy_one() {
    part=$WORK/part
    if ! head -c $((MAX_FILE + 1)) "$2" >"$part" 2>/dev/null; then
        return 1
    fi
    size=$(wc -c <"$part")
    size=$((size))
    if [ "$size" -gt "$MAX_FILE" ] || [ $((total + size)) -gt "$MAX_TOTAL" ] ||
        ! head -c 8 "$part" | cmp -s - "$WORK/png"; then
        return 1
    fi
    [ ! -d "$1" ] || return 1
    mkdir -p "${1%/*}" 2>/dev/null || return 1
    mv -f "$part" "$1" 2>/dev/null || return 1
    total=$((total + size))
}

mkdir "$NEW" || fail "cannot make $NEW"
printf '\211PNG\r\n\032\n' >"$WORK/png"
while read -r dest real; do
    if copy_one "$dest" "$real"; then
        copied=$((copied + 1))
    else
        refused=$((refused + 1))
    fi
done <"$WORK/plan"

line="lumen-icons: changed, $copied copied, $refused refused"
printf '%s\n' "$sum" >"$NEW/.sum" || fail "cannot write $NEW/.sum"
printf '%s\n' "$line" >"$NEW/status" || fail "cannot write $NEW/status"
# What a power cut leaves after this is whole.
sync
# A leftover lg-icons.old beside lg-icons is not needed, and mv would put
# lg-icons inside it.
if [ -d "$OUT" ]; then
    rm -rf "$OLD"
fi
if [ -e "$OUT" ] || [ -L "$OUT" ]; then
    mv "$OUT" "$OLD" || fail "cannot move $OUT aside"
fi
mv "$NEW" "$OUT" || fail "cannot put $NEW in place"
rm -rf "$OLD"
echo "$line"
