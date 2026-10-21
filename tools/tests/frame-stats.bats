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
