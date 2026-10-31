#!/bin/sh
# What Lumen Remote has on the TV, read only (docs/specs/remote.md, R2):
#
#   tools/remote.sh status      (which runs this with tools/tv.sh script)
#
# lumen-ctl's SHA-256, its settings, the key template, the log's size, the
# sessions running now, and the lumen-remote lines in root's authorized_keys
# with each key given as the SHA-256 of its base64, never the key itself.
# busybox sh.

AK=/home/root/.ssh/authorized_keys
DIR=/var/lib/lumen/remote
OURS='^restrict,command="/var/lib/lumen/remote/lumen-ctl serve" ssh-ed25519 [A-Za-z0-9+/]*=* lumen-remote$'

if [ -f $DIR/lumen-ctl ]; then
    echo "binary: $(sha256sum <$DIR/lumen-ctl | cut -d' ' -f1)"
else
    echo "binary: none"
fi
if [ -f $DIR/ctl.conf ]; then
    echo "ctl.conf: $(grep -v '^[[:space:]]*#' $DIR/ctl.conf | tr '\n' ';')"
else
    echo "ctl.conf: none (bus command, keys none)"
fi
if [ -f $DIR/keys.json ]; then
    echo "keys.json: $(sha256sum <$DIR/keys.json | cut -d' ' -f1)"
else
    echo "keys.json: none"
fi
for f in lumen-ctl.log lumen-ctl.log.1 authorized_keys.before; do
    if [ -f $DIR/$f ]; then
        echo "$f: $(wc -c <$DIR/$f | tr -d ' ') bytes"
    else
        echo "$f: none"
    fi
done
n=0
for p in /proc/[0-9]*; do
    if tr '\0' ' ' 2>/dev/null <"$p/cmdline" | grep -q "^$DIR/lumen-ctl serve"; then
        n=$((n + 1))
    fi
done
echo "sessions: $n"
if [ -f $AK ]; then
    echo "authorized_keys: $(grep -c '[^[:space:]]' $AK) lines"
    # The key's base64 hashed, so it can be compared with the PC's.
    grep ' lumen-remote$' $AK | while IFS= read -r line; do
        if printf '%s\n' "$line" | grep -q "$OURS"; then
            key=${line##*ssh-ed25519 }
            key=${key%% *}
            echo "lumen-remote line: key $(printf '%s' "$key" | sha256sum | cut -d' ' -f1)"
        else
            echo "lumen-remote line: other options or another key type"
        fi
    done
else
    echo "authorized_keys: none"
fi
