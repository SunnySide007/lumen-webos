#!/usr/bin/env bats
# Checks studio/tv/hdr-check.sh, which runs on the TV, against a stub luna-send.
# Nothing here reaches a TV.

setup() {
    SCRIPT="$BATS_TEST_DIRNAME/../../studio/tv/hdr-check.sh"
    export LUMEN_HDR_DIR="$BATS_TEST_TMPDIR/lumen-hdr"
    export STUB_DIR="$BATS_TEST_TMPDIR/stub"
    mkdir -p "$LUMEN_HDR_DIR" "$STUB_DIR/bin"

    # luna-send answers like the B9 (docs/device/recon-*.txt): picture settings
    # with a dimension block, which reads HDR once the player has launched.
    cat >"$STUB_DIR/bin/luna-send" <<'EOF'
#!/bin/sh
echo "$*" >>"$STUB_DIR/calls"
case $* in
    *applicationManager/launch*)
        printf '%s' "$5" >"$STUB_DIR/launch.json"
        echo '{"returnValue": true}'
        [ "${STUB_PLAYER:-hdr}" = fails ] || touch "$STUB_DIR/playing" ;;
    *getForegroundAppInfo*)
        echo '{"appId": "com.webos.app.photovideo", "returnValue": true}' ;;
    *getSystemSettings*)
        if [ -f "$STUB_DIR/playing" ] && [ "${STUB_PLAYER:-hdr}" = hdr ]; then
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

@test "an HDR switch is reported" {
    printf 'video' >"$LUMEN_HDR_DIR/loop.mp4"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [[ "$output" == *"before: pictureMode=expert2 dynamicRange=sdr"* ]]
    [[ "$output" == *"playing: pictureMode=hdrCinema dynamicRange=hdr"* ]]
    [[ "$output" == *"RESULT: HDR (hdr)"* ]]
}

@test "the player gets the loop as a file URL" {
    printf 'video' >"$LUMEN_HDR_DIR/loop.mp4"
    run sh "$SCRIPT"
    grep -qF "\"id\":\"com.webos.app.photovideo\"" "$STUB_DIR/calls"
    grep -qF "\"fullPath\":\"file://$LUMEN_HDR_DIR/loop.mp4\"" "$STUB_DIR/calls"
}

@test "the launch payload is valid JSON" {
    printf 'video' >"$LUMEN_HDR_DIR/loop.mkv"
    run sh "$SCRIPT"
    run python3 -c 'import json, sys; p = json.load(open(sys.argv[1])); e = p["params"]["payload"][0]; print(p["id"], e["fullPath"], e["dlnaInfo"]["protocolInfo"].split(":")[2])' "$STUB_DIR/launch.json"
    [ "$status" -eq 0 ]
    [ "$output" = "com.webos.app.photovideo file://$LUMEN_HDR_DIR/loop.mkv video/x-matroska" ]
}

@test "staying in SDR fails" {
    printf 'video' >"$LUMEN_HDR_DIR/loop.mp4"
    STUB_PLAYER=sdr run sh "$SCRIPT"
    [ "$status" -ne 0 ]
    [[ "$output" == *"RESULT: still SDR (dynamicRange=sdr)"* ]]
}

@test "an mkv loop is used when there is no mp4" {
    printf 'video' >"$LUMEN_HDR_DIR/loop.mkv"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    grep -qF "file://$LUMEN_HDR_DIR/loop.mkv" "$STUB_DIR/calls"
}

@test "a missing loop is reported before anything launches" {
    run sh "$SCRIPT"
    [ "$status" -ne 0 ]
    [[ "$output" == *"no loop in $LUMEN_HDR_DIR"* ]]
    [ ! -f "$STUB_DIR/calls" ]
}

@test "the script is shellcheck-clean" {
    shellcheck "$SCRIPT"
}
