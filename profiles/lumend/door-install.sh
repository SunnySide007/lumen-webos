#!/bin/sh
# Installs lumend's door as a boot hook, and retires the boot hooks whose
# work lumend now does (docs/specs/depth.md, item 18):
#
#   tools/lumend.sh door install   (after tools/backup.sh, and only once each
#                                   part it retires was tried by hand)
#
# The door comes from the inert copy tools/lumend.sh push left in
# /var/lib/lumen/lumend/door. A hook is retired only when lumend.conf
# switches on the part that replaces it: `s1 on` retires lumen-privacy,
# `lg-home on` retires lumen-home. Retired hooks are moved, not deleted, to
# /var/lib/lumen/lumend/retired/, and door-remove.sh puts them back. Lumen
# Guard's hook is not lumend's (lane H). Prints the installed door's
# SHA-256 last. busybox sh.
set -eu

DIR=/var/lib/lumen/lumend
INITD=/var/lib/webosbrew/init.d/
DOOR=${INITD}lumend
CONF=$DIR/lumend.conf
RETIRED=$DIR/retired

fail() {
    echo "door-install.sh: $*" >&2
    exit 1
}

if [ ! -f $DIR/door ] || [ -L $DIR/door ]; then fail "no $DIR/door: tools/lumend.sh push first; nothing was changed"; fi
if [ ! -f $DIR/lumend ] || [ -L $DIR/lumend ]; then fail "no $DIR/lumend; nothing was changed"; fi
if [ ! -d $INITD ] || [ -L ${INITD%/} ]; then fail "no Homebrew Channel boot hook folder; nothing was changed"; fi
sh -n $DIR/door || fail "the door does not parse; nothing was changed"

# on PART: whether lumend.conf switches PART on.
on() {
    [ -f $CONF ] && grep -qE "^[[:space:]]*$1[[:space:]]+on[[:space:]]*\$" $CONF
}

for pair in s1:lumen-privacy lg-home:lumen-home; do
    part=${pair%%:*} hook=${pair#*:}
    if [ ! -f "$INITD$hook" ]; then
        echo "no $hook hook to retire"
    elif on "$part"; then
        (umask 077 && mkdir -p $RETIRED)
        mv -f "$INITD$hook" "$RETIRED/$hook"
        echo "retired $hook (lumend's $part does its work) to $RETIRED/"
    else
        echo "kept $hook: lumend.conf does not switch $part on"
    fi
done

cp $DIR/door $DOOR.tmp
chmod 755 $DOOR.tmp
mv -f $DOOR.tmp $DOOR
echo "installed $DOOR"
sha256sum <$DOOR | cut -d' ' -f1
