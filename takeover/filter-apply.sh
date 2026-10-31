#!/bin/sh
# Puts the long-Home key filter in the compositor's chain (takeover/README.md,
# "The key filter"). The owner's decision: the chain is configd's value
# com.webos.surfacemanager.keyFilters, which configd keeps across reboots, so
# this persists at once, and neither the kill switch nor Homebrew Channel's
# failsafe reaches it. The README's trial plan says when and how. Run it with
#
#   tools/takeover.sh filter apply [--noop]
#
# which checks the file on the PC and pushes it, with its SHA-256, to
# /tmp/lumen-home-filter.js. Here the copy's SHA-256 is checked, the TV's
# node 0.12 compiles it without running it, the list as it is (without any
# Lumen entry) is saved to /var/lib/lumen/takeover/keyFilters.before once,
# the file goes to /var/lib/lumen/takeover/lumen-home.js, and the list is
# written with Lumen's entry just before LG's handleSystemKeys, then read
# back; a read-back that differs puts the old list back. Nothing is
# restarted. With the entry already in place only the file changes, which
# the compositor reads when it next loads the chain (a reboot).
# filter-undo.sh takes it out. busybox sh.

DIR=/var/lib/lumen/takeover
FILTER=$DIR/lumen-home.js
HANDLER=lumenHomeKeys
STAGED=/tmp/lumen-home-filter.js
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

# sha FILE: its SHA-256, or "none".
sha() {
    if [ -f "$1" ]; then sha256sum <"$1" | cut -d' ' -f1; else echo none; fi
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

refuse() {
    echo "takeover filter: $*; nothing changed"
    exit 1
}

[ ! -e /var/luna/preferences/lumen_disable ] || refuse "/var/luna/preferences/lumen_disable exists"
if [ ! -f "$STAGED" ] || [ ! -f "$STAGED.sha256" ]; then
    refuse "no $STAGED; run tools/takeover.sh filter apply"
fi
want=$(cat "$STAGED.sha256")
[ "$(sha "$STAGED")" = "$want" ] || refuse "$STAGED is not the file the PC checked"
command -v node >/dev/null 2>&1 || refuse "no node on the TV to check the file and the list"
# new Function compiles the file without running any of it.
F=$STAGED node -e 'new Function(require("fs").readFileSync(process.env.F, "utf8"));' ||
    refuse "$STAGED does not compile on the TV's node"
grep -q "^function $HANDLER(" "$STAGED" || refuse "$STAGED has no $HANDLER"

call "$CONFIG/getConfigs" "$GET"
current=$reply
was=$(chain array "$current") || refuse "the key-filter list could not be read"
before=$(chain strip "$current")
new=$(chain insert "$current") || refuse "the list has no handleSystemKeys entry"

if [ -f "$DIR/keyFilters.before" ]; then
    echo "takeover filter: kept the earlier save, $DIR/keyFilters.before"
elif mkdir -p "$DIR" && printf '%s\n' "$before" >"$DIR/keyFilters.before.tmp" &&
    mv "$DIR/keyFilters.before.tmp" "$DIR/keyFilters.before"; then
    echo "takeover filter: saved the list to $DIR/keyFilters.before"
else
    rm -f "$DIR/keyFilters.before.tmp"
    refuse "FAILED to save the list"
fi

# The file is in place before any entry names it.
if mkdir -p "$DIR" && cp "$STAGED" "$FILTER.tmp" && mv "$FILTER.tmp" "$FILTER" &&
    [ "$(sha "$FILTER")" = "$want" ]; then
    rm -f "$STAGED" "$STAGED.sha256"
    echo "takeover filter: $FILTER is in place ($want)"
else
    rm -f "$FILTER.tmp"
    echo "takeover filter: FAILED to put $FILTER in place; the list is unchanged"
    exit 1
fi

if [ "$was" = "$new" ]; then
    echo "takeover filter: Lumen's entry was already in place; the list is unchanged."
    echo "takeover filter: the compositor reads the new file when it next loads the chain: reboot."
    chain list "$current"
    exit 0
fi

call "$CONFIG/setConfigs" "{\"configs\":{\"$NAME\":$new}}"
call "$CONFIG/getConfigs" "$GET"
if [ "$(chain array "$reply")" != "$new" ]; then
    echo "takeover filter: FAILED: the list reads back otherwise; writing the old one back"
    call "$CONFIG/setConfigs" "{\"configs\":{\"$NAME\":$was}}"
    case $was in
        *"$FILTER"*) ;;
        *) rm -f "$FILTER" ;;
    esac
    exit 1
fi
chain list "$reply"
echo "takeover filter: Lumen's entry is in the chain, before handleSystemKeys. Nothing was"
echo "takeover filter: restarted: if the compositor follows the value it is live now, else"
echo "takeover filter: from the next reboot. Check every key now (takeover/README.md, the trial)."
