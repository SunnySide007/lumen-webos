#!/bin/sh
# Undoes privacy stage S1 without a reboot (docs/specs/privacy.md):
#
#   tools/tv.sh script profiles/privacy/s1-undo.sh
#
# Removes the tcp 11111 rule and starts LG's remote-diagnostics jobs again,
# in their boot order. A reboot does the same. busybox sh.

rule() {
    iptables "$1" INPUT -p tcp --dport 11111 ! -i lo -j REJECT
}

while iptables -L INPUT -n | grep -q 'tcp dpt:11111'; do
    rule -D || break
done
echo "s1 undo: 11111 rule removed"

# crashreportd also starts by itself once rdxd has; asking again is harmless.
for job in remotelogger rdxd crashreportd uploadd; do
    initctl start "$job" >/dev/null 2>&1
    initctl status "$job"
done
