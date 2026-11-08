#!/bin/sh
# Stops lumen-remote and takes out its INPUT rule (docs/specs/remote.md,
# R4), which leaves the TV as it was before web-start.sh:
#
#   tools/remote.sh web stop    (which runs this with tools/tv.sh script)
#
# Its lumen-ctl ends with it, as its link closes. Paired phones stay paired
# (devices.json); they reach nothing until lumen-remote runs again. Safe to
# run twice. busybox sh.

DIR=/var/lib/lumen/remote
PORT=8443

pids() {
    for p in /proc/[0-9]*; do
        if tr '\0' ' ' 2>/dev/null <"$p/cmdline" | grep -q "^$DIR/lumen-remote "; then
            echo "${p#/proc/}"
        fi
    done
}

list=$(pids)
if [ -n "$list" ]; then
    # shellcheck disable=SC2086
    kill $list 2>/dev/null
    i=0
    while [ -n "$(pids)" ] && [ $i -lt 5 ]; do
        sleep 1
        i=$((i + 1))
    done
    list=$(pids)
    # shellcheck disable=SC2086
    [ -z "$list" ] || kill -9 $list 2>/dev/null
    echo "web-stop: lumen-remote stopped"
else
    echo "web-stop: lumen-remote was not running"
fi

# Every rule on the port, by its number, whatever network it names.
n=0
while [ $n -lt 10 ]; do
    num=$(iptables -L INPUT -n --line-numbers | grep -E "tcp dpt:$PORT( |\$)" | head -n 1 | cut -d' ' -f1)
    [ -n "$num" ] || break
    iptables -D INPUT "$num" || break
    n=$((n + 1))
done
if iptables -L INPUT -n | grep -Eq "tcp dpt:$PORT( |\$)"; then
    echo "web-stop: FAILED to take out the $PORT rule" >&2
    exit 1
fi
echo "web-stop: no $PORT rule"
