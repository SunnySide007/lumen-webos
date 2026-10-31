#!/usr/bin/env bats
# Checks studio/tv/hdr-check.sh, which runs on the TV, against a stub luna-send.
# Nothing here reaches a TV.

setup() {
    SCRIPT="$BATS_TEST_DIRNAME/../../studio/tv/hdr-check.sh"
    # Root's stage, where tools/tv.sh push puts the loop, and the folder the
    # script copies it to for the player; the script makes the second.
    export LUMEN_HDR_STAGE="$BATS_TEST_TMPDIR/stage/hdr"
    export LUMEN_HDR_DIR="$BATS_TEST_TMPDIR/lumen-hdr"
    export STUB_DIR="$BATS_TEST_TMPDIR/stub"
    mkdir -p "$LUMEN_HDR_STAGE" "$STUB_DIR/bin"

    # luna-send answers like the B9 (docs/device/recon-*.txt): picture settings
    # with a dimension block, which reads HDR once the player has launched.
    # STUB_PLAYER: hdr (default), sdr (plays, stays SDR) or fails (no launch);
    # STUB_BEFORE=hdr starts in HDR; STUB_FRONT names the foreground app.
    cat >"$STUB_DIR/bin/luna-send" <<'EOF'
#!/bin/sh
echo "$*" >>"$STUB_DIR/calls"
# Over SSH luna-send must not read the script's stdin (see tools/tv.sh).
if read -r _; then echo "luna-send read stdin: $*" >>"$STUB_DIR/stdin"; fi
case $* in
    *applicationManager/closeByAppId*)
        rm -f "$STUB_DIR/playing"
        echo '{"returnValue": true}' ;;
    *applicationManager/launch*)
        printf '%s' "$5" >"$STUB_DIR/launch.json"
        if [ "${STUB_PLAYER:-hdr}" = fails ]; then
            echo '{"returnValue": false, "errorText": "stub"}'
        else
            touch "$STUB_DIR/playing"
            echo '{"returnValue": true}'
        fi ;;
    *getForegroundAppInfo*)
        echo "{\"appId\": \"${STUB_FRONT:-com.webos.app.photovideo}\", \"returnValue\": true}" ;;
    *getSystemSettings*)
        if [ "${STUB_BEFORE:-sdr}" = hdr ] ||
            { [ -f "$STUB_DIR/playing" ] && [ "${STUB_PLAYER:-hdr}" = hdr ]; }; then
            mode=hdrCinema; range=hdr
        else
            mode=expert2; range=sdr
        fi
        printf '{\n    "dimension": {\n        "pictureMode": "%s",\n        "dynamicRange": "%s"\n    },\n' "$mode" "$range"
        printf '    "settings": {\n        "pictureMode": "expert2"\n    }\n}\n' ;;
esac
EOF
    printf '#!/bin/sh\n' >"$STUB_DIR/bin/sleep"
    chmod +x "$STUB_DIR/bin/luna-send" "$STUB_DIR/bin/sleep"
    export PATH="$STUB_DIR/bin:$PATH"
}

# As tools/tv.sh script runs it: wrapped in braces, stdin /dev/null, via sh -s.
run_like_tv_sh() {
    run sh -c '{ echo "{"; cat "$1"; printf "\n} </dev/null\n"; } | sh -s' _ "$SCRIPT"
}

@test "an HDR switch is reported" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mp4"
    run_like_tv_sh
    [ "$status" -eq 0 ]
    [[ "$output" == *"before: pictureMode=expert2 dynamicRange=sdr"* ]]
    [[ "$output" == *"playing: pictureMode=hdrCinema dynamicRange=hdr"* ]]
    [[ "$output" == *"RESULT: HDR (hdr)"* ]]
    [ ! -f "$STUB_DIR/stdin" ]
}

@test "a player left open is closed before the test" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mp4"
    run_like_tv_sh
    head -n 1 "$STUB_DIR/calls" | grep -q "closeByAppId.*com.webos.app.photovideo"
}

@test "the player gets the loop as a file URL" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mp4"
    run_like_tv_sh
    grep -qF "\"id\":\"com.webos.app.photovideo\"" "$STUB_DIR/calls"
    grep -qF "\"fullPath\":\"file://$LUMEN_HDR_DIR/loop.mp4\"" "$STUB_DIR/calls"
}

@test "the launch payload is valid JSON" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mkv"
    run_like_tv_sh
    run python3 -c 'import json, sys; p = json.load(open(sys.argv[1])); e = p["params"]["payload"][0]; print(p["id"], e["fullPath"], e["dlnaInfo"]["protocolInfo"].split(":")[2])' "$STUB_DIR/launch.json"
    [ "$status" -eq 0 ]
    [ "$output" = "com.webos.app.photovideo file://$LUMEN_HDR_DIR/loop.mkv video/x-matroska" ]
}

@test "staying in SDR fails, after polling" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mp4"
    STUB_PLAYER=sdr run_like_tv_sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"RESULT: still SDR (dynamicRange=sdr) after 30 s"* ]]
    [ "$(grep -c getSystemSettings "$STUB_DIR/calls")" -gt 10 ]
}

@test "a launch that fails is not reported as SDR" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mp4"
    STUB_PLAYER=fails run_like_tv_sh
    [ "$status" -eq 2 ]
    [[ "$output" == *"RESULT: the player did not start"* ]]
}

@test "another app in front is not reported as SDR" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mp4"
    STUB_PLAYER=sdr STUB_FRONT=com.webos.app.home run_like_tv_sh
    [ "$status" -eq 2 ]
    [[ "$output" == *"RESULT: the player is not in front (com.webos.app.home)"* ]]
}

@test "HDR before the player starts is inconclusive" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mp4"
    STUB_BEFORE=hdr run_like_tv_sh
    [ "$status" -eq 3 ]
    [[ "$output" == *"RESULT: inconclusive"* ]]
    ! grep -q "applicationManager/launch" "$STUB_DIR/calls"
}

@test "an mkv loop is used when there is no mp4" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mkv"
    run_like_tv_sh
    [ "$status" -eq 0 ]
    grep -qF "file://$LUMEN_HDR_DIR/loop.mkv" "$STUB_DIR/calls"
}

@test "a missing loop is reported before anything runs" {
    run_like_tv_sh
    [ "$status" -ne 0 ]
    [[ "$output" == *"no loop in $LUMEN_HDR_STAGE"* ]]
    [ ! -f "$STUB_DIR/calls" ]
}

@test "the player gets a copy that anyone may read, outside the stage" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mp4"
    chmod 0600 "$LUMEN_HDR_STAGE/loop.mp4"
    run_like_tv_sh
    [ "$status" -eq 0 ]
    [ "$(cat "$LUMEN_HDR_DIR/loop.mp4")" = video ]
    [[ "$(ls -ln "$LUMEN_HDR_DIR/loop.mp4")" == -rw-r--r--* ]]
    [[ "$(ls -ldn "$LUMEN_HDR_DIR")" == drwxr-xr-x* ]]
    [[ "$output" == *"copied from $LUMEN_HDR_STAGE/loop.mp4"* ]]
}

@test "an earlier copy of the other kind is removed" {
    mkdir -p "$LUMEN_HDR_DIR"
    printf 'old' >"$LUMEN_HDR_DIR/loop.mp4"
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mkv"
    run_like_tv_sh
    [ "$status" -eq 0 ]
    [ ! -e "$LUMEN_HDR_DIR/loop.mp4" ]
    grep -qF "file://$LUMEN_HDR_DIR/loop.mkv" "$STUB_DIR/calls"
}

@test "a play folder others may write is refused" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mp4"
    mkdir -p "$LUMEN_HDR_DIR"
    chmod 0777 "$LUMEN_HDR_DIR"
    run_like_tv_sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"writable by others"* ]]
    [ ! -e "$LUMEN_HDR_DIR/loop.mp4" ]
    [ ! -f "$STUB_DIR/calls" ]
}

@test "a play folder that is a link is refused" {
    printf 'video' >"$LUMEN_HDR_STAGE/loop.mp4"
    mkdir -p "$BATS_TEST_TMPDIR/elsewhere"
    ln -s "$BATS_TEST_TMPDIR/elsewhere" "$LUMEN_HDR_DIR"
    run_like_tv_sh
    [ "$status" -eq 1 ]
    [[ "$output" == *"is not a real folder"* ]]
    [ ! -e "$BATS_TEST_TMPDIR/elsewhere/loop.mp4" ]
    [ ! -f "$STUB_DIR/calls" ]
}

@test "the script is shellcheck-clean" {
    shellcheck "$SCRIPT"
}
