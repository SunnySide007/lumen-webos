#!/bin/sh
# The long-Home key filter's state on the TV (takeover/README.md, "The key
# filter"), read-only. Run it with
#
#   tools/takeover.sh filter check
#
# Prints the compositor's key-filter list (configd's
# com.webos.surfacemanager.keyFilters) as configd answers it and numbered,
# whether Lumen's entry is in it, Lumen's filter file and saved list, the
# TV's node, and the lines of LG's StarfishKeyFilter.qml that say how the
# compositor loads the list (whether it follows the configd value while it
# runs). Every read is printed with its command; a failed one is said and
# skipped. busybox sh.

DIR=/var/lib/lumen/takeover
FILTER=$DIR/lumen-home.js
HANDLER=lumenHomeKeys
CONFIG=com.webos.service.config
GET='{"configNames":["com.webos.surfacemanager.keyFilters"]}'
LOADER=/usr/lib/qt5/qml/YSM/StarfishKeyFilter.qml

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

call "$CONFIG/getConfigs" "$GET"
if command -v node >/dev/null 2>&1; then
    chain list "$reply" || echo "(the answer holds no key-filter list)"
fi
case $reply in
    *"$FILTER"*) echo "Lumen's entry: in the chain" ;;
    *keyFilters*) echo "Lumen's entry: not in the chain" ;;
    *) echo "Lumen's entry: unknown, the list could not be read" ;;
esac
echo "filter file: $FILTER $(sha "$FILTER")"
if [ -f "$DIR/keyFilters.before" ]; then
    echo "saved list: $(cat "$DIR/keyFilters.before")"
else
    echo "saved list: none"
fi
echo "node: $(node -v 2>/dev/null || echo 'not found')"
echo "how the compositor loads the list ($LOADER, read-only):"
if [ -r "$LOADER" ]; then
    grep -n -E 'keyFilters|getConfigs|subscribe|include|evaluate|Component|handler' "$LOADER" | head -n 40
else
    echo "(no $LOADER)"
fi
