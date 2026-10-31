#!/bin/sh
# Read-only privacy and exposure survey of a rooted LG webOS TV.
# tools/recon.sh --privacy pipes this to the TV over SSH and saves what it
# prints; docs/specs/privacy.md says what each section answers.
#
# Nothing here writes to the TV: it reads /proc, /sys, /etc and /var, lists
# sockets, processes and upstart jobs, and calls luna-send on get* and list*
# methods only (luna() refuses anything else). iptables only lists, and only
# when its tables are already in the kernel, so nothing loads a module.
# Secrets never print: the root password shows as set, empty or locked, SSH
# keys as a count, IPv6 addresses as their kind.
# It runs under the TV's busybox sh, so it sticks to POSIX shell.

# Older busybox (webOS 4) spells it `timeout -t SECS`, newer `timeout SECS`.
if timeout 1 true >/dev/null 2>&1; then
    T="timeout 10"
elif timeout -t 1 true >/dev/null 2>&1; then
    T="timeout -t 10"
else
    T=""
fi

section() {
    printf '\n===== %s =====\n' "$1"
}

show() {
    printf '$ %s\n' "$*"
    "$@" 2>&1 || printf '(exit %s)\n' "$?"
}

showfile() {
    for f in "$@"; do
        printf -- '--- %s\n' "$f"
        if [ -r "$f" ]; then cat "$f"; else echo '(absent)'; fi
    done
}

luna() {
    case "$1" in
        */get*|*/list*) ;;
        *) echo "luna $1: refused, not a get or list method"; return ;;
    esac
    printf '$ luna %s %s\n' "$1" "$2"
    # shellcheck disable=SC2086
    $T luna-send -n 1 -f "luna://$1" "$2" </dev/null 2>&1 || echo '(no answer)'
}

section identity
show date -u
show uname -r
luna com.webos.service.tv.systemproperty/getSystemInfo \
    '{"keys":["modelName","firmwareVersion","sdkVersion"]}'

# LG's own switches: viewing data (Live Plus), ads, recommendations, and the
# settings that decide what runs while the TV is "off".
section consent-settings
luna com.webos.settingsservice/getSystemSettings \
    '{"category":"general","keys":["homePromotion","customizedAd","personalRecommend","lmt","aiNudge","screenSaverAd","nativeConsentAd","welcomeFeature","launchEulaByHome","homeAutoLaunch"]}'
luna com.webos.settingsservice/getSystemSettings \
    '{"category":"option","keys":["livePlus","enableIpControl","quickStartMode","appUpdateMode","autoSmartServiceCountry","ibb","ibbDnt","turnOnByVoice","speakToTv","channelplus"]}'
luna com.webos.settingsservice/getSystemSettings '{"category":"network"}'

# Can the kernel filter or route packets? Decides between an egress firewall
# and blackhole routes for addresses that bypass DNS.
section firewall
for tool in iptables ip6tables nft ip tc; do
    printf '%s: %s\n' "$tool" "$(command -v "$tool" 2>/dev/null || echo absent)"
done
showfile /proc/net/ip_tables_names /proc/net/ip6_tables_names \
    /proc/net/ip_tables_targets /proc/net/ip_tables_matches
if [ -r /proc/config.gz ]; then
    printf '$ kernel config (modules, netfilter, routing, IPv6, tun)\n'
    zcat /proc/config.gz | grep -E '^CONFIG_(MODULES|NETFILTER|NF_|IP_NF_|IP6_NF_|IP_ADVANCED_ROUTER|IP_MULTIPLE_TABLES|IPV6|TUN|NET_SCHED|NET_CLS|BPF)' \
        || echo '(none matched)'
else
    echo '/proc/config.gz: (absent)'
fi
printf '$ netfilter and tun modules on disk\n'
find /lib/modules -name '*.ko' 2>/dev/null \
    | grep -E '/(nf_|xt_|ipt_|ip6t_|iptable_|ip6table_|x_tables|ip_tables|ip6_tables|tun)' \
    || echo '(none)'
if [ -r /proc/net/ip_tables_names ] && command -v iptables >/dev/null 2>&1; then
    show iptables -L -n -v
fi
if [ -r /proc/net/ip6_tables_names ] && command -v ip6tables >/dev/null 2>&1; then
    show ip6tables -L -n -v
fi
if command -v ip >/dev/null 2>&1; then
    show ip rule
    show ip -4 route
    show ip -6 route
fi

# Where lookups go: the resolver the C library uses, who answers on port 53,
# and whether LG's processes see the same /etc/hosts as this shell.
section dns
show ls -l /etc/resolv.conf /etc/hosts /etc/nsswitch.conf
showfile /etc/resolv.conf /etc/nsswitch.conf /etc/connman/main.conf
printf '$ firmware update hosts in /etc/hosts\n'
for name in snu.lge.com su.lge.com su-ssl.lge.com su-dev.lge.com nsu.lge.com; do
    if grep -q -w -F "$name" /etc/hosts; then
        echo "$name: listed"
    else
        echo "$name: NOT listed"
    fi
done
printf 'mnt namespace, this shell: %s\n' "$(readlink /proc/self/ns/mnt)"
printf 'mnt namespace, init (pid 1): %s\n' "$(readlink /proc/1/ns/mnt)"
# busybox on webOS 4 has sha256sum but not always cmp.
mine=$(sha256sum < /etc/hosts 2>/dev/null)
inits=$(sha256sum < /proc/1/root/etc/hosts 2>/dev/null)
if [ -z "$mine" ] || [ -z "$inits" ]; then
    echo '/etc/hosts: could not compare with init'
elif [ "$mine" = "$inits" ]; then
    echo '/etc/hosts: the same for init and this shell'
else
    echo '/etc/hosts: DIFFERENT for init and this shell'
fi
printf '$ mounts over /etc and /usr (this shell, then init)\n'
for pid in self 1; do
    awk -v who="$pid" '$5 ~ /^\/(etc|usr)\// { print who ": " $4 " on " $5 }' \
        "/proc/$pid/mountinfo" 2>/dev/null
done

# The TV's IPv6 addresses can carry its MAC address to every server it
# talks to, which identifies it across networks.
section ipv6
for nic in all default wlan0 eth0; do
    for key in disable_ipv6 use_tempaddr accept_ra autoconf; do
        printf '%s/%s: %s\n' "$nic" "$key" \
            "$(cat "/proc/sys/net/ipv6/conf/$nic/$key" 2>/dev/null || echo n/a)"
    done
done
printf '$ IPv6 addresses, address withheld: interface, scope, kind, flags\n'
if [ -r /proc/net/if_inet6 ]; then
    awk '{
        scope = "scope " $4
        if ($4 == "00") scope = "global"
        if ($4 == "20") scope = "link"
        if ($4 == "10") scope = "host"
        kind = (substr($1, 23, 4) == "fffe") ? "EUI-64, built from the MAC" : "not from the MAC"
        print $6 ": " scope ", " kind ", flags " $5
    }' /proc/net/if_inet6
else
    echo '(no IPv6)'
fi

# What anyone on the home network can reach.
section listening
printf '$ netstat -lntup\n'
netstat -lntup 2>/dev/null || netstat -lntu 2>&1

# busybox netstat can't name the programs, so: /proc/net/* gives each
# listening socket's inode, and every process's fd links name the inodes
# it holds.
section listening-owners
links=$(for fd in /proc/[0-9]*/fd/*; do
    link=$(readlink "$fd" 2>/dev/null) || continue
    case $link in
        socket:*) pid=${fd#/proc/}; echo "${link#socket:} ${pid%%/*}" ;;
    esac
done)
printf 'table bound-to:port program\n'
for table in tcp tcp6 udp udp6; do
    [ -r "/proc/net/$table" ] || continue
    awk -v t="$table" 'NR != 1 && (($4 == "0A" && t ~ /tcp/) || ($4 == "07" && t ~ /udp/)) {
        split($2, a, ":"); print t, a[1], a[2], $10 }' "/proc/net/$table"
done | while read -r table addr port inode; do
    case $addr in
        00000000|00000000000000000000000000000000) where=any ;;
        0100007F|00000000000000000000000001000000) where=loopback ;;
        *) where=one-address ;;
    esac
    pid=$(printf '%s\n' "$links" | awk -v i="[$inode]" '$1 == i { print $2; exit }')
    prog='(unknown)'
    if [ -n "$pid" ]; then
        prog=$(tr '\0' '\n' < "/proc/$pid/cmdline" 2>/dev/null | head -n 1)
        [ -n "$prog" ] || prog=$(cat "/proc/$pid/comm" 2>/dev/null)
    fi
    printf '%s %s:%s %s\n' "$table" "$where" "$((0x$port))" "$prog"
done | sort -u

section remote-access
printf '$ processes that accept logins, with their arguments\n'
logins=0
for d in /proc/[0-9]*; do
    case $(cat "$d/comm" 2>/dev/null) in
        dropbear|sshd|telnetd|utelnetd)
            logins=$((logins + 1))
            tr '\0' ' ' < "$d/cmdline" 2>/dev/null
            echo
            ;;
    esac
done
[ "$logins" -gt 0 ] || echo '(none)'
if [ -r /etc/shadow ]; then
    hash=$(sed -n 's/^root:\([^:]*\):.*/\1/p' /etc/shadow)
    case $hash in
        '') state=empty ;;
        '!'*|'*'*) state=locked ;;
        *) state='set' ;;
    esac
    echo "root password: $state"
    unset hash
else
    echo '/etc/shadow: (unreadable)'
fi
for f in /home/root/.ssh/authorized_keys /var/lib/webosbrew/sshd/authorized_keys; do
    if [ -r "$f" ]; then
        printf '%s: %s key(s)\n' "$f" "$(grep -c -v -E '^[[:space:]]*(#|$)' "$f")"
    else
        echo "$f: (absent)"
    fi
done

# Who sends: upstart jobs, running process names and luna services, to pick
# the telemetry senders that can be stopped at run time.
section services
printf '$ initctl list\n'
initctl list 2>&1 || echo '(initctl failed)'
printf '$ process names (count)\n'
for d in /proc/[0-9]*; do cat "$d/comm" 2>/dev/null; done | sort | uniq -c | sort -rn
for dir in /usr/share/luna-service2/services.d /usr/share/dbus-1/system-services; do
    printf '$ ls %s\n' "$dir"
    ls "$dir" 2>/dev/null || echo '(absent)'
done

# How the jobs that look like reporters start, and whether upstart restarts
# them, before any is stopped by hand.
section sender-jobs
for job in remotelogger uploadd rdxd crashreportd crashd contentminer nudge \
    user-context-manager event-monitor qosm update kadpd dpp eim nopd asc utp gm tvrm; do
    f=/etc/init/$job.conf
    printf -- '--- %s\n' "$f"
    if [ -r "$f" ]; then
        grep -E '^[[:space:]]*(description|start on|stop on|respawn|exec|script)' "$f" \
            || echo '(no stanzas matched)'
    else
        echo '(absent)'
    fi
done
printf -- '--- xinetd services\n'
for f in /etc/xinetd.d/*; do
    [ -r "$f" ] || continue
    printf -- '--- %s\n' "$f"
    grep -E '^[[:space:]]*(service|disable|port|server|socket_type|only_from|bind)' "$f"
done

# Crash reports and logs waiting to be sent.
section reports
for d in /var/spool/rdxd /var/log /tmp/var/log /var/lib/sdx /mnt/lg/cache/sdp; do
    if [ -d "$d" ]; then
        printf '%s: %s KB in %s files\n' "$d" "$(du -sk "$d" 2>/dev/null | cut -f1)" \
            "$(find "$d" -type f 2>/dev/null | wc -l)"
    else
        echo "$d: (absent)"
    fi
done

section boot-hooks
show ls -l /var/lib/webosbrew/init.d

section end
echo "privacy survey complete"
