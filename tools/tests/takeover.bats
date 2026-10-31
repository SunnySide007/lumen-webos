#!/usr/bin/env bats
# Route D's TV scripts (takeover/*.sh and the boot hook takeover/lumen-home)
# against a fake luna-send that keeps the TV's state in this test's folder
# (the default home, SAM's running list, the app in front, configd's key-filter
# list), a fake sleep that moves a fake /proc/uptime on, and the real node for
# the scripts' JSON; and tools/takeover.sh against a fake tools/tv.sh that
# runs the scripts it generates (hook-install.sh, filter-apply.sh, each with
# its file and the PC's SHA-256) the same way, and can tamper with them on
# the way. The key-filter list is the B9's as configd answered it on
# 2026-10-31 (fixtures/takeover/keyfilters-b9.json). Nothing reaches a TV.

TAKEOVER="$BATS_TEST_DIRNAME/../../takeover"
CTL="$BATS_TEST_DIRNAME/../takeover.sh"
FIXTURE="$BATS_TEST_DIRNAME/fixtures/takeover/keyfilters-b9.json"
LUMEN=io.github.sunnyside007.lumen.shell
LG=com.webos.app.home
SCRIPTS="apply.sh undo.sh status.sh home-apply.sh hook-install.sh filter-check.sh filter-apply.sh filter-undo.sh lumen-home"

setup() {
    T="$BATS_TEST_TMPDIR"
    CALLS="$T/calls"
    REAL_SLEEP=$(command -v sleep)
    REAL_NODE=$(command -v node)
    REAL_TIMEOUT=$(command -v timeout)
    export T CALLS REAL_SLEEP REAL_NODE REAL_TIMEOUT
    : >"$CALLS"
    mkdir -p "$T/bin" "$T/tmp" "$T/apps/$LUMEN" "$T/initd"
    # /var/lib/lumen as on the TV: root's, writable by no one else.
    mkdir -m 755 "$T/lumen"

    # Every TV path moved into this test's folder, and root's uid to ours.
    # The file a generated script carries is left as the PC sent it.
    cat >"$T/remap.sed" <<EOF
/^staged_copy() {\$/,/^LUMEN_STAGED_FILE_END\$/b
s|/var/luna/preferences/lumen_disable|$T/lumen_disable|g
s|/var/lib/lumen/takeover|$T/takeover|g
s|/var/lib/lumen/stage|$T/lumen/stage|g
s|/var/lib/lumen/log|$T/lumen/log|g
s|/media/developer/apps/usr/palm/applications|$T/apps|g
s|/var/lib/webosbrew/init.d|$T/initd|g
s|/proc/uptime|$T/uptime|g
s|/usr/lib/qt5/qml/YSM|$T/ysm|g
s|^ROOT_UID=0\$|ROOT_UID=$(id -u)|
EOF
    for f in $SCRIPTS; do
        sed -f "$T/remap.sed" "$TAKEOVER/$f" >"$T/$f"
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
        if [ "$n" -ge "${FAKE_SAM_DOWN_FROM:-99999}" ]; then echo '{"returnValue":false,"errorText":"not ready"}'; exit 0; fi
        printf '{"returnValue":true,"running":['
        sep=
        while read -r id; do
            pid=42
            [ ! -f "$T/pid_$id" ] || pid=$(cat "$T/pid_$id")
            printf '%s{"id":"%s","processid":"%s","webprocessid":""}' "$sep" "$id" "$pid"
            sep=,
        done <"$T/running"
        echo ']}' ;;
    com.webos.applicationManager/getForegroundAppInfo)
        printf '{"appId":"%s","windowId":"","processId":"","returnValue":true}\n' "$(cat "$T/front")" ;;
    com.webos.applicationManager/closeByAppId)
        if [ -n "${FAKE_CLOSE_FAILS:-}" ]; then echo '{"returnValue":false,"errorText":"busy"}'; exit 0; fi
        # A slow exit: still listed, with its pid, until the next minute.
        if [ -n "${FAKE_SLOW_EXIT:-}" ]; then touch "$T/exiting"; echo '{"returnValue":true}'; exit 0; fi
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
    # sleep: a moment, and the uptime moves on by the seconds asked. A slowly
    # exiting LG's Home is gone after a minute's sleep; a fake SAM brings it
    # back, with the next pid, while it is kept alive; and the kill switch or
    # the home can change once the uptime passes a mark.
    cat >"$T/bin/sleep" <<'EOF'
#!/bin/sh
echo "sleep $1" >>"$CALLS"
read -r up _ <"$T/uptime"
up=$((${up%%.*} + $1))
echo "$up.00 0.00" >"$T/uptime"
if [ -e "$T/exiting" ] && [ "$1" -ge 60 ]; then
    grep -vx com.webos.app.home "$T/running" >"$T/running.new"
    mv "$T/running.new" "$T/running"
    rm "$T/exiting"
fi
if [ -n "${FAKE_KEEPALIVE:-}" ] && ! grep -qx com.webos.app.home "$T/running"; then
    echo com.webos.app.home >>"$T/running"
    echo $(($(cat "$T/pid_com.webos.app.home" 2>/dev/null || echo 42) + 1)) >"$T/pid_com.webos.app.home"
fi
[ -z "${FAKE_KILL_AT:-}" ] || [ "$up" -lt "$FAKE_KILL_AT" ] || touch "$T/lumen_disable"
[ -z "${FAKE_LG_HOME_AT:-}" ] || [ "$up" -lt "$FAKE_LG_HOME_AT" ] || echo com.webos.app.home >"$T/home"
exec "$REAL_SLEEP" 0.01
EOF
    # node, unless a test takes it away, from everyone or from the TV's
    # scripts only (they run it with -e; the PC runs node --check).
    cat >"$T/bin/node" <<'EOF'
#!/bin/sh
[ -z "${FAKE_NO_NODE:-}" ] || exit 127
if [ "$1" = -e ]; then
    [ -z "${FAKE_TV_NO_NODE:-}" ] || exit 127
    case $2 in *"new Function"*) [ -z "${FAKE_TV_COMPILE_FAILS:-}" ] || exit 1 ;; esac
fi
exec "$REAL_NODE" "$@"
EOF
    # timeout, unless a test takes it away: then neither form works.
    cat >"$T/bin/timeout" <<'EOF'
#!/bin/sh
[ -z "${FAKE_NO_TIMEOUT:-}" ] || exit 125
exec "$REAL_TIMEOUT" "$@"
EOF
    for tool in mount umount initctl reboot; do
        printf '#!/bin/sh\necho "%s $*" >>"$CALLS"\n' "$tool" >"$T/bin/$tool"
    done
    # tools/tv.sh: logs each call to its own file; runs the scripts
    # tools/takeover.sh generates here, after FAKE_TAMPER (a sed script) has
    # had its way with them, piped to sh -s the way tools/tv.sh script sends
    # them, and answers status.sh with the sums asked for.
    cat >"$T/tv.sh" <<'EOF'
#!/bin/sh
printf '%s|%s|%s\n' "$1" "$2" "${3:-}" >>"$T/tvcalls"
case $1 in
    script)
        case $2 in
            */status.sh)
                printf 'default home: Lumen\nhook in init.d: %s\nfilter file: %s\n' "${FAKE_TV_HOOK:-none}" "${FAKE_TV_FILTER:-none}" ;;
            */hook-install.sh | */filter-apply.sh)
                cp "$2" "$T/sent-raw.sh"
                sed -f "$T/remap.sed" "$2" >"$T/sent.sh"
                [ -z "${FAKE_TAMPER:-}" ] || sed -i "$FAKE_TAMPER" "$T/sent.sh"
                { echo '{'; cat "$T/sent.sh"; printf '\n} </dev/null\n'; } | sh -s ;;
            *) echo "ran $2" ;;
        esac ;;
esac
EOF
    chmod +x "$T/bin/"* "$T/tv.sh"
    export LUMEN_TV_SH="$T/tv.sh"
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
        grep -q 'lumen-home: done' "$T/lumen/log/lumen-home.log" 2>/dev/null && return 0
        "$REAL_SLEEP" 0.05
    done
    echo "the hook never finished:"
    cat "$T/lumen/log/lumen-home.log"
    return 1
}

# wait_calls PATTERN N: N calls that match, for a hook that logs nothing.
wait_calls() {
    for _ in $(seq 400); do
        [ "$(grep -c -- "$1" "$CALLS")" -ge "$2" ] && return 0
        "$REAL_SLEEP" 0.05
    done
    echo "never $2 calls of $1:"
    cat "$CALLS"
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

# The filter or its no-op build put on the fake TV through tools/takeover.sh.
filter_apply() {
    run "$CTL" filter apply "$@"
}

# Nothing is left in the stage, and the stage is root's alone.
stage_clean() {
    [ -z "$(ls -A "$T/lumen/stage")" ]
    [ "$(stat -c %a "$T/lumen/stage")" = 700 ]
}

# Code without comments or the words of messages, for the read-only checks;
# the log helpers the boot hooks share are checked in hook-logs.bats.
commands() {
    sed -e '/^chain() {/,/^}/d' -e '/^folder_ok() {/,/^}/d' -e '/^log_to() {/,/^}/d' -e 's/^[[:space:]]*#.*//' -e 's/[[:space:]]#.*//' \
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
    [[ "$output" == *"no log: the hook has not run, or its log folder failed its check"* ]]
    not grep -qE '/set[A-Z]|/close' "$CALLS"
}

@test "status shows the hook's log from /var/lib/lumen/log" {
    run_tv apply.sh
    run_hook
    wait_done
    run_tv status.sh
    [ "$status" -eq 0 ]
    [[ "$output" == *"hook log, the latest run:"*"round 1: closed LG Home"*"lumen-home: done"* ]]
}

# --- the boot hook

@test "the hook returns at once, waits for SAM, closes LG's Home and logs the running list 10 s later" {
    FAKE_SAM_DOWN_FOR=2 run_tv apply.sh
    : >"$CALLS"
    FAKE_SAM_DOWN_FOR=2 run_hook
    [ "$status" -eq 0 ]
    wait_done
    log=$(cat "$T/lumen/log/lumen-home.log")
    [[ "$log" == *"SAM answers"* ]]
    [[ "$log" == *"round 1: closed LG Home (pid 42)"* ]]
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
    log=$(cat "$T/lumen/log/lumen-home.log")
    [[ "$log" == *"round 1: LG Home is back 10 s later as pid 43 (was 42)"* ]]
    [[ "$log" == *"round 2: closed LG Home (pid 43)"* ]]
    run grep -c 'closeByAppId' "$CALLS"
    [ "$output" -ge 3 ]
    [ "$output" -le 5 ]
}

@test "the hook tells LG's Home still exiting 10 s later, the same pid, from keep-alive's restart" {
    run_tv apply.sh
    : >"$CALLS"
    FAKE_SLOW_EXIT=1 run_hook
    wait_done
    log=$(cat "$T/lumen/log/lumen-home.log")
    [[ "$log" == *"round 1: LG Home still exiting 10 s later (pid 42); running: com.webos.app.hdmi3 com.webos.app.home"* ]]
    [[ "$log" == *"round 2: LG Home is not running"* ]]
    not grep -q 'is back' <<<"$log"
    [ "$(grep -c closeByAppId "$CALLS")" -eq 1 ]
}

@test "the hook says so when it can't read LG's Home's pid, or SAM stops answering" {
    run_tv apply.sh
    : >"$T/pid_$LG"
    FAKE_SLOW_EXIT=1 run_hook
    wait_done
    log=$(cat "$T/lumen/log/lumen-home.log")
    [[ "$log" == *"round 1: closed LG Home (pid unknown)"* ]]
    [[ "$log" == *"round 1: LG Home still listed 10 s later, pid unknown"* ]]
    rm "$T/pid_$LG"
    echo "$LG" >>"$T/running"
    : >"$CALLS"
    # The wait for SAM and round 1's list answer; the rest don't.
    FAKE_SAM_DOWN_FROM=3 run_hook
    wait_done
    log=$(cat "$T/lumen/log/lumen-home.log")
    [[ "$log" == *"round 1: closed LG Home (pid 42)"* ]]
    [[ "$log" == *"round 1: SAM didn't answer 10 s later"* ]]
    [[ "$log" == *"round 2: SAM didn't answer"* ]]
    [ "$(grep -c closeByAppId "$CALLS")" -eq 1 ]
}

@test "the hook's work ends by itself within five minutes, even with a late SAM and a close each round" {
    run_tv apply.sh
    start=$(uptime_now)
    FAKE_SAM_DOWN_FOR=5 FAKE_KEEPALIVE=1 run_hook
    wait_done
    [ $(($(uptime_now) - start)) -le 300 ]
    not grep -q 'round 6' "$T/lumen/log/lumen-home.log"
}

@test "the hook leaves LG's Home alone when it is in front, or not running" {
    run_tv apply.sh
    echo "$LG" >"$T/front"
    run_hook
    wait_done
    [[ "$(cat "$T/lumen/log/lumen-home.log")" == *"round 1: LG Home is in front; left alone"* ]]
    not grep -q closeByAppId "$CALLS"
}

@test "with LG's Home as the default the hook does nothing and says so" {
    run_hook
    wait_done
    [[ "$(cat "$T/lumen/log/lumen-home.log")" == *"the default home is com.webos.app.home, not Lumen; nothing done"* ]]
    not grep -q closeByAppId "$CALLS"
}

@test "the hook stops when the default home is set back while it runs" {
    run_tv apply.sh
    FAKE_LG_HOME_AT=150 run_hook
    wait_done
    [[ "$(cat "$T/lumen/log/lumen-home.log")" == *"not Lumen; nothing done"* ]]
    [ "$(grep -c closeByAppId "$CALLS")" -eq 1 ]
}

@test "when SAM never answers the hook gives up and does nothing" {
    run_tv apply.sh
    : >"$CALLS"
    FAKE_SAM_DOWN_FOR=99 run_hook
    wait_done
    [[ "$(cat "$T/lumen/log/lumen-home.log")" == *"SAM never answered; nothing done"* ]]
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
    [ ! -e "$T/lumen/log/lumen-home.log" ]
    rm "$T/lumen_disable"
    FAKE_KILL_AT=140 run_hook
    wait_done
    [ "$(grep -c closeByAppId "$CALLS")" -eq 1 ]
    not grep -q 'round 3' "$T/lumen/log/lumen-home.log"
}

@test "the hook never sets anything, mounts, or touches a job, and writes only its log" {
    run_tv apply.sh
    : >"$CALLS"
    FAKE_KEEPALIVE=1 run_hook
    wait_done
    not grep -qE 'setSystemSettings|setConfigs|^mount|^umount|^initctl|^reboot' "$CALLS"
    code=$(commands "$TAKEOVER/lumen-home")
    not grep -Eq '\b(setSystemSettings|setConfigs|u?mount|initctl|reboot|launch)\b' <<<"$code"
    grep -qx 'LOGDIR=/var/lib/lumen/log' "$TAKEOVER/lumen-home"
    grep -qx 'LOG=$LOGDIR/lumen-home.log' "$TAKEOVER/lumen-home"
    run grep -Eo '>>?[[:space:]]*[^[:space:]&;)|]+' <<<"$code"
    for target in "${lines[@]}"; do
        [[ "$target" =~ ^\>\>?[[:space:]]*(\"\$OUT\"|/dev/null)$ ]] || {
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
    echo "$LG" >>"$T/running"
    FAKE_SLOW_EXIT=1 run sh "$T/home-apply.sh" 3>&-
    [ "$status" -eq 0 ]
    [[ "$output" == *"round 1: LG Home still exiting"* ]]
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

@test "without a working timeout command the hook does nothing and says so" {
    run_tv apply.sh
    : >"$CALLS"
    FAKE_NO_TIMEOUT=1 run_hook
    wait_done
    log=$(cat "$T/lumen/log/lumen-home.log")
    [[ "$log" == *"no timeout command; nothing done"* ]]
    not grep -q '^luna' "$CALLS"
    grep -q "$LG" "$T/running"
}

@test "the hook keeps one run before in lumen-home.log.1, in a folder only root may use" {
    run_tv apply.sh
    log="$T/lumen/log/lumen-home.log"
    run_hook
    wait_done
    grep -q 'round 1: closed LG Home' "$log"
    run_hook
    wait_done
    echo "$LG" >>"$T/running"
    echo "$LG" >"$T/front"
    run_hook
    wait_done
    grep -q 'round 1: LG Home is in front' "$log"
    grep -q 'round 1: LG Home is not running' "$log.1"
    not grep -q 'closed LG Home' "$log" "$log.1"
    [ "$(ls "$T/lumen/log" | tr '\n' ' ')" = "lumen-home.log lumen-home.log.1 " ]
    [ "$(stat -c %a "$T/lumen/log")" = 700 ]
}

@test "with a log folder others may write the hook logs nothing, says why, and still closes LG's Home" {
    run_tv apply.sh
    : >"$CALLS"
    mkdir "$T/lumen/log"
    chmod 777 "$T/lumen/log"
    # The default home goes back to LG's in round 2, so the hook ends there.
    FAKE_LG_HOME_AT=150 run_hook
    [ "$status" -eq 0 ]
    [[ "$output" == *"$T/lumen/log is writable by others"* ]]
    [[ "$output" == *"no log: this run's output goes to /dev/null"* ]]
    wait_calls getSystemSettings 2
    [ "$(grep -c closeByAppId "$CALLS")" -eq 1 ]
    [ -z "$(ls -A "$T/lumen/log")" ]
}

@test "home-apply.sh says when this run left no log, even with an earlier run's log there" {
    run_tv apply.sh
    mkdir -p "$T/takeover"
    cp "$T/lumen-home" "$T/takeover/boot-hook"
    run_hook
    wait_done
    chmod 777 "$T/lumen/log"
    : >"$CALLS"
    FAKE_LG_HOME_AT=150 run sh "$T/home-apply.sh" 3>&-
    [ "$status" -eq 1 ]
    [[ "$output" == *"is writable by others"* ]]
    [[ "$output" == *"takeover: no log from this run"* ]]
    # The hook still runs its rounds, unlogged, and the old log is untouched.
    wait_calls getSystemSettings 2
    not grep -q 'not Lumen' "$T/lumen/log/lumen-home.log"
    [ ! -e "$T/lumen/log/lumen-home.log.1" ]
}

# --- the key filter, put on the fake TV through tools/takeover.sh

@test "filter apply puts Lumen's entry just before handleSystemKeys, and saves the list without it" {
    filter_apply
    [ "$status" -eq 0 ]
    [ "$(handlers)" = "handleScreenSaver handleAccessibility handleAvReset handleLowSpeedDataBroadcast lumenHomeKeys handleSystemKeys handleHotkeys handleCPHotkeys handleFactoryKeys handleTvhotkeyAppLaunch handleLocalKeys handleMagicNumber" ]
    grep -qF "{\"file\":\"$FILTER\",\"handler\":\"lumenHomeKeys\"}" "$T/chain"
    [ "$(cat "$T/takeover/keyFilters.before")" = "$(canon <"$FIXTURE")" ]
    cmp "$FILTER" "$TAKEOVER/filters/lumen-home.js"
    [ "$(stat -c %a "$FILTER")" = 644 ]
    stage_clean
    [[ "$output" == *"5. lumenHomeKeys in $FILTER   (Lumen)"* ]]
    [[ "$output" == *"Nothing was"* ]]
    not grep -qE '^(initctl|reboot)' "$CALLS"
    # Only the script went to the TV: no push, nothing through /tmp.
    [ "$(wc -l <"$T/tvcalls")" -eq 1 ]
    [[ "$(cat "$T/tvcalls")" == "script|"*"/filter-apply.sh|" ]]
    [ -z "$(ls -A "$T/tmp")" ]
}

@test "the script sent carries the PC's SHA-256 and the file itself" {
    filter_apply
    [ "$(sed -n 1p "$T/sent-raw.sh")" = "WANT=$(sha256sum <"$TAKEOVER/filters/lumen-home.js" | cut -d' ' -f1)" ]
    sed -n "/^    cat <<'LUMEN_STAGED_FILE_END'\$/,/^LUMEN_STAGED_FILE_END\$/p" "$T/sent-raw.sh" |
        sed '1d;$d' >"$T/carried.js"
    cmp "$T/carried.js" "$TAKEOVER/filters/lumen-home.js"
    # The file comes before the template, and nothing else carries it.
    [ "$(grep -c '^staged_copy() {$' "$T/sent-raw.sh")" -eq 1 ]
}

@test "a staged filter that differs from the PC's is removed and changes nothing" {
    FAKE_TAMPER='s/^var lumenHomeFired = false;$/var lumenHomeFired = 0;/' filter_apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"the staged filter is not the one the PC checked; nothing changed"* ]]
    stage_clean
    [ ! -e "$FILTER" ]
    [ ! -e "$T/takeover/keyFilters.before" ]
    not grep -q setConfigs "$CALLS"
    cmp "$T/chain" "$FIXTURE"
}

@test "a SHA-256 changed on the way is refused just the same" {
    FAKE_TAMPER='1s/^WANT=.*/WANT=0000000000000000000000000000000000000000000000000000000000000000/' filter_apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"not the one the PC checked"* ]]
    stage_clean
    [ ! -e "$FILTER" ]
}

@test "filter apply refuses a stage a jail could reach: a link, a folder others may write, another owner" {
    mkdir -p "$T/lumen" "$T/elsewhere"
    ln -s "$T/elsewhere" "$T/lumen/stage"
    filter_apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"$T/lumen/stage is not a real folder"* ]]
    [ -z "$(ls -A "$T/elsewhere")" ]
    rm "$T/lumen/stage"
    mkdir "$T/lumen/stage"
    chmod 777 "$T/lumen/stage"
    filter_apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"is writable by others"* ]]
    [ -z "$(ls -A "$T/lumen/stage")" ]
    chmod 700 "$T/lumen/stage"
    FAKE_TAMPER='s/^ROOT_UID=.*/ROOT_UID=99999/' filter_apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"not root"* ]]
    # The folder above the stage counts too.
    rm -rf "$T/lumen"
    mkdir -p "$T/real"
    ln -s "$T/real" "$T/lumen"
    filter_apply
    [ "$status" -eq 1 ]
    [ -z "$(ls -A "$T/real")" ]
    [ ! -e "$FILTER" ]
    not grep -q setConfigs "$CALLS"
}

@test "filter-apply.sh run by itself, without the PC's SHA-256 and file, refuses" {
    run_tv filter-apply.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"no SHA-256 from the PC: run tools/takeover.sh filter apply"* ]]
    printf 'WANT=%064d\n' 0 | cat - "$T/filter-apply.sh" >"$T/no-file.sh"
    run sh "$T/no-file.sh"
    [ "$status" -eq 1 ]
    [[ "$output" == *"no file from the PC"* ]]
    [ ! -e "$T/lumen/stage" ] || stage_clean
    not grep -q luna "$CALLS"
}

@test "filter apply refuses a file the TV's node won't compile, no node on the TV, and the kill switch" {
    FAKE_TV_COMPILE_FAILS=1 filter_apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"does not compile on the TV's node"* ]]
    stage_clean
    FAKE_TV_NO_NODE=1 filter_apply
    [ "$status" -eq 1 ]
    stage_clean
    touch "$T/lumen_disable"
    filter_apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"lumen_disable exists; nothing changed"* ]]
    not grep -q setConfigs "$CALLS"
    [ ! -e "$FILTER" ]
    [ ! -e "$T/takeover/keyFilters.before" ]
    cmp "$T/chain" "$FIXTURE"
}

@test "filter apply refuses a list with no handleSystemKeys entry" {
    sed 's/handleSystemKeys/handleOtherKeys/' "$FIXTURE" >"$T/chain"
    filter_apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"no handleSystemKeys entry"* ]]
    not grep -q setConfigs "$CALLS"
    [ ! -e "$FILTER" ]
    stage_clean
}

@test "filter apply again changes only the file, and says a reboot loads it" {
    filter_apply
    : >"$CALLS"
    before=$(cat "$T/chain")
    filter_apply --noop
    [ "$status" -eq 0 ]
    [[ "$output" == *"already in place"* ]]
    [[ "$output" == *"reboot"* ]]
    grep -qx 'var lumenHomeActs = false;' "$FILTER"
    [ "$(cat "$T/chain")" = "$before" ]
    not grep -q setConfigs "$CALLS"
    stage_clean
}

@test "filter apply whose list reads back otherwise writes the old list back and removes the file" {
    FAKE_CONFIG_IGNORED=1 filter_apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"writing the old one back"* ]]
    [ "$(grep -c setConfigs "$CALLS")" -eq 2 ]
    [ ! -e "$FILTER" ]
    stage_clean
}

@test "filter undo takes Lumen's entry out, says the list is the saved one, and removes its files" {
    filter_apply
    run_tv filter-undo.sh
    [ "$status" -eq 0 ]
    [ "$(cat "$T/chain")" = "$(canon <"$FIXTURE")" ]
    [[ "$output" == *"the list is the one saved before filter-apply.sh"* ]]
    [ ! -e "$FILTER" ]
    [ ! -e "$T/takeover/keyFilters.before" ]
}

@test "filter undo keeps what changed in the list since apply" {
    filter_apply
    # Another filter put first after Lumen's apply, as Glasshouse puts its own.
    sed 's|^\[|[{"file":"/var/lib/tvweb/remotekeys-filter.js","handler":"remoteKeys"},|' "$T/chain" >"$T/chain.new"
    mv "$T/chain.new" "$T/chain"
    run_tv filter-undo.sh
    [ "$status" -eq 0 ]
    [ "$(handlers)" = "remoteKeys handleScreenSaver handleAccessibility handleAvReset handleLowSpeedDataBroadcast handleSystemKeys handleHotkeys handleCPHotkeys handleFactoryKeys handleTvhotkeyAppLaunch handleLocalKeys handleMagicNumber" ]
    [[ "$output" == *"differs from the one saved"* ]]
}

@test "filter undo without node writes the saved list back" {
    filter_apply
    FAKE_NO_NODE=1 run_tv filter-undo.sh
    [ "$status" -eq 0 ]
    [[ "$output" == *"writing the saved list back"* ]]
    [ "$(cat "$T/chain")" = "$(canon <"$FIXTURE")" ]
    [ ! -e "$FILTER" ]
}

@test "filter undo keeps its files while the entry stays, and tidies up when it is gone" {
    filter_apply
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
    filter_apply
    : >"$CALLS"
    run_tv filter-check.sh
    [ "$status" -eq 0 ]
    [[ "$output" == *"5. lumenHomeKeys in $FILTER   (Lumen)"* ]]
    [[ "$output" == *"6. handleSystemKeys in /usr/lib/qt5/qml/KeyFilters/systemUi.js"* ]]
    [[ "$output" == *"Lumen's entry: in the chain"* ]]
    [[ "$output" == *"(no $T/ysm/StarfishKeyFilter.qml)"* ]]
    not grep -qE '/set[A-Z]|/close' "$CALLS"
}

# --- the hook put on the fake TV through tools/takeover.sh

@test "hook push stages the hook in root's stage, checks it, and puts only the by-hand copy in place" {
    run "$CTL" hook push
    [ "$status" -eq 0 ]
    cmp "$T/takeover/boot-hook" "$TAKEOVER/lumen-home"
    [ "$(stat -c %a "$T/takeover/boot-hook")" = 644 ]
    [ ! -e "$T/initd/lumen-home" ]
    stage_clean
    [[ "$output" == *"boot-hook: $(sha256sum <"$TAKEOVER/lumen-home" | cut -d' ' -f1)"* ]]
    [[ "$output" == *"tools/tv.sh script takeover/home-apply.sh runs it by hand"* ]]
    [ "$(wc -l <"$T/tvcalls")" -eq 1 ]
    [[ "$(cat "$T/tvcalls")" == "script|"*"/hook-install.sh|" ]]
    [ -z "$(ls -A "$T/tmp")" ]
}

@test "hook install puts the hook into init.d as well, whole and executable, and doesn't run it" {
    run "$CTL" hook install
    [ "$status" -eq 0 ]
    [[ "$output" == *"installed /var/lib/webosbrew/init.d/lumen-home"* ]]
    cmp "$T/initd/lumen-home" "$TAKEOVER/lumen-home"
    [ "$(stat -c %a "$T/initd/lumen-home")" = 755 ]
    cmp "$T/takeover/boot-hook" "$TAKEOVER/lumen-home"
    [ -z "$(ls "$T/initd" | grep -v '^lumen-home$')" ]
    stage_clean
    not grep -q '^luna' "$CALLS"
    [ ! -e "$T/lumen/log/lumen-home.log" ]
}

@test "a staged hook that differs from the PC's installs nothing anywhere" {
    FAKE_TAMPER='s/^DEADLINE=260$/DEADLINE=99999/' run "$CTL" hook install
    [ "$status" -eq 1 ]
    [[ "$output" == *"the staged hook is not the one the PC sent; nothing changed"* ]]
    [ ! -e "$T/initd/lumen-home" ]
    [ ! -e "$T/takeover/boot-hook" ]
    stage_clean
    # An installed hook stays as it was.
    run "$CTL" hook install
    FAKE_TAMPER='s/^DEADLINE=260$/DEADLINE=99999/' run "$CTL" hook install
    [ "$status" -eq 1 ]
    cmp "$T/initd/lumen-home" "$TAKEOVER/lumen-home"
    cmp "$T/takeover/boot-hook" "$TAKEOVER/lumen-home"
}

@test "hook install refuses a stage a jail could reach, and a hook that doesn't parse" {
    mkdir -p "$T/lumen" "$T/elsewhere"
    ln -s "$T/elsewhere" "$T/lumen/stage"
    run "$CTL" hook install
    [ "$status" -eq 1 ]
    [[ "$output" == *"is not a real folder"* ]]
    [ -z "$(ls -A "$T/elsewhere")" ]
    [ ! -e "$T/initd/lumen-home" ]
    rm "$T/lumen/stage"
    root="$T/root"
    mkdir -p "$root/tools" "$root/takeover"
    cp "$CTL" "$root/tools/"
    cp "$TAKEOVER/hook-install.sh" "$root/takeover/"
    printf 'if then\n' >"$root/takeover/lumen-home"
    run "$root/tools/takeover.sh" hook install
    [ "$status" -eq 1 ]
    [[ "$output" == *"the hook does not parse"* ]]
    [ ! -e "$T/initd/lumen-home" ]
    stage_clean
}

@test "hook-install.sh run by itself refuses" {
    run_tv hook-install.sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"no SHA-256 from the PC: run tools/takeover.sh hook push"* ]]
    [ ! -e "$T/takeover/boot-hook" ]
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

@test "nothing in takeover/ writes outside /var/lib/lumen, but the hook into init.d" {
    for f in $SCRIPTS; do
        code=$(commands "$TAKEOVER/$f")
        # Each path variable is under /var/lib/lumen's takeover, stage or log
        # folder, never /tmp; hook-install.sh alone names init.d's hook.
        while IFS='=' read -r name value; do
            case $f:$name=$value in
                *=/var/lib/lumen/takeover | *='$DIR/'*) ;;
                *:LOGDIR=/var/lib/lumen/log | *:LOG='$LOGDIR/'* | *:LOG=/var/lib/lumen/log/lumen-home.log) ;;
                *:STAGE=/var/lib/lumen/stage | *:STAGED='$STAGE/'*) ;;
                hook-install.sh:HOOK_DEST=/var/lib/webosbrew/init.d/lumen-home) ;;
                *) echo "$f: $name=$value"; return 1 ;;
            esac
        done < <(grep -E '^(DIR|BEFORE|FILTER|STAGE|STAGED|SAVED|LOG|LOGDIR|HOOK_DEST)=' "$TAKEOVER/$f")
        run grep -Eo '>>?[[:space:]]*[^[:space:]&;)|]+' <<<"$code"
        for target in "${lines[@]}"; do
            [[ "$target" =~ ^\>\>?[[:space:]]*(/dev/null|\"\$OUT\"|\"\$BEFORE.tmp\"|\"\$DIR/keyFilters.before.tmp\"|\"\$STAGED\")$ ]] || {
                echo "$f writes to $target"
                return 1
            }
        done
        # Every file command names only those paths.
        run grep -Eo '\b(rm|mv|cp|touch|mkdir|rmdir|ln|chmod|tee|dd|truncate)\b[^;|&]*' <<<"$code"
        for cmd in "${lines[@]}"; do
            for word in ${cmd#* }; do
                word=${word%\'}
                word=${word%)}
                case $word in
                    -f | -p | 644 | 755 | EXIT | 2\>/dev/null | '"$DIR"' | '"$DIR/'* | '"$BEFORE'* | '"$FILTER'* | '"$STAGED'* | '"$STAGE"' | '"${STAGE%/*}"' | '"$SAVED"') ;;
                    '"$HOOK_DEST'*) [ "$f" = hook-install.sh ] || { echo "$f: $cmd"; return 1; } ;;
                    *) echo "$f: $cmd"; return 1 ;;
                esac
            done
        done
    done
}

@test "the scripts share their luna, setting, list, sum and stage helpers word for word" {
    for fn in call apps_of home_of chain sha stage_ok; do
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
    # Three copies of the list helper: check, apply and undo; two of the stage's.
    [ "$(grep -l '^chain() {' "$TAKEOVER"/*.sh | wc -l)" -eq 3 ]
    [ "$(grep -l '^stage_ok() {' "$TAKEOVER"/*.sh | wc -l)" -eq 2 ]
    # Nothing in takeover/ stages in /tmp.
    not grep -n 'lumen-home-filter\|lumen-home\.new\|\.sha256' "$TAKEOVER"/*.sh "$TAKEOVER/lumen-home" "$BATS_TEST_DIRNAME/../takeover.sh"
}

# --- tools/takeover.sh's own commands, against the fake tools/tv.sh

@test "takeover.sh: an unknown command fails with usage and runs nothing" {
    for args in frobnicate "hook" "hook frob" "filter" "filter apply --yes"; do
        # shellcheck disable=SC2086
        run "$CTL" $args
        [ "$status" -eq 2 ]
        [[ "$output" == *"tools/takeover.sh filter apply [--noop]"* ]]
    done
    [ ! -e "$T/tvcalls" ]
}

@test "takeover.sh apply reminds of the backup, then runs takeover/apply.sh; undo runs undo.sh" {
    run "$CTL" apply
    [ "$status" -eq 0 ]
    [[ "${lines[0]}" == "tools/backup.sh comes first"* ]]
    run "$CTL" undo
    run cat "$T/tvcalls"
    [[ "${lines[0]}" == "script|"*"/takeover/apply.sh|" ]]
    [[ "${lines[1]}" == "script|"*"/takeover/undo.sh|" ]]
    [ "${#lines[@]}" -eq 2 ]
}

@test "takeover.sh hook status reads the hook's logs from /var/lib/lumen/log" {
    run "$CTL" hook status
    [ "$status" -eq 0 ]
    calls=$(cat "$T/tvcalls")
    [[ "$calls" == *"tail -n 30 /var/lib/lumen/log/lumen-home.log;"* ]]
    [[ "$calls" == *"tail -n 5 /var/lib/lumen/log/lumen-home.log.1;"* ]]
    [[ "$calls" == *"no log: the hook has not run"* ]]
    [[ "$calls" != *"/tmp"* ]]
}

@test "takeover.sh hook remove deletes only the hook" {
    run "$CTL" hook remove
    [ "$status" -eq 0 ]
    [ "$(cat "$T/tvcalls")" = "run|rm -f /var/lib/webosbrew/init.d/lumen-home && echo 'removed /var/lib/webosbrew/init.d/lumen-home'|" ]
}

@test "takeover.sh filter apply --noop sends the build that differs in its one switch" {
    filter_apply --noop
    [ "$status" -eq 0 ]
    [[ "$output" == *"acts on nothing"* ]]
    run diff "$TAKEOVER/filters/lumen-home.js" "$FILTER"
    [ "$status" -eq 1 ]
    [ "$(printf '%s\n' "$output" | grep -c '^[<>]')" -eq 2 ]
    [[ "$output" == *"> var lumenHomeActs = false;"* ]]
    [ "$(sed -n 1p "$T/sent-raw.sh")" = "WANT=$(sha256sum <"$FILTER" | cut -d' ' -f1)" ]
}

@test "takeover.sh refuses a filter newer than ES5, or a file it can't carry whole, and sends nothing" {
    root="$T/root"
    mkdir -p "$root/tools" "$root/takeover/filters"
    cp "$CTL" "$root/tools/"
    cp "$TAKEOVER/filter-apply.sh" "$TAKEOVER/hook-install.sh" "$root/takeover/"
    sed 's/^var lumenHomeFired = false;$/let lumenHomeFired = false;/' "$TAKEOVER/filters/lumen-home.js" >"$root/takeover/filters/lumen-home.js"
    run "$root/tools/takeover.sh" filter apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"newer than ES5"* ]]
    { cat "$TAKEOVER/filters/lumen-home.js"; echo LUMEN_STAGED_FILE_END; } >"$root/takeover/filters/lumen-home.js"
    run "$root/tools/takeover.sh" filter apply
    [ "$status" -eq 1 ]
    [[ "$output" == *"holds the line LUMEN_STAGED_FILE_END"* ]]
    printf '#!/bin/sh\ntrue' >"$root/takeover/lumen-home"
    run "$root/tools/takeover.sh" hook push
    [ "$status" -eq 1 ]
    [[ "$output" == *"does not end in a newline"* ]]
    [ ! -e "$T/tvcalls" ]
}

@test "takeover.sh status names which hook and which filter build the TV has" {
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
