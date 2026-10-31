#!/bin/sh
# Lumen Guard, stage G1, started by hand (docs/specs/privacy.md, "Lumen Guard"):
#
#   tools/guard.sh push
#   tools/tv.sh script profiles/privacy/guard-apply.sh
#
# Runs the boot hook now, from the copy tools/guard.sh push left in
# /var/lib/lumen/guard/boot-hook. That copy is not in init.d, so a reboot
# undoes everything; guard-undo.sh does without one. It follows the hook's log
# (/tmp/lumen-guard.log) until the start-up ends, at most about five minutes,
# then prints it, the nat rules and Guard's status file. Exit status 0 means
# the TV's DNS goes through Guard. Safe to run twice. busybox sh.

DIR=/var/lib/lumen/guard
LOG=/tmp/lumen-guard.log

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

# Only the hook's lines from this run count.
from=1
[ -f "$LOG" ] && from=$(($(wc -l <"$LOG") + 1))
sh "$DIR/boot-hook"

outcome=
i=0
while [ "$i" -lt 330 ]; do
    case $(tail -n "+$from" "$LOG" 2>/dev/null) in
        *"goes through Guard"* | *"already runs"*) outcome=on; break ;;
        *"fail open"* | *"nothing done"*) outcome=off; break ;;
    esac
    sleep 1
    i=$((i + 1))
done

tail -n "+$from" "$LOG" 2>/dev/null
iptables -t nat -L OUTPUT -n -v
[ -f "$DIR/status" ] && cat "$DIR/status"
case $outcome in
    on) echo "guard: running; guard-undo.sh or a reboot undoes it" ;;
    off) echo "guard: not running, no nat rules"; exit 1 ;;
    *) echo "guard: still starting after $i s; see $LOG"; exit 1 ;;
esac
