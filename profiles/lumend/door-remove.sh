#!/bin/sh
# door-install.sh's undo:
#
#   tools/lumend.sh door remove
#
# Deletes lumend's door, so the next boot starts no lumend, and puts back
# the boot hooks it retired. A lumend that runs keeps running until it is
# stopped (tools/lumend.sh stop) or the TV restarts. Safe to run twice.
# busybox sh.
set -eu

DIR=/var/lib/lumen/lumend
INITD=/var/lib/webosbrew/init.d/
DOOR=${INITD}lumend
RETIRED=$DIR/retired

if [ -e $DOOR ] || [ -L $DOOR ]; then
    rm -f $DOOR
    echo "removed $DOOR"
else
    echo "no $DOOR"
fi
for hook in lumen-privacy lumen-home; do
    if [ -f "$RETIRED/$hook" ] && [ ! -L "$RETIRED/$hook" ]; then
        mv -f "$RETIRED/$hook" "$INITD$hook"
        echo "put $hook back"
    fi
done
