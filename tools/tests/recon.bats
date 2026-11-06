#!/usr/bin/env bats
# Checks tools/recon.sh's options, and that the privacy, remote and platform
# surveys only read. The guards read the surveys as text. The rest run the
# remote and platform surveys, and recon.sh --remote, against fake luna-send,
# ls-monitor, dropbear and initctl programs and fixture files, with a stand-in
# for tools/tv.sh in a temporary copy of the tools. None of these reach a TV.

setup() {
    TMP="$BATS_TEST_TMPDIR"
    TOOLS="$BATS_TEST_DIRNAME/.."
    RECON="$TOOLS/recon.sh"
    REMOTE="$TOOLS/recon/remote.sh"
    PLATFORM="$TOOLS/recon/platform.sh"
    SURVEYS="$TOOLS/recon/privacy.sh $REMOTE $PLATFORM"
    FIXTURES="$BATS_TEST_DIRNAME/fixtures/remote"
    PFIX="$BATS_TEST_DIRNAME/fixtures/platform"
    CALLS="$TMP/calls"
    : >"$CALLS"
    export CALLS
}

teardown() {
    for pid in $(cat "$TMP/pids" 2>/dev/null); do kill "$pid" 2>/dev/null || true; done
}

# A survey without its comment lines.
code() {
    grep -v -E '^[[:space:]]*#' "$1"
}

# Fails, printing the matching lines, when grep finds a match. A bare `! grep`
# does not fail a bats test: errexit ignores a negated command.
refute_grep() {
    if grep "$@"; then
        return 1
    fi
}

# Fails, naming the survey and the lines, when its code has a line matching $2.
assert_no_code() {
    local hits
    hits=$(code "$1" | grep -n -E -- "$2" || true)
    if [ -n "$hits" ]; then
        echo "$1:"
        echo "$hits"
        return 1
    fi
}

# Fake luna-send, ls-monitor and dropbear on PATH: each writes its arguments to
# $CALLS, and the first two refuse anything the survey is not meant to ask.
make_fakes() {
    mkdir -p "$TMP/bin"
    cat >"$TMP/bin/luna-send" <<'EOF'
#!/bin/sh
echo "luna-send $*" >>"$CALLS"
case $4 in
    */getSystemInfo) echo '{ "modelName": "OLED55B9PUA", "firmwareVersion": "05.50.00", "returnValue": true, "sdkVersion": "4.10.0" }' ;;
    */getstatus) echo '{ "wifi": { "ssid": "HomeNet-5G", "ipAddress": "192.168.1.50", "gateway": "192.168.1.1", "bssid": "aa:bb:cc:dd:ee:01", "displayName": "HomeNet-5G", "state": "connected" } }' ;;
    *) echo '{ "returnValue": true }' ;;
esac
EOF
    cat >"$TMP/bin/ls-monitor" <<'EOF'
#!/bin/sh
echo "ls-monitor $*" >>"$CALLS"
case $1 in
    -l)
        echo '  PID  SERVICE                          EXE                          TYPE'
        echo '  612  com.webos.service.networkinput   /usr/sbin/networkinputd       dynamic'
        echo '  700  com.webos.service.tvpower        /usr/sbin/tvpower             static'
        echo '  701  org.example.rcu.bridge           /media/x/rcu.so               dynamic'
        echo '  702  com.webos.service.keymanager     /usr/sbin/keymanager          static'
        ;;
    -i) echo "methods of $2: /category method" ;;
    *) echo 'FORBIDDEN ls-monitor call'; exit 99 ;;
esac
EOF
    # The option words are in the file because the survey searches the binary
    # for them, as strings.
    cat >"$TMP/bin/dropbear" <<'EOF'
#!/bin/sh
# command=" restrict no-pty no-port-forwarding no-agent-forwarding no-X11-forwarding
echo "dropbear $*" >>"$CALLS"
[ "$*" = -V ] || { echo 'FORBIDDEN dropbear call'; exit 98; }
echo 'Dropbear v2020.81' >&2
EOF
    chmod +x "$TMP/bin/luna-send" "$TMP/bin/ls-monitor" "$TMP/bin/dropbear"
}

# Processes the survey looks for: a bootd, a Home and a dropbear, each a copy of
# sleep under that name so that /proc shows it.
start_fake_processes() {
    mkdir -p "$TMP/proc"
    : >"$TMP/pids"
    for name in bootd com.webos.app.home dropbear; do
        cp "$(command -v sleep)" "$TMP/proc/$name"
        "$TMP/proc/$name" 60 >/dev/null 2>&1 3>&- &
        echo $! >>"$TMP/pids"
    done
}

# The remote survey against the fakes and the fixture files.
run_remote() {
    make_fakes
    start_fake_processes
    PATH="$TMP/bin:$PATH" \
        REMOTE_SURVEY_KEYFILES="$FIXTURES/authorized_keys $TMP/no-such-keys" \
        REMOTE_SURVEY_INPUT="$FIXTURES/input-devices" \
        run sh "$REMOTE"
}

# The remote fakes, with a luna-send that answers the clock with a place in
# it, and an initctl that refuses anything but list.
make_platform_fakes() {
    make_fakes
    cat >"$TMP/bin/luna-send" <<'EOF'
#!/bin/sh
echo "luna-send $*" >>"$CALLS"
case $4 in
    */getSystemInfo) echo '{ "modelName": "OLED55B9PUA", "firmwareVersion": "05.50.00", "returnValue": true }' ;;
    */getConfigs) printf '{\n    "configs": {\n        "com.webos.applicationManager.keepAliveApps": [ "com.webos.app.home" ]\n    },\n    "returnValue": true\n}\n' ;;
    */clock/getTime) printf '{\n    "source": "sdp",\n    "utc": 1793491200,\n    "timeZone": "America/Chicago",\n    "returnValue": true\n}\n' ;;
    */time/getSystemTime) printf '{\n    "utc": 1793491200,\n    "systemTimeSource": "sdp",\n    "timezone": "America/Chicago",\n    "TZ": "CST",\n    "City": "Chicago",\n    "Country": "United States",\n    "offset": -360,\n    "returnValue": true\n}\n' ;;
    *) echo '{ "returnValue": true }' ;;
esac
EOF
    cat >"$TMP/bin/initctl" <<'EOF'
#!/bin/sh
echo "initctl $*" >>"$CALLS"
[ "$*" = list ] || { echo 'FORBIDDEN initctl call'; exit 97; }
echo 'screensaver stop/waiting'
echo 'memorymanager start/running, process 410'
echo 'acr2 start/running, process 911'
EOF
    # A service whose introspection never answers, in a client that ignores
    # TERM.
    cat >"$TMP/bin/ls-monitor" <<'EOF'
#!/bin/sh
echo "ls-monitor $*" >>"$CALLS"
case $1 in
    -l) echo '  700  com.webos.service.tvpower        /usr/sbin/tvpower             static' ;;
    -i)
        if [ "$2" = com.webos.service.acr ]; then
            trap '' TERM
            exec sleep 60
        fi
        echo "methods of $2: /category method"
        ;;
    *) echo 'FORBIDDEN ls-monitor call'; exit 99 ;;
esac
EOF
    chmod +x "$TMP/bin/luna-send" "$TMP/bin/initctl" "$TMP/bin/ls-monitor"
}

# The parts of the TV that are not fixture files: a jail, a hook log folder,
# and a library and programs made of strings between bytes that are not
# text, as in a binary.
make_platform_tree() {
    mkdir -p "$TMP/tv/jail/var/run" "$TMP/tv/jail/tmp" "$TMP/tv/log" "$TMP/tv/lib" "$TMP/tv/bin"
    : >"$TMP/tv/log/lumen-home.txt"
    printf '\177ELF\0\1\2SDL_WEBOS_ACCESS_POLICY_KEYS_BACK\0SDL_VideoInit\0\3luna://com.webos.applicationManager/registerNativeApp\0' \
        >"$TMP/tv/lib/libSDL2-2.0.so.0"
    ln -s libSDL2-2.0.so.0 "$TMP/tv/lib/libSDL2.so"
    printf '\177ELF\0acrOnAllowed\0\2livePlus\0hello\0' >"$TMP/tv/bin/acr2"
    printf '\177ELF\0collectAllLog\0' >"$TMP/tv/bin/pmlogd"
    printf '\177ELF\0https://156.147.69.32:443/check\0' >"$TMP/tv/bin/update"
    printf '\177ELF\0' >"$TMP/tv/bin/iconnectivity"
}

# A shell for the survey to find: a copy of sleep named lumen-shell, with the
# names it may read and two it must not.
start_fake_shell() {
    mkdir -p "$TMP/proc"
    cp "$(command -v sleep)" "$TMP/proc/lumen-shell"
    env -i APPID=io.github.sunnyside007.lumen.shell WAYLAND_DISPLAY=wayland-0 \
        XDG_RUNTIME_DIR=/tmp/xdg PLEX_TOKEN=not-for-the-report-1 HOME=/not-for-the-report-2 \
        "$TMP/proc/lumen-shell" 60 >/dev/null 2>&1 3>&- &
    echo $! >>"$TMP/pids"
}

# The platform survey against the fakes, the fixture files and the tree.
run_platform() {
    make_platform_fakes
    make_platform_tree
    PATH="$TMP/bin:$PATH" \
        PLATFORM_SURVEY_APP_DIR="$PFIX/app" \
        PLATFORM_SURVEY_JAIL="$TMP/tv/jail" \
        PLATFORM_SURVEY_LUMEN_LOGS="$TMP/tv/log" \
        PLATFORM_SURVEY_GUARD_LOG="$PFIX/guard/lookups" \
        PLATFORM_SURVEY_LIB="$TMP/tv/lib" \
        PLATFORM_SURVEY_BINDIRS="$TMP/tv/bin" \
        PLATFORM_SURVEY_LS2="$PFIX/ls2" \
        PLATFORM_SURVEY_LS2_LEGACY="$PFIX/ls2-legacy" \
        PLATFORM_SURVEY_APPS="$PFIX/apps" \
        run sh "$PLATFORM"
}

# A copy of the tools with a stand-in tv.sh: it records the script it was asked
# to run, and runs it here (against the fakes) when FAKE_TV_RUNS is set.
make_fake_repo() {
    REPO="$TMP/repo"
    mkdir -p "$REPO/tools/recon"
    cp "$RECON" "$REPO/tools/recon.sh"
    cp "$TOOLS"/recon/*.sh "$TOOLS"/recon/redact.py "$REPO/tools/recon/"
    cat >"$REPO/tools/tv.sh" <<'EOF'
#!/bin/sh
[ "$1" = script ] || exit 64
echo "tv.sh script ${2##*/}" >>"$CALLS"
if [ -n "${FAKE_TV_RUNS:-}" ]; then exec sh "$2"; fi
echo "stand-in output of ${2##*/}"
EOF
    chmod +x "$REPO/tools/tv.sh" "$REPO/tools/recon.sh"
}

# --- recon.sh's options -----------------------------------------------------

@test "an unknown option fails with usage before anything connects" {
    run "$RECON" --frobnicate
    [ "$status" -eq 2 ]
    [[ "$output" == *"usage: tools/recon.sh [--privacy|--remote|--platform]"* ]]
}

@test "recon.sh picks the survey and the report name from its option" {
    make_fake_repo
    for pair in "|device.sh|recon" "--privacy|privacy.sh|privacy" "--remote|remote.sh|remote" \
        "--platform|platform.sh|platform"; do
        option=${pair%%|*}
        rest=${pair#*|}
        script=${rest%%|*}
        name=${rest#*|}
        : >"$CALLS"
        run "$REPO/tools/recon.sh" $option
        [ "$status" -eq 0 ]
        [ "$(cat "$CALLS")" = "tv.sh script $script" ]
        [ "$(ls "$REPO"/tools/out/"$name"-*.raw.txt | wc -l)" -eq 1 ]
        [ "$(grep -c "stand-in output of $script" "$REPO"/docs/device/"$name"-*.txt)" -eq 1 ]
        rm -rf "$REPO/tools/out" "$REPO/docs"
    done
}

@test "recon.sh --remote saves the redacted survey as docs/device/remote-<stamp>.txt" {
    make_fake_repo
    make_fakes
    start_fake_processes
    FAKE_TV_RUNS=1 PATH="$TMP/bin:$PATH" \
        REMOTE_SURVEY_KEYFILES="$FIXTURES/authorized_keys" \
        REMOTE_SURVEY_INPUT="$FIXTURES/input-devices" \
        run "$REPO/tools/recon.sh" --remote
    [ "$status" -eq 0 ]
    raw=$(ls "$REPO"/tools/out/remote-*.raw.txt)
    out=$(ls "$REPO"/docs/device/remote-*.txt)
    [ "$(tail -n 1 "$out")" = "remote survey complete" ]
    # What the survey is for stays, and what identifies the network goes.
    grep -q 'type: ssh-ed25519' "$out"
    grep -q '"firmwareVersion": "05.50.00"' "$out"
    grep -q '"state": "connected"' "$out"
    for secret in HomeNet-5G 192.168.1.50 192.168.1.1 aa:bb:cc:dd:ee:01; do
        grep -q -F "$secret" "$raw"
        refute_grep -F "$secret" "$out"
    done
}

# --- guards on both surveys -------------------------------------------------

@test "no survey runs a command that changes the TV" {
    for s in $SURVEYS; do
        assert_no_code "$s" \
            '(^|[;&|(`[:space:]])(rm|mv|cp|ln|mount|umount|touch|mkdir|rmdir|tee|dd|chmod|chown|kill|killall|pkill|reboot|sysctl|insmod|modprobe|rmmod|ifconfig|passwd|install|truncate|initctl (start|stop|restart|emit))([[:space:]]|$)|sed( -[a-zA-Z]+)* -[a-zA-Z]*i|--in-place'
    done
}

@test "no survey changes a route, an address or a firewall rule" {
    for s in $SURVEYS; do
        assert_no_code "$s" \
            'ip6?tables .*-[AIDRNXPEFZ]|ip( -[46])? (route|rule|addr|link|neigh) (add|del|change|replace|flush|set)'
    done
}

@test "no survey writes a file" {
    # Every redirection goes to /dev/null or to another descriptor.
    for s in $SURVEYS; do
        run sh -c "grep -v -E '^[[:space:]]*#' '$s' \
            | sed -e 's/[0-9]*>&[0-9]//g' -e 's|[0-9]*>/dev/null||g' | grep -n '>'"
        [ "$status" -eq 1 ] || { echo "$s: $output"; false; }
    done
}

@test "no survey calls luna-send except through its get/list guard" {
    for s in $SURVEYS; do
        [ "$(code "$s" | grep -c 'luna-send')" -eq 1 ] || { echo "$s: not exactly one luna-send"; false; }
        # Every call names a get or list method outright...
        assert_no_code "$s" '^[[:space:]]*luna[[:space:]]+[^A-Za-z]'
        run sh -c "grep -v -E '^[[:space:]]*#' '$s' \
            | grep -o -E '^[[:space:]]*luna [A-Za-z0-9./]+' | grep -v -E '/(get|list)[A-Za-z]*\$'"
        [ "$status" -eq 1 ] || { echo "$s: $output"; false; }
    done
}

@test "the luna guard refuses every method but get and list" {
    for s in $SURVEYS; do
        { echo 'T=""'; sed -n '/^luna() {/,/^}/p' "$s"; } >"$TMP/luna-guard.sh"
        make_fakes
        : >"$CALLS"
        run env PATH="$TMP/bin:$PATH" sh -c ". '$TMP/luna-guard.sh'
            luna com.webos.service.tvpower/power/turnOffScreen '{}'
            luna com.webos.audio/setMuted '{\"muted\":true}'
            luna com.webos.applicationManager/launch '{}'
            luna getaway '{}'
            luna com.webos.audio/getStatus '{}'
            luna com.webos.applicationManager/listApps '{}'"
        [ "$(grep -c 'refused' <<<"$output")" -eq 4 ] || { echo "$s: $output"; false; }
        [ "$(wc -l <"$CALLS")" -eq 2 ] || { echo "$s: $(cat "$CALLS")"; false; }
        refute_grep -E 'turnOffScreen|setMuted|launch ' "$CALLS"
    done
}

# --- guards on the remote survey --------------------------------------------

@test "the remote and platform surveys call ls-monitor only through their -l and -i guard" {
    # No option would start monitoring, which changes how the hub routes
    # messages; -s and -m call other services' private methods.
    for s in $REMOTE $PLATFORM; do
        run sh -c "grep -v -E '^[[:space:]]*#' '$s' | grep 'ls-monitor' \
            | grep -v -E '^[[:space:]]*(printf|echo)|command -v'"
        [ "$status" -eq 0 ]
        [ "${#lines[@]}" -eq 1 ] || { echo "$s: $output"; false; }
        [[ "$output" == *'$T ls-monitor "$@" </dev/null'* ]]
        # Every call, wherever it is in a line, is lsmon -l or lsmon -i SERVICE.
        options=$(code "$s" | grep -o -E 'lsmon +-[A-Za-z-]+' | sort -u)
        [ "$options" = "$(printf 'lsmon -i\nlsmon -l')" ] || { echo "$s: $options"; false; }
        assert_no_code "$s" 'lsmon +[^- ]|lsmon *$'
        assert_no_code "$s" 'lsmon -l +[^ )]'
    done
}

@test "the ls-monitor guard allows -l and -i SERVICE and nothing else" {
    make_fakes
    for s in $REMOTE $PLATFORM; do
        { echo 'T=""'; sed -n '/^lsmon() {/,/^}/p' "$s"; } >"$TMP/lsmon-guard.sh"
        : >"$CALLS"
        run env PATH="$TMP/bin:$PATH" sh -c ". '$TMP/lsmon-guard.sh'
            lsmon
            lsmon -m
            lsmon -s
            lsmon -v com.webos.audio
            lsmon -i
            lsmon -i com.webos.audio extra
            lsmon -i 'a;reboot'
            lsmon -i '\$(reboot)'
            lsmon -l -m
            lsmon --list"
        [ "$(grep -c 'refused' <<<"$output")" -eq 10 ] || { echo "$s: $output"; false; }
        [ ! -s "$CALLS" ]
        run env PATH="$TMP/bin:$PATH" sh -c ". '$TMP/lsmon-guard.sh'
            lsmon -l
            lsmon -i com.webos.service.tv.systemproperty"
        [ "$status" -eq 0 ]
        [ "$(cat "$CALLS")" = "$(printf 'ls-monitor -l\nls-monitor -i com.webos.service.tv.systemproperty')" ]
    done
}

@test "the remote survey runs dropbear only with -V" {
    assert_no_code "$REMOTE" '"\$bin" +-[A-UW-Za-z]'
    [ "$(code "$REMOTE" | grep -c -E '(\$T +|^[[:space:]]*)"\$bin" ')" -eq 1 ]
    run_remote
    [ "$status" -eq 0 ]
    [ -n "$(grep '^dropbear ' "$CALLS")" ]
    [ -z "$(grep '^dropbear ' "$CALLS" | grep -v -x 'dropbear -V')" ]
}

@test "the remote and platform surveys give tr -c a range, never a class: busybox's tr -c gets a class backwards" {
    # With tr -c '[:print:]' busybox swaps the printable bytes for newlines and
    # keeps the rest, so the search of a binary finds nothing. GNU tr does it
    # right, so only this text check can catch it here.
    for s in $REMOTE $PLATFORM; do
        assert_no_code "$s" "tr +-[a-z]*c[a-z]* +[\"']?\\[:"
        code "$s" | grep -q "tr -c ' -~'"
    done
}

@test "the remote survey reads no private key, host key, password hash or process environment" {
    assert_no_code "$REMOTE" 'shadow|host_key|id_(rsa|ed25519|ecdsa|dsa)|\.pem|environ($|[^m])|known_hosts'
}

@test "the remote survey never shows a line of a key file but its options and type" {
    # The loop over the key files touches each file only through ls, a count
    # and key_options, and key_options has one print, of the two words it keeps.
    loop=$(code "$REMOTE" | awk '/^for f in \$KEYFILES/,/^done$/')
    [ -n "$loop" ]
    refute_grep -E '(^|[^a-z_])(cat|showfile|head|tail|sed|cut|awk|od|xxd|strings|tr)([^a-z_]|$)' <<<"$loop"
    func=$(code "$REMOTE" | awk '/^key_options\(\) \{/,/^\}/')
    [ "$(grep -c 'print' <<<"$func")" -eq 1 ]
    grep -q 'printf "  key %d: options: %s; type: %s\\n", n, shown, typ' <<<"$func"
}

# --- the remote survey against fakes ----------------------------------------

@test "the remote survey runs to its last line and has its five sections" {
    run_remote
    [ "$status" -eq 0 ]
    [ "${lines[${#lines[@]}-1]}" = "remote survey complete" ]
    for section in identity ssh luna-methods keys firmware-date home end; do
        [[ "$output" == *"===== $section ====="* ]]
    done
    [[ "$output" != *FORBIDDEN* ]]
}

@test "the remote survey prints no key, only each key line's options and type" {
    run_remote
    [ "$status" -eq 0 ]
    [[ "$output" == *"7 key line(s)"* ]]
    [[ "$output" == *"  key 1: options: restrict,command=…; type: ssh-ed25519"* ]]
    [[ "$output" == *"  key 2: options: no-pty,no-port-forwarding,from=…; type: ssh-rsa"* ]]
    [[ "$output" == *"  key 3: options: none; type: ssh-ed25519"* ]]
    [[ "$output" == *"  key 4: options: command=…,no-agent-forwarding,no-X11-forwarding; type: ecdsa-sha2-nistp256"* ]]
    [[ "$output" == *"  key 5: options: restrict; type: sk-ssh-ed25519@openssh.com"* ]]
    # A blob with no type, and a line that is not a key, show as nothing.
    [[ "$output" == *"  key 6: options: ?; type: (unrecognised)"* ]]
    [[ "$output" == *"  key 7: options: ?; type: (unrecognised)"* ]]
    # No base64 key body, no key type followed by one, none of the comments,
    # and no value of an option.
    refute_grep -E 'AAAA[A-Za-z0-9+/]{16,}' <<<"$output"
    refute_grep -E 'ssh-[a-z0-9]+ +[A-Za-z0-9+/]{20,}' <<<"$output"
    refute_grep -E 'lumen-remote@pc|owner@laptop|FAKE|lumen-ctl|192\.168\.1\.0|two words|not a key' <<<"$output"
}

@test "the remote survey answers question 1: dropbear's version and the options in its binary" {
    run_remote
    [[ "$output" == *"Dropbear v2020.81"* ]]
    for opt in 'command=' restrict no-pty no-port-forwarding no-X11-forwarding; do
        [[ "$output" == *"$opt: found"* ]]
    done
    [[ "$output" == *"no-agent-forwarding: found"* ]]
    # The dropbear in /proc is found, and a program is run only when it is
    # named dropbear: the copy of sleep is, so it is run with -V as well.
    [[ "$output" == *"$TMP/proc/dropbear"* ]]
}

@test "the remote survey asks only for what the brief lists, through the guards" {
    run_remote
    [ "$status" -eq 0 ]
    # luna-send: always -n 1 -f, always a get or list method.
    [ "$(grep -c '^luna-send' "$CALLS")" -ge 14 ]
    [ -z "$(grep '^luna-send' "$CALLS" | grep -v -E '^luna-send -n 1 -f luna://[A-Za-z0-9._/-]+/(get|list)[A-Za-z]* ')" ]
    for method in tv.systemproperty/getSystemInfo tvpower/power/getPowerState audio/getVolume \
        applicationManager/getForegroundAppInfo eim/getAllInputStatus connectionmanager/getstatus; do
        grep -q "luna://.*$method " "$CALLS"
    done
    for category in picture sound option network general; do
        grep -q "getSystemSettings {\"category\":\"$category\"" "$CALLS"
    done
    grep -q 'homeAutoLaunch' "$CALLS"
    # ls-monitor: -l once, then -i for each service.
    [ "$(grep -c '^ls-monitor -l$' "$CALLS")" -eq 1 ]
    [ -z "$(grep '^ls-monitor' "$CALLS" | grep -v -E '^ls-monitor (-l|-i [A-Za-z0-9._-]+)$')" ]
    for svc in com.webos.service.tvpower com.webos.audio com.webos.applicationManager \
        com.webos.settingsservice com.webos.service.ime com.webos.service.eim \
        com.webos.service.tv.systemproperty com.webos.notification \
        com.webos.service.config com.webos.service.connectionmanager; do
        grep -q -x "ls-monitor -i $svc" "$CALLS"
    done
    # And the services whose names say input, rcu, key or remote.
    for svc in com.webos.service.networkinput org.example.rcu.bridge com.webos.service.keymanager; do
        grep -q -x "ls-monitor -i $svc" "$CALLS"
    done
}

@test "the remote survey withholds the input devices' Phys and Uniq" {
    run_remote
    [[ "$output" == *'N: Name="Controller"'* ]]
    [[ "$output" == *"P: Phys=(withheld)"* ]]
    [[ "$output" == *"U: Uniq=(withheld)"* ]]
    [[ "$output" == *"P: Phys="$'\n'* ]]
    refute_grep -i -E 'a4:?b1:?c2:?d3:?e4:?f5' <<<"$output"
}

@test "the remote survey says what it found of the running processes" {
    run_remote
    [[ "$output" == *"pid "*": uid "*" ("*"), gid "*", groups "*", capabilities "* ]]
    [[ "$output" == *"binary $TMP/proc/bootd, root /, mnt namespace "* ]]
    [[ "$output" == *"args: $TMP/proc/com.webos.app.home 60"* ]]
}

@test "the remote survey goes on without ls-monitor, luna-send or dropbear" {
    # PATH has only the system's own programs: none of the three exists.
    REMOTE_SURVEY_KEYFILES="$FIXTURES/authorized_keys" run sh "$REMOTE"
    [ "$status" -eq 0 ]
    [ "${lines[${#lines[@]}-1]}" = "remote survey complete" ]
    [[ "$output" == *"ls-monitor: absent on this TV"* ]]
    [[ "$output" == *"(not on PATH)"* ]]
    [[ "$output" == *"(no answer)"* ]]
}

# --- guards on the platform survey ------------------------------------------

@test "the platform survey runs initctl only to list jobs" {
    run sh -c "grep -v -E '^[[:space:]]*#' '$PLATFORM' | grep -o -E 'initctl +[A-Za-z-]+' | sort -u"
    [ "$output" = "initctl list" ]
}

@test "the platform survey reads no key or password hash, and an environment only for four names" {
    assert_no_code "$PLATFORM" 'shadow|host_key|id_(rsa|ed25519|ecdsa|dsa)|\.pem|known_hosts|authorized_keys'
    # environ is read in one place, straight into the grep for the four names.
    [ "$(code "$PLATFORM" | grep -c -E 'environ($|[^m])')" -eq 1 ]
    code "$PLATFORM" | grep -A 1 -E 'environ($|[^m])' \
        | grep -q -F "| grep -E '^(APPID|WAYLAND_DISPLAY|XDG_RUNTIME_DIR|DISPLAY_ID)='"
}

# --- the platform survey against fakes --------------------------------------

@test "the platform survey runs to its last line and has all its sections" {
    start_fake_shell
    run_platform
    [ "$status" -eq 0 ]
    [ "${lines[${#lines[@]}-1]}" = "platform survey complete" ]
    for section in identity lumen bus app-manager screensaver memory acr guard-log \
        lan-chain ports lg-addresses clock end; do
        [[ "$output" == *"===== $section ====="* ]]
    done
    [[ "$output" != *FORBIDDEN* ]]
    [[ "$output" != *refused* ]]
    # The acr service that never answers is given up on, not waited for (the
    # shell may say "Killed" first).
    grep -A 2 -F '$ ls-monitor -i com.webos.service.acr' <<<"$output" | grep -q -x '(no answer)'
}

@test "the platform survey shows the shell's four names and nothing else of its environment" {
    start_fake_shell
    run_platform
    [[ "$output" == *"binary $TMP/proc/lumen-shell, root /"* ]]
    [[ "$output" == *"    APPID=io.github.sunnyside007.lumen.shell"* ]]
    [[ "$output" == *"    WAYLAND_DISPLAY=wayland-0"* ]]
    [[ "$output" == *"    XDG_RUNTIME_DIR=/tmp/xdg"* ]]
    refute_grep -F 'not-for-the-report' <<<"$output"
    [[ "$output" != *"the shell is not running"* ]]
}

@test "the platform survey prints every time in UTC, whatever the TV's zone" {
    export TZ=PST8PDT
    [[ "$(date +%z)" == -0[78]00 ]]
    run_platform
    [ "$status" -eq 0 ]
    # GNU stat here names the offset; busybox's on the TV does not, but the
    # same TZ governs it.
    grep -q -E '^Modify: .* \+0000$' <<<"$output"
    refute_grep -E '^(Access|Modify|Change): .* [-+](0[1-9]|1[0-4])00$' <<<"$output"
}

@test "the platform survey says when the shell is not running" {
    run_platform
    [ "$status" -eq 0 ]
    [[ "$output" == *"(the shell is not running: facts 1, 5 and 9 need it; open Lumen and run the survey again)"* ]]
}

@test "the platform survey asks only for get and list methods, and lists upstart jobs only" {
    run_platform
    [ "$status" -eq 0 ]
    [ "$(grep -c '^luna-send' "$CALLS")" -eq 9 ]
    [ -z "$(grep '^luna-send' "$CALLS" | grep -v -E '^luna-send -n 1 -f luna://[A-Za-z0-9._/-]+/(get|list)[A-Za-z]* ')" ]
    grep -q -F 'getConfigs {"configNames":["com.webos.applicationManager.keepAliveApps"' "$CALLS"
    grep -q -F 'getAppInfo {"id":"io.github.sunnyside007.lumen.shell"}' "$CALLS"
    grep -q -F 'getSystemSettings {"category":"general","keys":["acrOnAllowed","acrAdAllowed","additional1Allowed"' "$CALLS"
    [ "$(grep -c '^ls-monitor -l$' "$CALLS")" -eq 1 ]
    [ -z "$(grep '^ls-monitor' "$CALLS" | grep -v -E '^ls-monitor (-l|-i [A-Za-z0-9._-]+)$')" ]
    grep -q -x 'ls-monitor -i com.webos.applicationManager' "$CALLS"
    [ -n "$(grep '^initctl ' "$CALLS")" ]
    [ -z "$(grep '^initctl ' "$CALLS" | grep -v -x 'initctl list')" ]
}

@test "the platform survey shows Guard's log as counts and times, never a reverse lookup's name" {
    run_platform
    [ "$status" -eq 0 ]
    # The rotated file first; a cut-off line is skipped.
    [[ "$output" == *"log: 13 lines, first 1793491200, last 1793577900 (unix seconds)"* ]]
    [[ "$output" == *"alphonso.tv: lookups 2, first 1793491260, last 1793577900; the first times: 1793491260 1793577900"* ]]
    [[ "$output" == *"lgsmartad.com: lookups 1, first 1793577660, last 1793577660"* ]]
    [[ "$output" == *"lgads.tv: lookups 0"* ]]
    [[ "$output" == *"elastic.lgwebostv.com: lookups 0"* ]]
    [[ "$output" == *"ueiwsp.com: lookups 1, first 1793491500"* ]]
    # 110.x is not 10.x, and 8.8.8.8 is not private.
    [[ "$output" == *"reverse lookups of private addresses: 4; other reverse lookups: 2"* ]]
    [[ "$output" == *"  unix day 20758: 2"$'\n'"  unix day 20759: 2"* ]]
    refute_grep -E 'in-addr|arpa|lgtvsdp|notalphonso|cut-off' <<<"$output"
    [[ "$output" == *"iconnectivity: $TMP/tv/bin/iconnectivity"* ]]
}

@test "the platform survey keeps the clock's source and drops its place" {
    run_platform
    clock=$(awk '/^===== clock =====$/, /^===== end =====$/' <<<"$output")
    [[ "$clock" == *'"source": "sdp"'* ]]
    [[ "$clock" == *'"systemTimeSource": "sdp"'* ]]
    [[ "$clock" == *'"utc": 1793491200'* ]]
    refute_grep -i -E 'zone|"tz"|offset' <<<"$clock"
    refute_grep -i -E 'chicago|cst|united states' <<<"$output"
}

@test "the platform survey shows Lumen's and Homebrew Channel's role files and only names the rest" {
    run_platform
    [[ "$output" == *"--- $PFIX/ls2/roles.d/io.github.sunnyside007.lumen.shell.app.json"* ]]
    [[ "$output" == *'"trustLevel": "dev"'* ]]
    [[ "$output" == *"--- $PFIX/ls2/roles.d/org.webosbrew.hbchannel.service.role.json"* ]]
    [[ "$output" == *"legacy-outbound-shown"* ]]
    grep -q -x 'com.example.other.role.json' <<<"$output"
    refute_grep -F 'not-shown-other' <<<"$output"
    [[ "$output" == *"--- $PFIX/ls2/services.d"$'\n''(absent)'* ]]
}

@test "the platform survey finds what it looks for in the library and the programs" {
    run_platform
    grep -q -x 'SDL_WEBOS_ACCESS_POLICY_KEYS_BACK' <<<"$output"
    grep -q -x 'luna://com.webos.applicationManager/registerNativeApp' <<<"$output"
    refute_grep -x 'SDL_VideoInit' <<<"$output"
    [[ "$output" == *"acr2: $TMP/tv/bin/acr2"* ]]
    grep -q -x 'acrOnAllowed' <<<"$output"
    grep -q -x 'livePlus' <<<"$output"
    refute_grep -x 'hello' <<<"$output"
    [[ "$output" == *"collectAllLog: found in pmlogd"* ]]
    grep -q -x '156.147.69.32:443' <<<"$output"
    grep -q -x "$PFIX/apps/com.example.sync-demo" <<<"$output"
    refute_grep -F 'com.example.player' <<<"$output"
    [[ "$output" == *'"main": "lumen-shell"'* ]]
    [[ "$output" == *"$TMP/tv/jail/var/run"* ]]
}

@test "the platform survey goes on without ls-monitor, luna-send or initctl" {
    # PATH has only the system's own programs, and the paths are the TV's,
    # which are not here.
    run sh "$PLATFORM"
    [ "$status" -eq 0 ]
    [ "${lines[${#lines[@]}-1]}" = "platform survey complete" ]
    [[ "$output" == *"ls-monitor: absent on this TV"* ]]
    [[ "$output" == *"(no answer)"* ]]
    [[ "$output" == *"/var/lib/lumen/guard/lookups.log: (absent)"* ]]
}
