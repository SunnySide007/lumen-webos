#!/bin/sh
# Undoes Lumen Remote on the TV (docs/specs/remote.md, R2):
#
#   tools/remote.sh remove      (which runs this with tools/tv.sh script)
#
# Takes Lumen Remote's own line out of root's authorized_keys: the one that
# starts with restrict,command="/var/lib/lumen/remote/lumen-ctl serve" and
# ends with lumen-remote, and no other. It refuses, changing nothing, when no
# other key would be left, since that would lock out tools/tv.sh. Then it
# deletes /var/lib/lumen/remote/: lumen-ctl, its settings, its log and the
# copy of authorized_keys from before the first install. A session that is
# open keeps running until its connection ends. Safe to run twice. busybox
# sh.
set -eu

AK=/home/root/.ssh/authorized_keys
DIR=/var/lib/lumen/remote
OURS='^restrict,command="/var/lib/lumen/remote/lumen-ctl serve" ssh-ed25519 [A-Za-z0-9+/]*=* lumen-remote$'
NEW=$AK.lumen-new

fail() {
    echo "remove.sh: $*; nothing was changed" >&2
    exit 1
}

if [ -f $AK ] && grep -q "$OURS" $AK; then
    [ ! -L $AK ] || fail "$AK is a link"
    # Keys left: lines that are not ours, not comments and not blank.
    left=$(grep -v "$OURS" $AK | grep -v '^[[:space:]]*#' | grep -c '[^[:space:]]' || true)
    [ "$left" -gt 0 ] || fail "Lumen Remote's is the only key in $AK, and removing it would leave no way in"
    ours=$(grep -c "$OURS" $AK)
    lines=$(grep -c '' $AK)
    rm -f $NEW
    cp -p $AK $NEW
    grep -v "$OURS" $AK >$NEW || true
    if [ "$(grep -c '' $NEW)" != $((lines - ours)) ] || grep -q "$OURS" $NEW; then
        rm -f $NEW
        fail "the copy without Lumen Remote's line came out wrong"
    fi
    mv $NEW $AK
    echo "removed Lumen Remote's line from $AK"
else
    echo "no Lumen Remote line in $AK"
fi

if [ -e $DIR ]; then
    rm -rf $DIR
    echo "removed $DIR"
else
    echo "no $DIR"
fi
