#!/bin/sh
# Route D's boot hook, run by hand (takeover/README.md):
#
#   tools/takeover.sh hook push
#   tools/tv.sh script takeover/home-apply.sh
#
# Runs the hook now, from the copy tools/takeover.sh hook push left in
# /var/lib/lumen/takeover/boot-hook. That copy is not in init.d, so nothing
# here outlasts a reboot, and the hook only ever closes LG's Home, which SAM
# starts again when anything asks for it. It follows the hook's log
# (/tmp/lumen-home.log) until the first round is over, at most about a
# minute and a half, then prints it and SAM's running list. The hook goes on
# in the background for its other rounds, about four minutes;
# tools/takeover.sh hook status shows them. Exit status 0 means a round ran.
# busybox sh.

DIR=/var/lib/lumen/takeover
LOG=/tmp/lumen-home.log

if [ -e /var/luna/preferences/lumen_disable ]; then
    echo "takeover: /var/luna/preferences/lumen_disable exists; nothing done"
    exit 1
fi
if [ ! -f "$DIR/boot-hook" ]; then
    echo "takeover: no $DIR/boot-hook; run tools/takeover.sh hook push first"
    exit 1
fi

# Only the hook's lines from this run count.
from=1
[ -f "$LOG" ] && from=$(($(wc -l <"$LOG") + 1))
sh "$DIR/boot-hook"

outcome=
i=0
while [ "$i" -lt 90 ]; do
    case $(tail -n "+$from" "$LOG" 2>/dev/null) in
        *"round 1: LG Home is back"* | *"round 1: LG Home stayed"* | \
            *"round 1: LG Home is not running"* | *"round 1: LG Home is in front"*)
            outcome=round
            break
            ;;
        *"nothing done"* | *"lumen-home: done"*)
            outcome=none
            break
            ;;
    esac
    sleep 1
    i=$((i + 1))
done

tail -n "+$from" "$LOG" 2>/dev/null
# busybox before 1.30 wants `timeout -t SECS`; coreutils wants `timeout SECS`.
if timeout -t 1 true >/dev/null 2>&1; then
    TIMEOUT="timeout -t 10"
elif timeout 1 true >/dev/null 2>&1; then
    TIMEOUT="timeout 10"
else
    TIMEOUT=""
fi
echo "\$ luna-send -n 1 luna://com.webos.applicationManager/running '{}'"
# shellcheck disable=SC2086 # $TIMEOUT is a command and its argument
$TIMEOUT luna-send -n 1 luna://com.webos.applicationManager/running '{}' </dev/null 2>&1 || echo "(no answer)"
case $outcome in
    round) echo "takeover: the first round ran; the hook goes on for about four minutes" ;;
    none) echo "takeover: the hook did nothing; see its log above"; exit 1 ;;
    *) echo "takeover: no round after $i s; see $LOG"; exit 1 ;;
esac
