#!/bin/sh
# Prints the TV's running state for tools/backup.sh: what is installed and
# running, and what Homebrew Channel, Glasshouse and Lumen have changed.
# Read-only: luna() calls get* and list* methods only. busybox sh.

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
show uptime
luna com.webos.service.tv.systemproperty/getSystemInfo \
    '{"keys":["modelName","firmwareVersion","sdkVersion","boardType","otaId"]}'

section apps
luna com.webos.applicationManager/listApps '{}'
luna com.webos.applicationManager/listLaunchPoints '{}'

section jobs
show initctl list

section changes
show iptables -L -n -v --line-numbers
show iptables -t nat -L -n -v --line-numbers
show cat /etc/hosts
show ls -la /var/luna/preferences
show ls -la /var/lib/webosbrew/init.d
show cat /proc/mounts

section storage
show df -k
