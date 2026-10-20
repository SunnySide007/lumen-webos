#!/usr/bin/env bats
# tools/store-safe-check.sh, offline against fixture hosts files and live
# against a fake tools/tv.sh that runs the generated lookup script here with
# a fake getent or nslookup. Nothing reaches a TV.

setup() {
    CHECK="$BATS_TEST_DIRNAME/../store-safe-check.sh"
    FIX="$BATS_TEST_DIRNAME/fixtures/store-safe"
    PROFILE="$FIX/profile"
}

@test "a store-safe hosts file passes at stage 4" {
    run "$CHECK" --profile "$PROFILE" --hosts-file "$FIX/hosts-pass"
    [ "$status" -eq 0 ]
    [[ "$output" == *"firmware hosts sinkholed: 2 of 2"* ]]
    [[ "$output" == *"custom-list hosts sinkholed: 1 of 1"* ]]
    [[ "$output" == *"PASS (stage 4)"* ]]
    [[ "$output" != *"FAIL"* ]]
}

@test "a sinkholed name under an allowlisted suffix fails, whatever its case" {
    run "$CHECK" --profile "$PROFILE" --hosts-file "$FIX/hosts-allow-sunk"
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL allowlisted name is sinkholed: api.netflix.com (rule netflix.com)"* ]]
}

@test "an exact rule covers the apex only" {
    run "$CHECK" --profile "$PROFILE" --hosts-file "$FIX/hosts-apex-sunk"
    [ "$status" -eq 1 ]
    [[ "$output" == *"lgtvsdp.com (rule =lgtvsdp.com)"* ]]
    # us.rdx2.lgtvsdp.com and notnetflix.com are sinkholed in every fixture.
    [[ "$output" != *"rdx2"* ]]
    [[ "$output" != *"notnetflix"* ]]
}

@test "missing blocks fail only from the stage that adds them" {
    run "$CHECK" --profile "$PROFILE" --hosts-file "$FIX/hosts-missing" --stage 2
    [ "$status" -eq 0 ]
    [[ "$output" == *"firmware hosts: not checked before stage 3"* ]]

    run "$CHECK" --profile "$PROFILE" --hosts-file "$FIX/hosts-missing" --stage 3
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL firmware host not sinkholed: su.lge.com"* ]]
    [[ "$output" != *"custom-list host not sinkholed"* ]]

    run "$CHECK" --profile "$PROFILE" --hosts-file "$FIX/hosts-missing"
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL custom-list host not sinkholed: ads.example.com"* ]]
    [[ "$output" == *"FAIL (stage 4)"* ]]
}

@test "a profile that blocks an allowlisted name fails" {
    p="$BATS_TEST_TMPDIR/profile"
    cp -r "$PROFILE" "$p"
    echo "www.netflix.com" >>"$p/required-blocks.txt"
    printf '0.0.0.0\tus.lgtvsdp.com\n' >>"$p/b9-aggressive.hosts"
    run "$CHECK" --profile "$p" --hosts-file "$FIX/hosts-pass" --stage 1
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL profile: www.netflix.com is in required-blocks.txt"* ]]
    [[ "$output" == *"FAIL profile: us.lgtvsdp.com is in b9-aggressive.hosts"* ]]
}

@test "the shipped profile is consistent" {
    hosts="$BATS_TEST_TMPDIR/hosts"
    profile="$BATS_TEST_DIRNAME/../../profiles/glasshouse"
    awk '{ sub(/#.*/, ""); if ($1 != "") print "0.0.0.0\t" $1 }' "$profile/required-blocks.txt" >"$hosts"
    awk '!/^[ \t]*#/ && $1 == "0.0.0.0" { print "0.0.0.0\t" $2 }' "$profile/b9-aggressive.hosts" >>"$hosts"
    # Every research candidate, sinkholed at once, must still be store-safe.
    awk '/^#[ \t]*0\.0\.0\.0[ \t]/ { print "0.0.0.0\t" $3 }' "$profile/b9-aggressive.hosts" >>"$hosts"
    run "$CHECK" --hosts-file "$hosts"
    [ "$status" -eq 0 ]
    [[ "$output" == *"firmware hosts sinkholed: 5 of 5"* ]]
}

@test "bad arguments exit 2" {
    run "$CHECK" --stage 5 --hosts-file "$FIX/hosts-pass"
    [ "$status" -eq 2 ]
    run "$CHECK" --hosts-file /nonexistent
    [ "$status" -eq 2 ]
    run "$CHECK" --profile /nonexistent --hosts-file "$FIX/hosts-pass"
    [ "$status" -eq 2 ]
    run "$CHECK" --frobnicate
    [ "$status" -eq 2 ]
}

# Live mode: the fake tv.sh serves a hosts file for `run` and runs `script`
# files here, with PATH limited to a fake resolver and the tools it needs.
fake_tv() {
    bin="$BATS_TEST_TMPDIR/bin"
    mkdir -p "$bin"
    for t in awk cat; do ln -sf "$(command -v "$t")" "$bin/$t"; done
    cat >"$BATS_TEST_TMPDIR/tv.sh" <<EOF
#!/bin/sh
case "\$1" in
    run) cat "\$FAKE_HOSTS" ;;
    script) sh -n "\$2" || exit 1; PATH="$bin" /bin/sh "\$2" ;;
esac
EOF
    chmod +x "$BATS_TEST_TMPDIR/tv.sh"
    export LUMEN_TV_SH="$BATS_TEST_TMPDIR/tv.sh" FAKE_HOSTS="$FIX/hosts-pass"
}

fake_getent() {
    cat >"$BATS_TEST_TMPDIR/bin/getent" <<'EOF'
#!/bin/sh
case "$2" in
    snu.lge.com | su.lge.com | ads.example.com) echo "0.0.0.0         $2"; echo "::              $2" ;;
    "${FAKE_SILENT:-none}") ;;
    *) echo "${FAKE_ADDR:-192.0.2.9}       $2" ;;
esac
EOF
    chmod +x "$BATS_TEST_TMPDIR/bin/getent"
}

@test "live: hosts and getent lookups pass" {
    fake_tv
    fake_getent
    run "$CHECK" --profile "$PROFILE"
    [ "$status" -eq 0 ]
    [[ "$output" == *"live lookups on the TV (getent):"* ]]
    [[ "$output" == *"ok   allow us.lgtvsdp.com -> 192.0.2.9"* ]]
    [[ "$output" == *"ok   block snu.lge.com -> 0.0.0.0 ::"* ]]
    [[ "$output" == *"ok   block ads.example.com"* ]]
    [[ "$output" == *"PASS (stage 4)"* ]]
}

@test "live: no answer for an allowlisted name fails" {
    fake_tv
    fake_getent
    FAKE_SILENT=www.amazon.com run "$CHECK" --profile "$PROFILE"
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL allow www.amazon.com: no answer"* ]]
}

@test "live: a TV with no working resolver tool fails" {
    fake_tv
    run "$CHECK" --profile "$PROFILE"
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL allow us.lgtvsdp.com: no answer"* ]]
    [[ "$output" == *"FAIL (stage 4)"* ]]
}

@test "live: lookups that never ran fail" {
    fake_tv
    cat >"$BATS_TEST_TMPDIR/tv.sh" <<'EOF'
#!/bin/sh
case "$1" in run) cat "$FAKE_HOSTS" ;; script) exit 0 ;; esac
EOF
    run "$CHECK" --profile "$PROFILE"
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL live lookups did not run on the TV"* ]]
}

@test "a hosts file with CRLF line endings is read correctly" {
    crlf="$BATS_TEST_TMPDIR/hosts-crlf"
    sed 's/$/\r/' "$FIX/hosts-allow-sunk" >"$crlf"
    run "$CHECK" --profile "$PROFILE" --hosts-file "$crlf" --stage 2
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL allowlisted name is sinkholed: api.netflix.com"* ]]
    sed 's/$/\r/' "$FIX/hosts-pass" >"$crlf"
    run "$CHECK" --profile "$PROFILE" --hosts-file "$crlf"
    [ "$status" -eq 0 ]
}

@test "a block that also has a real address does not count" {
    hosts="$BATS_TEST_TMPDIR/hosts-dup"
    { printf '192.0.2.1\tsu.lge.com\n'; cat "$FIX/hosts-pass"; } >"$hosts"
    run "$CHECK" --profile "$PROFILE" --hosts-file "$hosts" --stage 3
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL firmware host not sinkholed: su.lge.com"* ]]
}

@test "any 127.x address is a sinkhole" {
    hosts="$BATS_TEST_TMPDIR/hosts-127"
    { cat "$FIX/hosts-pass"; printf '127.0.0.2\twww.netflix.com\n'; } >"$hosts"
    run "$CHECK" --profile "$PROFILE" --hosts-file "$hosts" --stage 1
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL allowlisted name is sinkholed: www.netflix.com"* ]]
}

@test "live: an allowlisted name that resolves to a sinkhole fails" {
    fake_tv
    fake_getent
    FAKE_ADDR=0.0.0.0 run "$CHECK" --profile "$PROFILE"
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL us.lgtvsdp.com resolves to a sinkhole"* ]]
}

@test "live: busybox nslookup is the fallback" {
    fake_tv
    cat >"$BATS_TEST_TMPDIR/bin/nslookup" <<'EOF'
#!/bin/sh
echo "Server:    127.0.0.1"
echo "Address 1: 127.0.0.1 localhost"
echo ""
echo "Name:      $1"
case "$1" in
    snu.lge.com | su.lge.com | ads.example.com) echo "Address 1: 0.0.0.0" ;;
    *) echo "Address 1: 192.0.2.7 host.example"; echo "Address 2: 2001:db8::7" ;;
esac
EOF
    chmod +x "$BATS_TEST_TMPDIR/bin/nslookup"
    run "$CHECK" --profile "$PROFILE"
    [ "$status" -eq 0 ]
    [[ "$output" == *"live lookups on the TV (nslookup):"* ]]
    [[ "$output" == *"ok   allow plex.tv -> 192.0.2.7 2001:db8::7"* ]]
    [[ "$output" == *"ok   block su.lge.com -> 0.0.0.0"* ]]
}

@test "live: a firmware host that resolves to a real address fails" {
    fake_tv
    cat >"$BATS_TEST_TMPDIR/bin/getent" <<'EOF'
#!/bin/sh
echo "192.0.2.50      $2"
EOF
    chmod +x "$BATS_TEST_TMPDIR/bin/getent"
    FAKE_HOSTS="$FIX/hosts-pass" run "$CHECK" --profile "$PROFILE" --stage 3
    [ "$status" -eq 1 ]
    [[ "$output" == *"FAIL snu.lge.com resolves to 192.0.2.50, not a sinkhole"* ]]
}
