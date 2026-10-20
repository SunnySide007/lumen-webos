#!/usr/bin/env bats
# tools/capture.sh against a fake tools/tv.sh, and the on-TV helper
# (tools/capture/lumen-capture.sh) run here against a fake tcpdump in a
# temporary directory. Nothing reaches a TV.

setup() {
    CAPTURE="$BATS_TEST_DIRNAME/../capture.sh"
    HELPER="$BATS_TEST_DIRNAME/../capture/lumen-capture.sh"
    CALLS="$BATS_TEST_TMPDIR/calls"
    : >"$CALLS"

    # A fake tv.sh: logs each call; `run` answers from FAKE_* variables.
    FAKE_TV="$BATS_TEST_TMPDIR/tv.sh"
    cat >"$FAKE_TV" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$CALLS"
case "$1 $2" in
    "run sh "*" list") printf '%s\n' ${FAKE_LIST:-} ;;
    "run chmod"*) echo "${FAKE_SHA:-0000}  /media/developer/temp/lumen-capture/tcpdump" ;;
    "run cat "*) echo "pcap bytes" ;;
esac
exit 0
EOF
    chmod +x "$FAKE_TV"
    export CALLS LUMEN_TV_SH="$FAKE_TV" LUMEN_CAPTURE_OUT="$BATS_TEST_TMPDIR/out"

    # A fake tcpdump for the helper: records its arguments, runs until signalled.
    CAPDIR="$BATS_TEST_TMPDIR/cap"
    mkdir -p "$CAPDIR"
    cat >"$CAPDIR/tcpdump" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" >"$(dirname "$0")/args"
trap 'exit 0' TERM
while :; do sleep 1; done
EOF
    chmod +x "$CAPDIR/tcpdump"
    export LUMEN_CAPTURE_DIR="$CAPDIR"
}

teardown() {
    LUMEN_CAPTURE_DIR="$CAPDIR" sh "$HELPER" stop >/dev/null 2>&1 || true
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

@test "start passes the duration to the helper" {
    run "$CAPTURE" start 24
    [ "$status" -eq 0 ]
    grep -qx 'run sh /media/developer/temp/lumen-capture/lumen-capture.sh start 24' "$CALLS"
}

@test "push without a built tcpdump says how to build it" {
    LUMEN_CAPTURE_BIN=/nonexistent run "$CAPTURE" push
    [ "$status" -ne 0 ]
    [[ "$output" == *"make capture-tools"* ]]
    [ ! -s "$CALLS" ]
}

@test "push uploads both files and checks the hash" {
    bin="$BATS_TEST_TMPDIR/tcpdump"
    echo binary >"$bin"
    sha=$(sha256sum "$bin" | cut -d' ' -f1)
    LUMEN_CAPTURE_BIN="$bin" FAKE_SHA="$sha" run "$CAPTURE" push
    [ "$status" -eq 0 ]
    grep -qx "push $bin /media/developer/temp/lumen-capture/tcpdump" "$CALLS"
    grep -q "^push .*lumen-capture.sh /media/developer/temp/lumen-capture/lumen-capture.sh$" "$CALLS"
    [[ "$output" == *"matches"* ]]
}

@test "push fails when the TV's copy has another hash" {
    bin="$BATS_TEST_TMPDIR/tcpdump"
    echo binary >"$bin"
    LUMEN_CAPTURE_BIN="$bin" FAKE_SHA=deadbeef run "$CAPTURE" push
    [ "$status" -ne 0 ]
    [[ "$output" == *"SHA-256 deadbeef"* ]]
}

@test "fetch copies each pcap into the output directory" {
    FAKE_LIST="/media/developer/temp/lumen-capture/pcap/lumen-20261020-060000.pcap0
/media/developer/temp/lumen-capture/pcap/lumen-20261020-060000.pcap1" run "$CAPTURE" fetch
    [ "$status" -eq 0 ]
    [ -f "$LUMEN_CAPTURE_OUT/lumen-20261020-060000.pcap0" ]
    [ -f "$LUMEN_CAPTURE_OUT/lumen-20261020-060000.pcap1" ]
    [[ "$output" == *"fetched 2 file(s)"* ]]
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

@test "helper: refuses to work outside scratch space" {
    for bad in /home/root /tmp/../etc /tmp/a/../.. '/tmp/a b' /tmp; do
        LUMEN_CAPTURE_DIR="$bad" run sh "$HELPER" clean
        [ "$status" -ne 0 ]
        [[ "$output" == *"refusing to work in"* ]]
    done
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

@test "helper: start, status, stop and clean" {
    LUMEN_CAPTURE_TCPDUMP="$CAPDIR/tcpdump" run sh "$HELPER" start 1 1 2
    [ "$status" -eq 0 ]
    [[ "$output" == *"capturing: pid"* ]]
    [ -f "$CAPDIR/tcpdump.pid" ]

    # The filter and ring settings reach tcpdump.
    grep -qx -- '-i' "$CAPDIR/args"
    grep -qx 'any' "$CAPDIR/args"
    grep -qx -- '-C' "$CAPDIR/args"
    grep -q '^port 53$' "$CAPDIR/args"
    grep -q 'tcp-syn' "$CAPDIR/args"
    grep -q 'lumen-[0-9]*-[0-9]*\.pcap$' "$CAPDIR/args"

    run sh "$HELPER" status
    [[ "$output" == *"running: pid"* ]]

    LUMEN_CAPTURE_TCPDUMP="$CAPDIR/tcpdump" run sh "$HELPER" start 1 1 2
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

@test "push without sha256sum on the TV falls back to the size check" {
    bin="$BATS_TEST_TMPDIR/tcpdump"
    echo binary >"$bin"
    LUMEN_CAPTURE_BIN="$bin" FAKE_SHA=no-sha256sum run "$CAPTURE" push
    [ "$status" -eq 0 ]
    [[ "$output" == *"only the size was checked"* ]]
}
