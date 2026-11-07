#!/usr/bin/env bats
# lumend's door (profiles/lumend/lumend), its role, retire and remove
# scripts, with every TV path moved into this test's folder, and
# tools/lumend.sh's checks against a fake tools/tv.sh that records its
# calls. Nothing reaches a TV.

PROFILE="$BATS_TEST_DIRNAME/../../profiles/lumend"
LUMEND_SH="$BATS_TEST_DIRNAME/../lumend.sh"

setup() {
    T="$BATS_TEST_TMPDIR"
    CALLS="$T/calls"
    export T CALLS
    : >"$CALLS"
    mkdir -p "$T/bin" "$T/initd" "$T/ls2/roles.d" "$T/ls2/client-permissions.d" "$T/ls2/manifests.d"
    mkdir -m 755 "$T/lumen"
    mkdir -m 700 "$T/lumen/lumend"
    # A lumend that says what it was asked.
    printf '#!/bin/sh\necho "lumend $*" >>"$CALLS"\n' >"$T/lumen/lumend/lumend"
    chmod 755 "$T/lumen/lumend/lumend"
    for f in lumend role-install.sh role-remove.sh door-install.sh door-remove.sh remove.sh; do
        sed -e "s|/var/luna/preferences/lumen_disable|$T/kill|g" \
            -e "s|/var/lib/lumen|$T/lumen|g" \
            -e "s|/var/lib/webosbrew/init.d/|$T/initd/|g" \
            -e "s|/var/luna-service2-dev|$T/ls2|g" \
            -e "s|/proc/mounts|$T/mounts|g" \
            -e "s|^ROOT_UID=0\$|ROOT_UID=$(id -u)|" \
            "$PROFILE/$f" >"$T/$f"
    done
    : >"$T/mounts"
    # A timeout of either kind: busybox's old `-t N`, or `N`.
    cat >"$T/bin/timeout" <<'EOF'
#!/bin/sh
[ -z "${FAKE_NO_TIMEOUT:-}" ] || exit 127
[ "$1" = -t ] && shift
shift
echo "timeout $*" >>"$CALLS"
exec "$@"
EOF
    chmod 755 "$T/bin/timeout"
    PATH="$T/bin:$PATH"
}

# wait_for TEXT: the background start has written TEXT to CALLS.
wait_for() {
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        grep -qF "$1" "$CALLS" && return 0
        sleep 0.1
    done
    return 1
}

@test "the door starts lumend, time-boxed, and returns" {
    run sh "$T/lumend"
    [ "$status" -eq 0 ]
    wait_for "lumend start"
    grep -qF "timeout $T/lumen/lumend/lumend start" "$CALLS"
}

@test "the door does nothing with the kill switch set" {
    touch "$T/kill"
    run sh "$T/lumend"
    [ "$status" -eq 0 ]
    sleep 0.3
    [ ! -s "$CALLS" ]
}

@test "the door starts nothing without a timeout command" {
    FAKE_NO_TIMEOUT=1 run sh "$T/lumend"
    [ "$status" -eq 0 ]
    sleep 0.3
    [ ! -s "$CALLS" ]
}

@test "the door runs no binary others could change, nor a link" {
    chmod 777 "$T/lumen/lumend"
    run sh "$T/lumend"
    sleep 0.3
    [ ! -s "$CALLS" ]
    chmod 700 "$T/lumen/lumend"
    chmod 775 "$T/lumen/lumend/lumend"
    run sh "$T/lumend"
    sleep 0.3
    [ ! -s "$CALLS" ]
    chmod 755 "$T/lumen/lumend/lumend"
    mv "$T/lumen/lumend/lumend" "$T/lumen/lumend/real"
    ln -s real "$T/lumen/lumend/lumend"
    run sh "$T/lumend"
    sleep 0.3
    [ ! -s "$CALLS" ]
}

@test "the role needs groups, refuses odd ones and writes three files" {
    run sh "$T/role-install.sh"
    [ "$status" -eq 1 ]
    [[ "$output" == *"no groups"* ]]
    sed 's/@LUMEND_GROUPS@/a;b/' "$T/role-install.sh" >"$T/odd.sh"
    run sh "$T/odd.sh"
    [ "$status" -eq 1 ]
    [ -z "$(ls -A "$T/ls2/roles.d")" ]
    sed 's/@LUMEND_GROUPS@/applications settings.read/' "$T/role-install.sh" >"$T/ok.sh"
    run sh "$T/ok.sh"
    [ "$status" -eq 0 ]
    role=$(cat "$T/ls2/roles.d/io.github.sunnyside007.lumend.json")
    [[ "$role" == *"\"exeName\":\"$T/lumen/lumend/lumend\""* ]]
    [[ "$role" == *'"allowedNames":["io.github.sunnyside007.lumend","io.github.sunnyside007.lumend.try"]'* ]]
    [[ "$role" != *inbound* ]]
    [ "$(cat "$T/ls2/client-permissions.d/io.github.sunnyside007.lumend.json")" = \
        '{"io.github.sunnyside007.lumend*":["applications","settings.read"]}' ]
    grep -qF "$T/ls2/roles.d/io.github.sunnyside007.lumend.json" "$T/ls2/manifests.d/io.github.sunnyside007.lumend.json"
    run sh "$T/role-remove.sh"
    [ "$status" -eq 0 ]
    for d in roles.d client-permissions.d manifests.d; do
        [ -z "$(ls -A "$T/ls2/$d")" ]
    done
}

@test "door install retires only the hooks of parts switched on, and remove puts them back" {
    cp "$T/lumend" "$T/lumen/lumend/door"
    echo hook >"$T/initd/lumen-privacy"
    echo hook >"$T/initd/lumen-home"
    printf 's1 on\nlg-home off\n' >"$T/lumen/lumend/lumend.conf"
    run sh "$T/door-install.sh"
    [ "$status" -eq 0 ]
    [ -f "$T/lumen/lumend/retired/lumen-privacy" ] && [ ! -e "$T/initd/lumen-privacy" ]
    [ -f "$T/initd/lumen-home" ]
    [ -x "$T/initd/lumend" ]
    [ "${lines[${#lines[@]}-1]}" = "$(sha256sum <"$T/lumen/lumend/door" | cut -d' ' -f1)" ]
    run sh "$T/door-remove.sh"
    [ "$status" -eq 0 ]
    [ ! -e "$T/initd/lumend" ]
    [ -f "$T/initd/lumen-privacy" ] && [ -f "$T/initd/lumen-home" ]
}

@test "remove stops lumend, undoes its RAM changes and leaves nothing of it" {
    cp "$T/lumend" "$T/initd/lumend"
    mkdir -p "$T/lumen/lumend/retired" "$T/lumen/log"
    echo hook >"$T/lumen/lumend/retired/lumen-home"
    : >"$T/lumen/log/lumend.log"
    sed 's/@LUMEND_GROUPS@/applications/' "$T/role-install.sh" >"$T/ok.sh"
    sh "$T/ok.sh" >/dev/null
    run sh "$T/remove.sh"
    [ "$status" -eq 0 ]
    grep -qx "lumend stop" "$CALLS"
    grep -qx "lumend undo senders" "$CALLS"
    grep -qx "lumend undo link" "$CALLS"
    [ ! -e "$T/initd/lumend" ] && [ -f "$T/initd/lumen-home" ]
    [ ! -e "$T/lumen/lumend" ] && [ ! -e "$T/lumen/log/lumend.log" ]
    [ -z "$(ls -A "$T/ls2/roles.d")" ]
    run sh "$T/remove.sh"
    [ "$status" -eq 0 ]
}

@test "lumend.sh checks parts, senders and groups before it reaches the TV" {
    printf '#!/bin/sh\necho "tv $*" >>"$CALLS"\n' >"$T/bin/tv.sh"
    chmod 755 "$T/bin/tv.sh"
    export LUMEN_TV_SH="$T/bin/tv.sh" LUMEN_LUMEND_OUT="$T/out"
    run "$LUMEND_SH" try telemetry
    [ "$status" -eq 1 ]
    run "$LUMEND_SH" try senders admanager
    [ "$status" -eq 1 ]
    run "$LUMEND_SH" undo 'link; reboot'
    [ "$status" -eq 1 ]
    run "$LUMEND_SH" role install 'all;reboot'
    [ "$status" -eq 1 ]
    run "$LUMEND_SH" conf on sender:iconnectivity
    [ "$status" -eq 1 ]
    [ ! -s "$CALLS" ]
    run "$LUMEND_SH" try senders acr2
    [ "$status" -eq 0 ]
    grep -qx "tv run /var/lib/lumen/lumend/lumend try senders acr2" "$CALLS"
    run "$LUMEND_SH" role install applications
    [ "$status" -eq 0 ]
    grep -q "LUMEND_GROUPS='applications'" "$T/out/role-install.sh"
}
