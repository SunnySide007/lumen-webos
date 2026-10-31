#!/usr/bin/env bats
# tools/capture.sh against a fake tools/tv.sh, and the on-TV helper
# (tools/capture/lumen-capture.sh) run here against a fake tcpdump in a
# temporary directory, its /var/lib/lumen/capture moved there and root's uid
# changed to ours. Nothing reaches a TV.

setup() {
    CAPTURE="$BATS_TEST_DIRNAME/../capture.sh"
    CALLS="$BATS_TEST_TMPDIR/calls"
    : >"$CALLS"

    # A fake tv.sh: logs each call; `run` answers from FAKE_* variables:
    # FAKE_GOT for push's check, the two SHA-256 sums on the TV, one a line.
    FAKE_TV="$BATS_TEST_TMPDIR/tv.sh"
    cat >"$FAKE_TV" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$CALLS"
case "$1 $2" in
    "run sh "*" list") printf '%s\n' ${FAKE_LIST:-} ;;
    "run set -e"*) [ -z "${FAKE_GOT:-}" ] || printf '%s\n' $FAKE_GOT ;;
    "run cat "*) echo "pcap bytes" ;;
esac
exit 0
EOF
    chmod +x "$FAKE_TV"
    export CALLS LUMEN_TV_SH="$FAKE_TV" LUMEN_CAPTURE_OUT="$BATS_TEST_TMPDIR/out"

    # The helper, with its folder here; a fake tcpdump in it that records its
    # arguments and runs until signalled.
    CAPDIR="$BATS_TEST_TMPDIR/cap"
    mkdir -p "$CAPDIR"
    chmod 755 "$CAPDIR"
    HELPER="$BATS_TEST_TMPDIR/lumen-capture.sh"
    remap_helper "$(id -u)"
    cat >"$CAPDIR/tcpdump" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" >"$(dirname "$0")/args"
trap 'exit 0' TERM
while :; do sleep 1; done
EOF
    chmod +x "$CAPDIR/tcpdump"
}

not() {
    if "$@"; then
        echo "should have failed: $*"
        return 1
    fi
}

# remap_helper UID: the helper with its folder in CAPDIR and root's uid UID.
remap_helper() {
    sed -e "s|/var/lib/lumen/capture|$CAPDIR|g" -e "s|^ROOT_UID=0\$|ROOT_UID=$1|" \
        "$BATS_TEST_DIRNAME/../capture/lumen-capture.sh" >"$HELPER"
}

teardown() {
    remap_helper "$(id -u)"
    [ ! -L "$CAPDIR" ] || return 0
    sh "$HELPER" stop >/dev/null 2>&1 || true
}

@test "help lists the commands" {
    run "$CAPTURE" help
    [ "$status" -eq 0 ]
    [[ "$output" == *"fetch"* ]]
    [[ "$output" == *"clean"* ]]
}

@test "an unknown command fails" {
    run "$CAPTURE" frobnicate
    [ "$status" -ne 0 ]
    [[ "$output" == *"unknown command: frobnicate"* ]]
}

@test "start refuses bad durations before reaching the TV" {
    for h in abc 0 73 -1 1.5; do
        run "$CAPTURE" start "$h"
        [ "$status" -ne 0 ]
        [[ "$output" == *"HOURS must be"* ]]
    done
    [ ! -s "$CALLS" ]
}

@test "start passes the duration to the helper in root's capture folder" {
    run "$CAPTURE" start 24
    [ "$status" -eq 0 ]
    grep -qx 'run sh /var/lib/lumen/capture/lumen-capture.sh start 24' "$CALLS"
}

@test "push without a built tcpdump says how to build it" {
    LUMEN_CAPTURE_BIN=/nonexistent run "$CAPTURE" push
    [ "$status" -ne 0 ]
    [[ "$output" == *"make capture-tools"* ]]
    [ ! -s "$CALLS" ]
}

@test "push stages both files in root's stage, never /tmp or developer/temp, and checks both hashes" {
    bin="$BATS_TEST_TMPDIR/tcpdump"
    echo binary >"$bin"
    sha_bin=$(sha256sum "$bin" | cut -d' ' -f1)
    sha_helper=$(sha256sum "$BATS_TEST_DIRNAME/../capture/lumen-capture.sh" | cut -d' ' -f1)
    LUMEN_CAPTURE_BIN="$bin" FAKE_GOT="$sha_bin $sha_helper" run "$CAPTURE" push
    [ "$status" -eq 0 ]
    [[ "$output" == *"match this checkout"* ]]
    grep -qx "push $bin /var/lib/lumen/stage/capture/tcpdump" "$CALLS"
    grep -q "^push .*lumen-capture.sh /var/lib/lumen/stage/capture/lumen-capture.sh$" "$CALLS"
    # The check names both sums, and moves the files only into the capture folder.
    grep -qF "'$sha_bin $sha_helper '" "$CALLS"
    grep -q 'mv /var/lib/lumen/capture/\$f.tmp /var/lib/lumen/capture/\$f' "$CALLS"
    # Every upload goes to the stage; nothing on the TV names /tmp (the local
    # files here may be under it) or developer/temp.
    [ -z "$(awk '$1 == "push" && $3 !~ /^\/var\/lib\/lumen\/stage\// ' "$CALLS")" ]
    [ -z "$(grep -v '^push ' "$CALLS" | grep -E '/tmp|developer/temp')" ]
    grep -qx 'run /var/lib/lumen/capture/tcpdump --version' "$CALLS"
}

@test "push fails when the TV's copies have other hashes, or none" {
    bin="$BATS_TEST_TMPDIR/tcpdump"
    echo binary >"$bin"
    LUMEN_CAPTURE_BIN="$bin" FAKE_GOT="deadbeef cafe" run "$CAPTURE" push
    [ "$status" -ne 0 ]
    [[ "$output" == *"(deadbeef cafe ) are not this checkout's"* ]]
    LUMEN_CAPTURE_BIN="$bin" FAKE_GOT="" run "$CAPTURE" push
    [ "$status" -ne 0 ]
    [[ "$output" == *"(none) are not this checkout's"* ]]
    not grep -q -- '--version' "$CALLS"
}

@test "fetch copies each pcap into the output directory" {
    FAKE_LIST="/var/lib/lumen/capture/pcap/lumen-20261020-060000.pcap0
/var/lib/lumen/capture/pcap/lumen-20261020-060000.pcap1" run "$CAPTURE" fetch
    [ "$status" -eq 0 ]
    [ -f "$LUMEN_CAPTURE_OUT/lumen-20261020-060000.pcap0" ]
    [ -f "$LUMEN_CAPTURE_OUT/lumen-20261020-060000.pcap1" ]
    [[ "$output" == *"fetched 2 file(s)"* ]]
    grep -qx 'run cat /var/lib/lumen/capture/pcap/lumen-20261020-060000.pcap0' "$CALLS"
}

@test "fetch refuses odd names from the TV" {
    for bad in "/x/../../etc/passwd" "/x/notes.txt" '/x/lumen-$(reboot).pcap'; do
        FAKE_LIST="$bad" run "$CAPTURE" fetch
        [ "$status" -ne 0 ]
        [[ "$output" == *"unexpected file name"* ]]
    done
}

@test "fetch with nothing on the TV fails" {
    FAKE_LIST="" run "$CAPTURE" fetch
    [ "$status" -ne 0 ]
    [[ "$output" == *"no pcap files"* ]]
}

@test "helper: start refuses a folder others may write, another owner's, or a link" {
    chmod 777 "$CAPDIR"
    run sh "$HELPER" start 1 1 2
    [ "$status" -ne 0 ]
    [[ "$output" == *"$CAPDIR is writable by others (drwxrwxrwx"*"nothing started"* ]]
    [ ! -e "$CAPDIR/args" ]
    chmod 755 "$CAPDIR"
    mkdir -m 777 "$CAPDIR/pcap"
    chmod 777 "$CAPDIR/pcap"
    run sh "$HELPER" start 1 1 2
    [ "$status" -ne 0 ]
    [[ "$output" == *"$CAPDIR/pcap is writable by others"* ]]
    [ ! -e "$CAPDIR/args" ]
    rmdir "$CAPDIR/pcap"
    remap_helper 99999
    run sh "$HELPER" start 1 1 2
    [ "$status" -ne 0 ]
    [[ "$output" == *"is not root's"* ]]
    [ ! -e "$CAPDIR/args" ]
    remap_helper "$(id -u)"
    mv "$CAPDIR" "$CAPDIR.real"
    ln -s "$CAPDIR.real" "$CAPDIR"
    run sh "$HELPER" start 1 1 2
    [ "$status" -ne 0 ]
    [[ "$output" == *"$CAPDIR is not a real folder"* ]]
    [ ! -e "$CAPDIR.real/args" ]
}

@test "helper: start checks its limits" {
    run sh "$HELPER" start 0
    [ "$status" -ne 0 ]
    run sh "$HELPER" start 24 500
    [ "$status" -ne 0 ]
    [[ "$output" == *"FILE_MB"* ]]
    run sh "$HELPER" start 24 50 1
    [ "$status" -ne 0 ]
    [[ "$output" == *"FILES"* ]]
}

@test "helper: the default ring is 5 files of 20 MB" {
    grep -q 'mb=${2:-20} files=${3:-5}' "$BATS_TEST_DIRNAME/../capture/lumen-capture.sh"
    grep -q 'need=$(( (mb \* files + 100) \* 1024 ))' "$BATS_TEST_DIRNAME/../capture/lumen-capture.sh"
}

@test "helper: start, status, stop and clean" {
    run sh "$HELPER" start 1 1 2
    [ "$status" -eq 0 ]
    [[ "$output" == *"capturing: pid"* ]]
    [ -f "$CAPDIR/tcpdump.pid" ]
    [[ "$(ls -ldn "$CAPDIR/pcap")" == drwx------* ]]

    # The filter and ring settings reach tcpdump.
    grep -qx -- '-i' "$CAPDIR/args"
    grep -qx 'any' "$CAPDIR/args"
    grep -qx -- '-C' "$CAPDIR/args"
    grep -q '^port 53$' "$CAPDIR/args"
    grep -q 'tcp-syn' "$CAPDIR/args"
    grep -q "^$CAPDIR/pcap/lumen-[0-9]*-[0-9]*\.pcap$" "$CAPDIR/args"

    run sh "$HELPER" status
    [[ "$output" == *"running: pid"* ]]

    run sh "$HELPER" start 1 1 2
    [ "$status" -ne 0 ]
    [[ "$output" == *"already running"* ]]

    run sh "$HELPER" stop
    [ "$status" -eq 0 ]
    [[ "$output" == *"stopped pid"* ]]
    [ ! -f "$CAPDIR/tcpdump.pid" ]

    run sh "$HELPER" status
    [[ "$output" == *"not running"* ]]

    run sh "$HELPER" clean
    [ "$status" -eq 0 ]
    [ ! -d "$CAPDIR" ]
}

@test "helper: a stale pid file is not mistaken for a capture" {
    echo 1 >"$CAPDIR/tcpdump.pid"
    run sh "$HELPER" status
    [[ "$output" == *"not running"* ]]
    run sh "$HELPER" stop
    [ "$status" -eq 0 ]
    [[ "$output" == *"not running"* ]]
}

@test "helper: list prints only pcap files" {
    mkdir -p "$CAPDIR/pcap"
    touch "$CAPDIR/pcap/lumen-1.pcap0" "$CAPDIR/pcap/lumen-1.pcap1" "$CAPDIR/pcap/other"
    run sh "$HELPER" list
    [ "$status" -eq 0 ]
    [ "${#lines[@]}" -eq 2 ]
}

@test "helper: runs nothing from an environment variable, and names no folder in apps' reach" {
    f="$BATS_TEST_DIRNAME/../capture/lumen-capture.sh"
    not grep -q 'LUMEN_CAPTURE_' "$f"
    not grep -qE '^[^#]*(/tmp|developer/temp)' "$f"
    not grep -qE '^[^#]*(/tmp|developer/temp)' "$CAPTURE"
}
