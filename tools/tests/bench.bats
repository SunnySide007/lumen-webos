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
    export LUMEN_BENCH_LOCK="$TMP/tv.lock"
    export LUMEN_BENCH_PERSON="$TMP/person-at"
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
    # now hour fg power last under [dow, 1 = Monday]
    printf 'now=%s\nhour=%s\nfg=%s\npower=%s\npid=1\nlast=%s\nunder=%s\ndow=%s\n' "$@" >"$FAKE_WINDOW"
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

@test "in the owner's weekday slot an HDMI input in front is a window" {
    HDMI=com.webos.app.hdmi3
    # Monday to Friday, 10:00 to 15:00, 20 minutes idle.
    for d in 1 5; do
        for h in 10 14; do
            answer 10000 "$h" "$HDMI" Active 8800 "$HDMI" "$d"
            run sh "$BENCH" window
            [ "$status" -eq 0 ]
        done
    done
    # LG's screensaver over the input counts too.
    answer 10000 12 "$SAVER" "Screen Saver" 8800 "$HDMI" 3
    run sh "$BENCH" window
    [ "$status" -eq 0 ]
    # Still 20 minutes idle.
    answer 10000 12 "$HDMI" Active 9500 "$HDMI" 3
    run sh "$BENCH" window
    [ "$output" = "closed: idle 500 s of 1200" ]
}

@test "outside the slot, or with an app in front, the slot opens nothing" {
    HDMI=com.webos.app.hdmi3
    # Before 10:00, from 15:00, and at the weekend.
    for h in 09 15; do
        answer 10000 "$h" "$HDMI" Active 8800 "$HDMI" 3
        run sh "$BENCH" window
        [ "$output" = "closed: $HDMI is in front" ]
    done
    for d in 6 7; do
        answer 10000 12 "$HDMI" Active 8800 "$HDMI" "$d"
        run sh "$BENCH" window
        [ "$output" = "closed: $HDMI is in front" ]
    done
    # An app is somebody watching: never, slot or not.
    answer 10000 12 netflix Active 8800 netflix 3
    run sh "$BENCH" window
    [ "$output" = "closed: netflix is in front" ]
    answer 10000 12 "$SAVER" "Screen Saver" 8800 netflix 3
    run sh "$BENCH" window
    [[ "$output" == *"over netflix, not Lumen" ]]
    # The night window knows no slot.
    answer 10000 03 "$HDMI" Active 8800 "$HDMI" 3
    run sh "$BENCH" window --night
    [ "$output" = "closed: $HDMI is in front" ]
    # A TV whose answer has no weekday keeps the old rule.
    answer 10000 12 "$HDMI" Active 8800 "$HDMI"
    run sh "$BENCH" window
    [ "$output" = "closed: $HDMI is in front" ]
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

@test "the night window is for dark work only, with nothing else in front" {
    answer 10000 03 "$APP" "Screen Off" 8800 "$APP"
    run sh "$BENCH" window --night
    [ "$status" -eq 0 ]
    answer 10000 03 "" "Active Standby" "" ""
    run sh "$BENCH" window --night
    [ "$status" -eq 0 ]
    answer 10000 03 com.webos.app.hdmi3 Active 1 com.webos.app.hdmi3
    run sh "$BENCH" window --night
    [ "$output" = "closed: com.webos.app.hdmi3 is in front" ]
    answer 10000 03 "$APP" Active 9500 "$APP"
    run sh "$BENCH" window --night
    [ "$output" = "closed: idle 500 s of 1200" ]
    answer 10000 14 "$APP" Active 1 "$APP"
    run sh "$BENCH" window --night
    [[ "$output" == "closed: not night (14 h TV time)"* ]]
}

@test "run refuses bad options before touching the TV" {
    run sh "$BENCH" run --runs 0
    [ "$status" -eq 1 ]
    [[ "$output" == *"--runs must be 1 to 9"* ]]
    run sh "$BENCH" run --name 'a b'
    [[ "$output" == *"--name"* ]]
    run sh "$BENCH" run --ipk /nonexistent.ipk
    [[ "$output" == *"no such IPK"* ]]
    run sh "$BENCH" run --pattern /nonexistent.txt
    [[ "$output" == *"no such pattern"* ]]
    run sh "$BENCH" frobnicate
    [[ "$output" == *"unknown command"* ]]
}

@test "lean-try refuses a job that is not a candidate, or matches never-touch, before the TV" {
    mkdir -p "$TMP/lean"
    printf '# test\nvoiced  # voice, unused\naudiovoice # matches never-touch\n' >"$TMP/lean/candidates.txt"
    printf '# test\naudio\nlumen\n' >"$TMP/lean/never-touch.txt"
    export LUMEN_LEAN_PROFILE="$TMP/lean"
    run sh "$BENCH" lean-try tipsd
    [ "$status" -eq 1 ]
    [[ "$output" == *"tipsd is not in profiles/lean/candidates.txt"* ]]
    run sh "$BENCH" lean-try audiovoice
    [[ "$output" == *"matches never-touch \"audio\""* ]]
    run sh "$BENCH" lean-try 'voiced;reboot'
    [[ "$output" == *"not a plain job name"* ]]
    # A candidate passes the lists and then needs a recent backup.
    LUMEN_BACKUP_DIR="$TMP/no-backups" run sh "$BENCH" lean-try voiced
    [[ "$output" == *"no backup in the last 24 h"* ]]
}

@test "lean-plan is a dry run over the candidates" {
    mkdir -p "$TMP/lean"
    printf 'voiced  # voice, unused\naudiovoice # x\n' >"$TMP/lean/candidates.txt"
    printf 'audio\n' >"$TMP/lean/never-touch.txt"
    export LUMEN_LEAN_PROFILE="$TMP/lean"
    printf '===== jobs (name state pid rss_kB)\nvoiced start/running 812 23400\n===== processes\n' >"$TMP/survey.txt"
    run sh "$BENCH" lean-plan "$TMP/survey.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"would stop voiced (voiced start/running 812 23400)"* ]]
    [[ "$output" == *"REFUSED audiovoice"* ]]
    : >"$TMP/lean/candidates.txt"
    run sh "$BENCH" lean-plan "$TMP/survey.txt"
    [[ "$output" == *"no candidates yet"* ]]
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

@test "a person who stopped a run holds the window shut for 20 minutes" {
    answer 10000 14 "$APP" Active 8800 "$APP"
    echo 9500 >"$LUMEN_BENCH_PERSON"
    run sh "$BENCH" window
    [ "$status" -eq 1 ]
    [[ "$output" == *"a person stopped a run 500 s ago"* ]]
    echo 8700 >"$LUMEN_BENCH_PERSON"
    run sh "$BENCH" window
    [ "$status" -eq 0 ]
}

@test "with-lock runs its command and passes on its status" {
    run sh "$BENCH" with-lock sh -c 'cat "$LUMEN_BENCH_LOCK/pid" >/dev/null && exit 7'
    [ "$status" -eq 7 ]
}

@test "a lock whose process has gone is taken over" {
    mkdir "$LUMEN_BENCH_LOCK"
    echo 999999 >"$LUMEN_BENCH_LOCK/pid"
    run sh "$BENCH" with-lock true
    [ "$status" -eq 0 ]
    [ "$(cat "$LUMEN_BENCH_LOCK/pid")" != 999999 ]
}
