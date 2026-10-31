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

# The slow-frame lines and the burst line's "first" part (docs/specs/shell-mvp.md,
# "Frame time: finding the missed vsyncs"). slow.txt holds timed bursts of 6, 2,
# 3 and 60 frames and a one-frame burst; the numbers below were worked out by
# hand from it. The *.expected files hold what the script printed for the other
# fixtures before it read these lines.

@test "help lists the slow-frame lines" {
    run "$STATS" help
    [ "$status" -eq 0 ]
    [[ "$output" == *"slow frame 2: 28.41 ms = loop"* ]]
    [[ "$output" == *"slow frames: 2 more not logged"* ]]
    [[ "$output" == *"; first 28.41, 16.70, 16.68 ms; idle"* ]]
}

@test "logs in the old formats print byte for byte what they did before" {
    for fix in hello-gles shell timing; do
        "$STATS" "$FIX/$fix.txt" > "$BATS_TEST_TMPDIR/$fix.out"
        cmp "$FIX/$fix.expected" "$BATS_TEST_TMPDIR/$fix.out"
    done
}

@test "the example slow-frame lines give the whole summary" {
    log="$BATS_TEST_TMPDIR/example.log"
    {
        echo '[1792640000.371] drew 23 frames in 371 ms (avg 16.85 ms, worst 28.41 ms, 36 quads); first 28.41, 16.70, 16.68 ms; idle'
        echo '[1792640000.400] slow frame 2: 28.41 ms = loop 0.31 + batch 0.40 + upload 2.20 + submit 0.50 + swap 25.00 ms; 3 uploads, 12 KB, 9 glyphs; faults 4 minor, 1 major'
        echo '[1792640000.500] slow frame 5: 30.00 ms = loop 0.00 + batch 0.00 + upload 0.00 + submit 0.00 + swap 30.00 ms; 0 uploads, 0 KB, 0 glyphs; faults -'
        echo '[1792640000.600] slow frames: 2 more not logged'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    expected=$(cat <<'EOF'
bursts (drew ... idle): 1, 23 frames, 0.4 s
  average frame    16.85 ms (59.3 fps)
  worst frame      28.41 ms
  burst worst      p50 28.41  p95 28.41  p99 28.41 ms
  burst average    p50 16.85  p95 16.85  p99 16.85 ms
  missed vsync     1 of 1 bursts had a frame over 25 ms
  quads            36
note: the log keeps one average and one worst frame per window or burst, so percentiles are over those, not over single frames
slow frames (slow frame lines): 2, 2 more not logged
  which frame      frame 2: 1, frame 3: 0, frame 4: 0, later: 1
  loop             p50 0.00  max 0.31 ms
  batch            p50 0.00  max 0.40 ms
  upload           p50 0.00  max 2.20 ms
  submit           p50 0.00  max 0.50 ms
  swap             p50 25.00  max 30.00 ms
  uploads          1 of 2 had uploads, 12 KB in all
  glyphs           1 of 2 rasterised glyphs, 9 in all
  page faults      4 minor, 1 major; 1 of 1 counted had a major fault
  not counted      1 of 2 (faults -: not Linux, or getrusage failed)
first frames: in 1 bursts with first times, frame 2 over 25 ms in 1, frame 3 in 0, frame 4 in 0
EOF
)
    [ "$output" = "$expected" ]
}

@test "slow frames give which frame each was, each part's p50 and max, uploads, glyphs and faults" {
    run "$STATS" "$FIX/slow.txt"
    [ "$status" -eq 0 ]
    # Seven lines, for frames 2, 2, 3, 4, 9, 20 and 31; the p50 of seven is the 4th.
    [[ "$output" == *"slow frames (slow frame lines): 7, 2 more not logged"* ]]
    [[ "$output" == *"which frame      frame 2: 2, frame 3: 1, frame 4: 1, later: 3"* ]]
    [[ "$output" == *"loop             p50 0.30  max 0.50 ms"* ]]
    [[ "$output" == *"batch            p50 0.60  max 4.10 ms"* ]]
    [[ "$output" == *"upload           p50 0.00  max 2.20 ms"* ]]
    [[ "$output" == *"submit           p50 0.50  max 0.60 ms"* ]]
    # 23.70 25.00 26.60 32.00 32.40 32.50 33.50
    [[ "$output" == *"swap             p50 32.00  max 33.50 ms"* ]]
    # 12 + 64 KB; 9 + 2 + 14 glyphs; 4 + 1 + 2 + 12 minor and 1 + 3 major faults.
    [[ "$output" == *"uploads          2 of 7 had uploads, 76 KB in all"* ]]
    [[ "$output" == *"glyphs           3 of 7 rasterised glyphs, 25 in all"* ]]
    [[ "$output" == *"page faults      19 minor, 4 major; 2 of 7 counted had a major fault"* ]]
    [[ "$output" != *"not counted"* ]]
}

@test "first frames count frames 2, 3 and 4 over the miss line, each over the bursts that drew it" {
    run "$STATS" "$FIX/slow.txt"
    [ "$status" -eq 0 ]
    # Four bursts have first times; the one-frame burst has none. Frame 2: 28.41
    # and 33.30 are over 25 ms; frame 3, in three bursts: 26.00; frame 4, in two: 34.00.
    [[ "$output" == *"first frames: in 4 bursts with first times, frame 2 over 25 ms in 2, frame 3 in 1, frame 4 in 1"* ]]
}

@test "the first part leaves the burst statistics as they were" {
    run "$STATS" "$FIX/slow.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"bursts (drew ... idle): 4, 71 frames, 1.2 s"* ]]
    [[ "$output" == *"average frame    18.23 ms (54.9 fps)"* ]]
    [[ "$output" == *"burst worst      p50 28.41  p95 35.10  p99 35.10 ms"* ]]
    [[ "$output" == *"burst average    p50 19.00  p95 33.30  p99 33.30 ms"* ]]
    [[ "$output" == *"missed vsync     4 of 4 bursts had a frame over 25 ms"* ]]
    [[ "$output" == *"quads            36 to 104"* ]]
    with=$(printf '%s\n' "$output" | sed -n '1,/^note:/p')

    # The same log in the old format: no first parts and no slow-frame lines.
    sed -e 's/; first [^;]*; idle$/; idle/' -e '/] slow frame/d' "$FIX/slow.txt" > "$BATS_TEST_TMPDIR/old.log"
    run "$STATS" "$BATS_TEST_TMPDIR/old.log"
    [ "$status" -eq 0 ]
    [[ "$output" != *"slow frames"* ]]
    [[ "$output" != *"first frames"* ]]
    [ "$(printf '%s\n' "$output" | sed -n '1,/^note:/p')" = "$with" ]
}

@test "faults - is counted apart, and page faults only over the frames that counted them" {
    log="$BATS_TEST_TMPDIR/dash.log"
    {
        echo '[1792636000.000] slow frame 2: 30.00 ms = loop 0.00 + batch 0.00 + upload 0.00 + submit 0.00 + swap 30.00 ms; 0 uploads, 0 KB, 0 glyphs; faults -'
        echo '[1792636000.100] slow frame 3: 27.00 ms = loop 0.10 + batch 0.20 + upload 0.00 + submit 0.30 + swap 26.40 ms; 0 uploads, 0 KB, 0 glyphs; faults -'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"slow frames (slow frame lines): 2, 0 more not logged"* ]]
    [[ "$output" == *"not counted      2 of 2 (faults -: not Linux, or getrusage failed)"* ]]
    [[ "$output" != *"page faults"* ]]

    echo '[1792636000.200] slow frame 2: 26.00 ms = loop 0.10 + batch 0.20 + upload 0.00 + submit 0.30 + swap 25.40 ms; 0 uploads, 0 KB, 0 glyphs; faults 7 minor, 0 major' >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"page faults      7 minor, 0 major; 0 of 1 counted had a major fault"* ]]
    [[ "$output" == *"not counted      2 of 3 (faults -"* ]]
}

@test "the lines for slow frames not logged are summed, and alone print only the count" {
    log="$BATS_TEST_TMPDIR/cap.log"
    {
        echo '[1792636000.000] slow frames: 2 more not logged'
        echo '[1792636001.000] slow frames: 3 more not logged'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"slow frames (slow frame lines): 0, 5 more not logged"* ]]
    [[ "$output" != *"which frame"* ]]
    [[ "$output" != *"p50"* ]]
    [[ "$output" == *"slow frames"*"no frame statistics in the log for that period"* ]]
}

@test "slow-frame lines and first parts that do not read right are skipped" {
    ok='28.41 ms = loop 0.31 + batch 0.40 + upload 2.20 + submit 0.50 + swap 25.00 ms; 3 uploads, 12 KB, 9 glyphs'
    log="$BATS_TEST_TMPDIR/bad-slow.log"
    {
        echo '[1792636000.000] fps 60.0 avg 16.67 ms max 17.00 ms (60 frames, 1200 quads)'
        echo "[1792636000.100] slow frame: $ok; faults -"
        echo "[1792636000.200] slow frame 1: $ok; faults -"
        echo "[1792636000.300] slow frame two: $ok; faults -"
        echo '[1792636000.400] slow frame 2: soon ms = loop 0.31 + batch 0.40 + upload 2.20 + submit 0.50 + swap 25.00 ms; 3 uploads, 12 KB, 9 glyphs; faults -'
        echo '[1792636000.500] slow frame 2: 28.41 ms = loop 0.31 + batch 0.40 + upload 2.20 + submit 0.50 ms; 3 uploads, 12 KB, 9 glyphs; faults -'
        echo '[1792636000.600] slow frame 2: 28.41 ms = loop 0.31 + batch 0.40 + upload -2.20 + submit 0.50 + swap 25.00 ms; 3 uploads, 12 KB, 9 glyphs; faults -'
        echo '[1792636000.700] slow frame 2: 28.41 ms = loop 0.31 + batch 0.40 + upload 2.20 + submit 0.50 + swap 25.00 ms; many uploads, 12 KB, 9 glyphs; faults -'
        echo "[1792636000.800] slow frame 2: $ok; faults soon"
        echo "[1792636000.900] slow frame 2: $ok; faults 4 minor"
        echo "[1792636001.000] slow frame 2: $ok; faults 4 minor, -1 major"
        echo "[1792636001.100] slow frame 2: $ok"
        echo '[1792636001.200] slow frame 2 took 28.41 ms'
        echo '[1792636001.300] slow frames: some more not logged'
        echo '[1792636001.400] slow frames: 2 more'
        echo "slow frame 2: $ok; faults -"
        echo 'slow frames: 2 more not logged'
        echo '[1792636002.000] drew 3 frames in 33 ms (avg 16.60 ms, worst 30.00 ms, 36 quads); first soon ms; idle'
        echo '[1792636002.100] drew 3 frames in 33 ms (avg 16.60 ms, worst 30.00 ms, 36 quads); first 30.00, ms; idle'
        echo '[1792636002.200] drew 3 frames in 33 ms (avg 16.60 ms, worst 30.00 ms, 36 quads); first 30.00 16.60 ms; idle'
        echo '[1792636002.300] drew 3 frames in 33 ms (avg 16.60 ms, worst 30.00 ms, 36 quads); first 30.00, 16.60; idle'
        echo '[1792636002.400] drew 5 frames in 66 ms (avg 16.60 ms, worst 30.00 ms, 36 quads); first 30.00, 16.60, 16.60, 16.60 ms; idle'
        echo '[1792636002.500] drew 2 frames in 33 ms (avg 16.60 ms, worst 30.00 ms, 36 quads); first 30.00, 16.60 ms; idle'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    [[ "$output" != *"slow frames"* ]]
    [[ "$output" != *"first frames"* ]]
    # The burst lines still count as bursts: only their first part is left out.
    [[ "$output" == *"bursts (drew ... idle): 6, 19 frames"* ]]

    # The same lines with a place, faults and the right count of first times are
    # read, so the ones above were skipped for what they lack.
    echo "[1792636003.000] slow frame 2: $ok; faults -" >> "$log"
    echo '[1792636003.100] drew 3 frames in 33 ms (avg 16.60 ms, worst 30.00 ms, 36 quads); first 30.00, 16.60 ms; idle' >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    [[ "$output" == *"slow frames (slow frame lines): 1, 0 more not logged"* ]]
    [[ "$output" == *"first frames: in 1 bursts with first times, frame 2 over 25 ms in 1, frame 3 in 0, frame 4 in 0"* ]]
}

@test "--since and --until cut the slow-frame lines and first parts too" {
    # The 2- and 3-frame bursts only.
    run "$STATS" --since 1792640020 --until 1792640030.05 "$FIX/slow.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"slow frames (slow frame lines): 2, 0 more not logged"* ]]
    [[ "$output" == *"which frame      frame 2: 1, frame 3: 1, frame 4: 0, later: 0"* ]]
    [[ "$output" == *"first frames: in 2 bursts with first times, frame 2 over 25 ms in 1, frame 3 in 1, frame 4 in 0"* ]]

    # The long burst's slow frames without its drew line: printed, status 1.
    run "$STATS" --since 1792640040 --until 1792640040.9 "$FIX/slow.txt"
    [ "$status" -eq 1 ]
    [[ "$output" == *"slow frames (slow frame lines): 4, 0 more not logged"* ]]
    [[ "$output" == *"which frame      frame 2: 0, frame 3: 0, frame 4: 1, later: 3"* ]]
    [[ "$output" != *"first frames"* ]]
    [[ "$output" == *"slow frames"*"no frame statistics in the log for that period"* ]]

    # Its last stamp: the line for the frames not logged, and the burst itself.
    run "$STATS" --since 1792640041 "$FIX/slow.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"slow frames (slow frame lines): 0, 2 more not logged"* ]]
    [[ "$output" == *"first frames: in 1 bursts with first times, frame 2 over 25 ms in 0, frame 3 in 0, frame 4 in 1"* ]]
}

@test "--miss-ms moves the first-frame line, not which frames are slow" {
    run "$STATS" --miss-ms 16.69 "$FIX/slow.txt"
    [ "$status" -eq 0 ]
    # Frame 2: 28.41, 33.30, 16.70, 20.00; frame 3: 16.70, 26.00, 16.70; frame 4: 16.68, 34.00.
    [[ "$output" == *"first frames: in 4 bursts with first times, frame 2 over 16.69 ms in 4, frame 3 in 3, frame 4 in 1"* ]]
    [[ "$output" == *"missed vsync     4 of 4 bursts had a frame over 16.69 ms"* ]]
    # The shell picks the slow frames itself, at 25 ms.
    [[ "$output" == *"slow frames (slow frame lines): 7, 2 more not logged"* ]]

    # 20.00 is at the line, not over it.
    run "$STATS" --miss-ms 20 "$FIX/slow.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"frame 2 over 20 ms in 2, frame 3 in 1, frame 4 in 1"* ]]

    run "$STATS" --miss-ms 40 "$FIX/slow.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"frame 2 over 40 ms in 0, frame 3 in 0, frame 4 in 0"* ]]
}

@test "the slow-frame sections come after the note and before the timing sections" {
    log="$BATS_TEST_TMPDIR/order.log"
    {
        echo '[1792636000.000] fps 60.0 avg 16.67 ms max 30.00 ms (60 frames, 1200 quads)'
        echo '[1792636000.100] slow frame 2: 30.00 ms = loop 0.00 + batch 0.00 + upload 0.00 + submit 0.00 + swap 30.00 ms; 0 uploads, 0 KB, 0 glyphs; faults -'
        echo '[1792636000.200] drew 2 frames in 30 ms (avg 30.00 ms, worst 30.00 ms, 36 quads); first 30.00 ms; idle'
        echo '[1792636000.300] key 79: shown after 17 ms'
        echo '[1792636000.400] window focus lost: no key yet, 3 other events since'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    order=$(printf '%s\n' "$output" |
        grep -o -E '^(windows|bursts|note:|slow frames|first frames|key presses|focus losses)' |
        tr '\n' ',')
    [ "$order" = "windows,bursts,note:,slow frames,first frames,key presses,focus losses," ]
}

@test "carriage returns on slow-frame lines do not matter" {
    log="$BATS_TEST_TMPDIR/crlf-slow.log"
    printf '[1792636000.000] slow frame 2: 30.00 ms = loop 0.00 + batch 0.00 + upload 0.00 + submit 0.00 + swap 30.00 ms; 0 uploads, 0 KB, 0 glyphs; faults -\r\n' > "$log"
    printf '[1792636000.100] slow frame 3: 26.00 ms = loop 0.10 + batch 0.20 + upload 0.00 + submit 0.30 + swap 25.40 ms; 0 uploads, 0 KB, 0 glyphs; faults 7 minor, 1 major\r\n' >> "$log"
    printf '[1792636000.200] slow frames: 2 more not logged\r\n' >> "$log"
    printf '[1792636000.300] drew 3 frames in 56 ms (avg 28.00 ms, worst 30.00 ms, 36 quads); first 30.00, 26.00 ms; idle\r\n' >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    [[ "$output" == *"slow frames (slow frame lines): 2, 2 more not logged"* ]]
    [[ "$output" == *"page faults      7 minor, 1 major; 1 of 1 counted had a major fault"* ]]
    [[ "$output" == *"not counted      1 of 2"* ]]
    [[ "$output" == *"first frames: in 1 bursts with first times, frame 2 over 25 ms in 1, frame 3 in 1, frame 4 in 0"* ]]
    [[ "$output" != *$'\r'* ]]
}

# The input lines: the Magic Remote's pointer and clicks, LG's power state, the
# app in front, the screensaver's requests, failed luna watches and the idle
# heartbeat. input.txt is a run with all of them: two screensavers (the second
# after the front-app watch failed) and a launch of HDMI 1, each with a focus
# loss. input.expected and slow.expected are what the script printed for them;
# slow.expected was printed by the script before it read the input lines.

@test "help lists the input lines" {
    run "$STATS" help
    [ "$status" -eq 0 ]
    [[ "$output" == *"pointer shown"* ]]
    [[ "$output" == *"mouse button 1 down at 960,540: taken as OK"* ]]
    [[ "$output" == *"power <state>: last input"* ]]
    [[ "$output" == *"luna watch <what>: <error text>"* ]]
    [[ "$output" == *"no input for 90 s; focused yes; pointer hidden; power Active"* ]]
}

@test "logs without input lines print no input section, byte for byte as before" {
    for fix in hello-gles shell timing slow; do
        "$STATS" "$FIX/$fix.txt" > "$BATS_TEST_TMPDIR/$fix.out"
        cmp "$FIX/$fix.expected" "$BATS_TEST_TMPDIR/$fix.out"
        [ "$(grep -c -E '^(input \(|  reading )' "$BATS_TEST_TMPDIR/$fix.out")" -eq 0 ]
    done
}

@test "the input run prints the whole summary" {
    run "$STATS" "$FIX/input.txt"
    [ "$status" -eq 0 ]
    "$STATS" "$FIX/input.txt" > "$BATS_TEST_TMPDIR/input.out"
    cmp "$FIX/input.expected" "$BATS_TEST_TMPDIR/input.out"
    # The click taken as OK is a key press too: 7 keys and 1 click, all timed.
    [[ "$output" == *"key presses (key ...: lines): 8 shown, 0 not shown, 0 more not timed"* ]]
}

@test "the input lines leave the rest of the output as it was, and come last" {
    run "$STATS" "$FIX/input.txt"
    [ "$status" -eq 0 ]
    with=$(printf '%s\n' "$output" | sed '/^input (/,$d')
    order=$(printf '%s\n' "$output" |
        grep -o -E '^(windows|bursts|note:|first frames|key presses|start-ups|foreground|launches|focus losses|input)' |
        tr '\n' ',')
    [ "$order" = "windows,bursts,note:,first frames,key presses,start-ups,foreground,launches,focus losses,input," ]

    grep -v -E '^\[[0-9.]+\] (pointer|mouse button|power|front app|screensaver request|luna watch|no input for)([ :]|$)' \
        "$FIX/input.txt" > "$BATS_TEST_TMPDIR/without.log"
    run "$STATS" "$BATS_TEST_TMPDIR/without.log"
    [ "$status" -eq 0 ]
    [ "$output" = "$with" ]
}

@test "the pointer gives how often it showed and hid, and the last verdict on arrows" {
    log="$BATS_TEST_TMPDIR/pointer.log"
    {
        echo "[1792636000.000] pointer: SDL_webOSCursorVisibility not in LG's SDL; the pointer stays as LG leaves it"
        echo '[1792636000.100] pointer shown'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"input (pointer, clicks, power, front app, heartbeats):"$'\n'"  pointer          shown 1, hidden 0; pointer stays as LG leaves it"$'\n'* ]]

    echo "[1792636000.200] pointer: SDL_webOSCursorVisibility found in LG's SDL; arrows hide the pointer" >> "$log"
    echo '[1792636000.300] pointer hidden' >> "$log"
    run "$STATS" "$log"
    [[ "$output" == *"  pointer          shown 1, hidden 1; arrows hide the pointer"$'\n'* ]]

    # The verdict alone, and the counts alone.
    run "$STATS" --since 1792636000.2 --until 1792636000.2 "$log"
    [[ "$output" == *"  pointer          shown 0, hidden 0; arrows hide the pointer"$'\n'* ]]
    run "$STATS" --since 1792636000.3 "$log"
    [[ "$output" == *"  pointer          shown 0, hidden 1"$'\n'* ]]
    [[ "$output" != *"arrows"* ]]
}

@test "clicks count OKs, ignored ones by button in number order, and ups" {
    log="$BATS_TEST_TMPDIR/clicks.log"
    {
        echo '[1792636000.000] mouse button 1 down at 960,540: taken as OK'
        echo '[1792636000.100] mouse button 10 down at 0,0: ignored'
        echo '[1792636000.200] mouse button 2 down at 1919,1079: ignored'
        echo '[1792636000.300] mouse button 2 down at 5,5: ignored'
        echo '[1792636000.400] mouse button 2 up at 5,5'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"  clicks           1 taken as OK, 3 ignored (button 2: 2, button 10: 1), 1 ups"$'\n'* ]]
    [[ "$output" != *"  pointer "* ]]

    run "$STATS" --until 1792636000 "$log"
    [[ "$output" == *"  clicks           1 taken as OK, 0 ignored, 0 ups"$'\n'* ]]
}

@test "power and front app list each line in order, with its stamp and last input" {
    run "$STATS" "$FIX/input.txt"
    [ "$status" -eq 0 ]
    power=$(printf '%s\n' "$output" | grep '^  power ')
    [ "$power" = "  power            Active at 1792650001.490 (last input 1.4 s ago)
  power            Screen Saver at 1792650133.800 (last input 118.8 s ago)
  power            Active at 1792650180.000 (last input 165.0 s ago)
  power            Screen Saver at 1792650435.500 (last input 120.5 s ago)
  power            Screen Off at 1792650500.000 (last input 185.0 s ago)" ]
    front=$(printf '%s\n' "$output" | grep '^  front app ')
    [ "$front" = "  front app        io.github.sunnyside007.lumen.shell at 1792650001.495 (last input 1.4 s ago)
  front app        com.webos.app.screensaver at 1792650134.000 (last input 119.0 s ago)
  front app        io.github.sunnyside007.lumen.shell at 1792650180.200 (last input 165.2 s ago)
  front app        com.webos.app.hdmi1 at 1792650280.300 (last input 0.3 s ago)
  front app        io.github.sunnyside007.lumen.shell at 1792650310.000 (last input 30.0 s ago)" ]
}

@test "power, front app and heartbeat runs stop at 20 lines each and count the rest" {
    log="$BATS_TEST_TMPDIR/many.log"
    for i in $(seq 1 23); do
        echo "[$((1792636000 + i)).000] power Active: last input $i.0 s ago"
    done > "$log"
    for i in $(seq 1 20); do
        echo "[$((1792636100 + i)).000] front app com.example.app$i: last input $i.0 s ago"
    done >> "$log"
    # The same count each time: every heartbeat starts a run.
    for i in $(seq 1 21); do
        echo "[$((1792636200 + i)).000] no input for 30 s; focused yes; pointer hidden; power Active"
    done >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [ "$(printf '%s\n' "$output" | grep -c '^  power ')" -eq 21 ]
    [[ "$output" == *"  power            Active at 1792636020.000 (last input 20.0 s ago)"$'\n'"  power            ... 3 more"$'\n'* ]]
    [[ "$output" != *"last input 21.0 s ago"* ]]
    [ "$(printf '%s\n' "$output" | grep -c '^  front app ')" -eq 20 ]
    [[ "$output" == *"com.example.app20 at 1792636120.000"* ]]
    [[ "$output" != *"front app        ..."* ]]
    [ "$(printf '%s\n' "$output" | grep -c '^  no input ')" -eq 21 ]
    [[ "$output" == *"  no input         1792636220.000 to 1792636220.000: 1 heartbeats, up to 30 s"$'\n'"  no input         ... 1 more"* ]]
    [[ "$output" == *"  heartbeats       21 lines; longest no input for 30 s at 1792636201.000; 21 of 21 focused yes"* ]]
}

@test "screensaver requests give their count and the p50, min and max of last input" {
    log="$BATS_TEST_TMPDIR/ss.log"
    {
        echo '[1792636000.000] screensaver request: allowed; last input 120.1 s ago'
        echo '[1792636200.000] screensaver request: allowed; last input 112.4 s ago'
        echo '[1792636400.000] screensaver request: allowed; last input 118.0 s ago'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"  screensaver      3 requests; last input p50 118.0, min 112.4, max 120.1 s"* ]]
    # No power or front app line: nothing to read the focus losses by.
    [[ "$output" != *"reading"* ]]
}

@test "failed luna watches are listed once each, with their text and count" {
    log="$BATS_TEST_TMPDIR/watch.log"
    {
        echo '[1792636000.000] luna watch power: Denied method call "getPowerState" for category "/"'
        echo '[1792636000.100] luna watch front app: 100% %s denied'
        echo '[1792636000.200] luna watch power: Denied method call "getPowerState" for category "/"'
        echo '[1792636000.300] luna watch screensaver:'
        echo '[1792636000.400] luna launch: 38 ms'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    watch=$(printf '%s\n' "$output" | grep '^  luna watch ')
    [ "$watch" = '  luna watch       power: Denied method call "getPowerState" for category "/" (2 lines)
  luna watch       front app: 100% %s denied
  luna watch       screensaver' ]
}

@test "heartbeats give the longest wait, how many were focused, and each run" {
    log="$BATS_TEST_TMPDIR/heart.log"
    {
        echo '[1792636000.000] no input for 30 s; focused yes; pointer shown; power Active'
        echo '[1792636030.000] no input for 60 s; focused no; pointer hidden; power Screen Saver'
        echo '[1792636040.000] no input for 60 s; focused no; pointer hidden; power unknown'
        echo '[1792636070.000] no input for 90 s; focused no; pointer hidden; power Screen Off'
        echo '[1792636080.000] no input for 30 s; focused yes; pointer hidden; power Active'
        echo '[1792636110.000] no input for 90 s; focused yes; pointer hidden; power Active'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    # 60 after 60 is a new run, as is 30 after 90; the first 90 is the longest.
    [[ "$output" == *"  heartbeats       6 lines; longest no input for 90 s at 1792636070.000; 3 of 6 focused yes"* ]]
    runs=$(printf '%s\n' "$output" | grep '^  no input ')
    [ "$runs" = "  no input         1792636000.000 to 1792636030.000: 2 heartbeats, up to 60 s
  no input         1792636040.000 to 1792636070.000: 2 heartbeats, up to 90 s
  no input         1792636080.000 to 1792636110.000: 2 heartbeats, up to 90 s" ]
}

@test "the reading counts focus losses with a screensaver line within 2 s either side" {
    log="$BATS_TEST_TMPDIR/reading.log"
    {
        # 2 s after: counted.
        echo '[1792636000.000] window focus lost: no key yet, 3 other events since'
        echo '[1792636002.000] power Screen Saver: last input 120.0 s ago'
        # 2.001 s before: not counted.
        echo '[1792636097.999] front app com.webos.app.screensaver: last input 119.0 s ago'
        echo '[1792636100.000] window focus lost: last key 121.0 s ago, 2 other events since'
        # Another state, another app: not counted.
        echo '[1792636200.000] window focus lost: last key 0.2 s ago, 2 other events since'
        echo '[1792636200.500] power Screen Off: last input 0.7 s ago'
        echo '[1792636200.600] front app com.webos.app.hdmi1: last input 0.8 s ago'
        # 2 s before: counted.
        echo '[1792636298.000] front app com.webos.app.screensaver: last input 118.0 s ago'
        echo '[1792636300.000] window focus lost: last key 120.0 s ago, 2 other events since'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"focus losses (window focus lost lines): 4"* ]]
    [[ "$output" == *$'\n'"  reading          focus losses explained by the screensaver: 2 of 4"$'\n'* ]]

    # Without power or front app lines there is nothing to read them by.
    grep -v -E '] (power|front app) ' "$log" > "$BATS_TEST_TMPDIR/bare.log"
    echo '[1792636400.000] no input for 30 s; focused yes; pointer hidden; power unknown' >> "$BATS_TEST_TMPDIR/bare.log"
    run "$STATS" "$BATS_TEST_TMPDIR/bare.log"
    [[ "$output" == *"heartbeats"* ]]
    [[ "$output" != *"reading"* ]]

    # Nor without focus losses.
    grep -v 'window focus lost' "$log" > "$BATS_TEST_TMPDIR/nofocus.log"
    run "$STATS" "$BATS_TEST_TMPDIR/nofocus.log"
    [[ "$output" == *"  power "* ]]
    [[ "$output" != *"reading"* ]]
}

@test "--since and --until cut the input lines too" {
    run "$STATS" --until 1792650200 "$FIX/input.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"  pointer          shown 1, hidden 1; arrows hide the pointer"* ]]
    [[ "$output" == *"  clicks           1 taken as OK, 1 ignored (button 3: 1), 2 ups"* ]]
    [ "$(printf '%s\n' "$output" | grep -c '^  power ')" -eq 3 ]
    [ "$(printf '%s\n' "$output" | grep -c '^  front app ')" -eq 3 ]
    [[ "$output" == *"  screensaver      1 requests; last input p50 118.1, min 118.1, max 118.1 s"* ]]
    [[ "$output" != *"luna watch"* ]]
    [[ "$output" == *"  heartbeats       5 lines; longest no input for 150 s at 1792650165.000; 3 of 5 focused yes"* ]]
    [ "$(printf '%s\n' "$output" | grep -c '^  no input ')" -eq 1 ]
    [[ "$output" == *"explained by the screensaver: 1 of 1"* ]]

    # After the first screensaver: the start-up's pointer verdict is cut.
    run "$STATS" --since 1792650200 "$FIX/input.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"  pointer          shown 1, hidden 1"$'\n'* ]]
    [[ "$output" != *"  clicks "* ]]
    [[ "$output" == *"  power            Screen Saver at 1792650435.500 (last input 120.5 s ago)"* ]]
    [[ "$output" == *"  luna watch       front app: subscription cancelled: no reply from com.webos.applicationManager"* ]]
    [[ "$output" == *"  heartbeats       8 lines; longest no input for 150 s at 1792650465.000; 6 of 8 focused yes"* ]]
    [[ "$output" == *"explained by the screensaver: 1 of 2"* ]]

    # Up to the second focus loss but not the power line just after it.
    run "$STATS" --since 1792650200 --until 1792650435 "$FIX/input.txt"
    [[ "$output" != *"  power "* ]]
    [[ "$output" == *"explained by the screensaver: 0 of 2"* ]]

    # Frames and keys only: no input section.
    run "$STATS" --since 1792650002 --until 1792650009 "$FIX/input.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"key presses"* ]]
    [[ "$output" != *"input ("* ]]
}

@test "input lines that do not read right are skipped, and the rest still counts" {
    log="$BATS_TEST_TMPDIR/good.log"
    {
        echo '[1792636000.000] fps 60.0 avg 16.67 ms max 17.00 ms (60 frames, 1200 quads)'
        echo '[1792636000.100] pointer shown'
        echo "[1792636000.200] pointer: SDL_webOSCursorVisibility not in LG's SDL; the pointer stays as LG leaves it"
        echo '[1792636000.300] mouse button 1 down at 960,540: taken as OK'
        echo '[1792636000.400] mouse button 3 down at 960,540: ignored'
        echo '[1792636000.500] mouse button 1 up at 960,540'
        echo '[1792636000.600] power Screen Saver: last input 118.8 s ago'
        echo '[1792636000.700] front app com.webos.app.screensaver: last input 119.0 s ago'
        echo '[1792636000.800] screensaver request: allowed; last input 118.1 s ago'
        echo '[1792636000.900] luna watch power: denied'
        echo '[1792636001.000] no input for 30 s; focused yes; pointer hidden; power Active'
        echo '[1792636001.100] window focus lost: last key 118.3 s ago, 3 other events since'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    good=$output
    [[ "$good" == *"  pointer          shown 1, hidden 0; pointer stays as LG leaves it"* ]]
    [[ "$good" == *"  clicks           1 taken as OK, 1 ignored (button 3: 1), 1 ups"* ]]
    [[ "$good" == *"explained by the screensaver: 1 of 1"* ]]

    bad="$BATS_TEST_TMPDIR/bad.log"
    {
        echo '[1792636002.000] pointer shown now'
        echo '[1792636002.000] pointer'
        echo '[1792636002.000] pointer: shown'
        echo "[1792636002.000] pointer: SDL_webOSCursorVisibility maybe in LG's SDL; arrows hide the pointer"
        echo '[1792636002.000] pointer: SDL_webOSCursorVisibility found in SDL; arrows hide the pointer'
        echo '[1792636002.000] mouse button left down at 960,540: taken as OK'
        echo '[1792636002.000] mouse button 1 down at 960;540: taken as OK'
        echo '[1792636002.000] mouse button 1 down at 960,540 taken as OK'
        echo '[1792636002.000] mouse button 1 down at 960,540: pressed'
        echo '[1792636002.000] mouse button 1 down at 960,540:'
        echo '[1792636002.000] mouse button 1 up at 960,540 late'
        echo '[1792636002.000] mouse button 1 up at x,540'
        echo '[1792636002.000] power Active: last input soon s ago'
        echo '[1792636002.000] power : last input 1.0 s ago'
        echo '[1792636002.000] power Screen Saver last input 1.0 s ago'
        echo '[1792636002.000] power Active: last input 1.0 s'
        echo '[1792636002.000] power: last input 1.0 s ago'
        echo '[1792636002.000] front app com.x last input 1.0 s ago'
        echo '[1792636002.000] front app com.webos.app.screensaver: last input -1.0 s ago'
        echo '[1792636002.000] front app: last input 1.0 s ago'
        echo '[1792636002.000] front app a b: last input 1.0 s ago'
        echo '[1792636002.000] screensaver request: refused; last input 118.1 s ago'
        echo '[1792636002.000] screensaver request: allowed; last input soon s ago'
        echo '[1792636002.000] screensaver request: allowed'
        echo '[1792636002.000] luna watch tv: denied'
        echo '[1792636002.000] luna watch front: denied'
        echo '[1792636002.000] luna watched power: denied'
        echo '[1792636002.000] no input for soon s; focused yes; pointer hidden; power Active'
        echo '[1792636002.000] no input for 30 s; focused maybe; pointer hidden; power Active'
        echo '[1792636002.000] no input for 30 s; focused yes; pointer gone; power Active'
        echo '[1792636002.000] no input for 30 s; focused yes; pointer hidden; power'
        echo '[1792636002.000] no input for 30 s'
        echo '[1792636002.000] key down scancode 484 keycode 0x0 (None)'
        echo '[1792636002.000] raw event 0x401: 01 04 00 00|d3 30 00 00'
        echo '[1792636002.000] raw event 0x402: 02 04 00 00|6a 31 00 00'
        echo 'pointer shown'
        echo 'power Screen Saver: last input 1.0 s ago'
    } > "$bad"
    cat "$log" "$bad" > "$BATS_TEST_TMPDIR/both.log"
    run "$STATS" "$BATS_TEST_TMPDIR/both.log"
    [ "$status" -eq 0 ]
    [ "$output" = "$good" ]

    # Alone, they print no input section at all.
    run "$STATS" "$bad"
    [ "$status" -eq 1 ]
    [[ "$output" != *"input ("* ]]
}

@test "input lines without frame statistics are printed, and the status is still 1" {
    log="$BATS_TEST_TMPDIR/only.log"
    echo '[1792636000.000] no input for 30 s; focused yes; pointer hidden; power Active' > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == "input (pointer, clicks, power, front app, heartbeats):"*"heartbeats       1 lines"*"no frame statistics in the log for that period" ]]
}

@test "carriage returns and percent signs on input lines do not matter" {
    log="$BATS_TEST_TMPDIR/crlf-input.log"
    printf '[1792636000.000] pointer shown\r\n' > "$log"
    printf '[1792636000.100] mouse button 1 up at 960,540\r\n' >> "$log"
    printf '[1792636000.200] power 100%% %%s Saver: last input 1.0 s ago\r\n' >> "$log"
    printf '[1792636000.300] front app a.b: last input 2.0 s ago\r\n' >> "$log"
    printf '[1792636000.400] screensaver request: allowed; last input 3.0 s ago\r\n' >> "$log"
    printf '[1792636000.500] luna watch screensaver: 100%% %%d gone\r\n' >> "$log"
    printf '[1792636000.600] no input for 30 s; focused no; pointer shown; power Active\r\n' >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"  pointer          shown 1, hidden 0"* ]]
    [[ "$output" == *"  clicks           0 taken as OK, 0 ignored, 1 ups"* ]]
    [[ "$output" == *"  power            100% %s Saver at 1792636000.200 (last input 1.0 s ago)"* ]]
    [[ "$output" == *"  front app        a.b at 1792636000.300 (last input 2.0 s ago)"* ]]
    [[ "$output" == *"  screensaver      1 requests; last input p50 3.0, min 3.0, max 3.0 s"* ]]
    [[ "$output" == *"  luna watch       screensaver: 100% %d gone"* ]]
    [[ "$output" == *"  heartbeats       1 lines; longest no input for 30 s at 1792636000.600; 0 of 1 focused yes"* ]]
    [[ "$output" != *$'\r'* ]]
}

# Stage 3's lines (docs/specs/look-and-feel.md, "Log lines"): a burst's "<K> vsyncs
# skipped", "motion paced", "warm" and a dropped launch. paced.txt is the first 48
# lines of the stage 3 run on the B9 (3a89955, 2026-10-23) without its raw events:
# bursts of 28, 34, 31, 21 and 58 frames, the first with 2 vsyncs skipped, and four
# one-frame bursts. before-stage3.txt is lines of the hardware-float run (9456516)
# from just before it, with no stage 3 line; before-stage3.expected is what the
# script printed for it before it read them. launch.txt has that run's real
# launches and three dropped ones in the documented format, since no log holds a
# real one yet. The numbers below were worked out by hand from the fixtures.

@test "help lists the stage 3 lines" {
    run "$STATS" help
    [ "$status" -eq 0 ]
    [[ "$output" == *"52 quads); first 8.79, 33.45, 16.67 ms; 0 vsyncs skipped; idle"* ]]
    [[ "$output" == *"motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in 10.2 ms"* ]]
    [[ "$output" == *"warm: 137 glyphs, 134 uploads, 769 KB in 156.5 ms"* ]]
    [[ "$output" == *"launch <id>: dropped: the window was hidden"* ]]
    [[ "$output" != *"set -eu"* ]]
}

@test "the paced run prints the whole summary" {
    run "$STATS" "$FIX/paced.txt"
    [ "$status" -eq 0 ]
    "$STATS" "$FIX/paced.txt" > "$BATS_TEST_TMPDIR/paced.out"
    cmp "$FIX/paced.expected" "$BATS_TEST_TMPDIR/paced.out"
}

@test "a log from before stage 3 prints what it printed before, and none of the stage 3 statements" {
    "$STATS" "$FIX/before-stage3.txt" > "$BATS_TEST_TMPDIR/before.out"
    cmp "$FIX/before-stage3.expected" "$BATS_TEST_TMPDIR/before.out"
    for fix in hello-gles shell timing slow input before-stage3; do
        run "$STATS" "$FIX/$fix.txt"
        [[ "$output" != *"skip count"* ]]
        [[ "$output" != *"paced ("* ]]
        [[ "$output" != *"warm ("* ]]
        [[ "$output" != *"dropped"* ]]
    done
}

@test "the skip count gives the bursts that carry it, the vsyncs skipped, the rate and the worst burst" {
    run "$STATS" "$FIX/paced.txt"
    [ "$status" -eq 0 ]
    # Five bursts of 2 or more frames. The 2 vsyncs skipped are the first burst's
    # (28 frames). The five took 482 + 555 + 507 + 343 + 967 = 2854 ms, so 2 in
    # 2.854 s is 7.01 in 10 s.
    [[ "$output" == *"  skip count       5 of 5 bursts carry it"$'\n'* ]]
    [[ "$output" == *"  vsyncs skipped   2 in all, in 1 of 5 bursts"$'\n'* ]]
    [[ "$output" == *"  per 10 s         7.01 over 2.9 s of motion"$'\n'* ]]
    [[ "$output" == *"  worst burst      2 skipped, 28 frames at 1792723995.418"$'\n'* ]]
    # They close the burst section, after its quads.
    [[ "$output" == *"  quads            39 to 52"$'\n'"  skip count "* ]]
    [[ "$output" == *"at 1792723995.418"$'\n'"note: "* ]]
}

@test "the stage 3 lines change no number that the old format printed" {
    run "$STATS" "$FIX/paced.txt"
    [ "$status" -eq 0 ]
    new=$(printf '%s\n' "$output" |
        sed -E -e '/^  (skip count|vsyncs skipped|per 10 s|worst burst) /d' \
            -e '/^paced \(/,/^  read in /d' -e '/^warm \(/,/^  took /d')
    # The same run as a log of the format before stage 3: no skip parts, no
    # motion paced or warm line.
    sed -E -e 's/; [0-9]+ vsyncs skipped; idle$/; idle/' -e '/\] (motion paced|warm):/d' \
        "$FIX/paced.txt" > "$BATS_TEST_TMPDIR/old-format.log"
    run "$STATS" "$BATS_TEST_TMPDIR/old-format.log"
    [ "$status" -eq 0 ]
    [ "$output" = "$new" ]
    # The numbers the script printed for these lines before it read the field.
    [[ "$output" == *"bursts (drew ... idle): 5, 172 frames, 2.9 s"* ]]
    [[ "$output" == *"average frame    17.08 ms (58.5 fps)"* ]]
    [[ "$output" == *"worst frame      33.63 ms"* ]]
    [[ "$output" == *"burst worst      p50 32.54  p95 33.63  p99 33.63 ms"* ]]
    [[ "$output" == *"burst average    p50 16.96  p95 17.83  p99 17.83 ms"* ]]
    [[ "$output" == *"missed vsync     5 of 5 bursts had a frame over 25 ms"* ]]
    [[ "$output" == *"quads            39 to 52"* ]]
    # The first frames are read from lines with the field as from lines without.
    [[ "$output" == *"first frames: in 5 bursts with first times, frame 2 over 25 ms in 1, frame 3 in 4, frame 4 in 0"* ]]
}

@test "a run with a warm line but no pacer says it was not paced" {
    # The stage 3 run with the pacer's line and parts taken out: no burst has the
    # count, and the warm line (real clock only) shows the run is stage 3's.
    sed -E -e 's/; [0-9]+ vsyncs skipped; idle$/; idle/' -e '/\] motion paced:/d' \
        "$FIX/paced.txt" > "$BATS_TEST_TMPDIR/unpaced.log"
    run "$STATS" "$BATS_TEST_TMPDIR/unpaced.log"
    [ "$status" -eq 0 ]
    [[ "$output" == *"  skip count       none of the 5 bursts carries it; the run was not paced"$'\n'* ]]
    [[ "$output" != *"vsyncs skipped"* ]]
    [[ "$output" != *"per 10 s"* ]]
    [[ "$output" == *"paced (motion paced: lines): no motion paced line in this period"$'\n'* ]]
    [[ "$output" == *"warm (warm: lines): 1"$'\n'* ]]
}

@test "when only some bursts carry the skip count, those are counted over their own time" {
    log="$BATS_TEST_TMPDIR/mixed.log"
    {
        echo '[1792636000.000] drew 11 frames in 170 ms (avg 17.00 ms, worst 20.00 ms, 36 quads); first 9.00, 17.00, 17.00 ms; idle'
        echo '[1792636001.000] drew 21 frames in 340 ms (avg 17.00 ms, worst 40.00 ms, 36 quads); first 9.00, 40.00, 17.00 ms; 3 vsyncs skipped; idle'
        # No first part: the count is read from the end of the line.
        echo '[1792636002.000] drew 41 frames in 680 ms (avg 17.00 ms, worst 35.00 ms, 36 quads); 1 vsyncs skipped; idle'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    # 3 bursts, 2 with the count: 3 + 1 = 4 in 340 + 680 = 1020 ms, 39.22 in 10 s.
    [[ "$output" == *"bursts (drew ... idle): 3, 73 frames, "* ]]
    [[ "$output" == *"  skip count       2 of 3 bursts carry it"$'\n'* ]]
    [[ "$output" == *"  vsyncs skipped   4 in all, in 2 of 2 bursts"$'\n'* ]]
    [[ "$output" == *"  per 10 s         39.22 over 1.0 s of motion"$'\n'* ]]
    [[ "$output" == *"  worst burst      3 skipped, 21 frames at 1792636001.000"$'\n'* ]]
    # The first part is read with the count after it, and without a count.
    [[ "$output" == *"first frames: in 2 bursts with first times, frame 2 over 25 ms in 0, frame 3 in 1, frame 4 in 0"* ]]
}

@test "bursts that skipped no vsync say 0 and name no worst burst" {
    # The stage 3 run after its first burst: 34, 31, 21 and 58 frames, in
    # 555 + 507 + 343 + 967 = 2372 ms.
    run "$STATS" --since 1792723996 "$FIX/paced.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"  skip count       4 of 4 bursts carry it"$'\n'* ]]
    [[ "$output" == *"  vsyncs skipped   0 in all, in 0 of 4 bursts"$'\n'* ]]
    [[ "$output" == *"  per 10 s         0.00 over 2.4 s of motion"$'\n'* ]]
    [[ "$output" != *"worst burst"* ]]
}

@test "of equal bursts the first is the worst" {
    log="$BATS_TEST_TMPDIR/ties.log"
    {
        echo '[1792636000.000] drew 5 frames in 66 ms (avg 16.60 ms, worst 17.00 ms, 36 quads); 2 vsyncs skipped; idle'
        echo '[1792636001.000] drew 9 frames in 133 ms (avg 16.60 ms, worst 17.00 ms, 36 quads); 2 vsyncs skipped; idle'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    [[ "$output" == *"  vsyncs skipped   4 in all, in 2 of 2 bursts"$'\n'* ]]
    # 40 in 0.199 s is 201.01 in 10 s.
    [[ "$output" == *"  per 10 s         201.01 over 0.2 s of motion"$'\n'* ]]
    [[ "$output" == *"  worst burst      2 skipped, 5 frames at 1792636000.000"$'\n'* ]]
}

@test "a one-frame burst is no burst for the skip count, but shows the pacer was on" {
    log="$BATS_TEST_TMPDIR/one.log"
    {
        echo '[1792636000.000] drew 1 frames in 0 ms (avg 0.00 ms, worst 0.00 ms, 49 quads); 0 vsyncs skipped; idle'
        echo '[1792636001.000] drew 5 frames in 66 ms (avg 16.60 ms, worst 17.00 ms, 36 quads); idle'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    [[ "$output" == *"bursts (drew ... idle): 1, 5 frames, "* ]]
    # Said without claiming the run was not paced: its first burst may have come before the pacer.
    [[ "$output" == *"  skip count       none of the 1 bursts carries it"$'\n'* ]]
    [[ "$output" != *"not paced"* ]]
}

@test "skip parts that do not read right are left out, and the bursts still count" {
    log="$BATS_TEST_TMPDIR/bad-skip.log"
    burst='drew 3 frames in 33 ms (avg 16.60 ms, worst 17.00 ms, 36 quads)'
    {
        echo "[1792636000.000] $burst; 2 vsync skipped; idle"
        echo "[1792636000.100] $burst; many vsyncs skipped; idle"
        echo "[1792636000.200] $burst; -1 vsyncs skipped; idle"
        echo "[1792636000.300] $burst; 2 vsyncs skipped"
        echo "[1792636000.400] $burst; 2 vsyncs skipped; idle now"
        echo "[1792636000.500] $burst; 2 skipped; idle"
        echo "[1792636000.600] $burst; vsyncs skipped; idle"
        echo "[1792636000.700] $burst; first 17.00, 16.60 ms; 2 vsyncs skipped idle"
        echo "[1792636000.800] $burst; first 17.00, 16.60 ms; soon vsyncs skipped; idle"
        echo "[1792636000.900] $burst; 2.5 vsyncs skipped; idle"
        echo "$burst; 2 vsyncs skipped; idle"
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    [[ "$output" == *"bursts (drew ... idle): 10, 30 frames, "* ]]
    [[ "$output" != *"skip count"* ]]
    [[ "$output" != *"paced ("* ]]
    [[ "$output" != *"warm ("* ]]
    [[ "$output" != *"first frames"* ]]

    # The same line, right, is read, and its first part with it.
    echo "[1792636001.000] $burst; first 17.00, 16.60 ms; 2 vsyncs skipped; idle" >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    [[ "$output" == *"bursts (drew ... idle): 11, 33 frames, "* ]]
    [[ "$output" == *"  skip count       1 of 11 bursts carry it"$'\n'* ]]
    [[ "$output" == *"  vsyncs skipped   2 in all, in 1 of 1 bursts"$'\n'* ]]
    [[ "$output" == *"first frames: in 1 bursts with first times, frame 2 over 25 ms in 0, frame 3 in 0, frame 4 in 0"* ]]
}

@test "the paced section gives the period, where it came from, what each SDL call said and the read time" {
    run "$STATS" "$FIX/paced.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"paced (motion paced: lines): 1"$'\n'"  motion steps by  16.667 ms a frame (60.0 Hz), from the 60 Hz default"$'\n'"  LG refresh rate  SDL_webOSGetRefreshRate answered 120 Hz"$'\n'"  display mode     answered 0 Hz"$'\n'"  read in          10.2 ms"$'\n'* ]]

    log="$BATS_TEST_TMPDIR/paced-forms.log"
    echo '[1792636000.000] motion paced: 4.167 ms a frame, from the display mode; SDL_webOSGetRefreshRate 120 Hz, display mode 240 Hz; read in 0.4 ms' > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"  motion steps by  4.167 ms a frame (240.0 Hz), from the display mode"$'\n'"  LG refresh rate  SDL_webOSGetRefreshRate answered 120 Hz"$'\n'"  display mode     answered 240 Hz"$'\n'"  read in          0.4 ms"$'\n'* ]]

    echo '[1792636000.000] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate absent, display mode failed; read in 0.1 ms' > "$log"
    run "$STATS" "$log"
    [[ "$output" == *"  LG refresh rate  SDL_webOSGetRefreshRate absent"$'\n'"  display mode     failed"$'\n'* ]]

    echo '[1792636000.000] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate failed, display mode absent; read in 0.1 ms' > "$log"
    run "$STATS" "$log"
    [[ "$output" == *"  LG refresh rate  SDL_webOSGetRefreshRate failed"$'\n'"  display mode     absent"$'\n'* ]]

    # Whatever SDL said is printed as it said it; no period, no rate for it.
    echo '[1792636000.000] motion paced: 0.000 ms a frame, from the display mode; SDL_webOSGetRefreshRate -1 Hz, display mode 0 Hz; read in 0.0 ms' > "$log"
    run "$STATS" "$log"
    [[ "$output" == *"  motion steps by  0.000 ms a frame, from the display mode"$'\n'"  LG refresh rate  SDL_webOSGetRefreshRate answered -1 Hz"$'\n'* ]]
}

@test "a log of several runs shows the first motion paced line, with the count" {
    log="$BATS_TEST_TMPDIR/runs.log"
    {
        echo '[1792636000.000] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in 10.2 ms'
        echo '[1792636100.000] motion paced: 4.167 ms a frame, from the display mode; SDL_webOSGetRefreshRate 120 Hz, display mode 240 Hz; read in 0.4 ms'
        echo '[1792636200.000] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate absent, display mode failed; read in 0.1 ms'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"paced (motion paced: lines): 3, the first shown"$'\n'"  motion steps by  16.667 ms a frame (60.0 Hz), from the 60 Hz default"$'\n'"  LG refresh rate  SDL_webOSGetRefreshRate answered 120 Hz"$'\n'"  display mode     answered 0 Hz"$'\n'"  read in          10.2 ms"$'\n'* ]]
    [[ "$output" != *"240"* ]]
}

@test "motion paced lines that do not read right are skipped" {
    log="$BATS_TEST_TMPDIR/bad-paced.log"
    {
        echo '[1792636000.000] motion paced: 16.667 ms a frame, from the display; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in 10.2 ms'
        echo '[1792636000.100] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz display mode 0 Hz; read in 10.2 ms'
        echo '[1792636000.200] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate soon, display mode 0 Hz; read in 10.2 ms'
        echo '[1792636000.300] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode soon; read in 10.2 ms'
        echo '[1792636000.400] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in soon ms'
        echo '[1792636000.500] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in 10.2 ms late'
        echo '[1792636000.600] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz'
        echo '[1792636000.700] motion paced: soon ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in 10.2 ms'
        echo '[1792636000.800] motion paced: 16.667 ms a frame; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in 10.2 ms'
        echo '[1792636000.900] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120, display mode 0 Hz; read in 10.2 ms'
        echo '[1792636001.000] motion paced 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in 10.2 ms'
        echo '[1792636001.100] motion paced:'
        echo 'motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in 10.2 ms'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" != *"paced ("* ]]
    [[ "$output" != *"motion steps by"* ]]
    # Alone they are no sign of stage 3, so they cause no statement either.
    [[ "$output" != *"no warm line"* ]]
    [ "$output" = "frame-stats.sh: no frame statistics in the log for that period" ]
}

@test "the warm section gives the glyphs, uploads, KB and time, or says there was no warm line" {
    run "$STATS" "$FIX/paced.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"warm (warm: lines): 1"$'\n'"  made ahead       137 glyphs, 134 uploads, 769 KB"$'\n'"  took             156.5 ms"$'\n'* ]]

    # The stage 3 run after its warm line: the bursts carry the skip count, so it is stage 3's.
    run "$STATS" --since 1792723995 "$FIX/paced.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"warm (warm: lines): no warm line in this period"$'\n'* ]]
    [[ "$output" != *"made ahead"* ]]
}

@test "a log of several runs shows the first warm line, with the count" {
    log="$BATS_TEST_TMPDIR/warms.log"
    {
        echo '[1792636000.000] warm: 137 glyphs, 134 uploads, 769 KB in 156.5 ms'
        echo '[1792636100.000] warm: 12 glyphs, 3 uploads, 40 KB in 0.7 ms'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"warm (warm: lines): 2, the first shown"$'\n'"  made ahead       137 glyphs, 134 uploads, 769 KB"$'\n'"  took             156.5 ms"$'\n'* ]]
    [[ "$output" != *"0.7"* ]]
    # With a warm line, the missing pacer line is said.
    [[ "$output" == *"paced (motion paced: lines): no motion paced line in this period"$'\n'* ]]
}

@test "warm lines that do not read right are skipped" {
    log="$BATS_TEST_TMPDIR/bad-warm.log"
    {
        echo '[1792636000.000] warm: 137 glyphs, 134 uploads, 769 KB in soon ms'
        echo '[1792636000.100] warm: soon glyphs, 134 uploads, 769 KB in 156.5 ms'
        echo '[1792636000.200] warm: 137 glyphs, soon uploads, 769 KB in 156.5 ms'
        echo '[1792636000.300] warm: 137 glyphs, 134 uploads, soon KB in 156.5 ms'
        echo '[1792636000.400] warm: 137 glyphs, 134 uploads, 769 KB in 156.5 ms late'
        echo '[1792636000.500] warm: 137 glyphs, 134 uploads, 769 KB'
        echo '[1792636000.600] warm: 137 glyph, 134 uploads, 769 KB in 156.5 ms'
        echo '[1792636000.700] warm: -1 glyphs, 134 uploads, 769 KB in 156.5 ms'
        echo '[1792636000.800] warm 137 glyphs, 134 uploads, 769 KB in 156.5 ms'
        echo '[1792636000.900] warm:'
        echo 'warm: 137 glyphs, 134 uploads, 769 KB in 156.5 ms'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" != *"warm ("* ]]
    [ "$output" = "frame-stats.sh: no frame statistics in the log for that period" ]
}

@test "dropped launches are listed beside the others and counted by reason" {
    run "$STATS" "$FIX/launch.txt"
    # No frame statistics in launch.txt; the launches are printed first.
    [ "$status" -eq 1 ]
    [[ "$output" == *"launches (launch <id>: lines): 5, 0 failed, 3 dropped"$'\n'"  com.webos.app.hdmi1: answered in 17 ms"$'\n'"  com.webos.app.hdmi1: dropped: the window was hidden"$'\n'"  com.webos.app.hdmi1: answered in 16 ms"$'\n'"  org.webosbrew.hbchannel: dropped: the shell is leaving"$'\n'"  com.webos.app.hdmi1: dropped: the window was hidden"$'\n'* ]]
    # Target 6 is over the launches that were called: a dropped one has no answer.
    [[ "$output" == *"  target 6         2 of 2 within 100 ms (a dropped launch was never called)"$'\n'* ]]
    [[ "$output" == *"  dropped          1 the shell is leaving"$'\n'"  dropped          2 the window was hidden"$'\n'* ]]

    # With a failed launch among them, which still counts against target 6.
    {
        cat "$FIX/launch.txt"
        echo '[1792655800.000] launch com.example.missing: failed after 12 ms: Permission denied'
    } > "$BATS_TEST_TMPDIR/launch-failed.log"
    run "$STATS" "$BATS_TEST_TMPDIR/launch-failed.log"
    [[ "$output" == *"launches (launch <id>: lines): 6, 1 failed, 3 dropped"$'\n'* ]]
    [[ "$output" == *"  com.example.missing: failed after 12 ms: Permission denied"$'\n'* ]]
    [[ "$output" == *"  target 6         2 of 3 within 100 ms (a dropped launch was never called)"$'\n'* ]]
}

@test "launches with none dropped print as they did, with no dropped part" {
    run "$STATS" "$FIX/timing.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"launches (launch <id>: lines): 4, 1 failed"$'\n'* ]]
    [[ "$output" == *"  target 6         2 of 4 within 100 ms"$'\n'* ]]
    [[ "$output" != *"dropped"* ]]
    [[ "$output" != *"never called"* ]]
}

@test "a dropped launch alone has no target 6, and no reason is said so" {
    log="$BATS_TEST_TMPDIR/dropped-alone.log"
    echo '[1792636000.000] launch a.b: dropped:' > "$log"
    echo '[1792636000.100] launch c.d: dropped: nothing to draw' >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"launches (launch <id>: lines): 2, 0 failed, 2 dropped"$'\n'"  a.b: dropped: no reason given"$'\n'"  c.d: dropped: nothing to draw"$'\n'"  dropped          1 no reason given"$'\n'"  dropped          1 nothing to draw"$'\n'* ]]
    [[ "$output" != *"target 6"* ]]
}

@test "dropped lines that do not read right are skipped" {
    log="$BATS_TEST_TMPDIR/bad-dropped.log"
    {
        echo '[1792636000.000] launch a.b: dropped'
        echo '[1792636000.100] launch a.b dropped: the window was hidden'
        echo '[1792636000.200] launch: dropped: the window was hidden'
        echo '[1792636000.300] launched a.b: dropped: the window was hidden'
        echo '[1792636000.400] key 79: dropped: nothing to draw'
        echo 'launch a.b: dropped: the window was hidden'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [ "$output" = "frame-stats.sh: no frame statistics in the log for that period" ]
}

@test "--since and --until apply to the stage 3 lines like the others" {
    # Up to the first burst: the motion paced and warm lines, and one burst of 28
    # frames with 2 skipped in 482 ms (20 in 0.482 s is 41.49 in 10 s).
    run "$STATS" --until 1792723995.5 "$FIX/paced.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"  skip count       1 of 1 bursts carry it"$'\n'* ]]
    [[ "$output" == *"  vsyncs skipped   2 in all, in 1 of 1 bursts"$'\n'* ]]
    [[ "$output" == *"  per 10 s         41.49 over 0.5 s of motion"$'\n'* ]]
    [[ "$output" == *"paced (motion paced: lines): 1"$'\n'* ]]
    [[ "$output" == *"warm (warm: lines): 1"$'\n'* ]]

    # After the launch: the start-up's motion paced line is cut, as the start line is.
    run "$STATS" --since 1792723994 "$FIX/paced.txt"
    [ "$status" -eq 0 ]
    [[ "$output" == *"  skip count       5 of 5 bursts carry it"$'\n'* ]]
    [[ "$output" == *"paced (motion paced: lines): no motion paced line in this period"$'\n'* ]]
    [[ "$output" == *"warm (warm: lines): 1"$'\n'* ]]
    [[ "$output" != *"start-ups"* ]]

    # Before the first burst: no frame statistics, but the paced section is printed.
    run "$STATS" --until 1792723993.9 "$FIX/paced.txt"
    [ "$status" -eq 1 ]
    [[ "$output" == "paced (motion paced: lines): 1"*"warm (warm: lines): no warm line in this period"*"no frame statistics in the log for that period" ]]

    # The launches of launch.txt up to its first dropped one.
    run "$STATS" --until 1792655600 "$FIX/launch.txt"
    [[ "$output" == *"launches (launch <id>: lines): 2, 0 failed, 1 dropped"$'\n'* ]]
    [[ "$output" == *"  target 6         1 of 1 within 100 ms (a dropped launch was never called)"$'\n'* ]]
    run "$STATS" --since 1792655700 "$FIX/launch.txt"
    [[ "$output" == *"launches (launch <id>: lines): 2, 0 failed, 2 dropped"$'\n'* ]]
    [[ "$output" != *"target 6"* ]]
}

@test "the stage 3 sections come after the first frames and before the timing sections" {
    log="$BATS_TEST_TMPDIR/order3.log"
    {
        cat "$FIX/paced.txt" "$FIX/launch.txt"
        echo '[1792723999.100] window focus lost: no key yet, 3 other events since'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    order=$(printf '%s\n' "$output" |
        grep -o -E '^(windows|bursts|note:|slow frames|first frames|paced|warm|key presses|start-ups|launches|focus losses)' |
        tr '\n' ',')
    [ "$order" = "bursts,note:,slow frames,first frames,paced,warm,key presses,start-ups,launches,focus losses," ]
}

@test "stage 3 lines without frame statistics are printed, and the status is still 1" {
    log="$BATS_TEST_TMPDIR/only3.log"
    echo '[1792636000.000] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in 10.2 ms' > "$log"
    echo '[1792636000.100] warm: 12 glyphs, 3 uploads, 40 KB in 0.7 ms' >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == "paced (motion paced: lines): 1"*"warm (warm: lines): 1"*"no frame statistics in the log for that period" ]]
    [[ "$output" != *"skip count"* ]]
}

@test "carriage returns and percent signs on stage 3 lines do not matter" {
    log="$BATS_TEST_TMPDIR/crlf-stage3.log"
    printf '[1792636000.000] motion paced: 16.667 ms a frame, from the 60 Hz default; SDL_webOSGetRefreshRate 120 Hz, display mode 0 Hz; read in 10.2 ms\r\n' > "$log"
    printf '[1792636000.100] warm: 137 glyphs, 134 uploads, 769 KB in 156.5 ms\r\n' >> "$log"
    printf '[1792636000.200] drew 21 frames in 343 ms (avg 17.13 ms, worst 33.45 ms, 52 quads); first 8.79, 33.45, 16.67 ms; 2 vsyncs skipped; idle\r\n' >> "$log"
    printf '[1792636000.300] launch a.b: dropped: the window was hidden\r\n' >> "$log"
    printf '[1792636000.400] launch c.d: dropped: 100%% %%s odd\r\n' >> "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    [[ "$output" == *"  read in          10.2 ms"$'\n'* ]]
    [[ "$output" == *"  took             156.5 ms"$'\n'* ]]
    [[ "$output" == *"  vsyncs skipped   2 in all, in 1 of 1 bursts"$'\n'* ]]
    [[ "$output" == *"  a.b: dropped: the window was hidden"$'\n'* ]]
    [[ "$output" == *"  c.d: dropped: 100% %s odd"$'\n'* ]]
    [[ "$output" == *"  dropped          1 100% %s odd"$'\n'* ]]
    [[ "$output" != *$'\r'* ]]
}

# The Top Shelf hero's lines (lane J): the art, the heroes put into
# textures, the crossfades and the texture total.

@test "the top shelf section sums up the art, the heroes, the crossfades and the textures" {
    log="$BATS_TEST_TMPDIR/shelf.log"
    {
        echo '[1792799999.000] look: hero on, dither on, tilt on, light on, parallax on; art full size'
        echo '[1792800000.000] art-owner/"hulu.png": 1920x1080; the owner'"'"'s art is 1920x640 or 960x320; not used'
        echo '[1792800000.001] art-owner: art for 2 apps: com.webos.app.hdmi1, netflix'
        echo '[1792800000.100] hero hero-ambient: 3 layers, 9375 KB; decoded in 140.2 ms, uploaded in 30.5 ms'
        echo '[1792800000.200] drew 21 frames in 343 ms (avg 17.13 ms, worst 18.00 ms, 52 quads); idle'
        echo '[1792800000.700] crossfade gradient to hero-ambient: 43 frames; textures 17567 KB at most'
        echo '[1792800000.701] textures: 17567 KB in all (atlas 8192 KB, heroes 9375 KB); 17567 KB at most so far'
        echo '[1792800002.000] hero hero-palette-3: decoded in 60.0 ms; dropped, focus moved on'
        echo '[1792800003.000] hero hero-plex: 1 layer, 4800 KB; decoded in 92.4 ms, uploaded in 11.0 ms'
        echo '[1792800003.800] crossfade hero-ambient to hero-plex: 41 frames; textures 22367 KB at most'
        echo '[1792800004.000] hero hero-settings: hero-settings.png: not a PNG; the shelf shows its gradient'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    expected='top shelf (look, hero, crossfade, textures and art-owner lines):
  look             hero on, dither on, tilt on, light on, parallax on; art full size
  owner art        2 apps with art, 1 files left out
  heroes           2; decode p50 92.4, max 140.2 ms; upload p50 11.0, max 30.5 ms
  hero textures    9375 KB the most for one hero
  dropped          1 decoded after focus moved on
  failed           hero-settings: hero-settings.png: not a PNG
  crossfades       2; frames p50 41, max 43; textures 22367 KB at most
  textures         17567 KB in all at the first idle (atlas 8192 KB, heroes 9375 KB)
  budget           22367 KB at most of 23437 KB: within'
    [[ "$output" == *"$expected"* ]]
    # After the frame statistics.
    [[ "$output" == "bursts"*"top shelf ("* ]]
}

@test "the look line says which features a run had, and whether several runs were alike" {
    log="$BATS_TEST_TMPDIR/look.log"
    {
        echo '[1792800000.000] look: hero off, dither off, tilt off, light off, parallax off; art full size'
        echo '[1792800000.001] look: "sparkle" is not a feature; left out'
        echo '[1792800000.002] look: "3" is not a feature; left out'
        echo '[1792800000.200] drew 21 frames in 343 ms (avg 17.13 ms, worst 18.00 ms, 52 quads); idle'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 0 ]
    expected='top shelf (look, hero, crossfade, textures and art-owner lines):
  look             hero off, dither off, tilt off, light off, parallax off; art full size
  look words       2 left out, not features'
    [[ "$output" == *"$expected" ]]
    # A second run, the same look, then another.
    echo '[1792800100.000] look: hero off, dither off, tilt off, light off, parallax off; art full size' >> "$log"
    run "$STATS" "$log"
    [[ "$output" == *"  look             hero off, dither off, tilt off, light off, parallax off; art full size; the first of 2 runs, all alike"$'\n'* ]]
    echo '[1792800200.000] look: hero on, dither off, tilt on, light on, parallax on; art half size' >> "$log"
    run "$STATS" "$log"
    [[ "$output" == *"; the first of 3 runs, not all alike"$'\n'* ]]
    # A line that does not read right is no look.
    printf '%s\n' '[1792800000.000] look: hero off, dither off; art full size' \
        '[1792800000.200] drew 21 frames in 343 ms (avg 17.13 ms, worst 18.00 ms, 52 quads); idle' > "$log"
    run "$STATS" "$log"
    [[ "$output" != *"top shelf"* ]]
}

@test "a hero over the budget is counted and the budget says OVER" {
    log="$BATS_TEST_TMPDIR/over.log"
    {
        echo '[1792800000.000] hero hero-plex: 4800 KB more would take the textures to 25000 KB, over 23437 KB; not uploaded'
        echo '[1792800001.000] crossfade hero-ambient to hero-plex: 41 frames; textures 24000 KB at most'
        echo '[1792800002.000] textures: 12992 KB in all (atlas 8192 KB, heroes 4800 KB); 24100 KB at most so far'
        echo '[1792800003.000] textures: 12992 KB in all (atlas 8192 KB, heroes 4800 KB); 12992 KB at most so far'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [[ "$output" == *"  over budget      1 not uploaded"$'\n'* ]]
    [[ "$output" == *"  textures         12992 KB in all at the first idle (atlas 8192 KB, heroes 4800 KB), the first of 2 lines"$'\n'* ]]
    [[ "$output" == *"  budget           24100 KB at most of 23437 KB: OVER"$'\n'* ]]
    [[ "$output" != *"heroes   "* ]]
}

@test "top shelf lines that do not read right are skipped" {
    log="$BATS_TEST_TMPDIR/bad-shelf.log"
    {
        echo '[1792800000.000] hero hero-plex: 1 layer, 4800 KB; decoded in soon ms, uploaded in 11.0 ms'
        echo '[1792800000.100] hero hero-plex: 1 layer, 4800 KB; decoded in 92.4 ms, uploaded in 11.0 ms late'
        echo '[1792800000.200] hero hero-plex 1 layer, 4800 KB; decoded in 92.4 ms, uploaded in 11.0 ms'
        echo '[1792800000.300] hero hero-plex: decoded in 60.0 ms; dropped, focus moved'
        echo '[1792800000.400] hero hero-plex: not a PNG the shelf shows its gradient'
        echo '[1792800000.500] crossfade hero-ambient to hero-plex: many frames; textures 22367 KB at most'
        echo '[1792800000.600] crossfade hero-ambient hero-plex: 41 frames; textures 22367 KB at most'
        echo '[1792800000.700] textures: 12992 KB in all (atlas 8192 KB, heroes 4800 KB); 22366 KB at most'
        echo '[1792800000.800] art-owner: art for some apps: netflix'
        echo 'hero hero-plex: 1 layer, 4800 KB; decoded in 92.4 ms, uploaded in 11.0 ms'
    } > "$log"
    run "$STATS" "$log"
    [ "$status" -eq 1 ]
    [ "$output" = "frame-stats.sh: no frame statistics in the log for that period" ]
}
