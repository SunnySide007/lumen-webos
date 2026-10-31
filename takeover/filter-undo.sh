#!/bin/sh
# Takes the long-Home key filter out of the compositor's chain
# (takeover/README.md, "The key filter"). Run it with
#
#   tools/takeover.sh filter undo
#
# tools/uninstall-all.sh runs it too, before it deletes /var/lib/lumen. It
# reads configd's com.webos.surfacemanager.keyFilters and writes it back
# without Lumen's entry, keeping anything else that changed in the list
# since filter-apply.sh, and says whether the result is the list it saved.
# Without node on the TV it writes the saved list back instead. Once the
# list reads back without Lumen's entry it deletes the filter file and the
# saved list; until then it keeps both, so it can be run again. It works with
# the kill switch present: it is the way back. Nothing is restarted: if the
# compositor follows the value the change is live, else it comes with the
# next reboot. Safe to run twice. busybox sh.

DIR=/var/lib/lumen/takeover
FILTER=$DIR/lumen-home.js
HANDLER=lumenHomeKeys
SAVED=$DIR/keyFilters.before
CONFIG=com.webos.service.config
NAME=com.webos.surfacemanager.keyFilters
GET='{"configNames":["com.webos.surfacemanager.keyFilters"]}'

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

# chain MODE JSON: the key-filter list in JSON (a getConfigs answer or a
# bare array), through the TV's node 0.12. `list` numbers it, `array`
# prints it, `strip` prints it without Lumen's entry, and `insert` prints it
# with Lumen's entry just before handleSystemKeys (exit 3 when there is no
# handleSystemKeys). Exit 2 when JSON holds no list. Printed lists are one
# line, each entry's keys sorted, so two lists compare as strings.
chain() {
    KF_MODE=$1 KF_IN=$2 KF_FILE=$FILTER KF_HANDLER=$HANDLER node -e '
        var env = process.env, name = "com.webos.surfacemanager.keyFilters", v = null;
        try { v = JSON.parse(env.KF_IN); } catch (e) { v = null; }
        if (v && !Array.isArray(v) && v.configs) { v = v.configs[name]; }
        if (!Array.isArray(v)) { process.exit(2); }
        function canon(x) {
            var o;
            if (Array.isArray(x)) { return x.map(canon); }
            if (x && typeof x === "object") {
                o = {};
                Object.keys(x).sort().forEach(function (k) { o[k] = canon(x[k]); });
                return o;
            }
            return x;
        }
        function ours(e) { return !!e && e.file === env.KF_FILE; }
        var rest = v.filter(function (e) { return !ours(e); }), out = null, placed = false;
        if (env.KF_MODE === "list") {
            v.forEach(function (e, i) {
                console.log((i + 1) + ". " + (e && e.handler) + " in " + (e && e.file) + (ours(e) ? "   (Lumen)" : ""));
            });
        } else if (env.KF_MODE === "array") {
            out = v;
        } else if (env.KF_MODE === "strip") {
            out = rest;
        } else if (env.KF_MODE === "insert") {
            out = [];
            rest.forEach(function (e) {
                if (!placed && e && e.handler === "handleSystemKeys") {
                    out.push({"handler": env.KF_HANDLER, "file": env.KF_FILE});
                    placed = true;
                }
                out.push(e);
            });
            if (!placed) { process.exit(3); }
        } else {
            process.exit(4);
        }
        if (out) { console.log(JSON.stringify(canon(out))); }
    '
}

# The files go only once the list reads back without Lumen's entry.
tidy() {
    rm -f "$FILTER" "$SAVED"
    rmdir "$DIR" 2>/dev/null
    echo "takeover filter undo: removed $FILTER and the saved list"
}

call "$CONFIG/getConfigs" "$GET"
case $reply in
    *"$FILTER"*) ;;
    *keyFilters*)
        echo "takeover filter undo: Lumen's entry is not in the chain"
        tidy
        exit 0
        ;;
    *) echo "takeover filter undo: the list could not be read" ;;
esac

new=
if command -v node >/dev/null 2>&1; then
    new=$(chain strip "$reply")
fi
if [ -z "$new" ] && [ -f "$SAVED" ]; then
    echo "takeover filter undo: writing the saved list back"
    new=$(cat "$SAVED")
fi
case $new in
    \[*\]) ;;
    *)
        echo "takeover filter undo: FAILED: no list to write (no node, no save); Lumen's entry stays"
        exit 1
        ;;
esac

call "$CONFIG/setConfigs" "{\"configs\":{\"$NAME\":$new}}"
call "$CONFIG/getConfigs" "$GET"
case $reply in
    *"$FILTER"*)
        echo "takeover filter undo: FAILED: Lumen's entry is still in the list; its files are kept"
        exit 1
        ;;
    *keyFilters*) ;;
    *)
        echo "takeover filter undo: FAILED: the list does not read back; its files are kept"
        exit 1
        ;;
esac
if now=$(chain array "$reply") 2>/dev/null; then
    chain list "$reply"
    if [ -f "$SAVED" ] && was=$(chain array "$(cat "$SAVED")"); then
        if [ "$now" = "$was" ]; then
            echo "takeover filter undo: the list is the one saved before filter-apply.sh"
        else
            echo "takeover filter undo: the list differs from the one saved before filter-apply.sh,"
            echo "takeover filter undo: by changes made since, which are kept. The save was:"
            cat "$SAVED"
        fi
    fi
fi
tidy
echo "takeover filter undo: Lumen's entry is out; live if the compositor follows the value, else after a reboot"
