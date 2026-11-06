#!/bin/sh
# Takes lumend away (docs/safety.md, rule 2):
#
#   tools/lumend.sh remove       (tools/uninstall-all.sh runs it first)
#
# In order: the door (so no boot starts lumend) and the hooks it retired put
# back; the daemon stopped; its RAM changes undone by its own `undo`: the
# stand-ins unmounted, so LG's programs show again, and the socket out of
# Lumen's jail; its luna role; /var/lib/lumen/lumend and its log. What S1
# stopped stays stopped until the next boot, as the old hook's did. If the
# binary is gone, a stand-in still mounted over a sender's program is
# unmounted here. Safe to run twice. busybox sh.
set -u

DIR=/var/lib/lumen/lumend
BIN=$DIR/lumend
INITD=/var/lib/webosbrew/init.d/
ID=io.github.sunnyside007.lumend
LS2=/var/luna-service2-dev
# senders.rs's SENDERS: the programs a stand-in may be mounted over.
PROGRAMS=/usr/sbin/acr2
failed=0

if [ -e ${INITD}lumend ] || [ -L ${INITD}lumend ]; then
    rm -f ${INITD}lumend && echo "removed the door"
fi
for hook in lumen-privacy lumen-home; do
    if [ -f "$DIR/retired/$hook" ] && [ ! -L "$DIR/retired/$hook" ]; then
        mv -f "$DIR/retired/$hook" "$INITD$hook" && echo "put $hook back"
    fi
done

if [ -f $BIN ] && [ ! -L $BIN ]; then
    $BIN stop || failed=1
    $BIN undo senders || failed=1
    $BIN undo link || failed=1
else
    echo "no $BIN to stop or undo with"
fi
for p in $PROGRAMS; do
    if grep -q " $p " /proc/mounts; then
        if umount "$p" 2>/dev/null || umount -l "$p"; then
            echo "unmounted the stand-in over $p"
        else
            echo "FAILED to unmount the stand-in over $p" >&2
            failed=1
        fi
    fi
done

for f in $LS2/roles.d/$ID.json $LS2/client-permissions.d/$ID.json $LS2/manifests.d/$ID.json; do
    if [ -e "$f" ] || [ -L "$f" ]; then
        rm -f "$f" && echo "removed $f"
    fi
done
if command -v ls-control >/dev/null 2>&1; then
    ls-control scan-services >/dev/null 2>&1 || true
fi

rm -rf $DIR && echo "removed $DIR"
rm -f /var/lib/lumen/log/lumend.log /var/lib/lumen/log/lumend.log.1
[ $failed -eq 0 ] || {
    echo "remove.sh: a step failed; see above" >&2
    exit 1
}
