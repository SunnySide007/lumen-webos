#!/bin/sh
# Starts lumen-remote, the phone's door, by hand (docs/specs/remote.md, R4):
#
#   tools/remote.sh web start   (which runs this with tools/tv.sh script)
#
# Reads /var/lib/lumen/remote/web.conf (tools/remote.sh web install wrote
# it), checks that uid and gid 61101, which lumen-remote drops to, are
# nobody's on the TV and that nothing listens on tcp 8443, then rejects tcp
# 8443 from outside the home network with an INPUT rule and starts
# lumen-remote in a session of its own. It runs until web-stop.sh, `tools/
# remote.sh web remove` or the TV turning off: nothing here lasts past a
# reboot, and nothing starts it at boot. Safe to run twice. busybox sh.

DIR=/var/lib/lumen/remote
CONF=$DIR/web.conf
PORT=8443
UID_=61101

fail() {
    echo "web-start: $*" >&2
    exit 1
}

if [ -e /var/luna/preferences/lumen_disable ]; then
    fail "/var/luna/preferences/lumen_disable exists; nothing done"
fi

running() {
    for p in /proc/[0-9]*; do
        if tr '\0' ' ' 2>/dev/null <"$p/cmdline" | grep -q "^$DIR/lumen-remote "; then
            echo "${p#/proc/}"
            return 0
        fi
    done
    return 1
}

if pid=$(running); then
    echo "web-start: lumen-remote is running already (pid $pid)"
    exit 0
fi

for f in lumen-remote lumen-ctl web/index.html tls/server.crt tls/server.key web.conf; do
    [ -f "$DIR/$f" ] || fail "no $DIR/$f: tools/remote.sh web install first"
done

# web.conf: "listen ADDR", "allow NET/BITS" and "name NAME" lines, checked
# here again since they go on a command line.
listen=$(sed -n 's/^listen \([0-9.]*:[0-9]*\)$/\1/p' $CONF | tail -n 1)
allow=$(sed -n 's|^allow \([0-9.]*/[0-9]*\)$|\1|p' $CONF | tail -n 1)
names=$(sed -n 's/^name \([a-z0-9.-]*\)$/\1/p' $CONF | tr '\n' ' ')
if [ -z "$listen" ] || [ -z "$allow" ] || [ -z "$names" ]; then
    fail "$CONF lacks listen, allow or name"
fi
[ "${listen##*:}" = $PORT ] || fail "$CONF listens on ${listen##*:}, not $PORT"

if grep -q "^[^:]*:[^:]*:$UID_:" /etc/passwd 2>/dev/null; then
    fail "uid $UID_ is in /etc/passwd; lumen-remote needs a uid nothing on the TV uses"
fi
if grep -q "^[^:]*:[^:]*:$UID_:" /etc/group 2>/dev/null; then
    fail "gid $UID_ is in /etc/group; lumen-remote needs a gid nothing on the TV uses"
fi
for p in /proc/[0-9]*; do
    if grep -q "^Uid:[[:space:]]*${UID_}[[:space:]]" "$p/status" 2>/dev/null; then
        fail "a process runs as uid $UID_ (${p#/proc/})"
    fi
done
if netstat -lnt 2>/dev/null | grep -q ":$PORT "; then
    fail "something listens on tcp $PORT already"
fi

rule() {
    iptables "$1" INPUT -p tcp --dport $PORT ! -s "$allow" -j REJECT
}

# Ours only: a REJECT on the port from outside this network. Another rule
# on the port (an ACCEPT someone added) doesn't count, and ours goes above it.
ours() {
    iptables -L INPUT -n | grep '^REJECT ' | grep -F -- "!$allow " | grep -Eq "tcp dpt:$PORT( |\$)"
}

added=
if ours; then
    echo "web-start: the $PORT rule is already in place"
elif rule -I; then
    added=1
    echo "web-start: rejecting tcp $PORT from outside $allow"
else
    fail "could not add the $PORT rule; lumen-remote not started"
fi

set --
for n in $names; do set -- "$@" --name "$n"; done
rm -f $DIR/lumen-remote.out
# shellcheck disable=SC2086
setsid $DIR/lumen-remote --listen "$listen" --allow "$allow" "$@" \
    </dev/null >$DIR/lumen-remote.out 2>&1 &

i=0
while [ $i -lt 10 ]; do
    if grep -q '^lumen-remote: listening on' $DIR/lumen-remote.out 2>/dev/null; then
        break
    fi
    sleep 1
    i=$((i + 1))
done
if pid=$(running) && grep -q '^lumen-remote: listening on' $DIR/lumen-remote.out; then
    echo "web-start: lumen-remote runs (pid $pid), at https://${names%% *}:$PORT"
    echo "web-start: stop it with tools/remote.sh web stop"
    exit 0
fi
echo "web-start: lumen-remote did not start:" >&2
cat $DIR/lumen-remote.out >&2 2>/dev/null
if pid=$(running); then kill "$pid"; fi
if [ -n "$added" ]; then rule -D; fi
exit 1
