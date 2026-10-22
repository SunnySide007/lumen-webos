#!/usr/bin/env bats
# tools/frame-stats.sh against fixture logs in the formats hello-gles and the
# shell write. The expected numbers were worked out by hand from the fixtures.
# Nothing reaches a TV.

setup() {
    STATS="$BATS_TEST_DIRNAME/../frame-stats.sh"
    FIX="$BATS_TEST_DIRNAME/fixtures/frame-stats"
}

@test "help lists the options" {
    run "$STATS" help
    [ "$status" -eq 0 ]
    [[ "$output" == *"--since EPOCH"* ]]
    [[ "$output" == *"--miss-ms MS"* ]]
    [[ "$output" != *"set -eu"* ]]
}

@test "a steady hello-gles run averages 60 fps once the warm-up window is cut" {
    run "$STATS" --since 1760000002 "$FIX/hello-gles.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"windows (fps lines): 10, about 600 frames, 10.0 s"* ]]
    [[ "$output" == *"average frame    16.67 ms (60.0 fps)"* ]]
    [[ "$output" == *"worst frame      33.30 ms"* ]]
    [[ "$output" == *"window worst     p50 18.00  p95 33.30  p99 33.30 ms"* ]]
    [[ "$output" == *"window average   p50 16.67  p95 16.69  p99 16.69 ms"* ]]
    [[ "$output" == *"missed vsync     1 of 10 windows had a frame over 25 ms"* ]]
    [[ "$output" == *"quads            3000"* ]]
    [[ "$output" != *"bursts"* ]]
}

@test "the whole log counts the warm-up window too" {
    run "$STATS" "$FIX/hello-gles.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"windows (fps lines): 11, about 655 frames, 11.0 s"* ]]
    [[ "$output" == *"average frame    16.79 ms (59.5 fps)"* ]]
    [[ "$output" == *"worst frame      48.00 ms"* ]]
    [[ "$output" == *"missed vsync     2 of 11 windows"* ]]
}

@test "--until cuts the end, and the log can come from standard input" {
    run "$STATS" --since 1760000002 --until 1760000006.5 < "$FIX/hello-gles.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"windows (fps lines): 5, about 300 frames, 5.0 s"* ]]
    [[ "$output" == *"worst frame      19.00 ms"* ]]
    [[ "$output" == *"missed vsync     0 of 5 windows"* ]]
}

@test "--miss-ms moves the line a frame must cross to count as missed" {
    run "$STATS" --since 1760000002 --miss-ms 17.5 "$FIX/hello-gles.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"missed vsync     6 of 10 windows had a frame over 17.5 ms"* ]]
}

@test "the shell's windows are weighted by their frame counts" {
    run "$STATS" "$FIX/shell.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"windows (fps lines): 2, 118 frames, 2.0 s"* ]]
    [[ "$output" == *"average frame    16.95 ms (59.0 fps)"* ]]
    [[ "$output" == *"window worst     p50 17.00  p95 33.30  p99 33.30 ms"* ]]
    [[ "$output" == *"window average   p50 16.67  p95 17.24  p99 17.24 ms"* ]]
    [[ "$output" == *"quads            1200 to 1631"* ]]
}

@test "bursts are weighted by their frame gaps, and a one-frame burst is skipped" {
    run "$STATS" "$FIX/shell.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"bursts (drew ... idle): 3, 64 frames, 1.1 s"* ]]
    [[ "$output" == *"average frame    17.76 ms (56.3 fps)"* ]]
    [[ "$output" == *"burst worst      p50 17.00  p95 33.30  p99 33.30 ms"* ]]
    [[ "$output" == *"burst average    p50 16.67  p95 20.00  p99 20.00 ms"* ]]
    [[ "$output" == *"missed vsync     1 of 3 bursts had a frame over 25 ms"* ]]
    [[ "$output" == *"quads            856 to 1631"* ]]
}

@test "SDL repeat lines give the first-repeat delay and the gap after it" {
    run "$STATS" "$FIX/shell.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"key repeats (SDL repeat lines): 6 lines from 2 holds"* ]]
    [[ "$output" == *"first repeat     p50 500 ms after the press (500 to 520)"* ]]
    [[ "$output" == *"gap after that   p50 33 ms (33 to 34)"* ]]
}

@test "a period inside the shell log keeps only the lines in it" {
    run "$STATS" --since 1760000103 --until 1760000104.5 "$FIX/shell.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"windows (fps lines): 1, 58 frames"* ]]
    [[ "$output" == *"bursts (drew ... idle): 1, 21 frames"* ]]
    [[ "$output" != *"key repeats"* ]]
}

@test "the note says percentiles are over windows, not frames" {
    run "$STATS" "$FIX/hello-gles.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"percentiles are over those, not over single frames"* ]]
}

@test "carriage returns from the SSH stream do not matter" {
    log="$BATS_TEST_TMPDIR/crlf.log"
    printf '[1760000002.2] fps 60.0 avg 16.67 ms max 17.00 ms (3000 quads)\r\n' > "$log"
    printf '[1760000003.2] fps 60.0 avg 16.67 ms max 18.00 ms (3000 quads)\r\n' >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    [[ "$output" == *"windows (fps lines): 2, about 120 frames"* ]]
    [[ "$output" == *"quads            3000"* ]]
}

@test "a period with no frame statistics fails with a message" {
    run "$STATS" --since 1770000000 "$FIX/hello-gles.txt"
    [ "$status" -eq 1 ]
    [[ "$output" == *"no frame statistics in the log for that period"* ]]
}

@test "lines without the app's timestamp are not counted" {
    log="$BATS_TEST_TMPDIR/bare.log"
    echo 'fps 60.0 avg 16.67 ms max 17.00 ms (3000 quads)' > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"no frame statistics"* ]]
}

@test "bad arguments are refused" {
    run "$STATS" --frobnicate
    [ "$status" -eq 2 ]
    [[ "$output" == *"unknown option: --frobnicate"* ]]

    run "$STATS" --since soon "$FIX/hello-gles.txt"
    [ "$status" -eq 2 ]
    [[ "$output" == *"--since needs a number, not 'soon'"* ]]

    run "$STATS" --until
    [ "$status" -eq 2 ]
    [[ "$output" == *"--until needs a Unix time"* ]]

    run "$STATS" /nonexistent.log
    [ "$status" -eq 2 ]
    [[ "$output" == *"no such file: /nonexistent.log"* ]]

    run "$STATS" "$FIX/hello-gles.txt" "$FIX/shell.txt"
    [ "$status" -eq 2 ]
    [[ "$output" == *"one log at a time"* ]]
}

# The timing lines the shell writes (docs/specs/look-and-feel.md, "Log lines").
# timing.txt holds two runs: one before 1792635900 with no frame statistics,
# and one after it. The numbers below were worked out by hand from it.

@test "help lists the timing lines" {
    run "$STATS" help
    [ "$status" -eq 0 ]
    [[ "$output" == *"shown after"* ]]
    [[ "$output" == *"launch <id>"* ]]
    [[ "$output" == *"window focus lost"* ]]
}

@test "key presses give p50, p95, max and the share within 33 ms" {
    run "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"key presses (key ...: lines): 10 shown, 4 not shown, 7 more not timed"* ]]
    [[ "$output" == *"shown after      p50 17  p95 342  max 342 ms"* ]]
    [[ "$output" == *"target 2         9 of 10 within 33 ms"* ]]
}

@test "a launch suffix is counted, and the number stays the shown-after time" {
    run "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    # 342 ms is the shown-after time; the 320 ms in launch is not added to it.
    [[ "$output" == *"max 342 ms"* ]]
    [[ "$output" == *"in a launch      1 of 10 had launch time, up to 320 ms"* ]]
}

@test "presses not timed are summed over the keys: lines" {
    run "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    # keys: 4 and keys: 3
    [[ "$output" == *"7 more not timed"* ]]
}

@test "presses not shown are counted by reason, with or without a launch suffix" {
    run "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"not shown        1 nothing to draw"* ]]
    [[ "$output" == *"not shown        1 the shell is leaving"* ]]
    [[ "$output" == *"not shown        2 the window lost focus"* ]]
}

@test "the time of a press that was not shown stays out of the percentiles" {
    log="$BATS_TEST_TMPDIR/notshown.log"
    echo '[1792636000.000] key 79: not shown after 5000 ms: nothing to draw' > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"key presses (key ...: lines): 0 shown, 1 not shown, 0 more not timed"* ]]
    [[ "$output" == *"not shown        1 nothing to draw"* ]]
    [[ "$output" != *"p50"* ]]
    [[ "$output" != *"within 33 ms"* ]]
}

@test "33 ms and 100 ms are inside the targets, 34 and 101 are not" {
    log="$BATS_TEST_TMPDIR/edge.log"
    {
        echo '[1792636000.000] key 79: shown after 33 ms'
        echo '[1792636000.100] key 79: shown after 34 ms'
        echo '[1792636000.200] launch a.b: answered in 100 ms'
        echo '[1792636000.300] launch c.d: answered in 101 ms'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"target 2         1 of 2 within 33 ms"* ]]
    [[ "$output" == *"target 6         1 of 2 within 100 ms"* ]]
}

@test "each start line is split into the time its phases took" {
    run "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"start-ups (start: lines): 2"* ]]
    [[ "$output" == *"  sdl 352 ms, gl 28 ms, renderer 643 ms, assets 380 ms, apps 50 ms, first frame 51 ms (1504 ms in all)"* ]]
    [[ "$output" == *"  sdl 340 ms, gl 26 ms, renderer 334 ms, assets 300 ms, apps 40 ms; no first frame: the app went to the background (1040 ms to the apps)"* ]]
}

@test "foreground gives the count, p50 and max, and the reasons it was not shown" {
    run "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"foreground (foreground: lines): 3 shown, 1 not shown"* ]]
    [[ "$output" == *"first frame      p50 140  max 300 ms"* ]]
    [[ "$output" == *"not shown        1 the app went to the background"* ]]
}

@test "launches list each id and time, and a failed launch is never within 100 ms" {
    run "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"launches (launch <id>: lines): 4, 1 failed"* ]]
    [[ "$output" == *"  com.example.missing: failed after 12 ms: Permission denied"* ]]
    [[ "$output" == *"  com.webos.app.hdmi1: answered in 45 ms"* ]]
    [[ "$output" == *"  io.github.sunnyside007.lumen.shell: answered in 100 ms"* ]]
    [[ "$output" == *"  com.webos.app.browser: answered in 320 ms"* ]]
    # 45 and 100 are within; 320 is not; the 12 ms failure is not an answer.
    [[ "$output" == *"target 6         2 of 4 within 100 ms"* ]]
}

@test "focus losses count the ones with no key pressed, none yet or over 5 s before" {
    run "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    # no key yet, 12.3 s, 5.0 s and 1.2 s: only the first two count; 5.0 is not over 5.
    [[ "$output" == *"focus losses (window focus lost lines): 4"* ]]
    [[ "$output" == *"no key pressed   2 of 4 (no key yet, or the last key over 5 s before)"* ]]
}

@test "the timing sections follow the old output, in order, after the note" {
    run "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    order=$(printf '%s\n' "$output" |
        grep -o -E '^(windows|bursts|key repeats|note:|key presses|start-ups|foreground|launches|focus losses)' |
        tr '\n' ',')
    [ "$order" = "windows,bursts,key repeats,note:,key presses,start-ups,foreground,launches,focus losses," ]
    # What it printed before the timing lines is unchanged.
    [[ "$output" == *"windows (fps lines): 2, 118 frames, 2.0 s"* ]]
    [[ "$output" == *"bursts (drew ... idle): 1, 40 frames, 0.7 s"* ]]
    [[ "$output" == *"key repeats (SDL repeat lines): 1 lines from 1 holds"* ]]
}

@test "key down, luna and near-miss lines are not mistaken for timings" {
    for fix in shell.txt hello-gles.txt; do
        run "$STATS" "$FIX/$fix"
        [ "$status" -eq 0 ]
        for head in "key presses" "start-ups" "foreground (" "launches" "focus losses"; do
            [[ "$output" != *"$head"* ]]
        done
    done
    log="$BATS_TEST_TMPDIR/near.log"
    {
        echo '[1792636000.000] fps 60.0 avg 16.67 ms max 17.00 ms (60 frames, 1200 quads)'
        echo '[1792636000.100] key down scancode 79 keycode 0x4000004f (RIGHT)'
        echo '[1792636000.200] luna launch: 38 ms'
        echo '[1792636000.300] luna listApps: 51 ms'
        echo '[1792636000.400] launched com.webos.app.hdmi1'
        echo '[1792636000.500] key 79 shown after 12 ms'
        echo '[1792636000.600] key 79: shown after soon ms'
        echo '[1792636000.700] key down: shown after 12 ms'
        echo '[1792636000.800] launch: answered in 5 ms'
        echo '[1792636000.900] launch x: answered in soon ms'
        echo '[1792636001.000] start: sdl 5 apps 7'
        echo '[1792636001.100] foreground: first frame after soon ms'
        echo '[1792636001.200] keys: many more presses not timed'
        echo '[1792636001.300] window focus lost'
        echo '[1792636001.400] window focus lost: soon'
        echo 'key 79: shown after 12 ms'
        echo 'launch a.b: answered in 5 ms'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    [[ "$output" == *"windows (fps lines): 1"* ]]
    for head in "key presses" "start-ups" "foreground (" "launches" "focus losses"; do
        [[ "$output" != *"$head"* ]]
    done
}

@test "--since and --until cut the timing lines too" {
    # The second run only: no first start, no failed launch, no early focus loss.
    run "$STATS" --since 1792635900 "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"start-ups (start: lines): 1"* ]]
    [[ "$output" != *"no first frame"* ]]
    [[ "$output" == *"foreground (foreground: lines): 2 shown, 0 not shown"* ]]
    [[ "$output" == *"first frame      p50 140  max 300 ms"* ]]
    [[ "$output" == *"launches (launch <id>: lines): 3, 0 failed"* ]]
    [[ "$output" == *"target 6         2 of 3 within 100 ms"* ]]
    [[ "$output" == *"focus losses (window focus lost lines): 3"* ]]
    [[ "$output" == *"no key pressed   1 of 3 "* ]]

    # A stretch inside it: four presses, with the first and last stamps counted.
    run "$STATS" --since 1792635960 --until 1792635963.9 "$FIX/timing.txt"
    [ "$status" -eq 1 ]
    [[ "$output" == *"4 shown, 0 not shown, 4 more not timed"* ]]
    [[ "$output" == *"shown after      p50 15  p95 31  max 31 ms"* ]]
    [[ "$output" == *"target 2         4 of 4 within 33 ms"* ]]

    run "$STATS" --since 1792635960 --until 1792635960 "$FIX/timing.txt"
    [[ "$output" == *"1 shown, 0 not shown, 0 more not timed"* ]]
    [[ "$output" == *"shown after      p50 17  p95 17  max 17 ms"* ]]
}

@test "a stretch with frames and timings prints both" {
    run "$STATS" --since 1792635951 --until 1792635953.5 "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"windows (fps lines): 2, 118 frames"* ]]
    [[ "$output" == *"start-ups (start: lines): 1"* ]]
    [[ "$output" == *"foreground (foreground: lines): 1 shown, 0 not shown"* ]]
    [[ "$output" == *"first frame      p50 140  max 140 ms"* ]]
    [[ "$output" != *"key presses"* ]]
    [[ "$output" != *"launches"* ]]
    [[ "$output" != *"focus losses"* ]]
}

@test "timings without frame statistics are printed, and the status is still 1" {
    run "$STATS" --until 1792635900 "$FIX/timing.txt"
    [ "$status" -eq 1 ]
    [[ "$output" == *"start-ups (start: lines): 1"* ]]
    [[ "$output" == *"no first frame: the app went to the background (1040 ms to the apps)"* ]]
    [[ "$output" == *"foreground (foreground: lines): 1 shown, 1 not shown"* ]]
    [[ "$output" == *"launches (launch <id>: lines): 1, 1 failed"* ]]
    [[ "$output" == *"target 6         0 of 1 within 100 ms"* ]]
    [[ "$output" == *"focus losses (window focus lost lines): 1"* ]]
    [[ "$output" == *"no key pressed   1 of 1 "* ]]
    # The message comes after the sections, and the note about windows is not shown.
    [[ "$output" == *"focus losses"*"no frame statistics in the log for that period"* ]]
    [[ "$output" != *"note:"* ]]
    [[ "$output" != *"key presses"* ]]

    # The message is on standard error, the sections on standard output.
    run sh -c '"$1" --until 1792635900 "$2" 2>/dev/null' _ "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 1 ]
    [[ "$output" == *"start-ups (start: lines): 1"* ]]
    [[ "$output" != *"no frame statistics"* ]]
}

@test "the note about windows is printed only when windows or bursts are shown" {
    run "$STATS" "$FIX/timing.txt"
    [[ "$output" == *"percentiles are over those, not over single frames"* ]]
    run "$STATS" --since 1792635960 --until 1792635970 "$FIX/timing.txt"
    [ "$status" -eq 1 ]
    [[ "$output" == *"key presses"* ]]
    [[ "$output" != *"percentiles are over those"* ]]
}

@test "reason and error text prints as written, percent signs and all" {
    log="$BATS_TEST_TMPDIR/pct.log"
    {
        echo '[1792636000.000] launch app.x: failed after 7 ms: 100% %s %d denied'
        echo '[1792636000.100] key 79: not shown after 5 ms: 100% %s odd key'
        echo '[1792636000.200] foreground: not shown after 5 ms: 100% %s odd screen'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"app.x: failed after 7 ms: 100% %s %d denied"* ]]
    [[ "$output" == *"not shown        1 100% %s odd key"* ]]
    [[ "$output" == *"not shown        1 100% %s odd screen"* ]]
}

@test "carriage returns on timing lines do not matter" {
    log="$BATS_TEST_TMPDIR/crlf-timing.log"
    printf '[1792636000.000] key 79: shown after 12 ms\r\n' > "$log"
    printf '[1792636000.100] key 79: not shown after 5 ms: nothing to draw\r\n' >> "$log"
    printf '[1792636000.200] launch a.b: failed after 9 ms: nope\r\n' >> "$log"
    printf '[1792636000.300] start: sdl 5, gl 6, renderer 7, assets 8, apps 9, first frame 10 ms\r\n' >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"shown after      p50 12  p95 12  max 12 ms"* ]]
    [[ "$output" == *"not shown        1 nothing to draw"* ]]
    [[ "$output" == *"a.b: failed after 9 ms: nope"* ]]
    [[ "$output" == *"first frame 1 ms (10 ms in all)"* ]]
    [[ "$output" != *$'\r'* ]]
}
