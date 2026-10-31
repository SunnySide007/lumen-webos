#!/bin/sh
# Privacy stage S1, applied by hand (docs/specs/privacy.md, "Stages"):
#
#   tools/tv.sh script profiles/privacy/s1-apply.sh
#
# Stops LG's remote-diagnostics jobs (uploadd, rdxd, crashreportd,
# remotelogger) and rejects tcp 11111, crashreportd's port, from everything
# but loopback. Nothing here lasts past a reboot; s1-undo.sh puts it back
# without one. Safe to run twice. busybox sh.

if [ -e /var/luna/preferences/lumen_disable ]; then
    echo "s1: /var/luna/preferences/lumen_disable exists; nothing done"
    exit 1
fi

rule() {
    iptables "$1" INPUT -p tcp --dport 11111 ! -i lo -j REJECT
}

# crashreportd starts whenever rdxd does, so it stops first. A job that is
# already stopped makes initctl complain, which is fine.
for job in crashreportd rdxd uploadd remotelogger; do
    initctl stop "$job" >/dev/null 2>&1
    initctl status "$job"
done

if iptables -L INPUT -n | grep -q 'tcp dpt:11111'; then
    echo "s1: the 11111 rule is already in place"
elif rule -I; then
    echo "s1: rejecting tcp 11111 from the LAN"
else
    echo "s1: FAILED to add the 11111 rule"
fi
iptables -L INPUT -n -v --line-numbers

if netstat -lnt | grep -q ':11111 '; then
    echo "s1: something still listens on 11111"
else
    echo "s1: nothing listens on 11111"
fi
