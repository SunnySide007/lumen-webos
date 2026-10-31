#!/bin/sh
# Puts route D's boot hook (takeover/lumen-home) on the TV, for
#
#   tools/takeover.sh hook push       its by-hand copy, /var/lib/lumen/takeover/boot-hook
#   tools/takeover.sh hook install    that copy, and /var/lib/webosbrew/init.d/lumen-home
#
# which send this file with the hook in staged_copy, the PC's SHA-256 of the
# hook in WANT and INITD=1 to install it. Nothing passes through /tmp, which
# every app's jail shares: the hook is written to /var/lib/lumen/stage, a
# folder only root may change, checked against WANT and with sh -n, and only
# then copied, each copy moved into place whole. The staged copy is removed
# on every exit. A hook that differs from the PC's by a byte changes nothing.
# It prints each copy's SHA-256 for the PC to compare. busybox sh.

DIR=/var/lib/lumen/takeover
HOOK_DEST=/var/lib/webosbrew/init.d/lumen-home
STAGE=/var/lib/lumen/stage
STAGED=$STAGE/lumen-home
ROOT_UID=0
WANT=${WANT:-}
INITD=${INITD:-0}

# sha FILE: its SHA-256, or "none".
sha() {
    if [ -f "$1" ]; then sha256sum <"$1" | cut -d' ' -f1; else echo none; fi
}

refuse() {
    echo "takeover hook: $*; nothing changed"
    exit 1
}

# stage_ok FOLDER...: each is a real folder, not a link, owned by root and
# writable by no one else, so nothing in an app's jail can put a file there
# or change one. The folder above the stage is checked before the stage is
# made in it, and the stage before anything is written there.
stage_ok() {
    for d in "$@"; do
        if [ -L "$d" ] || [ ! -d "$d" ]; then
            echo "takeover: $d is not a real folder"
            return 1
        fi
        # ls -ln is POSIX, and stat is not: its mode and owner, split into words.
        # shellcheck disable=SC2046,SC2012
        set -- $(ls -ldn "$d")
        case $1 in
            d????[!w]??[!w]*) ;;
            *)
                echo "takeover: $d is writable by others ($1)"
                return 1
                ;;
        esac
        if [ "$3" != "$ROOT_UID" ]; then
            echo "takeover: $d is owned by uid $3, not root"
            return 1
        fi
    done
}

case $WANT in
    *[!0-9a-f]* | '') refuse "no SHA-256 from the PC: run tools/takeover.sh hook push" ;;
esac
[ ${#WANT} -eq 64 ] || refuse "no SHA-256 from the PC: run tools/takeover.sh hook push"
command -v staged_copy >/dev/null 2>&1 || refuse "no hook from the PC: run tools/takeover.sh hook push"

mkdir -p "$DIR" "${STAGE%/*}" || refuse "could not make $DIR"
stage_ok "${STAGE%/*}" || refuse "${STAGE%/*} is not safe to stage in"
(umask 077 && mkdir -p "$STAGE") || refuse "could not make $STAGE"
stage_ok "$STAGE" || refuse "$STAGE is not safe to stage in"
umask 077
trap 'rm -f "$STAGED"' EXIT
rm -f "$STAGED"
staged_copy >"$STAGED" || refuse "could not write $STAGED"
[ "$(sha "$STAGED")" = "$WANT" ] || refuse "the staged hook is not the one the PC sent"
sh -n "$STAGED" || refuse "the hook does not parse"

if cp "$STAGED" "$DIR/boot-hook.tmp" && chmod 644 "$DIR/boot-hook.tmp" &&
    mv "$DIR/boot-hook.tmp" "$DIR/boot-hook"; then
    echo "boot-hook: $(sha "$DIR/boot-hook")"
else
    rm -f "$DIR/boot-hook.tmp"
    echo "takeover hook: FAILED to put $DIR/boot-hook in place"
    exit 1
fi
if [ "$INITD" = 1 ]; then
    # run-parts skips a name with a dot, so a boot never meets the .tmp.
    if cp "$STAGED" "$HOOK_DEST.tmp" && chmod 755 "$HOOK_DEST.tmp" &&
        mv "$HOOK_DEST.tmp" "$HOOK_DEST"; then
        echo "init.d: $(sha "$HOOK_DEST")"
    else
        rm -f "$HOOK_DEST.tmp"
        echo "takeover hook: FAILED to install $HOOK_DEST"
        exit 1
    fi
fi
