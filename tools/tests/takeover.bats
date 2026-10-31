#!/usr/bin/env bats
# Route D's TV scripts (takeover/*.sh and the boot hook takeover/lumen-home)
# against a fake luna-send that keeps the TV's state in this test's folder
# (the default home, SAM's running list, the app in front, configd's key-filter
# list), a fake sleep that moves a fake /proc/uptime on, and the real node for
# the scripts' JSON; and tools/takeover.sh against a fake tools/tv.sh. The
# key-filter list is the B9's as configd answered it on 2026-10-31
# (fixtures/takeover/keyfilters-b9.json). Nothing reaches a TV.

TAKEOVER="$BATS_TEST_DIRNAME/../../takeover"
FIXTURE="$BATS_TEST_DIRNAME/fixtures/takeover/keyfilters-b9.json"
LUMEN=io.github.sunnyside007.lumen.shell
LG=com.webos.app.home
SCRIPTS="apply.sh undo.sh status.sh home-apply.sh filter-check.sh filter-apply.sh filter-undo.sh lumen-home"

setup() {
    T="$BATS_TEST_TMPDIR"
    CALLS="$T/calls"
    REAL_SLEEP=$(command -v sleep)
    REAL_NODE=$(command -v node)
    export T CALLS REAL_SLEEP REAL_NODE
    : >"$CALLS"
    mkdir -p "$T/bin" "$T/tmp" "$T/apps/$LUMEN" "$T/initd"

    # The scripts with every TV path moved into this test's folder.
    for f in $SCRIPTS; do
        sed -e "s|/var/luna/preferences/lumen_disable|$T/lumen_disable|g" \
            -e "s|/var/lib/lumen/takeover|$T/takeover|g" \
            -e "s|/tmp/lumen-home|$T/tmp/lumen-home|g" \
            -e "s|/media/developer/apps/usr/palm/applications|$T/apps|g" \
            -e "s|/var/lib/webosbrew/init.d|$T/initd|g" \
            -e "s|/proc/uptime|$T/uptime|g" \
            -e "s|/usr/lib/qt5/qml/YSM|$T/ysm|g" \
            "$TAKEOVER/$f" >"$T/$f"
    done
    FILTER="$T/takeover/lumen-home.js"
    echo "$LG" >"$T/home"
    printf '%s\n' com.webos.app.hdmi3 "$LG" >"$T/running"
    echo com.webos.app.hdmi3 >"$T/front"
    cp "$FIXTURE" "$T/chain"
    echo "100.00 50.00" >"$T/uptime"

    # luna-send -n 1 luna://SERVICE/METHOD PAYLOAD, answering as the B9 does,
    # on one line.
    cat >"$T/bin/luna-send" <<'EOF'
#!/bin/sh
while [ $# -gt 2 ]; do shift; done
uri=${1#luna://} payload=$2
echo "luna $uri $payload" >>"$CALLS"
case $uri in
    com.webos.settingsservice/getSystemSettings)
        [ -z "${FAKE_SETTINGS_DOWN:-}" ] || exit 1
        extra=
        [ -z "${FAKE_EXTRA_APP:-}" ] || extra=',"tv":"com.webos.app.livetv"'
        printf '{"subscribed":false,"category":"general","method":"getSystemSettings","settings":{"defaultApps":{"home":"%s"%s}},"returnValue":true}\n' "$(cat "$T/home")" "$extra" ;;
    com.webos.settingsservice/setSystemSettings)
        [ -n "${FAKE_SET_IGNORED:-}" ] || printf '%s\n' "$payload" | sed -n 's/.*"home":"\([^"]*\)".*/\1/p' >"$T/home"
        echo '{"returnValue":true}' ;;
    com.webos.applicationManager/running)
        n=$(grep -c "^luna $uri" "$CALLS")
        if [ "$n" -le "${FAKE_SAM_DOWN_FOR:-0}" ]; then echo '{"returnValue":false,"errorText":"not ready"}'; exit 0; fi
        printf '{"returnValue":true,"running":['
        sep=
        while read -r id; do printf '%s{"id":"%s","processid":"42","webprocessid":""}' "$sep" "$id"; sep=,; done <"$T/running"
        echo ']}' ;;
    com.webos.applicationManager/getForegroundAppInfo)
        printf '{"appId":"%s","windowId":"","processId":"","returnValue":true}\n' "$(cat "$T/front")" ;;
    com.webos.applicationManager/closeByAppId)
        if [ -n "${FAKE_CLOSE_FAILS:-}" ]; then echo '{"returnValue":false,"errorText":"busy"}'; exit 0; fi
        id=$(printf '%s\n' "$payload" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
        grep -vx "$id" "$T/running" >"$T/running.new"; mv "$T/running.new" "$T/running"
        echo '{"returnValue":true}' ;;
    com.webos.service.config/getConfigs)
        [ -z "${FAKE_CONFIG_DOWN:-}" ] || exit 1
        printf '{"configs":{"com.webos.surfacemanager.keyFilters":%s},"returnValue":true}\n' "$(cat "$T/chain")" ;;
    com.webos.service.config/setConfigs)
        [ -n "${FAKE_CONFIG_IGNORED:-}" ] ||
            printf '%s\n' "$payload" | sed -n 's/^{"configs":{"com.webos.surfacemanager.keyFilters":\(.*\)}}$/\1/p' >"$T/chain"
        echo '{"returnValue":true}' ;;
    *) echo '{"returnValue":false,"errorText":"unknown method"}' ;;
esac
EOF
    # sleep: a moment, and the uptime moves on by the seconds asked. A fake
    # SAM brings LG's Home back while it is kept alive, and the kill switch or
    # the home can change once the uptime passes a mark.
    cat >"$T/bin/sleep" <<'EOF'
#!/bin/sh
echo "sleep $1" >>"$CALLS"
read -r up _ <"$T/uptime"
up=$((${up%%.*} + $1))
echo "$up.00 0.00" >"$T/uptime"
if [ -n "${FAKE_KEEPALIVE:-}" ] && ! grep -qx com.webos.app.home "$T/running"; then
    echo com.webos.app.home >>"$T/running"
fi
[ -z "${FAKE_KILL_AT:-}" ] || [ "$up" -lt "$FAKE_KILL_AT" ] || touch "$T/lumen_disable"
[ -z "${FAKE_LG_HOME_AT:-}" ] || [ "$up" -lt "$FAKE_LG_HOME_AT" ] || echo com.webos.app.home >"$T/home"
exec "$REAL_SLEEP" 0.01
EOF
    # node, unless a test takes it away.
    cat >"$T/bin/node" <<'EOF'
#!/bin/sh
[ -z "${FAKE_NO_NODE:-}" ] || exit 127
exec "$REAL_NODE" "$@"
EOF
    for tool in mount umount initctl reboot; do
        printf '#!/bin/sh\necho "%s $*" >>"$CALLS"\n' "$tool" >"$T/bin/$tool"
    done
    chmod +x "$T/bin/"*
    PATH="$T/bin:$PATH"
}

# Fails if the command succeeds: a bare `!` never fails a bats test.
not() {
    if "$@"; then
        echo "should have failed: $*"
        return 1
    fi
}

run_tv() {
    run sh "$T/$1"
}

# FD 3 closed: the hook's background work must not hold bats' output open.
run_hook() {
    run sh "$T/lumen-home" 3>&-
}

wait_done() {
    for _ in $(seq 400); do
        grep -q 'lumen-home: done' "$T/tmp/lumen-home.log" 2>/dev/null && return 0
        "$REAL_SLEEP" 0.05
    done
    echo "the hook never finished:"
    cat "$T/tmp/lumen-home.log"
    return 1
}

uptime_now() {
    read -r up _ <"$T/uptime"
    echo "${up%%.*}"
}

# A list as the scripts print it: one line, each entry's keys sorted.
canon() {
    node -e 'const v = JSON.parse(require("fs").readFileSync(0, "utf8"));
        console.log(JSON.stringify(v.map((e) => ({ file: e.file, handler: e.handler }))));'
}

handlers() {
    node -e 'console.log(JSON.parse(require("fs").readFileSync(0, "utf8")).map((e) => e.handler).join(" "))' <"$T/chain"
}

# The filter as tools/takeover.sh pushes it.
stage_filter() {
    cp "$TAKEOVER/filters/lumen-home.js" "$T/tmp/lumen-home-filter.js"
    sha256sum <"$T/tmp/lumen-home-filter.js" | cut -d' ' -f1 >"$T/tmp/lumen-home-filter.js.sha256"
}

# Code without comments or the words of messages, for the read-only checks.
commands() {
    sed -e '/^chain() {/,/^}/d' -e 's/^[[:space:]]*#.*//' -e 's/[[:space:]]#.*//' \
        -e 's/\(say\|echo\|refuse\) *"[^"]*"/\1 ""/g' "$1"
}

# --- the setting

@test "apply saves LG's value once, writes Lumen's id, reads it back, and prints each call" {
    run_tv apply.sh
    [ "$status" -eq 0 ]
    [ "$(cat "$T/home")" = "$LUMEN" ]
    [ "$(cat "$T/takeover/defaultApps.before")" = '{"home":"com.webos.app.home"}' ]
    [[ "$output" == *"\$ luna-send -n 1 luna://com.webos.settingsservice/setSystemSettings '{\"category\":\"general\",\"settings\":{\"defaultApps\":{\"home\":\"$LUMEN\"}}}'"* ]]
    [[ "$output" == *"the default home is Lumen"* ]]
    run grep -c '^luna com.webos.settingsservice/' "$CALLS"
    [ "$output" -eq 3 ]
}

@test "apply never overwrites an earlier save" {
    mkdir -p "$T/takeover"
    echo '{"home":"com.webos.app.fullhome"}' >"$T/takeover/defaultApps.before"
    run_tv apply.sh
    [ "$status" -eq 0 ]
    [ "$(cat "$T/takeover/defaultApps.before")" = '{"home":"com.webos.app.fullhome"}' ]
    [[ "$output" == *"kept the earlier save"* ]]
}

@test "apply with Lumen already the home and no save saves nothing, and says undo writes LG's Home" {
    echo "$LUMEN" >"$T/home"
    run_tv apply.sh
    [ "$status" -eq 0 ]
    [ ! -e "$T/takeover/defaultApps.before" ]
    [[ "$output" == *"undo.sh will write com.webos.app.home"* ]]
}

@test "apply refuses without Lumen's app, with the kill switch, or when defaultApps can't be read" {
    rmdir "$T/apps/$LUMEN"
    run_tv apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"Lumen is not installed"* ]]
    mkdir "$T/apps/$LUMEN"
    touch "$T/lumen_disable"
    run_tv apply.sh
    [ "$status" -eq 1 ]
    rm "$T/lumen_disable"
    FAKE_SETTINGS_DOWN=1 run_tv apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"could not read defaultApps.home"* ]]
    FAKE_EXTRA_APP=1 run_tv apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"holds more than home"* ]]
    not grep -q setSystemSettings "$CALLS"
    [ "$(cat "$T/home")" = "$LG" ]
    [ ! -e "$T/takeover/defaultApps.before" ]
}

@test "apply fails when the setting reads back otherwise" {
    FAKE_SET_IGNORED=1 run_tv apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAILED: the default home reads back as 'com.webos.app.home'"* ]]
}

@test "undo writes the saved value back, reads it, then deletes the save" {
    run_tv apply.sh
    echo com.webos.app.fullhome >"$T/home"
    run_tv apply.sh
    run_tv undo.sh
    [ "$status" -eq 0 ]
    [ "$(cat "$T/home")" = "$LG" ]
    [ ! -e "$T/takeover/defaultApps.before" ]
    [[ "$output" == *"the saved home is com.webos.app.home"* ]]
    [[ "$output" == *"the default home is com.webos.app.home again"* ]]
}

@test "undo with nothing saved, or a save that names Lumen or no plain id, writes LG's Home" {
    echo "$LUMEN" >"$T/home"
    run_tv undo.sh
    [ "$status" -eq 0 ]
    [ "$(cat "$T/home")" = "$LG" ]
    [[ "$output" == *"nothing saved"* ]]
    for save in "{\"home\":\"$LUMEN\"}" '{"home":"x; reboot"}' 'garbage'; do
        mkdir -p "$T/takeover"
        printf '%s\n' "$save" >"$T/takeover/defaultApps.before"
        echo "$LUMEN" >"$T/home"
        run_tv undo.sh
        [ "$status" -eq 0 ]
        [ "$(cat "$T/home")" = "$LG" ]
        [[ "$output" == *"names no usable home"* ]]
    done
    not grep -q reboot "$CALLS"
}

@test "undo keeps the save when the setting doesn't read back, and works with the kill switch" {
    run_tv apply.sh
    touch "$T/lumen_disable"
    FAKE_SET_IGNORED=1 run_tv undo.sh
    [ "$status" -eq 1 ]
    [ -f "$T/takeover/defaultApps.before" ]
    [[ "$output" == *"the save is kept"* ]]
    run_tv undo.sh
    [ "$status" -eq 0 ]
    [ "$(cat "$T/home")" = "$LG" ]
}

@test "status reports the home, the app, the save, LG's Home, the hook and the filter, and writes nothing" {
    run_tv apply.sh
    : >"$CALLS"
    run_tv status.sh
    [ "$status" -eq 0 ]
    [[ "$output" == *"default home: Lumen"* ]]
    [[ "$output" == *"Lumen's app: installed"* ]]
    [[ "$output" == *'saved before: {"home":"com.webos.app.home"}'* ]]
    [[ "$output" == *"LG Home: running"* ]]
    [[ "$output" == *"hook in init.d: none"* ]]
    [[ "$output" == *"filter file: none"* ]]
    [[ "$output" == *"filter: not in the chain"* ]]
    not grep -qE '/set[A-Z]|/close' "$CALLS"
}

# --- the boot hook

@test "the hook returns at once, waits for SAM, closes LG's Home and logs the running list 10 s later" {
    FAKE_SAM_DOWN_FOR=2 run_tv apply.sh
    : >"$CALLS"
    FAKE_SAM_DOWN_FOR=2 run_hook
    [ "$status" -eq 0 ]
    wait_done
    log=$(cat "$T/tmp/lumen-home.log")
    [[ "$log" == *"SAM answers"* ]]
    [[ "$log" == *"round 1: closed LG Home"* ]]
    [[ "$log" == *"round 1: LG Home stayed closed 10 s later; running: com.webos.app.hdmi3"* ]]
    [[ "$log" == *"round 2: LG Home is not running"* ]]
    run grep -c '^luna com.webos.applicationManager/closeByAppId {"id":"com.webos.app.home","forceMode":true}$' "$CALLS"
    [ "$output" -eq 1 ]
    not grep -q "$LG" "$T/running"
}

@test "the hook closes LG's Home again in each round that keep-alive brings it back, five rounds at most" {
    run_tv apply.sh
    : >"$CALLS"
    FAKE_KEEPALIVE=1 run_hook
    wait_done
    [[ "$(cat "$T/tmp/lumen-home.log")" == *"round 1: LG Home is back 10 s later"* ]]
    run grep -c 'closeByAppId' "$CALLS"
    [ "$output" -ge 3 ]
    [ "$output" -le 5 ]
}

@test "the hook's work ends by itself within five minutes, even with a late SAM and a close each round" {
    run_tv apply.sh
    start=$(uptime_now)
    FAKE_SAM_DOWN_FOR=5 FAKE_KEEPALIVE=1 run_hook
    wait_done
    [ $(($(uptime_now) - start)) -le 300 ]
    not grep -q 'round 6' "$T/tmp/lumen-home.log"
}

@test "the hook leaves LG's Home alone when it is in front, or not running" {
    run_tv apply.sh
    echo "$LG" >"$T/front"
    run_hook
    wait_done
    [[ "$(cat "$T/tmp/lumen-home.log")" == *"round 1: LG Home is in front; left alone"* ]]
    not grep -q closeByAppId "$CALLS"
}

@test "with LG's Home as the default the hook does nothing and says so" {
    run_hook
    wait_done
    [[ "$(cat "$T/tmp/lumen-home.log")" == *"the default home is com.webos.app.home, not Lumen; nothing done"* ]]
    not grep -q closeByAppId "$CALLS"
}

@test "the hook stops when the default home is set back while it runs" {
    run_tv apply.sh
    FAKE_LG_HOME_AT=150 run_hook
    wait_done
    [[ "$(cat "$T/tmp/lumen-home.log")" == *"not Lumen; nothing done"* ]]
    [ "$(grep -c closeByAppId "$CALLS")" -eq 1 ]
}

@test "when SAM never answers the hook gives up and does nothing" {
    run_tv apply.sh
    : >"$CALLS"
    FAKE_SAM_DOWN_FOR=99 run_hook
    wait_done
    [[ "$(cat "$T/tmp/lumen-home.log")" == *"SAM never answered; nothing done"* ]]
    [ "$(grep -c '^luna com.webos.applicationManager/running' "$CALLS")" -eq 6 ]
    not grep -qE 'getSystemSettings|closeByAppId' "$CALLS"
}

@test "with the kill switch the hook does nothing at all, and stops when it appears" {
    run_tv apply.sh
    : >"$CALLS"
    touch "$T/lumen_disable"
    run_hook
    [ "$status" -eq 0 ]
    "$REAL_SLEEP" 0.3
    [ ! -s "$CALLS" ]
    [ ! -e "$T/tmp/lumen-home.log" ]
    rm "$T/lumen_disable"
    FAKE_KILL_AT=140 run_hook
    wait_done
    [ "$(grep -c closeByAppId "$CALLS")" -eq 1 ]
    not grep -q 'round 3' "$T/tmp/lumen-home.log"
}

@test "the hook never sets anything, mounts, or touches a job, and writes only its log" {
    run_tv apply.sh
    : >"$CALLS"
    FAKE_KEEPALIVE=1 run_hook
    wait_done
    not grep -qE 'setSystemSettings|setConfigs|^mount|^umount|^initctl|^reboot' "$CALLS"
    code=$(commands "$TAKEOVER/lumen-home")
    not grep -Eq '\b(setSystemSettings|setConfigs|u?mount|initctl|reboot|launch)\b' <<<"$code"
    grep -qx 'LOG=/tmp/lumen-home.log' "$TAKEOVER/lumen-home"
    run grep -Eo '>>?[[:space:]]*[^[:space:]&;)|]+' <<<"$code"
    for target in "${lines[@]}"; do
        [[ "$target" =~ ^\>\>?[[:space:]]*(\"\$LOG\"|/dev/null)$ ]] || {
            echo "writes to $target"
            return 1
        }
    done
    not grep -Eq '\b(rm|mv|cp|touch|mkdir|rmdir|ln|chmod|tee|dd|truncate)\b' <<<"$code"
}

@test "home-apply.sh runs the pushed copy and reports its first round" {
    run_tv apply.sh
    mkdir -p "$T/takeover"
    cp "$T/lumen-home" "$T/takeover/boot-hook"
    run sh "$T/home-apply.sh" 3>&-
    [ "$status" -eq 0 ]
    [[ "$output" == *"round 1: LG Home stayed closed"* ]]
    [[ "$output" == *"luna-send -n 1 luna://com.webos.applicationManager/running"* ]]
    [[ "$output" == *"the first round ran"* ]]
    wait_done
}

@test "home-apply.sh needs hook push first, and respects the kill switch" {
    run_tv home-apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"run tools/takeover.sh hook push first"* ]]
    mkdir -p "$T/takeover"
    cp "$T/lumen-home" "$T/takeover/boot-hook"
    touch "$T/lumen_disable"
    run_tv home-apply.sh
    [ "$status" -eq 1 ]
    [ ! -s "$CALLS" ]
}

# --- the key filter

@test "filter apply puts Lumen's entry just before handleSystemKeys, and saves the list without it" {
    stage_filter
    run_tv filter-apply.sh
    [ "$status" -eq 0 ]
    [ "$(handlers)" = "handleScreenSaver handleAccessibility handleAvReset handleLowSpeedDataBroadcast lumenHomeKeys handleSystemKeys handleHotkeys handleCPHotkeys handleFactoryKeys handleTvhotkeyAppLaunch handleLocalKeys handleMagicNumber" ]
    grep -qF "{\"file\":\"$FILTER\",\"handler\":\"lumenHomeKeys\"}" "$T/chain"
    [ "$(cat "$T/takeover/keyFilters.before")" = "$(canon <"$FIXTURE")" ]
    cmp "$FILTER" "$TAKEOVER/filters/lumen-home.js"
    [ ! -e "$T/tmp/lumen-home-filter.js" ]
    [[ "$output" == *"5. lumenHomeKeys in $FILTER   (Lumen)"* ]]
    [[ "$output" == *"Nothing was"* ]]
    not grep -qE '^(initctl|reboot)' "$CALLS"
}

@test "filter apply refuses a copy whose SHA-256 differs, a file that doesn't compile, and the kill switch" {
    stage_filter
    echo 0000 >"$T/tmp/lumen-home-filter.js.sha256"
    run_tv filter-apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"is not the file the PC checked"* ]]
    printf 'function lumenHomeKeys(key, pressed, autoRepeat) {\n  return (;\n}\n' >"$T/tmp/lumen-home-filter.js"
    sha256sum <"$T/tmp/lumen-home-filter.js" | cut -d' ' -f1 >"$T/tmp/lumen-home-filter.js.sha256"
    run_tv filter-apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"does not compile on the TV's node"* ]]
    stage_filter
    touch "$T/lumen_disable"
    run_tv filter-apply.sh
    [ "$status" -eq 1 ]
    not grep -q setConfigs "$CALLS"
    [ ! -e "$FILTER" ]
    [ ! -e "$T/takeover/keyFilters.before" ]
    cmp "$T/chain" "$FIXTURE"
}

@test "filter apply refuses without node, without the push, or with no handleSystemKeys in the list" {
    run_tv filter-apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"run tools/takeover.sh filter apply"* ]]
    stage_filter
    FAKE_NO_NODE=1 run_tv filter-apply.sh
    [ "$status" -eq 1 ]
    sed 's/handleSystemKeys/handleOtherKeys/' "$FIXTURE" >"$T/chain"
    run_tv filter-apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"no handleSystemKeys entry"* ]]
    not grep -q setConfigs "$CALLS"
    [ ! -e "$FILTER" ]
}

@test "filter apply again changes only the file, and says a reboot loads it" {
    stage_filter
    run_tv filter-apply.sh
    : >"$CALLS"
    sed 's/^var lumenHomeActs = true;$/var lumenHomeActs = false;/' "$TAKEOVER/filters/lumen-home.js" >"$T/tmp/lumen-home-filter.js"
    sha256sum <"$T/tmp/lumen-home-filter.js" | cut -d' ' -f1 >"$T/tmp/lumen-home-filter.js.sha256"
    before=$(cat "$T/chain")
    run_tv filter-apply.sh
    [ "$status" -eq 0 ]
    [[ "$output" == *"already in place"* ]]
    [[ "$output" == *"reboot"* ]]
    grep -qx 'var lumenHomeActs = false;' "$FILTER"
    [ "$(cat "$T/chain")" = "$before" ]
    not grep -q setConfigs "$CALLS"
}

@test "filter apply whose list reads back otherwise writes the old list back and removes the file" {
    stage_filter
    FAKE_CONFIG_IGNORED=1 run_tv filter-apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"writing the old one back"* ]]
    [ "$(grep -c setConfigs "$CALLS")" -eq 2 ]
    [ ! -e "$FILTER" ]
}

@test "filter undo takes Lumen's entry out, says the list is the saved one, and removes its files" {
    stage_filter
    run_tv filter-apply.sh
    run_tv filter-undo.sh
    [ "$status" -eq 0 ]
    [ "$(cat "$T/chain")" = "$(canon <"$FIXTURE")" ]
    [[ "$output" == *"the list is the one saved before filter-apply.sh"* ]]
    [ ! -e "$FILTER" ]
    [ ! -e "$T/takeover/keyFilters.before" ]
}

@test "filter undo keeps what changed in the list since apply" {
    stage_filter
    run_tv filter-apply.sh
    # Another filter put first after Lumen's apply, as Glasshouse puts its own.
    sed 's|^\[|[{"file":"/var/lib/tvweb/remotekeys-filter.js","handler":"remoteKeys"},|' "$T/chain" >"$T/chain.new"
    mv "$T/chain.new" "$T/chain"
    run_tv filter-undo.sh
    [ "$status" -eq 0 ]
    [ "$(handlers)" = "remoteKeys handleScreenSaver handleAccessibility handleAvReset handleLowSpeedDataBroadcast handleSystemKeys handleHotkeys handleCPHotkeys handleFactoryKeys handleTvhotkeyAppLaunch handleLocalKeys handleMagicNumber" ]
    [[ "$output" == *"differs from the one saved"* ]]
}

@test "filter undo without node writes the saved list back" {
    stage_filter
    run_tv filter-apply.sh
    FAKE_NO_NODE=1 run_tv filter-undo.sh
    [ "$status" -eq 0 ]
    [[ "$output" == *"writing the saved list back"* ]]
    [ "$(cat "$T/chain")" = "$(canon <"$FIXTURE")" ]
    [ ! -e "$FILTER" ]
}

@test "filter undo keeps its files while the entry stays, and tidies up when it is gone" {
    stage_filter
    run_tv filter-apply.sh
    FAKE_CONFIG_IGNORED=1 run_tv filter-undo.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"still in the list; its files are kept"* ]]
    [ -f "$FILTER" ]
    [ -f "$T/takeover/keyFilters.before" ]
    cp "$FIXTURE" "$T/chain"
    : >"$CALLS"
    run_tv filter-undo.sh
    [ "$status" -eq 0 ]
    [[ "$output" == *"not in the chain"* ]]
    not grep -q setConfigs "$CALLS"
    [ ! -e "$FILTER" ]
}

@test "filter check numbers the list, finds Lumen's entry, and writes nothing" {
    stage_filter
    run_tv filter-apply.sh
    : >"$CALLS"
    run_tv filter-check.sh
    [ "$status" -eq 0 ]
    [[ "$output" == *"5. lumenHomeKeys in $FILTER   (Lumen)"* ]]
    [[ "$output" == *"6. handleSystemKeys in /usr/lib/qt5/qml/KeyFilters/systemUi.js"* ]]
    [[ "$output" == *"Lumen's entry: in the chain"* ]]
    [[ "$output" == *"(no $T/ysm/StarfishKeyFilter.qml)"* ]]
    not grep -qE '/set[A-Z]|/close' "$CALLS"
}

# --- what every script may touch

@test "each script calls only its own luna methods; only apply and undo set the home, only the filter's set the list" {
    for f in $SCRIPTS; do
        code=$(commands "$TAKEOVER/$f")
        case $f in
            apply.sh | undo.sh) want=setSystemSettings ;;
            filter-apply.sh | filter-undo.sh) want=setConfigs ;;
            lumen-home) want=closeByAppId ;;
            *) want= ;;
        esac
        got=$(grep -oE '\b(set[A-Z][A-Za-z]*|close[A-Za-z]*|launch[A-Za-z]*|delete[A-Za-z]*|remove[A-Za-z]*|kill[A-Za-z]*|reboot|install)\b' <<<"$code" | sort -u | tr '\n' ' ')
        [ "$got" = "${want:+$want }" ] || {
            echo "$f calls: $got"
            return 1
        }
        not grep -Eq '\b(u?mount|initctl|iptables)\b' <<<"$code"
    done
}

@test "nothing in takeover/ writes outside /var/lib/lumen/takeover and /tmp" {
    for f in $SCRIPTS; do
        code=$(commands "$TAKEOVER/$f")
        # Each path variable is under one of the two.
        while IFS='=' read -r name value; do
            case $value in
                /var/lib/lumen/takeover | '$DIR/'* | /tmp/lumen-home*) ;;
                *) echo "$f: $name=$value"; return 1 ;;
            esac
        done < <(grep -E '^(DIR|BEFORE|FILTER|STAGED|SAVED|LOG)=' "$TAKEOVER/$f")
        run grep -Eo '>>?[[:space:]]*[^[:space:]&;)|]+' <<<"$code"
        for target in "${lines[@]}"; do
            [[ "$target" =~ ^\>\>?[[:space:]]*(/dev/null|\"\$LOG\"|\"\$BEFORE.tmp\"|\"\$DIR/keyFilters.before.tmp\")$ ]] || {
                echo "$f writes to $target"
                return 1
            }
        done
        # Every file command names only those paths.
        run grep -Eo '\b(rm|mv|cp|touch|mkdir|rmdir|ln|chmod|tee|dd|truncate)\b[^;|&]*' <<<"$code"
        for cmd in "${lines[@]}"; do
            for word in ${cmd#* }; do
                case $word in
                    -f | -p | 2\>/dev/null | '"$DIR"' | '"$DIR/'* | '"$BEFORE'* | '"$FILTER'* | '"$STAGED'* | '"$SAVED"') ;;
                    *) echo "$f: $cmd"; return 1 ;;
                esac
            done
        done
    done
}

@test "the scripts share their luna, setting and list helpers word for word" {
    for fn in call apps_of home_of chain sha; do
        first=
        for f in $SCRIPTS; do
            body=$(sed -n "/^$fn() {/,/^}/p" "$TAKEOVER/$f")
            [ -n "$body" ] || continue
            if [ -z "$first" ]; then
                first=$body
            else
                [ "$body" = "$first" ] || {
                    echo "$fn differs in $f"
                    return 1
                }
            fi
        done
        [ -n "$first" ]
    done
    # Three copies of the list helper: check, apply and undo.
    [ "$(grep -l '^chain() {' "$TAKEOVER"/*.sh | wc -l)" -eq 3 ]
}

# --- tools/takeover.sh, against a fake tools/tv.sh

ctl_setup() {
    CTL="$BATS_TEST_DIRNAME/../takeover.sh"
    cat >"$T/tv.sh" <<'EOF'
#!/bin/sh
printf '%s|%s|%s\n' "$1" "$2" "${3:-}" >>"$CALLS"
case $1 in
    push) cp "$2" "$T/pushed$(echo "$3" | tr / _)" ;;
    run)
        case $2 in
            *'sha256sum <'*'| cut'*)
                if [ -n "${FAKE_BAD_SHA:-}" ]; then echo 0000
                else sha256sum <"$T/pushed_tmp_lumen-home.new" | cut -d' ' -f1; fi ;;
        esac ;;
    script)
        case $2 in
            */status.sh) printf 'default home: Lumen\nhook in init.d: %s\nfilter file: %s\n' "${FAKE_TV_HOOK:-none}" "${FAKE_TV_FILTER:-none}" ;;
            *) echo "ran $2" ;;
        esac ;;
esac
EOF
    chmod +x "$T/tv.sh"
    export LUMEN_TV_SH="$T/tv.sh"
}

@test "takeover.sh: an unknown command fails with usage and runs nothing" {
    ctl_setup
    for args in frobnicate "hook" "hook frob" "filter" "filter apply --yes"; do
        # shellcheck disable=SC2086
        run "$CTL" $args
        [ "$status" -eq 2 ]
        [[ "$output" == *"tools/takeover.sh filter apply [--noop]"* ]]
    done
    [ ! -s "$CALLS" ]
}

@test "takeover.sh apply reminds of the backup, then runs takeover/apply.sh; undo runs undo.sh" {
    ctl_setup
    run "$CTL" apply
    [ "$status" -eq 0 ]
    [[ "${lines[0]}" == "tools/backup.sh comes first"* ]]
    run "$CTL" undo
    run cat "$CALLS"
    [[ "${lines[0]}" == "script|"*"/takeover/apply.sh|" ]]
    [[ "${lines[1]}" == "script|"*"/takeover/undo.sh|" ]]
    [ "${#lines[@]}" -eq 2 ]
}

@test "takeover.sh hook push checks the copy and moves it whole into /var/lib/lumen/takeover" {
    ctl_setup
    run "$CTL" hook push
    [ "$status" -eq 0 ]
    run cat "$CALLS"
    [[ "${lines[0]}" == "push|"*"/takeover/lumen-home|/tmp/lumen-home.new" ]]
    [[ "${lines[1]}" == "run|set -e; sh -n /tmp/lumen-home.new;"*"mv /var/lib/lumen/takeover/boot-hook.tmp /var/lib/lumen/takeover/boot-hook"* ]]
    [ "${#lines[@]}" -eq 2 ]
    FAKE_BAD_SHA=1 run "$CTL" hook push
    [ "$status" -eq 1 ]
    [[ "$output" == *"is not this checkout's"* ]]
}

@test "takeover.sh hook install pushes, installs the hook whole into init.d, and doesn't run it" {
    ctl_setup
    run "$CTL" hook install
    [ "$status" -eq 0 ]
    [[ "$output" == *"installed /var/lib/webosbrew/init.d/lumen-home"* ]]
    run cat "$CALLS"
    [ "${#lines[@]}" -eq 4 ]
    [[ "${lines[3]}" == "run|sh -n /tmp/lumen-home.new && cp "*"&& mv /var/lib/webosbrew/init.d/lumen-home.tmp /var/lib/webosbrew/init.d/lumen-home"* ]]
    not grep -qx 'run|/var/lib/webosbrew/init.d/lumen-home|' "$CALLS"
    : >"$CALLS"
    FAKE_BAD_SHA=1 run "$CTL" hook install
    [ "$status" -eq 1 ]
    [ "$(wc -l <"$CALLS")" -eq 2 ]
}

@test "takeover.sh hook remove deletes only the hook" {
    ctl_setup
    run "$CTL" hook remove
    [ "$status" -eq 0 ]
    [ "$(cat "$CALLS")" = "run|rm -f /var/lib/webosbrew/init.d/lumen-home && echo 'removed /var/lib/webosbrew/init.d/lumen-home'|" ]
}

@test "takeover.sh filter apply pushes the checked file and its SHA-256, then runs filter-apply.sh" {
    ctl_setup
    run "$CTL" filter apply
    [ "$status" -eq 0 ]
    cmp "$T/pushed_tmp_lumen-home-filter.js" "$TAKEOVER/filters/lumen-home.js"
    [ "$(cat "$T/pushed_tmp_lumen-home-filter.js.sha256")" = "$(sha256sum <"$TAKEOVER/filters/lumen-home.js" | cut -d' ' -f1)" ]
    run cat "$CALLS"
    [[ "${lines[2]}" == "script|"*"/takeover/filter-apply.sh|" ]]
    [ "${#lines[@]}" -eq 3 ]
}

@test "takeover.sh filter apply --noop pushes the build that differs in its one switch" {
    ctl_setup
    run "$CTL" filter apply --noop
    [ "$status" -eq 0 ]
    [[ "$output" == *"acts on nothing"* ]]
    run diff "$TAKEOVER/filters/lumen-home.js" "$T/pushed_tmp_lumen-home-filter.js"
    [ "$status" -eq 1 ]
    [ "$(printf '%s\n' "$output" | grep -c '^[<>]')" -eq 2 ]
    [[ "$output" == *"> var lumenHomeActs = false;"* ]]
    [ "$(cat "$T/pushed_tmp_lumen-home-filter.js.sha256")" = "$(sha256sum <"$T/pushed_tmp_lumen-home-filter.js" | cut -d' ' -f1)" ]
}

@test "takeover.sh filter apply refuses a filter with syntax newer than ES5 and pushes nothing" {
    ctl_setup
    root="$T/root"
    mkdir -p "$root/tools" "$root/takeover/filters"
    cp "$CTL" "$root/tools/"
    sed 's/^var lumenHomeFired = false;$/let lumenHomeFired = false;/' "$TAKEOVER/filters/lumen-home.js" >"$root/takeover/filters/lumen-home.js"
    run "$root/tools/takeover.sh" filter apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"newer than ES5"* ]]
    [ ! -s "$CALLS" ]
}

@test "takeover.sh status names which hook and which filter build the TV has" {
    ctl_setup
    hook=$(sha256sum <"$TAKEOVER/lumen-home" | cut -d' ' -f1)
    noop=$(sed 's/^var lumenHomeActs = true;$/var lumenHomeActs = false;/' "$TAKEOVER/filters/lumen-home.js" | sha256sum | cut -d' ' -f1)
    FAKE_TV_HOOK=$hook FAKE_TV_FILTER=$noop run "$CTL" status
    [ "$status" -eq 0 ]
    [[ "$output" == *"hook: installed, this checkout's"* ]]
    [[ "$output" == *"filter file: this checkout's no-op build"* ]]
    FAKE_TV_HOOK=0000 run "$CTL" status
    [[ "$output" == *"hook: installed, but NOT this checkout's"* ]]
    [[ "$output" == *"filter file: none"* ]]
}
