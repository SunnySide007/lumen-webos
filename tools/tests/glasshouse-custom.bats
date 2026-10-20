#!/usr/bin/env bats
# tools/glasshouse-custom.sh against a fake curl. Nothing reaches a TV.

setup() {
    UPLOAD="$BATS_TEST_DIRNAME/../glasshouse-custom.sh"
    LOG="$BATS_TEST_TMPDIR/curl.log"
    : >"$LOG"
    # The fake curl logs its arguments, keeps the body it was sent, and
    # answers like Glasshouse.
    cat >"$BATS_TEST_TMPDIR/curl" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$LOG"
for a; do case $a in @*) cp "${a#@}" "$(dirname "$LOG")/body.json" ;; esac; done
case "$*" in
    *api/privacy*) [ -z "${FAKE_NO_TUNNEL:-}" ] || exit 7 ;;
    *api/adblock/custom*) echo "${FAKE_REPLY:-{\"ok\":true,\"custom\":{\"names\":2}}}" ;;
esac
EOF
    chmod +x "$BATS_TEST_TMPDIR/curl"
    export LOG LUMEN_CURL="$BATS_TEST_TMPDIR/curl"
    HOSTS="$BATS_TEST_TMPDIR/list.hosts"
    printf '# header\n0.0.0.0\tAds.Example.com\n::\tads.example.com\n# 0.0.0.0\tcandidate.example\n0.0.0.0 track.example.net # why\n' >"$HOSTS"
}

@test "the shipped list has no active entries yet, so nothing is sent" {
    run "$UPLOAD"
    [ "$status" -ne 0 ]
    [[ "$output" == *"no active entries"* ]]
    [ ! -s "$LOG" ]
}

@test "the default is a dry run listing the active names" {
    run "$UPLOAD" --file "$HOSTS"
    [ "$status" -eq 0 ]
    [[ "$output" == *"would add 2 name(s)"* ]]
    [[ "$output" == *"0.0.0.0 ads.example.com"* ]]
    [[ "$output" == *"0.0.0.0 track.example.net"* ]]
    [[ "$output" != *"candidate"* ]]
    [ ! -s "$LOG" ]
}

@test "--yes posts JSON in add mode through the tunnel" {
    run "$UPLOAD" --file "$HOSTS" --yes --port 9090
    [ "$status" -eq 0 ]
    grep -q 'http://127.0.0.1:9090/api/privacy' "$LOG"
    grep -q -- '-X POST -H Content-Type: application/json' "$LOG"
    grep -q 'http://127.0.0.1:9090/api/adblock/custom' "$LOG"
    body=$(cat "$BATS_TEST_TMPDIR/body.json")
    [ "$body" = '{"mode":"add","file":"list.hosts","text":"0.0.0.0 ads.example.com\n0.0.0.0 track.example.net\n"}' ]
    [[ "$output" == *"done: add 2 name(s)"* ]]
}

@test "--clear --yes removes the custom list" {
    run "$UPLOAD" --clear --yes
    [ "$status" -eq 0 ]
    [ "$(cat "$BATS_TEST_TMPDIR/body.json")" = '{"clear":true}' ]
}

@test "no tunnel: a clear message and no upload" {
    FAKE_NO_TUNNEL=1 run "$UPLOAD" --file "$HOSTS" --yes
    [ "$status" -ne 0 ]
    [[ "$output" == *"open the tunnel first"* ]]
    [ "$(grep -c 'adblock/custom' "$LOG")" -eq 0 ]
}

@test "a refusal from Glasshouse fails" {
    FAKE_REPLY='{"ok":false,"error":"controls disabled in config"}' run "$UPLOAD" --file "$HOSTS" --yes
    [ "$status" -ne 0 ]
    [[ "$output" == *"controls disabled"* ]]
    [[ "$output" == *"refused"* ]]
}

@test "a list that blocks an allowlisted name is never sent" {
    printf '0.0.0.0\tapi.netflix.com\n' >>"$HOSTS"
    run "$UPLOAD" --file "$HOSTS" --yes
    [ "$status" -ne 0 ]
    [[ "$output" == *"FAIL allowlisted name is sinkholed: api.netflix.com"* ]]
    [ ! -s "$LOG" ]
}

@test "odd names and bad ports are refused" {
    printf '0.0.0.0\tbad"name.example\n' >>"$HOSTS"
    run "$UPLOAD" --file "$HOSTS"
    [ "$status" -ne 0 ]
    [[ "$output" == *"characters a host name cannot have"* ]]
    run "$UPLOAD" --port 80x
    [ "$status" -ne 0 ]
    [[ "$output" == *"not a port number"* ]]
}
