#!/usr/bin/env bats
# Lumen Guard's boot hook (profiles/privacy/lumen-guard), guard-apply.sh and
# guard-undo.sh against a fake lumen-guard, iptables, mount, umount and sleep,
# and tools/guard.sh against a fake tools/tv.sh that keeps the TV's files in
# this test's folder. Nothing reaches a TV.

PROFILES="$BATS_TEST_DIRNAME/../../profiles/privacy"
# The rules for an upstream of 192.0.2.1, as the hook adds them.
UDP_RULE='-p udp --dport 53 ! -o lo ! -d 192.0.2.1 -m dscp ! --dscp 7 -j DNAT --to-destination 127.0.0.2:53'
TCP_RULE='-p tcp --dport 53 ! -o lo ! -d 192.0.2.1 -m dscp ! --dscp 7 -j DNAT --to-destination 127.0.0.2:53'
CONNMAN='nameserver 127.0.0.1'
OURS='# Lumen Guard
nameserver 127.0.0.2'

setup() {
    T="$BATS_TEST_TMPDIR"
    CALLS="$T/calls"
    REAL_SLEEP=$(command -v sleep)
    export T CALLS REAL_SLEEP
    : >"$CALLS"
    mkdir -p "$T/bin" "$T/tmp" "$T/guard/rules" "$T/connman" "$T/misc" "$T/etc" "$T/mstack"

    # The scripts with every TV path moved into this test's folder.
    for f in lumen-guard guard-apply.sh guard-undo.sh; do
        sed -e "s|/var/luna/preferences/lumen_disable|$T/lumen_disable|g" \
            -e "s|/tmp/lumen-guard|$T/tmp/lumen-guard|g" \
            -e "s|/var/lib/lumen/guard|$T/guard|g" \
            -e "s|/var/lib/connman|$T/connman|g" \
            -e "s|/var/lib/misc/resolv.conf|$T/misc/resolv.conf|g" \
            -e "s|/etc/resolv.conf|$T/etc/resolv.conf|g" \
            -e "s|/proc/mounts|$T/mounts|g" \
            "$PROFILES/$f" >"$T/$f"
    done
    cp "$T/lumen-guard" "$T/guard/boot-hook"
    : >"$T/guard/rules/allowlist.txt"
    # connman's resolv.conf, and /etc/resolv.conf linked to it as on the TV.
    echo "$CONNMAN" >"$T/misc/resolv.conf"
    ln -s "$T/misc/resolv.conf" "$T/etc/resolv.conf"
    : >"$T/mounts"

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
    "-t nat -S OUTPUT")
        echo "-P OUTPUT ACCEPT"
        sed 's/^/-A OUTPUT /' "$T/nat" ;;
    "-t nat -L"*)
        echo "Chain OUTPUT (policy ACCEPT)"
        sed 's/^-p \([a-z]*\).*--to-destination \(.*\)$/DNAT \1 -- 0.0.0.0\/0 0.0.0.0\/0 \1 dpt:53 DSCP match !0x07 to:\2/' "$T/nat" ;;
esac
EOF
    # lumen-guard: runs until SIGTERM. --probe answers unless told not to;
    # a probe for a name takes its turn's line from $T/named, where `miss`
    # means no answer.
    cat >"$T/guard/lumen-guard" <<'EOF'
#!/bin/sh
echo "guard $*" >>"$CALLS"
if [ "$1" = --probe ]; then
    [ -z "${FAKE_PROBE_FAILS:-}${FAKE_GUARD_DIES:-}" ] || exit 1
    if [ "${3:-}" = --name ]; then
        n=$(grep -c -- '^guard --probe .* --name ' "$CALLS")
        [ "$(sed -n "${n}p" "$T/named" 2>/dev/null)" != miss ] || exit 1
    fi
    exit 0
fi
[ -z "${FAKE_GUARD_DIES:-}" ] || exit 1
trap 'echo "guard stopped" >>"$CALLS"; exit 0' TERM
# `wait` returns as soon as SIGTERM comes, so the trap runs at once.
while :; do "$REAL_SLEEP" 0.2 & wait $!; done
EOF
    # mount --bind: the source is hard-linked in place of the target, so the
    # two are one file as through a bind mount, and the target's file goes on
    # a stack for umount to put back. /proc/mounts gets a line unless told not.
    # With no arguments it lists the mounts.
    cat >"$T/bin/mount" <<'EOF'
#!/bin/sh
if [ $# -eq 0 ]; then
    cat "$T/mounts"
    exit 0
fi
echo "mount $*" >>"$CALLS"
[ "$1" = --bind ] && [ -z "${FAKE_MOUNT_FAILS:-}" ] || exit 1
n=$(ls "$T/mstack" | wc -l)
ln -f "$3" "$T/mstack/$n"
ln -f "$2" "$3"
[ -n "${FAKE_MOUNT_UNLISTED:-}" ] || echo "/dev/fake $3 ext4 rw 0 0" >>"$T/mounts"
EOF
    cat >"$T/bin/umount" <<'EOF'
#!/bin/sh
echo "umount $*" >>"$CALLS"
[ -z "${FAKE_UMOUNT_FAILS:-}" ] || exit 1
last=$(ls "$T/mstack" | sort -n | tail -n 1)
if [ -z "$last" ]; then
    echo "umount: $1: not mounted" >&2
    exit 1
fi
# A copy over a copy is one file with the one under it: mv won't, so rm first.
rm -f "$1"
mv "$T/mstack/$last" "$1"
awk -v m=" $1 " 'index($0, m) { k = NR } { l[NR] = $0 } END { for (i = 1; i <= NR; i++) if (i != k) print l[i] }' \
    "$T/mounts" >"$T/mounts.new"
mv "$T/mounts.new" "$T/mounts"
EOF
    printf '#!/bin/sh\nexec "$REAL_SLEEP" 0.01\n' >"$T/bin/sleep"
    printf '#!/bin/sh\necho "initctl $*" >>"$CALLS"\n' >"$T/bin/initctl"
    chmod +x "$T/bin/"* "$T/guard/lumen-guard"
}

teardown() {
    p=$(cat "$T/guard/lumen-guard.pid" 2>/dev/null) && kill "$p" 2>/dev/null
    # A hook still watching Guard fails open and removes the pid file.
    if grep -q 'Guard started' "$T/tmp/lumen-guard.log" 2>/dev/null; then
        for _ in $(seq 40); do
            [ -e "$T/guard/lumen-guard.pid" ] || break
            sleep 0.05
        done
    fi
    true
}

# Fails if the command succeeds: a bare `!` never fails a bats test.
not() {
    if "$@"; then
        echo "should have failed: $*"
        return 1
    fi
}

# The number of the first or last line in $CALLS that matches.
first() {
    grep -n -- "$1" "$CALLS" | head -n 1 | cut -d: -f1
}
last() {
    grep -n -- "$1" "$CALLS" | tail -n 1 | cut -d: -f1
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
    kill "$(cat "$T/guard/lumen-guard.pid")"
    wait_for 'Guard exited'
}

# connman's resolv.conf is back, and nothing is mounted over it.
connmans() {
    [ "$(cat "$T/etc/resolv.conf")" = "$CONNMAN" ]
    [ ! -s "$T/mounts" ]
    [ -z "$(ls "$T/mstack")" ]
}

@test "the hook returns at once; once Guard answers it mounts Guard's resolv.conf, checks a real name, then adds the rules" {
    connman
    run_hook
    [ "$status" -eq 0 ]
    [ -z "$output" ]
    wait_for 'goes through Guard'
    grep -qxF "guard --listen 127.0.0.2:53 --upstream 192.0.2.1:53 --rules $T/guard/rules --log $T/guard --status $T/guard/status --dscp 7" "$CALLS"
    [ "$(cat "$T/guard/lumen-guard.pid")" -gt 1 ]
    [ "$(cat "$T/guard/resolv.conf")" = "$OURS" ]
    grep -qxF "mount --bind $T/guard/resolv.conf $T/misc/resolv.conf" "$CALLS"
    [ "$(cat "$T/etc/resolv.conf")" = "$OURS" ]
    [ "$(cat "$T/nat")" = "$UDP_RULE
$TCP_RULE" ]
    [ "$(grep -c -- '-t nat -A' "$CALLS")" -eq 2 ]
    # In this order: Guard answers, the mount, a real name through Guard,
    # the rules.
    probe=$(first '^guard --probe 127.0.0.2:53$')
    mount=$(first '^mount ')
    named=$(first '^guard --probe 127.0.0.2:53 --name us.lgtvsdp.com$')
    add=$(first '-t nat -A')
    [ "$probe" -lt "$mount" ]
    [ "$mount" -lt "$named" ]
    [ "$named" -lt "$add" ]
}

@test "when Guard exits the hook unmounts first, then removes the rules" {
    connman
    run_hook
    wait_for 'goes through Guard'
    stop_guard
    connmans
    [ ! -s "$T/nat" ]
    [ ! -e "$T/guard/lumen-guard.pid" ]
    [ -n "$(first '^umount ')" ]
    [ "$(last '^umount ')" -lt "$(first '-t nat -D')" ]
    grep -q "Guard exited; resolv.conf is connman's, no nat rules, Guard stopped (fail open)" "$T/tmp/lumen-guard.log"
}

@test "two heartbeats in a row without an answer change nothing" {
    connman
    printf 'ok\nmiss\nmiss\nok\nmiss\nmiss\nok\n' >"$T/named"
    run_hook
    wait_for 'goes through Guard'
    # The start's check and seven heartbeats.
    for _ in $(seq 200); do
        [ "$(grep -c -- '--name us.lgtvsdp.com' "$CALLS")" -ge 8 ] && break
        sleep 0.05
    done
    [ "$(grep -c -- '--name us.lgtvsdp.com' "$CALLS")" -ge 8 ]
    [ "$(grep -c 'heartbeat: no answer for us.lgtvsdp.com' "$T/tmp/lumen-guard.log")" -eq 4 ]
    grep -q '(2 of 3)' "$T/tmp/lumen-guard.log"
    not grep -q '(3 of 3)\|fail open' "$T/tmp/lumen-guard.log"
    not grep -q '^umount\|-t nat -D' "$CALLS"
    [ "$(cat "$T/etc/resolv.conf")" = "$OURS" ]
    [ "$(cat "$T/nat")" = "$UDP_RULE
$TCP_RULE" ]
}

@test "three heartbeats in a row without an answer fail open: resolv.conf, then the rules, then Guard" {
    connman
    printf 'ok\nmiss\nok\nmiss\nmiss\nmiss\n' >"$T/named"
    run_hook
    wait_for 'three heartbeats in a row got no answer'
    [ "$(grep -c '(1 of 3)' "$T/tmp/lumen-guard.log")" -eq 2 ]
    grep -q '(3 of 3)' "$T/tmp/lumen-guard.log"
    grep -q "resolv.conf is connman's, no nat rules, Guard stopped (fail open)" "$T/tmp/lumen-guard.log"
    connmans
    [ ! -s "$T/nat" ]
    [ ! -e "$T/guard/lumen-guard.pid" ]
    [ "$(last '^umount ')" -lt "$(first '-t nat -D')" ]
    [ "$(last '-t nat -D')" -lt "$(first '^guard stopped$')" ]
}

@test "the kill switch appearing while Guard runs fails open" {
    connman
    run_hook
    wait_for 'goes through Guard'
    touch "$T/lumen_disable"
    wait_for 'the kill switch appeared'
    connmans
    [ ! -s "$T/nat" ]
    grep -qx 'guard stopped' "$CALLS"
}

@test "without connman's nameserver the hook uses the owner's upstream file, and the rules pass it" {
    printf '  192.0.2.53  # the router\n' >"$T/guard/upstream"
    run_hook
    wait_for 'goes through Guard'
    grep -q -- '--upstream 192.0.2.53:53 ' "$CALLS"
    grep -qxF -- "${UDP_RULE/192.0.2.1/192.0.2.53}" "$T/nat"
    grep -qxF -- "${TCP_RULE/192.0.2.1/192.0.2.53}" "$T/nat"
}

@test "with no upstream the hook starts nothing, mounts nothing and adds no rule" {
    run_hook
    wait_for 'fail open'
    grep -q 'no upstream resolver' "$T/tmp/lumen-guard.log"
    not grep -q '^guard\|^iptables\|^mount' "$CALLS"
}

@test "a loopback upstream is refused: Guard would ask connman, which asks Guard" {
    echo 127.0.0.1 >"$T/guard/upstream"
    run_hook
    wait_for 'fail open'
    not grep -q '^guard --listen' "$CALLS"
}

@test "a Guard that never answers is stopped after its tries; nothing mounted, no rules" {
    connman
    FAKE_PROBE_FAILS=1 run_hook
    wait_for 'did not answer'
    grep -q 'fail open' "$T/tmp/lumen-guard.log"
    [ "$(grep -c -- '^guard --probe 127.0.0.2:53$' "$CALLS")" -eq 40 ]
    not grep -q '^mount\|^umount\|-t nat -A' "$CALLS"
    grep -qx 'guard stopped' "$CALLS"
    connmans
    [ ! -e "$T/guard/lumen-guard.pid" ]
}

@test "a Guard that dies at start is noticed; nothing mounted, no rules" {
    connman
    FAKE_GUARD_DIES=1 run_hook
    wait_for "Guard exited; resolv.conf is connman's, no nat rules, Guard stopped (fail open)"
    not grep -q '^mount\|-t nat -A' "$CALLS"
}

@test "a mount that fails leaves connman's file, no rules and Guard stopped" {
    connman
    FAKE_MOUNT_FAILS=1 run_hook
    wait_for 'FAILED to mount'
    grep -q 'fail open' "$T/tmp/lumen-guard.log"
    connmans
    not grep -q -- '-t nat -A' "$CALLS"
    grep -qx 'guard stopped' "$CALLS"
}

@test "when /etc/resolv.conf doesn't show the mount it comes out again, with no rules" {
    connman
    rm "$T/etc/resolv.conf"
    echo "$CONNMAN" >"$T/etc/resolv.conf"
    run_hook
    wait_for 'does not name 127.0.0.2 after the mount'
    grep -q 'fail open' "$T/tmp/lumen-guard.log"
    [ "$(cat "$T/misc/resolv.conf")" = "$CONNMAN" ]
    [ ! -s "$T/mounts" ]
    not grep -q -- '-t nat -A' "$CALLS"
    grep -qx 'guard stopped' "$CALLS"
}

@test "with no answer for the real name through Guard the mount comes out, with no rules" {
    connman
    echo miss >"$T/named"
    run_hook
    wait_for 'no answer for us.lgtvsdp.com through Guard'
    connmans
    not grep -q -- '-t nat -A' "$CALLS"
    grep -qx 'guard stopped' "$CALLS"
}

@test "rules that fail to go in: the mount comes out, no rules, Guard stopped" {
    connman
    FAKE_ADD_FAILS=1 run_hook
    wait_for 'FAILED to add the nat rules'
    connmans
    [ ! -s "$T/nat" ]
    grep -qx 'guard stopped' "$CALLS"
}

@test "a mount that won't come out is logged, and the rules and Guard still go" {
    connman
    printf 'ok\nmiss\nmiss\nmiss\n' >"$T/named"
    FAKE_UMOUNT_FAILS=1 run_hook
    wait_for 'three heartbeats in a row got no answer'
    grep -q "no answer; FAILED to unmount $T/misc/resolv.conf, Guard stopped (fail open)" "$T/tmp/lumen-guard.log"
    [ "$(grep -c '^umount ' "$CALLS")" -eq 10 ]
    [ ! -s "$T/nat" ]
    grep -qx 'guard stopped' "$CALLS"
    [ ! -e "$T/guard/lumen-guard.pid" ]
}

@test "a mount left from an earlier run comes out before the hook mounts, so copies never stack" {
    connman
    printf '%s\n' "$OURS" >"$T/guard/resolv.conf"
    PATH="$T/bin:$PATH" mount --bind "$T/guard/resolv.conf" "$T/misc/resolv.conf"
    run_hook
    wait_for 'goes through Guard'
    [ "$(grep -c '^mount ' "$CALLS")" -eq 2 ]
    [ "$(first '^umount ')" -lt "$(last '^mount ')" ]
    [ "$(wc -l <"$T/mounts")" -eq 1 ]
    [ "$(ls "$T/mstack")" = 0 ]
}

@test "with Guard already running the hook does nothing" {
    connman
    "$T/guard/lumen-guard" --listen 127.0.0.2:53 3>&- &
    echo $! >"$T/guard/lumen-guard.pid"
    run_hook
    wait_for 'already runs'
    [ "$(grep -c '^guard --listen' "$CALLS")" -eq 1 ]
    not grep -q '^iptables\|^mount' "$CALLS"
}

@test "a pid file left from an earlier boot, its pid now another program's, doesn't stop the hook" {
    connman
    # A shell whose command line names the hook, as the hook's own does.
    sh -c 'sleep 5; :' "$T/guard/lumen-guard" 3>&- &
    other=$!
    echo "$other" >"$T/guard/lumen-guard.pid"
    run_hook
    wait_for 'goes through Guard'
    [ "$(cat "$T/guard/lumen-guard.pid")" != "$other" ]
    kill -0 "$other"
    kill "$other"
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

# The scripts without comments and with their messages blanked, so only the
# commands they run are left to check.
commands() {
    sed -e 's/^[[:space:]]*#.*//' -e 's/[[:space:]]#.*//' \
        -e 's/\(say\|fail_open\|state=\|echo\) *"[^"]*"/\1 ""/g' "$1"
}

@test "the hook writes only its log, its pid file and Guard's resolv.conf, and mounts only that over connman's" {
    connman
    run_hook
    wait_for 'goes through Guard'
    stop_guard
    not grep -q '^initctl' "$CALLS"

    hook="$PROFILES/lumen-guard"
    code=$(commands "$hook")
    not grep -Eq '\b(initctl|luna-send|reboot)\b' <<<"$code"
    grep -qx 'LOG=/tmp/lumen-guard.log' "$hook"
    grep -qx 'DIR=/var/lib/lumen/guard' "$hook"
    grep -qx 'PIDFILE=$DIR/lumen-guard.pid' "$hook"
    grep -qx 'OURS=$DIR/resolv.conf' "$hook"
    grep -qx 'RESOLV=/var/lib/misc/resolv.conf' "$hook"
    # Every redirection goes to the log, the pid file, Guard's resolv.conf
    # or /dev/null ...
    run grep -Eo '>>?[[:space:]]*[^[:space:]&;)|]+' <<<"$code"
    [ "${#lines[@]}" -gt 0 ]
    for target in "${lines[@]}"; do
        [[ "$target" =~ ^\>\>?[[:space:]]*(\"\$LOG\"|\"\$PIDFILE\"|\"\$OURS\"|/dev/null)$ ]] || {
            echo "writes to $target"
            return 1
        }
    done
    # ... the only file command removes the pid file ...
    run grep -Eo '\b(rm|mv|cp|touch|mkdir|ln|chmod|tee|dd|truncate)\b[^;|&]*' <<<"$code"
    for cmd in "${lines[@]}"; do
        [ "$cmd" = 'rm -f "$PIDFILE"' ] || {
            echo "unexpected: $cmd"
            return 1
        }
    done
    # ... and the only mount is Guard's file over connman's, and its undoing.
    run grep -Eo '\bu?mount\b[^;|&]*[^;|&[:space:]]' <<<"$code"
    [ "${#lines[@]}" -eq 2 ]
    [ "${lines[0]}" = 'umount "$RESOLV"' ]
    [ "${lines[1]}" = 'mount --bind "$OURS" "$RESOLV"' ]
    grep -qF -- '--log "$DIR" --status "$DIR/status"' "$hook"
}

@test "guard-undo.sh writes nothing but removes the pid file, adds nothing, and only unmounts" {
    undo="$PROFILES/guard-undo.sh"
    code=$(commands "$undo")
    run grep -Eo '>>?[[:space:]]*[^[:space:]&;)|]+' <<<"$code"
    for target in "${lines[@]}"; do
        [ "$target" = '>/dev/null' ] || {
            echo "writes to $target"
            return 1
        }
    done
    run grep -Eo '\b(rm|mv|cp|touch|mkdir|ln|chmod|tee|dd|truncate)\b[^;|&]*' <<<"$code"
    [ "${#lines[@]}" -eq 1 ]
    [ "${lines[0]}" = 'rm -f "$PIDFILE"' ]
    run grep -Eo '\bu?mount\b[^;|&]*[^;|&[:space:]]' <<<"$code"
    [ "${#lines[@]}" -eq 1 ]
    [ "${lines[0]}" = 'umount "$RESOLV"' ]
    not grep -Eq -- '-[AIP] OUTPUT|\b(initctl|luna-send|reboot)\b' <<<"$code"
}

@test "the hook and guard-undo.sh share their paths and their mount, rule and pid helpers" {
    for fn in remove_rules mounted unmount alive running; do
        hook=$(sed -n "/^$fn() {/,/^}/p" "$PROFILES/lumen-guard")
        undo=$(sed -n "/^$fn() {/,/^}/p" "$PROFILES/guard-undo.sh")
        [ -n "$hook" ] || {
            echo "no $fn in the hook"
            return 1
        }
        [ "$hook" = "$undo" ] || {
            echo "$fn differs"
            return 1
        }
    done
    for line in 'DIR=/var/lib/lumen/guard' 'PIDFILE=$DIR/lumen-guard.pid' \
        'OURS=$DIR/resolv.conf' 'RESOLV=/var/lib/misc/resolv.conf' 'LISTEN=127.0.0.2:53'; do
        grep -qxF "$line" "$PROFILES/lumen-guard"
        grep -qxF "$line" "$PROFILES/guard-undo.sh"
    done
}

@test "guard-apply.sh runs the pushed hook and reports Guard running, with resolv.conf and the mount" {
    connman
    PATH="$T/bin:$PATH" run sh "$T/guard-apply.sh" 3>&-
    [ "$status" -eq 0 ]
    [[ "$output" == *"goes through Guard"* ]]
    [[ "$output" == *"nameserver 127.0.0.2"* ]]
    [[ "$output" == *"/dev/fake $T/misc/resolv.conf ext4"* ]]
    [[ "$output" == *"to:127.0.0.2:53"* ]]
    [[ "$output" == *"guard: running"* ]]

    # Run twice, it leaves the first Guard alone.
    PATH="$T/bin:$PATH" run sh "$T/guard-apply.sh" 3>&-
    [ "$status" -eq 0 ]
    [[ "$output" == *"already runs"* ]]
    [ "$(grep -c '^guard --listen' "$CALLS")" -eq 1 ]
    [ "$(grep -c '^mount ' "$CALLS")" -eq 1 ]
}

@test "guard-apply.sh reports a start that failed open" {
    PATH="$T/bin:$PATH" run sh "$T/guard-apply.sh" 3>&-
    [ "$status" -eq 1 ]
    [[ "$output" == *"no upstream resolver"* ]]
    [[ "$output" == *"$CONNMAN"* ]]
    [[ "$output" == *"no mount over resolv.conf"* ]]
    [[ "$output" == *"guard: not running; resolv.conf is connman's, no nat rules"* ]]
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

@test "guard-undo.sh unmounts every copy, then removes Guard's rules, then stops Guard, and shows connman's file" {
    printf '%s\n' "$OURS" >"$T/guard/resolv.conf"
    PATH="$T/bin:$PATH" mount --bind "$T/guard/resolv.conf" "$T/misc/resolv.conf"
    # A second copy /proc/mounts doesn't list is still found: connman's path
    # shows Guard's file.
    FAKE_MOUNT_UNLISTED=1 PATH="$T/bin:$PATH" mount --bind "$T/guard/resolv.conf" "$T/misc/resolv.conf"
    printf '%s\n%s\n%s\n' "$UDP_RULE" "$UDP_RULE" "$TCP_RULE" >"$T/nat"
    other='-p udp --dport 53 -j DNAT --to-destination 192.0.2.99:53'
    echo "$other" >>"$T/nat"
    "$T/guard/lumen-guard" --listen 127.0.0.2:53 3>&- &
    echo $! >"$T/guard/lumen-guard.pid"
    PATH="$T/bin:$PATH" run sh "$T/guard-undo.sh"
    [ "$status" -eq 0 ]
    [[ "$output" == *"nothing mounted over $T/misc/resolv.conf"* ]]
    [[ "$output" == *"no nat rules to 127.0.0.2:53"* ]]
    [[ "$output" == *"Guard (pid "*") stopped"* ]]
    [[ "${lines[-1]}" == "$CONNMAN" ]]
    connmans
    [ "$(grep -c '^umount ' "$CALLS")" -eq 2 ]
    # Someone else's rule stays.
    [ "$(cat "$T/nat")" = "$other" ]
    [ ! -e "$T/guard/lumen-guard.pid" ]
    [ "$(last '^umount ')" -lt "$(first '-t nat -D')" ]
    [ "$(last '-t nat -D')" -lt "$(first '^guard stopped$')" ]
}

@test "guard-undo.sh with nothing to undo says so" {
    PATH="$T/bin:$PATH" run sh "$T/guard-undo.sh"
    [ "$status" -eq 0 ]
    [[ "$output" == *"nothing mounted over"* ]]
    [[ "$output" == *"no nat rules to 127.0.0.2:53"* ]]
    [[ "$output" == *"Guard not running"* ]]
    not grep -q '^umount\|-t nat -D' "$CALLS"
}

@test "guard-undo.sh leaves a pid file's other process alone" {
    sh -c 'sleep 5; :' "$T/guard/lumen-guard" 3>&- &
    other=$!
    echo "$other" >"$T/guard/lumen-guard.pid"
    PATH="$T/bin:$PATH" run sh "$T/guard-undo.sh"
    [ "$status" -eq 0 ]
    [[ "$output" == *"Guard not running"* ]]
    kill -0 "$other"
    kill "$other"
}

# tools/guard.sh against a fake tv.sh: `push` copies into $R, `run` and
# `script` run here with /tmp/, /var/lib/, /etc/resolv.conf and /proc/mounts
# moved under $R and $T, and the hook's own path is only recorded.
fake_tv() {
    R="$T/tv"
    mkdir -p "$R/tmp" "$R/var/lib/webosbrew/init.d" "$R/var/lib/misc" "$R/var/lib/lumen/guard" "$R/etc"
    echo "$CONNMAN" >"$R/var/lib/misc/resolv.conf"
    ln -s "$R/var/lib/misc/resolv.conf" "$R/etc/resolv.conf"
    cat >"$T/tv.sh" <<'EOF'
#!/bin/sh
printf '%s|%s|%s\n' "$1" "$2" "${3:-}" >>"$CALLS"
on_tv() {
    sed -e "s|/tmp/|$R/tmp/|g" -e "s|/var/lib/|$R/var/lib/|g" \
        -e "s|/etc/resolv.conf|$R/etc/resolv.conf|g" -e "s|/proc/mounts|$T/mounts|g"
}
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

@test "guard.sh remove deletes the hook, then unmounts, removes the rules and stops Guard" {
    fake_tv
    touch "$R/var/lib/webosbrew/init.d/lumen-guard" "$R/var/lib/webosbrew/init.d/lumen-privacy"
    printf '%s\n' "$OURS" >"$R/var/lib/lumen/guard/resolv.conf"
    PATH="$T/bin:$PATH" mount --bind "$R/var/lib/lumen/guard/resolv.conf" "$R/var/lib/misc/resolv.conf"
    printf '%s\n%s\n' "$UDP_RULE" "$TCP_RULE" >"$T/nat"
    guardctl remove
    [ "$status" -eq 0 ]
    [ ! -e "$R/var/lib/webosbrew/init.d/lumen-guard" ]
    [ -e "$R/var/lib/webosbrew/init.d/lumen-privacy" ]
    [ "$(cat "$R/etc/resolv.conf")" = "$CONNMAN" ]
    [ ! -s "$T/mounts" ]
    [ ! -s "$T/nat" ]
    run grep -v '^mount \|^umount \|^iptables ' "$CALLS"
    [[ "${lines[0]}" == "run|rm -f /var/lib/webosbrew/init.d/lumen-guard && echo"* ]]
    [[ "${lines[1]}" == "script|"*"/profiles/privacy/guard-undo.sh|" ]]
}

@test "guard.sh log fetches both files and prints the names by count" {
    fake_tv
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

@test "guard.sh status shows both hashes, Guard, resolv.conf and the mount, the rules and the status file" {
    fake_tv
    cp "$PROFILES/lumen-guard" "$R/var/lib/webosbrew/init.d/lumen-guard"
    printf 'mode log\nlookups 12\n' >"$R/var/lib/lumen/guard/status"
    printf '%s\n' "$OURS" >"$R/var/lib/lumen/guard/resolv.conf"
    PATH="$T/bin:$PATH" mount --bind "$R/var/lib/lumen/guard/resolv.conf" "$R/var/lib/misc/resolv.conf"
    printf '%s\n' "$UDP_RULE" >"$T/nat"
    guardctl status
    [ "$status" -eq 0 ]
    sha=$(sha256sum <"$PROFILES/lumen-guard" | cut -d' ' -f1)
    [[ "$output" == *"this checkout: hook $sha"* ]]
    [[ "$output" == *"on the TV:    hook $sha"* ]]
    [[ "$output" == *"on the TV:    no binary"* ]]
    [[ "$output" == *"Guard: not running"* ]]
    [[ "$output" == *"$R/etc/resolv.conf:nameserver 127.0.0.2"* ]]
    [[ "$output" == *"/dev/fake $R/var/lib/misc/resolv.conf ext4"* ]]
    [[ "$output" == *"DNAT udp"*"to:127.0.0.2:53"* ]]
    [[ "$output" == *"lookups 12"* ]]
}

@test "guard.sh status says when Guard runs, by the pid file under /var/lib/lumen/guard" {
    fake_tv
    "$T/guard/lumen-guard" --listen 127.0.0.2:53 3>&- &
    pid=$!
    echo "$pid" >"$R/var/lib/lumen/guard/lumen-guard.pid"
    guardctl status
    kill "$pid"
    [ "$status" -eq 0 ]
    [[ "$output" == *"Guard: running, pid $pid"* ]]
    [[ "$output" == *"no mount over resolv.conf"* ]]
}
