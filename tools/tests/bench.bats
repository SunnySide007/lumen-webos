#!/usr/bin/env bats
# tools/bench.sh's lab window rules and its refusals, against a fake
# tools/tv.sh that answers the window step with canned values, and the
# window step itself (tools/bench/tv/window.sh) run here against a fake log.
# Nothing reaches a TV.

setup() {
    BENCH="$BATS_TEST_DIRNAME/../bench.sh"
    TMP=$(mktemp -d)
    export LUMEN_TV_SH="$TMP/tv.sh"
    export FAKE_WINDOW="$TMP/window.txt"
    cat >"$LUMEN_TV_SH" <<'EOF'
#!/bin/sh
case $1 in
    script) cat "$FAKE_WINDOW" ;;
    *) exit 1 ;;
esac
EOF
    chmod +x "$LUMEN_TV_SH"
}

teardown() {
    rm -rf "$TMP"
}

answer() {
    # now hour fg power last under
    printf 'now=%s\nhour=%s\nfg=%s\npower=%s\npid=1\nlast=%s\nunder=%s\n' "$@" >"$FAKE_WINDOW"
}

APP=io.github.sunnyside007.lumen.shell
SAVER=com.webos.app.screensaver

@test "Lumen in front and 20 minutes idle in the day is a window" {
    answer 10000 14 "$APP" Active 8800 "$APP"
    run sh "$BENCH" window
    [ "$status" -eq 0 ]
    [ "$output" = "open" ]
}

@test "less than 20 minutes idle is not" {
    answer 10000 14 "$APP" Active 8801 "$APP"
    run sh "$BENCH" window
    [ "$status" -eq 1 ]
    [ "$output" = "closed: idle 1199 s of 1200" ]
}

@test "the screen stays dark from midnight to six" {
    for h in 00 03 05; do
        answer 10000 "$h" "$APP" Active 1 "$APP"
        run sh "$BENCH" window
        [ "$status" -eq 1 ]
        [[ "$output" == "closed: night ($h h TV time)"* ]]
    done
    answer 10000 06 "$APP" Active 1 "$APP"
    run sh "$BENCH" window
    [ "$status" -eq 0 ]
}

@test "another app in front is not a window" {
    answer 10000 14 netflix Active 1 netflix
    run sh "$BENCH" window
    [ "$status" -eq 1 ]
    [ "$output" = "closed: netflix is in front" ]
}

@test "LG's screensaver counts only over Lumen" {
    answer 10000 14 "$SAVER" "Screen Saver" 1 "$APP"
    run sh "$BENCH" window
    [ "$status" -eq 0 ]
    answer 10000 14 "$SAVER" "Screen Saver" 1 com.webos.app.hdmi3
    run sh "$BENCH" window
    [ "$status" -eq 1 ]
    [[ "$output" == *"over com.webos.app.hdmi3, not Lumen" ]]
}

@test "a screen that is off, or no record of input, is not a window" {
    answer 10000 14 "$APP" "Screen Off" 1 "$APP"
    run sh "$BENCH" window
    [ "$output" = "closed: power state Screen Off" ]
    answer 10000 14 "$APP" Active "" "$APP"
    run sh "$BENCH" window
    [ "$output" = "closed: no record of the last input" ]
}

@test "a TV that does not answer is not a window" {
    : >"$FAKE_WINDOW"
    run sh "$BENCH" window
    [ "$status" -eq 1 ]
    [ "$output" = "closed: the TV does not answer" ]
}

@test "run refuses bad options before touching the TV" {
    run sh "$BENCH" run --runs 0
    [ "$status" -eq 1 ]
    [[ "$output" == *"--runs must be 1 to 9"* ]]
    run sh "$BENCH" run --name 'a b'
    [[ "$output" == *"--name"* ]]
    run sh "$BENCH" run --ipk /nonexistent.ipk
    [[ "$output" == *"no such IPK"* ]]
    run sh "$BENCH" frobnicate
    [[ "$output" == *"unknown command"* ]]
}

# The TV-side step, run here: lib.sh's lumen_dir finds the log in /tmp, so
# the step is given a copy that reads from a folder of the test's own.
window_step() {
    mkdir -p "$TMP/stage"
    sed -e "s|/var/lib/lumen/stage|$TMP/stage|g" \
        "$BATS_TEST_DIRNAME/../bench/tv/window.sh" >"$TMP/window-step.sh"
    {
        echo "lumen_dir() { echo '$TMP'; }"
        echo "luna() { :; }"
        echo "field() { cat >/dev/null; }"
        echo "pidof() { echo 1; }"
        cat "$TMP/window-step.sh"
    } >"$TMP/run.sh"
    sh "$TMP/run.sh"
}

@test "the window step finds the last person, skipping the bench's own runs" {
    cat >"$TMP/lumen-shell.log" <<EOF
[1000.100] key down scancode 79 keycode 0x0 (Some(Move(Right)))
[1100.000] front app $APP: last input 0.1 s ago
[1200.000] front app $SAVER: last input 120.0 s ago
[1250.000] power Screen Saver: last input 150.0 s ago
[2000.500] key down scancode 81 keycode 0x0 (Some(Move(Down)))
[2010.000] front app $APP: last input 0.0 s ago
EOF
    run window_step
    [[ "$output" == *"last=2010"* ]]
    [[ "$output" == *"under=$APP"* ]]
    # The bench ran from 1990 to 2020: its key and launch are not a person.
    echo "1990 2020" >"$TMP/stage/bench-ranges"
    run window_step
    [[ "$output" == *"last=1100"* ]]
    [[ "$output" == *"under=$APP"* ]]
}
