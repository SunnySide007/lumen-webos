#!/bin/sh
# Undoes Lumen Guard, stage G1, without a reboot (docs/specs/privacy.md):
#
#   tools/tv.sh script profiles/privacy/guard-undo.sh
#
# In the hook's fail-open order: unmounts Guard's resolv.conf (every copy),
# so lookups go through connman to the router again, then removes the nat
# rules, then stops Guard: SIGTERM, so it writes its status, and SIGKILL
# after 5 s. Prints connman's file afterwards. Guard's files and log stay in
# /var/lib/lumen/guard. A reboot does the same unless the boot hook is
# installed, which tools/guard.sh remove deletes before it runs this;
# tools/uninstall-all.sh runs it too. Safe to run twice. busybox sh.

DIR=/var/lib/lumen/guard
PIDFILE=$DIR/lumen-guard.pid
OURS=$DIR/resolv.conf
RESOLV=/var/lib/misc/resolv.conf
LISTEN=127.0.0.2:53

# The last rule to Guard by its number, for an iptables whose -S output
# doesn't feed back into -D.
remove_rule_by_number() {
    num=$(iptables -t nat -L OUTPUT -n --line-numbers |
        awk -v to="to:$LISTEN" 'index($0, to) { last = $1 } END { print last }')
    [ -n "$num" ] && iptables -t nat -D OUTPUT "$num"
}

# Every rule that sends lookups to Guard, deleted by the spec iptables prints
# for it, so it needs no upstream.
remove_rules() {
    n=0
    while :; do
        spec=$(iptables -t nat -S OUTPUT |
            sed -n "s/^-A \(OUTPUT .* --to-destination $LISTEN\)\$/\1/p" | head -n 1)
        [ -n "$spec" ] || return 0
        [ "$n" -lt 10 ] || return 1
        # shellcheck disable=SC2086 # the spec's words; none has a space
        iptables -t nat -D $spec 2>/dev/null || remove_rule_by_number || return 1
        n=$((n + 1))
    done
}

# Guard's file shows at connman's path, or a mount over it is listed.
mounted() {
    # shellcheck disable=SC3013 # busybox's test has -ef
    [ "$RESOLV" -ef "$OURS" ] || grep -q " $RESOLV " /proc/mounts 2>/dev/null
}

# Every copy of the mount, until connman's file shows again.
unmount() {
    n=0
    while mounted; do
        [ "$n" -lt 10 ] || return 1
        umount "$RESOLV" || sleep 1
        n=$((n + 1))
    done
}

# Whether $pid runs; a zombie still answers kill -0.
alive() {
    kill -0 "$pid" 2>/dev/null &&
        ! grep -q '^State:[[:space:]]*Z' "/proc/$pid/status" 2>/dev/null
}

# Guard's pid, if the pid file names a running Guard. The file outlives a
# reboot, so its pid may belong to anything by now, this hook's shell too.
running() {
    p=$(cat "$PIDFILE" 2>/dev/null) || return 1
    case $p in '' | *[!0-9]*) return 1 ;; esac
    tr '\0' ' ' 2>/dev/null <"/proc/$p/cmdline" |
        grep -q "lumen-guard --listen $LISTEN" || return 1
    echo "$p"
}

if unmount; then
    echo "guard undo: nothing mounted over $RESOLV"
else
    echo "guard undo: $RESOLV is still mounted:"
    grep " $RESOLV " /proc/mounts
fi
if remove_rules; then
    echo "guard undo: no nat rules to $LISTEN"
else
    echo "guard undo: a nat rule to $LISTEN is still there:"
    iptables -t nat -L OUTPUT -n --line-numbers
fi

if pid=$(running); then
    kill -TERM "$pid"
    i=0
    while alive && [ "$i" -lt 5 ]; do
        sleep 1
        i=$((i + 1))
    done
    if alive; then
        kill -KILL "$pid"
        echo "guard undo: Guard (pid $pid) killed"
    else
        echo "guard undo: Guard (pid $pid) stopped"
    fi
else
    echo "guard undo: Guard not running"
fi
rm -f "$PIDFILE"
echo "guard undo: /etc/resolv.conf now reads:"
cat /etc/resolv.conf
