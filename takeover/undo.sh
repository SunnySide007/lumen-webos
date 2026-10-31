#!/bin/sh
# Undoes route D's setting (takeover/README.md): writes the defaultApps value
# apply.sh saved in /var/lib/lumen/takeover/defaultApps.before back to
# settingsservice, or LG's Home, com.webos.app.home, when nothing was saved,
# reads it back, then deletes the save. Run it with
#
#   tools/takeover.sh undo
#
# tools/uninstall-all.sh runs it too, before it removes Lumen's app, so the
# TV is never left with a Home that isn't installed. It works with the kill
# switch present: it is the way back. No reboot needed; a short Home press
# opens LG's Home at once. Safe to run twice. busybox sh.

ID=io.github.sunnyside007.lumen.shell
LG=com.webos.app.home
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

# The home to go back to: the saved one, if it is a plain app id other than
# Lumen's, else LG's. Only the id is taken from the file, never its JSON.
want=$LG
if [ -f "$BEFORE" ]; then
    saved=$(home_of "$(cat "$BEFORE")")
    case $saved in
        '' | "$ID" | *[!A-Za-z0-9._-]*)
            echo "takeover undo: the save in $BEFORE names no usable home ($(cat "$BEFORE")); using $LG"
            ;;
        *)
            want=$saved
            echo "takeover undo: the saved home is $want"
            ;;
    esac
else
    echo "takeover undo: nothing saved in $BEFORE; using $LG"
fi

call "$SETTINGS/setSystemSettings" "{\"category\":\"general\",\"settings\":{\"defaultApps\":{\"home\":\"$want\"}}}"
call "$SETTINGS/getSystemSettings" "$GET"
now=$(home_of "$(apps_of "$reply")")
if [ "$now" = "$want" ]; then
    rm -f "$BEFORE"
    rmdir "$DIR" 2>/dev/null
    echo "takeover undo: the default home is $want again"
else
    echo "takeover undo: FAILED: the default home reads back as '${now:-nothing}', not $want; the save is kept"
    exit 1
fi
