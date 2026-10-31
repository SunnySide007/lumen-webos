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
    # The hook with its kill switch and log folder moved into this test's
    # folder, and root's uid to ours; /var/lib/lumen as on the TV.
    mkdir -m 755 "$T/lumen"
    LOG="$T/lumen/log/lumen-privacy.log"
    sed -e "s|/var/luna/preferences/lumen_disable|$T/lumen_disable|" \
        -e "s|/var/lib/lumen/log|$T/lumen/log|" \
        -e "s|^ROOT_UID=0\$|ROOT_UID=$(id -u)|" \
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
        grep -q 'lumen-privacy: done' "$LOG" 2>/dev/null && return 0
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
    grep -q 'tcp 11111 rejected from the LAN' "$LOG"
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
    grep -q 'rule already in place' "$LOG"
}

@test "with the kill switch the hook does nothing at all" {
    touch "$T/lumen_disable"
    run_hook
    [ "$status" -eq 0 ]
    sleep 0.5
    [ ! -s "$CALLS" ]
    [ ! -e "$LOG" ]
}

@test "the hook keeps one run before in lumen-privacy.log.1, in a folder only root may use" {
    run_hook
    wait_done
    run_hook
    wait_done
    grep -q 'tcp 11111 rejected from the LAN' "$LOG.1"
    grep -q 'stopped crashreportd' "$LOG.1"
    grep -q 'tcp 11111 rule already in place' "$LOG"
    run grep -q 'stopped' "$LOG"
    [ "$status" -eq 1 ]
    [ "$(grep -c 'lumen-privacy: start' "$LOG")" -eq 1 ]
    [ "$(ls "$T/lumen/log" | tr '\n' ' ')" = "lumen-privacy.log lumen-privacy.log.1 " ]
    [ "$(stat -c %a "$T/lumen/log")" = 700 ]
}

# The hook with no safe log folder: it says why ($1), logs nothing, and still
# does all its work: the rule, the four jobs, and its sixty rounds.
works_unlogged() {
    : >"$CALLS"
    run_hook
    [ "$status" -eq 0 ]
    [[ "$output" == *"$1"* ]]
    [[ "$output" == *"no log: this run's output goes to /dev/null"* ]]
    for _ in $(seq 100); do
        [ "$(grep -c '^initctl status remotelogger' "$CALLS")" -ge 60 ] && break
        sleep 0.1
    done
    [ "$(grep -c '^initctl status remotelogger' "$CALLS")" -eq 60 ]
    [ "$(grep -c '^initctl stop' "$CALLS")" -eq 4 ]
    grep -q '^iptables -I INPUT -p tcp --dport 11111' "$CALLS"
}

@test "with a log folder that is a link the hook logs nothing, and still does its work" {
    mkdir "$T/elsewhere"
    ln -s "$T/elsewhere" "$T/lumen/log"
    works_unlogged "$T/lumen/log is not a real folder"
    [ -z "$(ls -A "$T/elsewhere")" ]
}

@test "with a log folder others may write the hook logs nothing, and still does its work" {
    mkdir "$T/lumen/log"
    chmod 777 "$T/lumen/log"
    works_unlogged "$T/lumen/log is writable by others"
    [ -z "$(ls -A "$T/lumen/log")" ]
}

@test "with /var/lib/lumen owned by someone else the hook makes no log folder, and still does its work" {
    sed -i 's/^ROOT_UID=.*/ROOT_UID=99999/' "$T/hook"
    works_unlogged "$T/lumen is owned by uid $(id -u), not root"
    [ ! -e "$T/lumen/log" ]
}

@test "with /var/lib/lumen a link the hook writes nothing through it, and still does its work" {
    rmdir "$T/lumen"
    mkdir "$T/real"
    ln -s "$T/real" "$T/lumen"
    works_unlogged "$T/lumen is not a real folder"
    [ -z "$(ls -A "$T/real")" ]
}

@test "privacy-hook.sh status reads the hook's logs from /var/lib/lumen/log" {
    run "$HOOKCTL" status
    [ "$status" -eq 0 ]
    calls=$(cat "$CALLS")
    [[ "$calls" == *"tail -n 20 /var/lib/lumen/log/lumen-privacy.log;"* ]]
    [[ "$calls" == *"tail -n 5 /var/lib/lumen/log/lumen-privacy.log.1;"* ]]
    [[ "$calls" == *"no log: the hook has not run"* ]]
    [[ "$calls" != *"/tmp"* ]]
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
