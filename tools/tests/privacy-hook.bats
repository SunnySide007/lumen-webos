#!/usr/bin/env bats
# The privacy boot hook (profiles/privacy/lumen-privacy) against fake initctl,
# iptables and sleep, and tools/privacy-hook.sh against a fake tools/tv.sh.
# Nothing reaches a TV.

setup() {
    T="$BATS_TEST_TMPDIR"
    CALLS="$T/calls"
    : >"$CALLS"
    export CALLS
    mkdir -p "$T/bin"
    # The hook with its kill switch and log moved into this test's folder.
    sed -e "s|/var/luna/preferences/lumen_disable|$T/lumen_disable|" \
        -e "s|/tmp/lumen-privacy.log|$T/log|" \
        "$BATS_TEST_DIRNAME/../../profiles/privacy/lumen-privacy" >"$T/hook"
    cat >"$T/bin/initctl" <<'EOF'
#!/bin/sh
echo "initctl $*" >>"$CALLS"
if [ "$1" = status ]; then
    if [ -n "${FAKE_STOPPED:-}" ] || grep -qx "initctl stop $2" "$CALLS"; then
        echo "$2 stop/waiting"
    else
        echo "$2 start/running, process 42"
    fi
fi
EOF
    cat >"$T/bin/iptables" <<'EOF'
#!/bin/sh
echo "iptables $*" >>"$CALLS"
if [ "$1" = -L ] && { [ -n "${FAKE_RULE:-}" ] || grep -q '^iptables -I' "$CALLS"; }; then
    echo 'REJECT tcp -- 0.0.0.0/0 0.0.0.0/0 tcp dpt:11111 reject-with icmp-port-unreachable'
fi
EOF
    printf '#!/bin/sh\nexit 0\n' >"$T/bin/sleep"
    chmod +x "$T/bin/initctl" "$T/bin/iptables" "$T/bin/sleep"

    HOOKCTL="$BATS_TEST_DIRNAME/../privacy-hook.sh"
    cat >"$T/tv.sh" <<'EOF'
#!/bin/sh
printf '%s|%s|%s\n' "$1" "$2" "${3:-}" >>"$CALLS"
case $1 in
    push) cp "$2" "$FAKE_DIR/pushed" ;;
    run)
        case $2 in
            *'mv /var/lib/webosbrew/init.d/lumen-privacy.tmp'*)
                if [ -n "${FAKE_BAD_SHA:-}" ]; then echo 0000
                else sha256sum <"$FAKE_DIR/pushed" | cut -d' ' -f1; fi ;;
        esac ;;
esac
EOF
    chmod +x "$T/tv.sh"
    export FAKE_DIR="$T" LUMEN_TV_SH="$T/tv.sh"
}

run_hook() {
    PATH="$T/bin:$PATH" run sh "$T/hook"
}

# The hook works in the background; wait for its last line.
wait_done() {
    for _ in $(seq 100); do
        grep -q 'lumen-privacy: done' "$T/log" 2>/dev/null && return 0
        sleep 0.1
    done
    return 1
}

@test "the hook returns at once and stops each running job once, crashreportd first" {
    run_hook
    [ "$status" -eq 0 ]
    wait_done
    run grep '^initctl stop' "$CALLS"
    [ "${lines[*]}" = "initctl stop crashreportd initctl stop rdxd initctl stop uploadd initctl stop remotelogger" ]
}

@test "the hook rejects tcp 11111 once, from everything but loopback" {
    run_hook
    wait_done
    run grep '^iptables -I' "$CALLS"
    [ "${#lines[@]}" -eq 1 ]
    [ "${lines[0]}" = "iptables -I INPUT -p tcp --dport 11111 ! -i lo -j REJECT" ]
    grep -q 'tcp 11111 rejected from the LAN' "$T/log"
}

@test "the hook ends by itself after its sixty rounds" {
    run_hook
    wait_done
    [ "$(grep -c '^initctl status crashreportd' "$CALLS")" -eq 60 ]
}

@test "with the rule and the jobs already in place the hook changes nothing" {
    FAKE_RULE=1 FAKE_STOPPED=1 run_hook
    wait_done
    run grep -q -E '^(iptables -I|initctl stop)' "$CALLS"
    [ "$status" -eq 1 ]
    grep -q 'rule already in place' "$T/log"
}

@test "with the kill switch the hook does nothing at all" {
    touch "$T/lumen_disable"
    run_hook
    [ "$status" -eq 0 ]
    sleep 0.5
    [ ! -s "$CALLS" ]
    [ ! -e "$T/log" ]
}

@test "privacy-hook.sh: an unknown command fails with usage" {
    run "$HOOKCTL" frobnicate
    [ "$status" -eq 2 ]
    [[ "$output" == *"tools/privacy-hook.sh install"* ]]
    [ ! -s "$CALLS" ]
}

@test "privacy-hook.sh install checks the copy, moves it into place whole, then runs it" {
    run "$HOOKCTL" install
    [ "$status" -eq 0 ]
    [[ "$output" == *"installed /var/lib/webosbrew/init.d/lumen-privacy"* ]]
    run cat "$CALLS"
    # Staged where only root can write, never /tmp, which the app jails share.
    [[ "${lines[0]}" == "run|umask 077; mkdir -p /var/lib/lumen/stage && [ ! -L /var/lib/lumen/stage ]|" ]]
    [[ "${lines[1]}" == "push|"*"/profiles/privacy/lumen-privacy|/var/lib/lumen/stage/lumen-privacy" ]]
    [[ "${lines[2]}" == "run|sh -n /var/lib/lumen/stage/lumen-privacy && "*"&& mv /var/lib/webosbrew/init.d/lumen-privacy.tmp /var/lib/webosbrew/init.d/lumen-privacy"* ]]
    [ "${lines[3]}" = "run|/var/lib/webosbrew/init.d/lumen-privacy|" ]
    # The staged copy is checked against this checkout before it is copied.
    sha=$(sha256sum <"$BATS_TEST_DIRNAME/../../profiles/privacy/lumen-privacy" | cut -d' ' -f1)
    grep -qF "| cut -d' ' -f1)\" = $sha ] && cp /var/lib/lumen/stage/lumen-privacy /var/lib/webosbrew/init.d/lumen-privacy.tmp" "$CALLS"
}

@test "privacy-hook.sh install stops if the TV's copy differs, and doesn't run it" {
    FAKE_BAD_SHA=1 run "$HOOKCTL" install
    [ "$status" -eq 1 ]
    [[ "$output" == *"is not this checkout's"* ]]
    run grep -qx 'run|/var/lib/webosbrew/init.d/lumen-privacy|' "$CALLS"
    [ "$status" -eq 1 ]
}

@test "privacy-hook.sh remove deletes only the hook" {
    run "$HOOKCTL" remove
    [ "$status" -eq 0 ]
    [ "$(cat "$CALLS")" = "run|rm -f /var/lib/webosbrew/init.d/lumen-privacy && echo 'removed /var/lib/webosbrew/init.d/lumen-privacy'|" ]
}
