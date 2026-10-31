#!/bin/sh
# Route D by hand (takeover/README.md): makes Lumen the TV's Home through
# LG's own switch, settingsservice's defaultApps.home, the setting LG's
# home_launcher.sh uses to choose between its two homes. Run it with
#
#   tools/takeover.sh apply
#
# which runs this file through tools/tv.sh script. tools/backup.sh comes
# first: this script takes no backup of its own. It refuses unless Lumen's
# app folder exists, saves the defaultApps value it replaces to
# /var/lib/lumen/takeover/defaultApps.before once (an earlier save is kept),
# writes Lumen's id, reads it back and fails if the TV answers anything
# else. The setting persists across reboots; undo.sh puts the saved value
# back. Nothing else is written. Safe to run twice. busybox sh.

ID=io.github.sunnyside007.lumen.shell
APP=/media/developer/apps/usr/palm/applications/$ID
DIR=/var/lib/lumen/takeover
BEFORE=$DIR/defaultApps.before
SETTINGS=com.webos.settingsservice
GET='{"category":"general","key":"defaultApps"}'

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

# The defaultApps object in a getSystemSettings answer, if it is a flat one,
# and the home it names.
apps_of() {
    printf '%s\n' "$1" | sed -n 's/.*"defaultApps"[[:space:]]*:[[:space:]]*\({[^{}]*}\).*/\1/p'
}
home_of() {
    printf '%s\n' "$1" | sed -n 's/.*"home"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p'
}

if [ -e /var/luna/preferences/lumen_disable ]; then
    echo "takeover: /var/luna/preferences/lumen_disable exists; nothing done"
    exit 1
fi
if [ ! -d "$APP" ]; then
    echo "takeover: Lumen is not installed (no $APP); nothing done"
    exit 1
fi

call "$SETTINGS/getSystemSettings" "$GET"
apps=$(apps_of "$reply")
home=$(home_of "$apps")
if [ -z "$home" ]; then
    echo "takeover: could not read defaultApps.home; nothing done"
    exit 1
fi
# The call below writes the whole object; it holds only home on the B9.
case $apps in
    *,*)
        echo "takeover: defaultApps holds more than home ($apps); nothing done"
        exit 1
        ;;
esac

if [ -f "$BEFORE" ]; then
    echo "takeover: kept the earlier save, $BEFORE: $(cat "$BEFORE")"
elif [ "$home" = "$ID" ]; then
    echo "takeover: Lumen is already the default home and nothing was saved;"
    echo "takeover: undo.sh will write com.webos.app.home"
else
    if mkdir -p "$DIR" && printf '%s\n' "$apps" >"$BEFORE.tmp" && mv "$BEFORE.tmp" "$BEFORE"; then
        echo "takeover: saved $apps to $BEFORE"
    else
        rm -f "$BEFORE.tmp"
        echo "takeover: FAILED to save $apps to $BEFORE; nothing changed"
        exit 1
    fi
fi

call "$SETTINGS/setSystemSettings" "{\"category\":\"general\",\"settings\":{\"defaultApps\":{\"home\":\"$ID\"}}}"
call "$SETTINGS/getSystemSettings" "$GET"
now=$(home_of "$(apps_of "$reply")")
if [ "$now" = "$ID" ]; then
    echo "takeover: the default home is Lumen; a short Home press opens it"
else
    echo "takeover: FAILED: the default home reads back as '${now:-nothing}', not $ID"
    exit 1
fi
