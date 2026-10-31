#!/bin/sh
# Adds Lumen Remote's key to root's authorized_keys with lumen-ctl as its
# forced command (docs/specs/remote.md, R2). tools/remote.sh install puts the
# PC's public key in place of the placeholder below and runs this with
# tools/tv.sh script, once lumen-ctl is in /var/lib/lumen/remote/.
#
# The line, last in the file:
#
#   restrict,command="/var/lib/lumen/remote/lumen-ctl serve" ssh-ed25519 <key> lumen-remote
#
# The file is copied to authorized_keys.before in /var/lib/lumen/remote/ the
# first time, and never after. It is changed only if no lumen-remote line is
# there yet, through a copy that is checked and then moved over it whole, so
# a dropped connection never leaves half a file. Every other line stays as it
# was. tools/remote.sh remove takes the line out. busybox sh.
set -eu

KEY='@KEY@'
AK=/home/root/.ssh/authorized_keys
DIR=/var/lib/lumen/remote
LINE="restrict,command=\"/var/lib/lumen/remote/lumen-ctl serve\" ssh-ed25519 $KEY lumen-remote"
NEW=$AK.lumen-new

fail() {
    echo "authorize.sh: $*; authorized_keys was not changed" >&2
    exit 1
}

# The key went through tools/remote.sh's check already; it lands in the
# file, so check it again here.
case $KEY in
    *[!A-Za-z0-9+/]*) fail "the key is not base64: was the placeholder filled in?" ;;
    AAAAC3NzaC1lZDI1NTE5AAAAI*) ;;
    *) fail "the key is not an ed25519 key" ;;
esac
[ ${#KEY} -eq 68 ] || fail "the key is not 68 characters long"

[ -x $DIR/lumen-ctl ] || fail "no $DIR/lumen-ctl for the line to run"
if [ ! -f $AK ] || [ -L $AK ]; then
    fail "$AK is not a plain file"
fi

[ -e $DIR/authorized_keys.before ] || cp -p $AK $DIR/authorized_keys.before

if grep -q ' lumen-remote$' $AK; then
    if grep -qxF "$LINE" $AK; then
        echo "authorized_keys has this key's line already"
        exit 0
    fi
    fail "a lumen-remote line for another key or with other options is there; tools/remote.sh remove takes it out"
fi

others=$(grep -v ' lumen-remote$' $AK | sha256sum)
rm -f $NEW
cp -p $AK $NEW
# A last line without its newline would run into ours.
if [ -s $NEW ] && [ -n "$(tail -c 1 $NEW)" ]; then
    echo >>$NEW
fi
printf '%s\n' "$LINE" >>$NEW
if [ "$(grep -v ' lumen-remote$' $NEW | sha256sum)" != "$others" ] ||
    [ "$(grep -cxF "$LINE" $NEW)" != 1 ]; then
    rm -f $NEW
    fail "the new file's other lines differ from the old one's"
fi
mv $NEW $AK
echo "added Lumen Remote's line to $AK"
