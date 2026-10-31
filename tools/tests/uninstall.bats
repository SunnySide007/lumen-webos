#!/usr/bin/env bats
# tools/uninstall-all.sh against a fake tools/tv.sh and a fake
# tools/remote.sh that only log their calls. Nothing reaches a TV.

setup() {
    UNINSTALL="$BATS_TEST_DIRNAME/../uninstall-all.sh"
    CALLS="$BATS_TEST_TMPDIR/calls"
    : >"$CALLS"
    cat >"$BATS_TEST_TMPDIR/tv.sh" <<'EOF'
#!/bin/sh
printf '%s|%s\n' "$1" "$2" >>"$CALLS"
case "$1 $2" in "remove ${FAKE_FAIL_REMOVE:-none}") exit 1 ;; esac
case "$1 $2" in "run "*"${FAKE_FAIL_RUN:-no such step}"*) exit 1 ;; esac
case "$1 $2" in "script "*"${FAKE_FAIL_SCRIPT:-no such script}") exit 1 ;; esac
exit 0
EOF
    cat >"$BATS_TEST_TMPDIR/remote.sh" <<'EOF'
#!/bin/sh
printf 'remote|%s\n' "$*" >>"$CALLS"
[ -z "${FAKE_FAIL_REMOTE:-}" ]
EOF
    chmod +x "$BATS_TEST_TMPDIR/tv.sh" "$BATS_TEST_TMPDIR/remote.sh"
    export CALLS LUMEN_TV_SH="$BATS_TEST_TMPDIR/tv.sh" LUMEN_REMOTE_SH="$BATS_TEST_TMPDIR/remote.sh"
}

@test "the default is a dry run that runs nothing" {
    run "$UNINSTALL"
    [ "$status" -eq 0 ]
    [[ "$output" == *"Dry run"* ]]
    [[ "$output" == *"tools/tv.sh remove io.github.sunnyside007.lumen.shell"* ]]
    [[ "$output" == *"rm -rf /var/lib/lumen"* ]]
    [[ "$output" == *"rm -f /var/lib/webosbrew/init.d/lumen-*"* ]]
    [[ "$output" == *"tools/tv.sh script profiles/privacy/guard-undo.sh"* ]]
    [[ "$output" == *"tools/tv.sh script takeover/filter-undo.sh"* ]]
    [[ "$output" == *"tools/tv.sh script takeover/undo.sh"* ]]
    [[ "$output" == *"tools/tv.sh remove io.github.sunnyside007.lumen.shell   (only if the default home was set back)"* ]]
    [[ "$output" == *"  tools/remote.sh remove"* ]]
    [[ "$output" == *"lumen-capture"* ]]
    [[ "$output" == *"Glasshouse is kept"* ]]
    [[ "$output" != *"tvwebctl"* ]]
    [[ "$output" != *"com.beb.plxnative"* ]]
    [ ! -s "$CALLS" ]
}

@test "--glasshouse adds the documented undo, in order, and still runs nothing" {
    run "$UNINSTALL" --glasshouse
    [ "$status" -eq 0 ]
    [ ! -s "$CALLS" ]
    # The line number of each undo step must increase.
    prev=0
    for marker in 'tvwebctl restore-home;' 'tvwebctl restore-keys;' \
        'tvwebctl restore-glasshouse-home;' 'tvwebctl stop;' 'rm -rf /var/lib/tvweb ' \
        'remove io.github.rorygallagher2024.lg-webos-dashboard'; do
        line=$(printf '%s\n' "$output" | grep -nF -- "$marker" | head -n 1 | cut -d: -f1)
        [ -n "$line" ]
        [ "$line" -gt "$prev" ]
        prev=$line
    done
    [[ "$output" == *"/var/lib/webosbrew/init.d/50-tvweb*"* ]]
    [[ "$output" == *"/var/lib/webosbrew/init.d/20-tvweb-services"* ]]
    [[ "$output" == *"/var/lib/webosbrew/init.d/20-services.sh"* ]]
    [[ "$output" == *"/var/lib/webosbrew/tvweb-boot.log*"* ]]
    [[ "$output" == *"/media/developer/temp/glasshouse-install"* ]]
}

@test "--yes runs every step through tv.sh" {
    run "$UNINSTALL" --yes --plx-native
    [ "$status" -eq 0 ]
    grep -qx 'remove|io.github.sunnyside007.lumen.shell' "$CALLS"
    grep -qx 'run|rm -rf /var/lib/lumen' "$CALLS"
    grep -qx 'run|rm -rf /var/lib/lumen-hdr' "$CALLS"
    grep -qx 'remove|com.beb.plxnative' "$CALLS"
    grep -qx 'remote|remove' "$CALLS"
    [ "$(wc -l <"$CALLS")" -eq 12 ]
    [[ "$output" == *"Cold boot"* ]]
}

@test "Guard's rules and process go after its hook and before its files" {
    run "$UNINSTALL" --yes
    [ "$status" -eq 0 ]
    hooks=$(grep -nxF 'run|rm -f /var/lib/webosbrew/init.d/lumen-*' "$CALLS" | cut -d: -f1)
    guard=$(grep -nx 'script|.*/profiles/privacy/guard-undo.sh' "$CALLS" | cut -d: -f1)
    files=$(grep -nx 'run|rm -rf /var/lib/lumen' "$CALLS" | cut -d: -f1)
    [ -n "$hooks" ] && [ -n "$guard" ] && [ -n "$files" ]
    [ "$hooks" -lt "$guard" ]
    [ "$guard" -lt "$files" ]
    # The script is this checkout's, and exists.
    [ -f "$(sed -n 's/^script|\(.*guard-undo\.sh\)$/\1/p' "$CALLS")" ]
}

@test "route D is undone first: the key filter, then the default home, then Lumen's app and files" {
    run "$UNINSTALL" --yes
    [ "$status" -eq 0 ]
    filter=$(grep -nx 'script|.*/takeover/filter-undo.sh' "$CALLS" | cut -d: -f1)
    home=$(grep -nx 'script|.*/takeover/undo.sh' "$CALLS" | cut -d: -f1)
    app=$(grep -nx 'remove|io.github.sunnyside007.lumen.shell' "$CALLS" | cut -d: -f1)
    files=$(grep -nx 'run|rm -rf /var/lib/lumen' "$CALLS" | cut -d: -f1)
    [ -n "$filter" ] && [ -n "$home" ] && [ -n "$app" ] && [ -n "$files" ]
    [ "$filter" -lt "$home" ]
    [ "$home" -lt "$app" ]
    [ "$app" -lt "$files" ]
    # The scripts are this checkout's, and exist.
    run sed -n 's|^script\|\(.*/takeover/.*\)$|\1|p' "$CALLS"
    [ "${#lines[@]}" -eq 2 ]
    for f in "${lines[@]}"; do
        [ -f "$f" ]
    done
}

@test "when the default home can't be set back, Lumen's app is kept and the rest still runs" {
    FAKE_FAIL_SCRIPT=/takeover/undo.sh run "$UNINSTALL" --yes
    [ "$status" -eq 1 ]
    [ "$(grep -c 'remove|io.github.sunnyside007.lumen.shell' "$CALLS")" -eq 0 ]
    grep -qxF 'run|rm -f /tmp/lumen-*' "$CALLS"
    [[ "$output" == *"skipped (the default home was not set back)"* ]]
    [[ "$output" == *"Lumen's app was kept"* ]]
}

@test "Lumen Remote's line goes through remote.sh remove, before /var/lib/lumen" {
    run "$UNINSTALL" --yes
    [ "$status" -eq 0 ]
    guard=$(grep -nx 'script|.*/profiles/privacy/guard-undo.sh' "$CALLS" | cut -d: -f1)
    remote=$(grep -nxF 'remote|remove' "$CALLS" | cut -d: -f1)
    files=$(grep -nx 'run|rm -rf /var/lib/lumen' "$CALLS" | cut -d: -f1)
    [ -n "$guard" ] && [ -n "$remote" ] && [ -n "$files" ]
    [ "$guard" -lt "$remote" ]
    [ "$remote" -lt "$files" ]
    # Only through remote.sh: tools/tv.sh has no such command.
    [ "$(grep -c '|remove$' "$CALLS")" -eq 1 ]
}

@test "a failed remote.sh remove does not stop the rest, and fails the run" {
    FAKE_FAIL_REMOTE=1 run "$UNINSTALL" --yes
    [ "$status" -eq 1 ]
    grep -qx 'run|rm -rf /var/lib/lumen' "$CALLS"
    grep -qxF 'run|rm -f /tmp/lumen-*' "$CALLS"
    [[ "$output" == *"1 step(s) failed"* ]]
}

@test "a script step that names Homebrew Channel's settings stops everything" {
    root="$BATS_TEST_TMPDIR/root"
    mkdir -p "$root/tools" "$root/profiles/privacy" "$root/takeover"
    cp "$UNINSTALL" "$root/tools/"
    : >"$root/takeover/filter-undo.sh"
    : >"$root/takeover/undo.sh"
    printf 'rm -f /var/luna/preferences/webosbrew_failsafe\n' >"$root/profiles/privacy/guard-undo.sh"
    run "$root/tools/uninstall-all.sh" --yes
    [ "$status" -eq 2 ]
    [[ "$output" == *"must never touch"* ]]
    [ ! -s "$CALLS" ]
}

@test "a failed step does not stop the rest, and fails the run" {
    FAKE_FAIL_REMOVE=io.github.sunnyside007.lumen.shell run "$UNINSTALL" --yes
    [ "$status" -eq 1 ]
    grep -qxF 'run|rm -f /tmp/lumen-*' "$CALLS"
    [[ "$output" == *"1 step(s) failed"* ]]
}

@test "nothing ever names Homebrew Channel, its settings or the SSH key" {
    run "$UNINSTALL" --glasshouse --plx-native
    [ "$status" -eq 0 ]
    [[ "$output" != *"webosbrew_"* ]]
    [[ "$output" != *"org.webosbrew"* ]]
    [[ "$output" != *"authorized_keys"* ]]
    [[ "$output" != *".ssh"* ]]
    run "$UNINSTALL" --glasshouse --plx-native --yes
    [ "$(grep -c 'webosbrew_\|org\.webosbrew\|authorized_keys\|\.ssh' "$CALLS")" -eq 0 ]
}

@test "an unknown argument fails before anything runs" {
    run "$UNINSTALL" --everything --yes
    [ "$status" -eq 2 ]
    [ ! -s "$CALLS" ]
}

@test "a failed Glasshouse restore keeps its files for a retry" {
    FAKE_FAIL_RUN="tvwebctl restore-keys" run "$UNINSTALL" --glasshouse --yes
    [ "$status" -eq 1 ]
    # The later restores still run; the purge steps do not.
    grep -q 'restore-glasshouse-home' "$CALLS"
    [ "$(grep -c 'rm -rf /var/lib/tvweb' "$CALLS")" -eq 0 ]
    [ "$(grep -c 'remove|io.github.rorygallagher2024.lg-webos-dashboard' "$CALLS")" -eq 0 ]
    [[ "$output" == *"skipped (a Glasshouse restore failed)"* ]]
    [[ "$output" == *"files were kept"* ]]
}

@test "the dry run marks the steps that wait for the restores" {
    run "$UNINSTALL" --glasshouse
    [[ "$output" == *"rm -rf /var/lib/tvweb /media/developer/temp/glasshouse-install'   (only if the restores above succeeded)"* ]]
}
