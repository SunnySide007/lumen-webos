#!/bin/sh
# Undoes Lumen Guard, stage G1, without a reboot (docs/specs/privacy.md):
#
#   tools/tv.sh script profiles/privacy/guard-undo.sh
#
# Removes the nat rules first, so lookups go straight to the router again,
# then stops Guard: SIGTERM, so it writes its status, and SIGKILL after 5 s.
# Its files and log stay in /var/lib/lumen/guard. A reboot does the same
# unless the boot hook is installed, which tools/guard.sh remove deletes
# before it runs this; tools/uninstall-all.sh runs it too. Safe to run twice.
# busybox sh.

PIDFILE=/tmp/lumen-guard.pid
LISTEN=127.0.0.2:53
DSCP=7

rule() {
    iptables -t nat "$1" OUTPUT -p "$2" --dport 53 ! -d 127.0.0.0/8 \
        -m dscp ! --dscp "$DSCP" -j DNAT --to-destination "$LISTEN"
}

for proto in udp tcp; do
    n=0
    while [ "$n" -lt 10 ] && rule -D "$proto" 2>/dev/null; do
        n=$((n + 1))
    done
done
if iptables -t nat -L OUTPUT -n | grep -q "to:$LISTEN"; then
    echo "guard undo: a nat rule to $LISTEN is still there:"
    iptables -t nat -L OUTPUT -n --line-numbers
else
    echo "guard undo: no nat rules to $LISTEN"
fi

pid=$(cat "$PIDFILE" 2>/dev/null)
case $pid in '' | *[!0-9]*) pid= ;; esac
if [ -n "$pid" ] && grep -q lumen-guard "/proc/$pid/cmdline" 2>/dev/null; then
    kill -TERM "$pid"
    i=0
    while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 5 ]; do
        sleep 1
        i=$((i + 1))
    done
    if kill -0 "$pid" 2>/dev/null; then
        kill -KILL "$pid"
        echo "guard undo: Guard (pid $pid) killed"
    else
        echo "guard undo: Guard (pid $pid) stopped"
    fi
else
    echo "guard undo: Guard not running"
fi
rm -f "$PIDFILE"
