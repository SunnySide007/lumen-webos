#!/usr/bin/env bats
# Lumen Guard's boot hook (profiles/privacy/lumen-guard), guard-apply.sh and
# guard-undo.sh against a fake lumen-guard, iptables and sleep, and
# tools/guard.sh against a fake tools/tv.sh that keeps the TV's files in this
# test's folder. Nothing reaches a TV.

PROFILES="$BATS_TEST_DIRNAME/../../profiles/privacy"
UDP_RULE='-p udp --dport 53 ! -d 127.0.0.0/8 -m dscp ! --dscp 7 -j DNAT --to-destination 127.0.0.2:53'
TCP_RULE='-p tcp --dport 53 ! -d 127.0.0.0/8 -m dscp ! --dscp 7 -j DNAT --to-destination 127.0.0.2:53'

setup() {
    T="$BATS_TEST_TMPDIR"
    CALLS="$T/calls"
    REAL_SLEEP=$(command -v sleep)
    export T CALLS REAL_SLEEP
    : >"$CALLS"
    mkdir -p "$T/bin" "$T/tmp" "$T/guard/rules" "$T/connman"

    # The scripts with every TV path moved into this test's folder.
    for f in lumen-guard guard-apply.sh guard-undo.sh; do
        sed -e "s|/var/luna/preferences/lumen_disable|$T/lumen_disable|g" \
            -e "s|/tmp/lumen-guard|$T/tmp/lumen-guard|g" \
            -e "s|/var/lib/lumen/guard|$T/guard|g" \
            -e "s|/var/lib/connman|$T/connman|g" \
            "$PROFILES/$f" >"$T/$f"
    done
    cp "$T/lumen-guard" "$T/guard/boot-hook"
    : >"$T/guard/rules/allowlist.txt"

    # iptables keeps the nat rules, one spec per line, in $T/nat.
    cat >"$T/bin/iptables" <<'EOF'
#!/bin/sh
echo "iptables $*" >>"$CALLS"
touch "$T/nat"
case "$*" in
    "-t nat -A OUTPUT "*)
        [ -z "${FAKE_ADD_FAILS:-}" ] || exit 1
        echo "${*#-t nat -A OUTPUT }" >>"$T/nat" ;;
    "-t nat -D OUTPUT "*)
        spec=${*#-t nat -D OUTPUT }
        grep -qxF -- "$spec" "$T/nat" || exit 1
        awk -v s="$spec" '!done && $0 == s { done = 1; next } { print }' "$T/nat" >"$T/nat.new"
        mv "$T/nat.new" "$T/nat" ;;
    "-t nat -L"*)
        echo "Chain OUTPUT (policy ACCEPT)"
        sed 's/^-p \([a-z]*\).*--to-destination \(.*\)$/DNAT \1 -- 0.0.0.0\/0 !127.0.0.0\/8 \1 dpt:53 DSCP match !0x07 to:\2/' "$T/nat" ;;
esac
EOF
    # lumen-guard: runs until SIGTERM; --probe answers unless told not to,
    # and a probe through connman shows up in the lookup log as the real
    # Guard's would.
    cat >"$T/guard/lumen-guard" <<'EOF'
#!/bin/sh
echo "guard $*" >>"$CALLS"
if [ "$1" = --probe ]; then
    [ -z "${FAKE_PROBE_FAILS:-}${FAKE_GUARD_DIES:-}" ] || exit 1
    if [ "$2" = 127.0.0.1:53 ] && [ -z "${FAKE_CONNMAN_LOST:-}" ]; then
        echo "1793318400 A $4 allow" >>"$T/guard/lookups.log"
    fi
    exit 0
fi
[ -z "${FAKE_GUARD_DIES:-}" ] || exit 1
trap 'echo "guard stopped" >>"$CALLS"; exit 0' TERM
# `wait` returns as soon as SIGTERM comes, so the trap runs at once.
while :; do "$REAL_SLEEP" 0.2 & wait $!; done
EOF
    printf '#!/bin/sh\nexec "$REAL_SLEEP" 0.01\n' >"$T/bin/sleep"
    for tool in initctl mount umount; do
        printf '#!/bin/sh\necho "%s $*" >>"$CALLS"\n' "$tool" >"$T/bin/$tool"
    done
    chmod +x "$T/bin/"* "$T/guard/lumen-guard"
}

teardown() {
    p=$(cat "$T/tmp/lumen-guard.pid" 2>/dev/null) && kill "$p" 2>/dev/null
    true
}

# Fails if the command succeeds: a bare `!` never fails a bats test.
not() {
    if "$@"; then
        echo "should have failed: $*"
        return 1
    fi
}

# Two saved connman services; the newer one's first nameserver wins.
connman() {
    mkdir -p "$T/connman/wifi_old_managed_psk" "$T/connman/wifi_new_managed_psk"
    printf '[wifi_old]\nNameservers=192.0.2.9;\n' >"$T/connman/wifi_old_managed_psk/settings"
    touch -d '2026-01-01 00:00' "$T/connman/wifi_old_managed_psk/settings"
    printf '[wifi_new]\nName=home\nNameservers=192.0.2.1;192.0.2.2;\n' >"$T/connman/wifi_new_managed_psk/settings"
}

# FD 3 closed: what the hook leaves running must not hold bats' output open.
run_hook() {
    PATH="$T/bin:$PATH" run sh "$T/lumen-guard" 3>&-
}

# The hook works in the background; wait for a line in its log.
wait_for() {
    for _ in $(seq 200); do
        grep -q -- "$1" "$T/tmp/lumen-guard.log" 2>/dev/null && return 0
        sleep 0.05
    done
    echo "never logged: $1"
    cat "$T/tmp/lumen-guard.log"
    return 1
}

stop_guard() {
    kill "$(cat "$T/tmp/lumen-guard.pid")"
    wait_for 'Guard exited'
}

@test "the hook returns at once, starts Guard against connman's resolver and adds both rules once" {
    connman
    run_hook
    [ "$status" -eq 0 ]
    [ -z "$output" ]
    wait_for 'goes through Guard'
    grep -qxF "guard --listen 127.0.0.2:53 --upstream 192.0.2.1:53 --rules $T/guard/rules --log $T/guard --status $T/guard/status --dscp 7" "$CALLS"
    [ "$(cat "$T/nat")" = "$UDP_RULE
$TCP_RULE" ]
    [ "$(grep -c -- '-t nat -A' "$CALLS")" -eq 2 ]
    # Guard answered before the rules went in ...
    probe=$(grep -n -- 'guard --probe 127.0.0.2:53' "$CALLS" | head -n 1 | cut -d: -f1)
    add=$(grep -n -- '-t nat -A' "$CALLS" | head -n 1 | cut -d: -f1)
    [ "$probe" -lt "$add" ]
    # ... and a lookup through connman was checked after.
    grep -q -- 'guard --probe 127.0.0.1:53 --name lumen-check-[0-9]*-[0-9]*.invalid' "$CALLS"
}

@test "when Guard exits the hook removes the rules, so lookups go to the router again" {
    connman
    run_hook
    wait_for 'goes through Guard'
    stop_guard
    [ ! -s "$T/nat" ]
    [ ! -e "$T/tmp/lumen-guard.pid" ]
    grep -q 'lookups go to the router again' "$T/tmp/lumen-guard.log"
}

@test "without connman's nameserver the hook uses the owner's upstream file" {
    printf '  192.0.2.53  # the router\n' >"$T/guard/upstream"
    run_hook
    wait_for 'goes through Guard'
    grep -q -- '--upstream 192.0.2.53:53 ' "$CALLS"
}

@test "with no upstream the hook starts nothing and adds no rule" {
    run_hook
    wait_for 'fail open'
    grep -q 'no upstream resolver' "$T/tmp/lumen-guard.log"
    not grep -q '^guard\|^iptables' "$CALLS"
}

@test "a loopback upstream is refused: Guard would ask connman, which asks Guard" {
    echo 127.0.0.1 >"$T/guard/upstream"
    run_hook
    wait_for 'fail open'
    not grep -q '^guard --listen' "$CALLS"
}

@test "a Guard that never answers is stopped after its tries, with no rules" {
    connman
    FAKE_PROBE_FAILS=1 run_hook
    wait_for 'did not answer'
    grep -q 'fail open' "$T/tmp/lumen-guard.log"
    [ "$(grep -c -- '^guard --probe 127.0.0.2:53' "$CALLS")" -eq 40 ]
    not grep -q -- '-t nat -A' "$CALLS"
    grep -qx 'guard stopped' "$CALLS"
}

@test "a Guard that dies at start is noticed, with no rules" {
    connman
    FAKE_GUARD_DIES=1 run_hook
    wait_for 'Guard exited; Guard stopped, no rules (fail open)'
    not grep -q -- '-t nat -A' "$CALLS"
}

@test "when connman's lookups don't reach Guard the rules come out again" {
    connman
    FAKE_CONNMAN_LOST=1 run_hook
    wait_for 'did not reach Guard'
    grep -q 'fail open' "$T/tmp/lumen-guard.log"
    [ "$(grep -c -- '-t nat -A' "$CALLS")" -eq 2 ]
    [ ! -s "$T/nat" ]
    grep -qx 'guard stopped' "$CALLS"
}

@test "rules that fail to go in are taken out and Guard stopped" {
    connman
    FAKE_ADD_FAILS=1 run_hook
    wait_for 'FAILED to add the nat rules'
    [ ! -s "$T/nat" ]
    grep -qx 'guard stopped' "$CALLS"
}

@test "with Guard already running the hook does nothing" {
    connman
    "$T/guard/lumen-guard" --listen 127.0.0.2:53 3>&- &
    echo $! >"$T/tmp/lumen-guard.pid"
    run_hook
    wait_for 'already runs'
    [ "$(grep -c '^guard --listen' "$CALLS")" -eq 1 ]
    not grep -q '^iptables' "$CALLS"
}

@test "with the kill switch the hook does nothing at all" {
    connman
    touch "$T/lumen_disable"
    run_hook
    [ "$status" -eq 0 ]
    sleep 0.3
    [ ! -s "$CALLS" ]
    [ ! -e "$T/tmp/lumen-guard.log" ]
}

@test "the hook never calls initctl or mount, and writes only under /tmp and /var/lib/lumen" {
    connman
    run_hook
    wait_for 'goes through Guard'
    stop_guard
    not grep -q '^initctl\|^mount\|^umount' "$CALLS"

    hook="$PROFILES/lumen-guard"
    code=$(sed -e 's/^[[:space:]]*#.*//' -e 's/[[:space:]]#.*//' "$hook")
    not grep -Eq '\b(initctl|mount|umount|luna-send|reboot)\b' <<<"$code"
    grep -qx 'LOG=/tmp/lumen-guard.log' "$hook"
    grep -qx 'PIDFILE=/tmp/lumen-guard.pid' "$hook"
    grep -qx 'DIR=/var/lib/lumen/guard' "$hook"
    # Every redirection goes to the log, the pid file or /dev/null ...
    run grep -Eo '>>?[[:space:]]*[^[:space:]&;)|]+' <<<"$code"
    [ "${#lines[@]}" -gt 0 ]
    for target in "${lines[@]}"; do
        [[ "$target" =~ ^\>\>?[[:space:]]*(\"\$LOG\"|\"\$PIDFILE\"|/dev/null)$ ]] || {
            echo "writes to $target"
            return 1
        }
    done
    # ... the only file command removes the pid file, and Guard's own files
    # (its log and status) are under $DIR.
    run grep -Eo '\b(rm|mv|cp|touch|mkdir|ln|chmod|tee|dd|truncate)\b[^;|&]*' <<<"$code"
    for cmd in "${lines[@]}"; do
        [ "$cmd" = 'rm -f "$PIDFILE"' ] || {
            echo "unexpected: $cmd"
            return 1
        }
    done
    grep -qF -- '--log "$DIR" --status "$DIR/status"' "$hook"
}

@test "the hook and guard-undo.sh name the same rule" {
    hook=$(sed -n '/^rule() {/,/^}/p' "$PROFILES/lumen-guard")
    undo=$(sed -n '/^rule() {/,/^}/p' "$PROFILES/guard-undo.sh")
    [ -n "$hook" ]
    [ "$hook" = "$undo" ]
    for f in lumen-guard guard-undo.sh; do
        grep -qx 'LISTEN=127.0.0.2:53' "$PROFILES/$f"
        grep -qx 'DSCP=7' "$PROFILES/$f"
    done
}

@test "guard-apply.sh runs the pushed hook and reports Guard running" {
    connman
    PATH="$T/bin:$PATH" run sh "$T/guard-apply.sh" 3>&-
    [ "$status" -eq 0 ]
    [[ "$output" == *"goes through Guard"* ]]
    [[ "$output" == *"to:127.0.0.2:53"* ]]
    [[ "$output" == *"guard: running"* ]]

    # Run twice, it leaves the first Guard alone.
    PATH="$T/bin:$PATH" run sh "$T/guard-apply.sh" 3>&-
    [ "$status" -eq 0 ]
    [[ "$output" == *"already runs"* ]]
    [ "$(grep -c '^guard --listen' "$CALLS")" -eq 1 ]
}

@test "guard-apply.sh reports a start that failed open" {
    PATH="$T/bin:$PATH" run sh "$T/guard-apply.sh" 3>&-
    [ "$status" -eq 1 ]
    [[ "$output" == *"no upstream resolver"* ]]
    [[ "$output" == *"guard: not running, no nat rules"* ]]
}

@test "guard-apply.sh needs tools/guard.sh push first, and respects the kill switch" {
    rm "$T/guard/boot-hook"
    PATH="$T/bin:$PATH" run sh "$T/guard-apply.sh"
    [ "$status" -eq 1 ]
    [[ "$output" == *"run tools/guard.sh push first"* ]]
    touch "$T/lumen_disable"
    PATH="$T/bin:$PATH" run sh "$T/guard-apply.sh"
    [ "$status" -eq 1 ]
    [[ "$output" == *"lumen_disable exists"* ]]
    [ ! -s "$CALLS" ]
}

@test "guard-undo.sh removes every rule first, then stops Guard by its pid file" {
    printf '%s\n%s\n%s\n' "$UDP_RULE" "$UDP_RULE" "$TCP_RULE" >"$T/nat"
    "$T/guard/lumen-guard" --listen 127.0.0.2:53 3>&- &
    echo $! >"$T/tmp/lumen-guard.pid"
    PATH="$T/bin:$PATH" run sh "$T/guard-undo.sh"
    [ "$status" -eq 0 ]
    [[ "$output" == *"no nat rules to 127.0.0.2:53"* ]]
    [[ "$output" == *"Guard (pid "*") stopped"* ]]
    [ ! -s "$T/nat" ]
    [ ! -e "$T/tmp/lumen-guard.pid" ]
    last_delete=$(grep -n -- '-t nat -D' "$CALLS" | tail -n 1 | cut -d: -f1)
    stopped=$(grep -n 'guard stopped' "$CALLS" | cut -d: -f1)
    [ "$last_delete" -lt "$stopped" ]
}

@test "guard-undo.sh leaves a pid file's other process alone" {
    "$REAL_SLEEP" 5 3>&- &
    other=$!
    echo "$other" >"$T/tmp/lumen-guard.pid"
    PATH="$T/bin:$PATH" run sh "$T/guard-undo.sh"
    [ "$status" -eq 0 ]
    [[ "$output" == *"Guard not running"* ]]
    kill -0 "$other"
    kill "$other"
}

# tools/guard.sh against a fake tv.sh: `push` copies into $R, `run` and
# `script` run here with /tmp/ and /var/lib/ moved under $R, and the hook's
# own path is only recorded.
fake_tv() {
    R="$T/tv"
    mkdir -p "$R/tmp" "$R/var/lib/webosbrew/init.d"
    cat >"$T/tv.sh" <<'EOF'
#!/bin/sh
printf '%s|%s|%s\n' "$1" "$2" "${3:-}" >>"$CALLS"
on_tv() { sed -e "s|/tmp/|$R/tmp/|g" -e "s|/var/lib/|$R/var/lib/|g"; }
case $1 in
    push) mkdir -p "$(dirname "$R$3")" && cp "$2" "$R$3" ;;
    run)
        [ "$2" = /var/lib/webosbrew/init.d/lumen-guard ] && exit 0
        cmd=$(printf '%s\n' "$2" | on_tv)
        case $2 in
            *"${FAKE_BAD_SHA:-no such command}"*) sh -c "$cmd" | sed '1s/^[0-9a-f]*/0000/' ;;
            *) sh -c "$cmd" ;;
        esac ;;
    script) on_tv <"$2" >"$R/script.sh" && sh "$R/script.sh" ;;
esac
EOF
    chmod +x "$T/tv.sh"
    echo "not really an ARM binary" >"$T/lumen-guard.bin"
    export R LUMEN_TV_SH="$T/tv.sh" LUMEN_GUARD_BIN="$T/lumen-guard.bin" LUMEN_GUARD_OUT="$T/out"
    GUARDCTL="$BATS_TEST_DIRNAME/../guard.sh"
}

guardctl() {
    PATH="$T/bin:$PATH" run "$GUARDCTL" "$@"
}

@test "guard.sh: an unknown command fails with usage" {
    fake_tv
    guardctl frobnicate
    [ "$status" -eq 2 ]
    [[ "$output" == *"tools/guard.sh install"* ]]
    [ ! -s "$CALLS" ]
}

@test "guard.sh push puts the binary, the rules and the hook in place, checked" {
    fake_tv
    guardctl push
    [ "$status" -eq 0 ]
    [[ "$output" == *"SHA-256 checked"* ]]
    d="$R/var/lib/lumen/guard"
    cmp "$T/lumen-guard.bin" "$d/lumen-guard"
    [ -x "$d/lumen-guard" ]
    for f in allowlist.txt required-blocks.txt b9-aggressive.hosts; do
        cmp "$BATS_TEST_DIRNAME/../../profiles/glasshouse/$f" "$d/rules/$f"
    done
    cmp "$PROFILES/lumen-guard" "$d/boot-hook"
    [ ! -e "$R/tmp/lumen-guard.new" ]
    # Uploads go only to /tmp, and nothing goes into init.d.
    [ -z "$(grep '^push|' "$CALLS" | grep -v '|/tmp/lumen-guard.new/')" ]
    [ -z "$(ls "$R/var/lib/webosbrew/init.d")" ]
}

@test "guard.sh push stops when a file on the TV differs, or nothing is built" {
    fake_tv
    FAKE_BAD_SHA='mkdir -p' guardctl push
    [ "$status" -eq 1 ]
    [[ "$output" == *"are not this checkout's"* ]]
    LUMEN_GUARD_BIN="$T/nothing" guardctl push
    [ "$status" -eq 1 ]
    [[ "$output" == *"tools/dev.sh make guard"* ]]
}

@test "guard.sh install pushes, moves the hook into place whole, then runs it once" {
    fake_tv
    guardctl install
    [ "$status" -eq 0 ]
    [[ "$output" == *"installed /var/lib/webosbrew/init.d/lumen-guard"* ]]
    cmp "$PROFILES/lumen-guard" "$R/var/lib/webosbrew/init.d/lumen-guard"
    [ -x "$R/var/lib/webosbrew/init.d/lumen-guard" ]
    [ -f "$R/var/lib/lumen/guard/lumen-guard" ]
    [ "$(tail -n 1 "$CALLS")" = "run|/var/lib/webosbrew/init.d/lumen-guard|" ]
    grep -q '^run|sh -n /tmp/lumen-guard.new.hook && cp .*&& mv /var/lib/webosbrew/init.d/lumen-guard.tmp /var/lib/webosbrew/init.d/lumen-guard' "$CALLS"
}

@test "guard.sh install stops if the TV's copy of the hook differs, and doesn't run it" {
    fake_tv
    FAKE_BAD_SHA='init.d/lumen-guard.tmp' guardctl install
    [ "$status" -eq 1 ]
    [[ "$output" == *"is not this checkout's"* ]]
    not grep -qx 'run|/var/lib/webosbrew/init.d/lumen-guard|' "$CALLS"
}

@test "guard.sh remove deletes the hook, then removes the rules and stops Guard" {
    fake_tv
    touch "$R/var/lib/webosbrew/init.d/lumen-guard" "$R/var/lib/webosbrew/init.d/lumen-privacy"
    printf '%s\n%s\n' "$UDP_RULE" "$TCP_RULE" >"$T/nat"
    guardctl remove
    [ "$status" -eq 0 ]
    [ ! -e "$R/var/lib/webosbrew/init.d/lumen-guard" ]
    [ -e "$R/var/lib/webosbrew/init.d/lumen-privacy" ]
    [ ! -s "$T/nat" ]
    run cat "$CALLS"
    [[ "${lines[0]}" == "run|rm -f /var/lib/webosbrew/init.d/lumen-guard && echo"* ]]
    [[ "${lines[1]}" == "script|"*"/profiles/privacy/guard-undo.sh|" ]]
}

@test "guard.sh log fetches both files and prints the names by count" {
    fake_tv
    mkdir -p "$R/var/lib/lumen/guard"
    printf '1793318400 A www.netflix.com allow\n' >"$R/var/lib/lumen/guard/lookups.log.1"
    printf '1793318460 A www.netflix.com allow\n1793318461 A snu.lge.com would-block\n' \
        >"$R/var/lib/lumen/guard/lookups.log"
    guardctl log
    [ "$status" -eq 0 ]
    cmp "$R/var/lib/lumen/guard/lookups.log" "$T/out/lookups.log"
    cmp "$R/var/lib/lumen/guard/lookups.log.1" "$T/out/lookups.log.1"
    [[ "$output" == *"3 lookups of 2 names"* ]]
    [[ "$output" == *"| www.netflix.com | 2 | A |"* ]]
    [[ "$output" == *"| snu.lge.com | 1 | A |"*"| would-block | block |"* ]]
}

@test "guard.sh log with no log on the TV says so" {
    fake_tv
    guardctl log
    [ "$status" -eq 1 ]
    [[ "$output" == *"no lookup log on the TV"* ]]
}

@test "guard.sh status shows both hashes, Guard, the rules and the status file" {
    fake_tv
    cp "$PROFILES/lumen-guard" "$R/var/lib/webosbrew/init.d/lumen-guard"
    mkdir -p "$R/var/lib/lumen/guard"
    printf 'mode log\nlookups 12\n' >"$R/var/lib/lumen/guard/status"
    printf '%s\n' "$UDP_RULE" >"$T/nat"
    guardctl status
    [ "$status" -eq 0 ]
    sha=$(sha256sum <"$PROFILES/lumen-guard" | cut -d' ' -f1)
    [[ "$output" == *"this checkout: hook $sha"* ]]
    [[ "$output" == *"on the TV:    hook $sha"* ]]
    [[ "$output" == *"on the TV:    no binary"* ]]
    [[ "$output" == *"Guard: not running"* ]]
    [[ "$output" == *"DNAT udp"*"to:127.0.0.2:53"* ]]
    [[ "$output" == *"lookups 12"* ]]
}
