#!/bin/sh
# Route D's state on the TV (takeover/README.md), read-only. Run it with
#
#   tools/takeover.sh status
#
# which compares the SHA-256 sums printed here with the checkout's. It prints
# the default home as settingsservice answers it, whether Lumen's app is
# installed, the defaultApps value apply.sh saved, whether LG's Home runs,
# the boot hook (installed, its by-hand copy, its log) and the long-Home key
# filter (in the compositor's chain or not, its file, its saved chain).
# Every read is printed with its command; a failed one is said and skipped.
# busybox sh.

ID=io.github.sunnyside007.lumen.shell
APP=/media/developer/apps/usr/palm/applications/$ID
DIR=/var/lib/lumen/takeover
HOOK=/var/lib/webosbrew/init.d/lumen-home
FILTER=$DIR/lumen-home.js
LOG=/var/lib/lumen/log/lumen-home.log

# busybox before 1.30 wants `timeout -t SECS`; coreutils wants `timeout SECS`.
if timeout -t 1 true >/dev/null 2>&1; then
    TIMEOUT="timeout -t 10"
elif timeout 1 true >/dev/null 2>&1; then
    TIMEOUT="timeout 10"
else
    TIMEOUT=""
fi

# call METHOD PAYLOAD: prints the call, then luna's answer, which it keeps in
# $reply (one line, empty when there was none).
call() {
    echo "\$ luna-send -n 1 luna://$1 '$2'"
    # shellcheck disable=SC2086 # $TIMEOUT is a command and its argument
    reply=$($TIMEOUT luna-send -n 1 "luna://$1" "$2" </dev/null 2>&1)
    echo "${reply:-(no answer)}"
}

# sha FILE: its SHA-256, or "none".
sha() {
    if [ -f "$1" ]; then sha256sum <"$1" | cut -d' ' -f1; else echo none; fi
}

echo "== the setting"
call com.webos.settingsservice/getSystemSettings '{"category":"general","key":"defaultApps"}'
case $reply in
    *"\"home\":\"$ID\""* | *"\"home\": \"$ID\""*) echo "default home: Lumen" ;;
    *'"home":"com.webos.app.home"'* | *'"home": "com.webos.app.home"'*) echo "default home: LG Home" ;;
    *) echo "default home: neither Lumen nor LG Home, or unread" ;;
esac
if [ -d "$APP" ]; then echo "Lumen's app: installed"; else echo "Lumen's app: NOT installed ($APP)"; fi
if [ -f "$DIR/defaultApps.before" ]; then
    echo "saved before: $(cat "$DIR/defaultApps.before")"
else
    echo "saved before: none"
fi

echo "== LG Home"
call com.webos.applicationManager/running '{}'
case $reply in
    *'"id":"com.webos.app.home"'* | *'"id": "com.webos.app.home"'*) echo "LG Home: running" ;;
    *) echo "LG Home: not running, or unread" ;;
esac
call com.webos.applicationManager/getForegroundAppInfo '{}'

echo "== the boot hook"
echo "hook in init.d: $(sha "$HOOK")"
echo "hook's by-hand copy: $(sha "$DIR/boot-hook")"
if [ -f "$LOG" ]; then
    echo "hook log, the latest run:"
    tail -n 20 "$LOG"
else
    echo "no log: the hook has not run, or its log folder failed its check"
fi

echo "== the key filter"
echo "filter file: $(sha "$FILTER")"
call com.webos.service.config/getConfigs '{"configNames":["com.webos.surfacemanager.keyFilters"]}'
case $reply in
    *"$FILTER"*) echo "filter: in the compositor's chain" ;;
    *keyFilters*) echo "filter: not in the chain" ;;
    *) echo "filter: the chain could not be read" ;;
esac
if [ -f "$DIR/keyFilters.before" ]; then echo "saved chain: $DIR/keyFilters.before"; else echo "saved chain: none"; fi
