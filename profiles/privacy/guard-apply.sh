#!/bin/sh
# Lumen Guard, stage G1, started by hand (docs/specs/privacy.md, "Lumen Guard"):
#
#   tools/guard.sh push
#   tools/tv.sh script profiles/privacy/guard-apply.sh
#
# Runs the boot hook now, from the copy tools/guard.sh push left in
# /var/lib/lumen/guard/boot-hook. That copy is not in init.d, so a reboot
# undoes everything; guard-undo.sh does without one. It follows the hook's log
# (/var/lib/lumen/log/lumen-guard.log) until the start-up ends, at most about
# five minutes, then prints it, /etc/resolv.conf, the mount over connman's
# copy, the nat rules and Guard's status file. Exit status 0 means the TV's
# DNS goes through Guard. A hook with no safe log folder (it says why) starts
# Guard unlogged, so there is nothing to follow: exit status 1, and
# tools/guard.sh status after five minutes. Safe to run twice. busybox sh.

DIR=/var/lib/lumen/guard
LOG=/var/lib/lumen/log/lumen-guard.log

if [ -e /var/luna/preferences/lumen_disable ]; then
    echo "guard: /var/luna/preferences/lumen_disable exists; nothing done"
    exit 1
fi
for f in "$DIR/boot-hook" "$DIR/lumen-guard" "$DIR/rules/allowlist.txt"; do
    if [ ! -f "$f" ]; then
        echo "guard: no $f; run tools/guard.sh push first"
        exit 1
    fi
done

# The file's inode, or nothing.
inode() {
    # shellcheck disable=SC2046,SC2012
    set -- $(ls -id "$1" 2>/dev/null)
    echo "${1:-}"
}

# The hook moves its last log to $LOG.1 and makes $LOG anew before it returns,
# so only a $LOG that is not the file there before is this run's.
was=$(inode "$LOG")
sh "$DIR/boot-hook"
now=$(inode "$LOG")
if [ -z "$now" ] || [ "$now" = "$was" ]; then
    echo "guard: no log from this run; tools/guard.sh status in five minutes says whether Guard runs"
    exit 1
fi

outcome=
i=0
while [ "$i" -lt 330 ]; do
    case $(cat "$LOG") in
        *"goes through Guard"* | *"already runs"*) outcome=on; break ;;
        *"fail open"* | *"nothing done"*) outcome=off; break ;;
    esac
    sleep 1
    i=$((i + 1))
done

cat "$LOG"
echo "/etc/resolv.conf:"
cat /etc/resolv.conf
mount | grep resolv || echo "no mount over resolv.conf"
iptables -t nat -L OUTPUT -n -v
[ -f "$DIR/status" ] && cat "$DIR/status"
case $outcome in
    on) echo "guard: running; guard-undo.sh or a reboot undoes it" ;;
    off) echo "guard: not running; resolv.conf is connman's, no nat rules"; exit 1 ;;
    *) echo "guard: still starting after $i s; see $LOG"; exit 1 ;;
esac
