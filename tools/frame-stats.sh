#!/bin/sh
# Summarises the frame times and the timings that hello-gles and Lumen's shell
# write to their log on the TV (/tmp/lumen-shell.log), for
# .claude/skills/lumen-tv-check.
#
#   tools/frame-stats.sh [options] [LOG]
#
# LOG is a lumen-shell.log; without it the log is read from standard input.
#
# Options:
#   --since EPOCH   only lines logged at or after this Unix time, in seconds
#                   (the TV's clock stamps the log: read it with `date +%s`)
#   --until EPOCH   only lines logged at or before it
#   --miss-ms MS    a frame longer than this is a missed vsync (default 25,
#                   a frame and a half at 60 Hz)
#
# Lines read, each stamped "[<unix seconds>] ":
#   fps 60.0 avg 16.67 ms max 17.50 ms (3000 quads)              hello-gles, a window
#   fps 60.0 avg 16.67 ms max 17.50 ms (60 frames, 1200 quads)   the shell, a window
#   drew 40 frames in 650 ms (avg 16.30 ms, worst 17.10 ms, 1200 quads); idle
#                                                                the shell, a burst of motion
#   drew 23 frames in 371 ms (avg 16.85 ms, worst 28.41 ms, 36 quads); first 28.41, 16.70, 16.68 ms; idle
#                                                                the same, with its frames 2 to 4
#   SDL repeat 2 of scancode 79: 533 ms after the press, 33 ms after the last
#
# A window is about a second of frames. The log keeps one average and one worst
# frame for each window or burst, not every frame, so the percentiles are over
# windows or bursts (nearest rank), never over single frames. The average is
# weighted by frames; a hello-gles window does not print its frame count, so it
# counts as fps frames. A burst of one frame has no frame time and is skipped.
#
# Timing lines, only from the shell, in whole milliseconds (<why> is a reason
# such as "the window lost focus"):
#   key 79: shown after 17 ms                  a press, to the frame showing it
#   key 79: shown after 342 ms (320 ms in launch)
#   key 79: not shown after 41 ms: <why>       (also with the "(B ms in launch)")
#   keys: 7 more presses not timed
#   start: sdl N, gl N, renderer N, assets N, apps N, first frame N ms
#   start: sdl N, gl N, renderer N, assets N, apps N ms; no first frame: <why>
#   foreground: first frame after 140 ms
#   foreground: not shown after 5700 ms: <why>
#   launch <id>: answered in 45 ms
#   launch <id>: failed after 12 ms: <error text>
#   window focus lost: last key 12.3 s ago, 1 other events since
#   window focus lost: no key yet, 3 other events since
# These lines are matched by their fixed fields, so "key down scancode ..." and
# "luna launch: 38 ms" are not timings. After the frame statistics (and the
# note below) come one section per kind that the period holds: key presses with
# p50, p95 and max of "shown after" and how many were within 33 ms (target 2 of
# docs/specs/look-and-feel.md) and the presses not shown by reason; each start
# line split into the time each phase took (the line has running totals from
# main()); the foreground wake-ups; each launch with how many answered within
# 100 ms (target 6; a failed launch never counts); and the focus losses with no
# key pressed (no key yet, or the last key over 5 s before). Their percentiles
# are over single events, nearest rank. Other window and app lines are not read.
#
# Slow-frame lines, only from the shell (each is one line in the log):
#   slow frame 2: 28.41 ms = loop 0.31 + batch 0.40 + upload 2.20 + submit 0.50
#       + swap 25.00 ms; 3 uploads, 12 KB, 9 glyphs; faults 4 minor, 1 major
#   slow frame 5: 30.00 ms = loop 0.00 + batch 0.00 + upload 0.00 + submit 0.00
#       + swap 30.00 ms; 0 uploads, 0 KB, 0 glyphs; faults -
#   slow frames: 2 more not logged
# The shell logs a frame over 25 ms (whatever --miss-ms says) with its place in
# the burst, 2 or more, since a burst's first frame follows an idle wait and is
# not timed. A burst logs at most 4 and counts the rest on a "slow frames:" line
# just before its drew line. "faults -" means the page faults were not counted
# (not Linux, or getrusage failed). A burst line's "first" part holds the times
# of its frames 2, 3 and 4, as many as it drew; a burst of one frame has none.
# Two sections come between the frame statistics and the timing sections, and
# like those are printed only when the period holds such lines: the slow
# frames, with which frame of its burst each was, the p50 and max of each part
# of its time (nearest rank), how many uploaded (and the KB) or rasterised
# glyphs, and their page faults; then the first frames, with in how many bursts
# frame 2, 3 and 4 took over --miss-ms, each over the bursts that drew it.
#
# Input lines, only from the shell (<state> is LG's power state, such as
# "Active" or "Screen Saver", and may hold spaces; "last input" is the seconds
# since the shell last had input; <what> is power, front app or screensaver):
#   pointer shown                              the Magic Remote's pointer
#   pointer hidden
#   pointer: SDL_webOSCursorVisibility found in LG's SDL; arrows hide the pointer
#   pointer: SDL_webOSCursorVisibility not in LG's SDL; the pointer stays as LG leaves it
#   mouse button 1 down at 960,540: taken as OK
#   mouse button 3 down at 1210,302: ignored
#   mouse button 1 up at 960,540
#   power <state>: last input 118.8 s ago
#   front app <id>: last input 0.3 s ago
#   screensaver request: allowed; last input 118.1 s ago
#   luna watch <what>: <error text>            a subscription that failed
#   no input for 90 s; focused yes; pointer hidden; power Active
#                                              the heartbeat while no input comes
# A click taken as OK is also timed by a "key 40: shown after" line, so the key
# presses count it. "key down scancode 484 ..." and the raw event lines are not
# input lines. The input section comes last, and only when the period holds
# such lines: how often the pointer showed and hid, and whether arrows hide it;
# the clicks, the ignored ones by button; each power state and front app with
# its time and last input, 20 of each at most; the screensaver requests with
# the p50, min and max of their last input; each failed luna watch; the
# heartbeats, the longest wait among them and how many came while focused, and
# each run of them (a run is one stretch with no input: its count only grows,
# so a count no higher than the last starts the next) from its first stamp to
# its last, 20 at most. When the period also holds a power or front app line,
# a reading follows: of the focus losses, how many had a "power Screen Saver"
# or "front app com.webos.app.screensaver" line within 2 s before or after.
#
# Exit status: 0 with a summary; 1 when the log holds no frame statistics in
# the period (any slow-frame and timing sections are still printed first); 2
# for a bad argument.
set -eu

SINCE=
UNTIL=
MISS=25
LOG=

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "frame-stats.sh: $*" >&2
    exit 2
}

# A number is digits with at most one dot.
need_number() {
    case $2 in
        '' | . | *[!0-9.]* | *.*.*) die "$1 needs a number, not '$2'" ;;
    esac
}

while [ $# -gt 0 ]; do
    case $1 in
        --since) [ $# -ge 2 ] || die "--since needs a Unix time"; need_number --since "$2"; SINCE=$2; shift 2 ;;
        --until) [ $# -ge 2 ] || die "--until needs a Unix time"; need_number --until "$2"; UNTIL=$2; shift 2 ;;
        --miss-ms) [ $# -ge 2 ] || die "--miss-ms needs milliseconds"; need_number --miss-ms "$2"; MISS=$2; shift 2 ;;
        -h | --help | help) usage; exit 0 ;;
        -*) usage >&2; die "unknown option: $1" ;;
        *) [ -z "$LOG" ] || die "one log at a time: $LOG and $1"; LOG=$1; shift ;;
    esac
done

if [ -n "$LOG" ] && [ ! -f "$LOG" ]; then
    die "no such file: $LOG"
fi

# Single quotes on purpose: this is awk's program, not the shell's.
# shellcheck disable=SC2016
PROG='
# Insertion sort of src[1..n] into dst; a run has tens of windows, not millions.
function isort(src, n, dst,   i, j, v) {
    for (i = 1; i <= n; i++) {
        v = src[i]
        for (j = i - 1; j >= 1 && dst[j] > v; j--) dst[j + 1] = dst[j]
        dst[j + 1] = v
    }
}
# Nearest rank: the value at ceil(p / 100 * n) in the sorted a[1..n].
function pct(a, n, p,   r, k) {
    r = p * n / 100
    k = int(r)
    if (k < r) k++
    if (k < 1) k = 1
    return a[k]
}
function spread(label, src, n,   s) {
    isort(src, n, s)
    printf "  %-16s p50 %.2f  p95 %.2f  p99 %.2f ms\n", label, pct(s, n, 50), pct(s, n, 95), pct(s, n, 99)
}
function quads(lo, hi) {
    if (lo == hi) printf "  quads            %d\n", lo
    else printf "  quads            %d to %d\n", lo, hi
}
# Fields i to NF of the line, joined by single spaces.
function rest(i,   s) {
    if (i > NF) return ""
    s = $i
    for (i++; i <= NF; i++) s = s " " $i
    return s
}
# Fields i to j of the line, joined by single spaces.
function fields(i, j,   s) {
    s = $i
    for (i++; i <= j; i++) s = s " " $i
    return s
}
# The line stamp as written, without its brackets.
function stamp() { return substr($1, 2, length($1) - 2) }
# The line stamp in whole milliseconds, to compare two stamps exactly.
function msec() { return int(ts * 1000 + 0.5) }
# Digits, and for a number inside a start line a comma after them.
function whole(s) { return s ~ /^[0-9]+$/ }
function stage(s) { return s ~ /^[0-9]+,?$/ }
# Milliseconds as the frame lines print them: digits, then maybe a fraction.
function decimal(s) { return s ~ /^[0-9]+(\.[0-9]+)?$/ }
# The "first 28.41, 16.70, 16.68 ms" part of a burst line, from field 17: the
# times of frames 2, 3 and 4, as many as a burst of n frames timed. A part that
# does not read so is left out, and the burst still counts.
function first_times(n,   i, k, t, s, comma) {
    k = 0
    for (i = 17; k < 3; i++) {
        s = $i
        comma = (s ~ /,$/)
        if (comma) s = substr(s, 1, length(s) - 1)
        if (!decimal(s)) return
        t[++k] = s + 0
        if (!comma) break
    }
    if (comma || $(i + 1) != "ms;" || $(i + 2) != "idle" || k > n - 1) return
    nbf++
    for (i = 1; i <= k; i++) if (t[i] > missv) bfover[i + 1]++
}
# The keys of counts[] in alphabetical order in names[1..n]; returns n.
function sorted_keys(counts, names,   k, n, raw) {
    n = 0
    for (k in counts) raw[++n] = k
    if (n > 0) isort(raw, n, names)
    return n
}
# One line for each reason a press or a wake-up was not shown.
function by_reason(counts,   names, n, i) {
    n = sorted_keys(counts, names)
    for (i = 1; i <= n; i++) printf "  %-16s %d %s\n", "not shown", counts[names[i]], names[i]
}

# One part of the slow frames: its p50 (nearest rank) and max.
function slow_part(label, src,   s) {
    isort(src, nsl, s)
    printf "  %-16s p50 %.2f  max %.2f ms\n", label, pct(s, nsl, 50), s[nsl]
}
function report_slow() {
    if (nsl + nsm == 0) return
    printf "slow frames (slow frame lines): %d, %d more not logged\n", nsl, smore
    if (nsl == 0) return
    printf "  %-16s frame 2: %d, frame 3: %d, frame 4: %d, later: %d\n", "which frame", splace[2], splace[3], splace[4], splace[5]
    slow_part("loop", sloop)
    slow_part("batch", sbatch)
    slow_part("upload", supload)
    slow_part("submit", ssubmit)
    slow_part("swap", sswap)
    printf "  %-16s %d of %d had uploads, %d KB in all\n", "uploads", nsup, nsl, skb
    printf "  %-16s %d of %d rasterised glyphs, %d in all\n", "glyphs", nsgl, nsl, sglyphs
    if (nsdash < nsl) printf "  %-16s %d minor, %d major; %d of %d counted had a major fault\n", "page faults", sminor, smajor, nsmajor, nsl - nsdash
    if (nsdash > 0) printf "  %-16s %d of %d (faults -: not Linux, or getrusage failed)\n", "not counted", nsdash, nsl
}
# Each of frames 2, 3 and 4 is counted over the bursts that drew it.
function report_first() {
    if (nbf == 0) return
    printf "first frames: in %d bursts with first times, frame 2 over %g ms in %d, frame 3 in %d, frame 4 in %d\n", nbf, missv, bfover[2], bfover[3], bfover[4]
}
function report_keys(   ks) {
    if (nks + nkn + nku == 0) return
    printf "key presses (key ...: lines): %d shown, %d not shown, %d more not timed\n", nks, nkn, kuntimed
    if (nks > 0) {
        isort(kshown, nks, ks)
        printf "  %-16s p50 %d  p95 %d  max %d ms\n", "shown after", pct(ks, nks, 50), pct(ks, nks, 95), ks[nks]
        printf "  %-16s %d of %d within 33 ms\n", "target 2", kwithin, nks
        if (nklaunch > 0) printf "  %-16s %d of %d had launch time, up to %d ms\n", "in a launch", nklaunch, nks, klmax
    }
    if (nkn > 0) by_reason(knot)
}
function report_starts(   i) {
    if (nst == 0) return
    printf "start-ups (start: lines): %d\n", nst
    for (i = 1; i <= nst; i++) printf "  %s\n", stline[i]
}
function report_foreground(   fs) {
    if (nfs + nfn == 0) return
    printf "foreground (foreground: lines): %d shown, %d not shown\n", nfs, nfn
    if (nfs > 0) {
        isort(fshown, nfs, fs)
        printf "  %-16s p50 %d  max %d ms\n", "first frame", pct(fs, nfs, 50), fs[nfs]
    }
    if (nfn > 0) by_reason(fnot)
}
function report_launches(   i) {
    if (nl == 0) return
    printf "launches (launch <id>: lines): %d, %d failed\n", nl, nlfail
    for (i = 1; i <= nl; i++) printf "  %s\n", lline[i]
    printf "  %-16s %d of %d within 100 ms\n", "target 6", lwithin, nl
}
function report_focus() {
    if (nfl == 0) return
    printf "focus losses (window focus lost lines): %d\n", nfl
    printf "  %-16s %d of %d (no key yet, or the last key over 5 s before)\n", "no key pressed", nfl_nokey, nfl
}

# Up to 20 of a[1..n], one line each under the label, then how many more.
function listed(label, a, n,   i) {
    for (i = 1; i <= n && i <= 20; i++) printf "  %-16s %s\n", label, a[i]
    if (n > 20) printf "  %-16s ... %d more\n", label, n - 20
}
# The ignored clicks by button, in the order of the buttons.
function by_button(   k, n, raw, b, i, s) {
    n = 0
    for (k in cign_by) raw[++n] = k + 0
    isort(raw, n, b)
    for (i = 1; i <= n; i++) s = s ((i > 1) ? ", " : "") sprintf("button %d: %d", b[i], cign_by[b[i]])
    return s
}
# The focus losses with a screensaver line within 2 s before or after; read
# only when the watches of power or the front app logged something.
function report_reading(   i, j, d, k) {
    if (nfl == 0 || npw + nfa == 0) return
    k = 0
    for (i = 1; i <= nfl; i++) {
        for (j = 1; j <= nsv; j++) {
            d = flms[i] - svms[j]
            if (d >= -2000 && d <= 2000) { k++; break }
        }
    }
    printf "  %-16s focus losses explained by the screensaver: %d of %d\n", "reading", k, nfl
}
function report_input(   s, gaps, i, runs) {
    if (nin == 0) return
    print "input (pointer, clicks, power, front app, heartbeats):"
    if (npshown + nphidden > 0 || pverdict != "") {
        s = sprintf("shown %d, hidden %d", npshown, nphidden)
        if (pverdict != "") s = s "; " pverdict
        printf "  %-16s %s\n", "pointer", s
    }
    if (ncok + ncign + ncup > 0) {
        s = sprintf("%d taken as OK, %d ignored", ncok, ncign)
        if (ncign > 0) s = s " (" by_button() ")"
        printf "  %-16s %s, %d ups\n", "clicks", s, ncup
    }
    listed("power", pwline, npw)
    listed("front app", faline, nfa)
    if (nsr > 0) {
        isort(srgap, nsr, gaps)
        printf "  %-16s %d requests; last input p50 %.1f, min %.1f, max %.1f s\n", "screensaver", nsr, pct(gaps, nsr, 50), gaps[1], gaps[nsr]
    }
    for (i = 1; i <= nlw; i++) {
        s = lwkey[i]
        if (lwcount[s] > 1) s = s sprintf(" (%d lines)", lwcount[s])
        printf "  %-16s %s\n", "luna watch", s
    }
    if (nhb > 0) {
        printf "  %-16s %d lines; longest no input for %d s at %s; %d of %d focused yes\n", "heartbeats", nhb, hbmax, hbmaxat, hbfocus, nhb
        for (i = 1; i <= nrun; i++) runs[i] = sprintf("%s to %s: %d heartbeats, up to %d s", runfrom[i], runto[i], runcount[i], runmax[i])
        listed("no input", runs, nrun)
    }
    report_reading()
}

BEGIN {
    missv = miss + 0
    wq_lo = -1; bq_lo = -1
}

{ sub(/\r$/, "") }

# Every line the app logs starts with its Unix time in brackets.
$1 !~ /^\[[0-9]+(\.[0-9]*)?\]$/ { next }
{ ts = substr($1, 2, length($1) - 2) + 0 }
since != "" && ts < since + 0 { next }
until != "" && ts > until + 0 { next }

# fps 60.0 avg 16.67 ms max 17.50 ms (3000 quads)  or  (60 frames, 1200 quads)
$2 == "fps" && $4 == "avg" && $6 == "ms" && $7 == "max" {
    fps = $3 + 0; avg = $5 + 0; mx = $8 + 0
    if ($11 == "frames,") { frames = substr($10, 2) + 0; q = $12 + 0 }
    else { frames = fps; q = substr($10, 2) + 0; estimated = 1 }
    nw++
    wavg[nw] = avg; wmax[nw] = mx
    wframes += frames; wtime += frames * avg
    if (mx > wworst) wworst = mx
    if (mx > missv) wmissed++
    if (wq_lo < 0 || q < wq_lo) wq_lo = q
    if (q > wq_hi) wq_hi = q
    next
}

# drew 40 frames in 650 ms (avg 16.30 ms, worst 17.10 ms, 1200 quads); idle
# drew 23 frames in 371 ms (avg 16.85 ms, worst 28.41 ms, 36 quads); first 28.41, 16.70, 16.68 ms; idle
# The "first" part comes after every field the statistics read, so they are the
# same with or without it.
$2 == "drew" && $4 == "frames" && $5 == "in" && $8 == "(avg" {
    n = $3 + 0
    if (n < 2) next
    nb++
    bavg[nb] = $9 + 0; bmax[nb] = $12 + 0; q = $14 + 0
    bframes += n; bgaps += n - 1; btime += (n - 1) * bavg[nb]
    if (bmax[nb] > bworst) bworst = bmax[nb]
    if (bmax[nb] > missv) bmissed++
    if (bq_lo < 0 || q < bq_lo) bq_lo = q
    if (q > bq_hi) bq_hi = q
    if ($15 == "quads);" && $16 == "first") first_times(n)
    next
}

# The slow-frame lines are matched on fixed fields like the timing lines below,
# and a line that has the fields but not the numbers is skipped.

# slow frame 2: 28.41 ms = loop 0.31 + batch 0.40 + upload 2.20 + submit 0.50 + swap 25.00 ms;
#     3 uploads, 12 KB, 9 glyphs; faults 4 minor, 1 major  (or "faults -")
# The frame is the 2nd of its burst or later: the first follows an idle wait and
# is not timed.
$2 == "slow" && $3 == "frame" && $4 ~ /^[0-9]+:$/ && decimal($5) && $6 == "ms" && $7 == "=" &&
    $8 == "loop" && decimal($9) && $10 == "+" && $11 == "batch" && decimal($12) && $13 == "+" &&
    $14 == "upload" && decimal($15) && $16 == "+" && $17 == "submit" && decimal($18) && $19 == "+" &&
    $20 == "swap" && decimal($21) && $22 == "ms;" &&
    whole($23) && $24 == "uploads," && whole($25) && $26 == "KB," && whole($27) && $28 == "glyphs;" &&
    $29 == "faults" && ($30 == "-" || (whole($30) && $31 == "minor," && whole($32) && $33 == "major")) {
    place = substr($4, 1, length($4) - 1) + 0
    if (place < 2) next
    nsl++
    # Frames 2, 3 and 4 each, and splace[5] for any later one.
    splace[(place > 4) ? 5 : place]++
    sloop[nsl] = $9 + 0; sbatch[nsl] = $12 + 0; supload[nsl] = $15 + 0
    ssubmit[nsl] = $18 + 0; sswap[nsl] = $21 + 0
    if ($23 + 0 > 0) nsup++
    skb += $25
    if ($27 + 0 > 0) nsgl++
    sglyphs += $27
    if ($30 == "-") nsdash++
    else {
        sminor += $30; smajor += $32
        if ($32 + 0 > 0) nsmajor++
    }
    next
}

# slow frames: 2 more not logged  (past the 4 a burst logs, before its drew line)
$2 == "slow" && $3 == "frames:" && whole($4) && $5 == "more" && $6 == "not" && $7 == "logged" {
    nsm++
    smore += $4
    next
}

# SDL repeat 2 of scancode 79: 533 ms after the press, 33 ms after the last
$2 == "SDL" && $3 == "repeat" && $5 == "of" && $6 == "scancode" {
    nrep++
    if ($4 + 0 == 1) { nfirst++; first[nfirst] = $8 + 0 }
    else { ngap++; gap[ngap] = $13 + 0 }
    next
}

# The timing lines below are matched on fixed fields, so that "key down
# scancode 79 ..." and "luna launch: 38 ms" are never mistaken for them. A line
# that has the fields but not the numbers is skipped.

# key 79: shown after 17 ms  or  key 79: shown after 342 ms (320 ms in launch)
$2 == "key" && $3 ~ /^[0-9]+:$/ && $4 == "shown" && $5 == "after" && whole($6) && $7 == "ms" {
    nks++
    kshown[nks] = $6 + 0
    if ($6 + 0 <= 33) kwithin++
    if ($8 ~ /^\([0-9]+$/ && $9 == "ms" && $10 == "in" && $11 == "launch)") {
        nklaunch++
        b = substr($8, 2) + 0
        if (b > klmax) klmax = b
    }
    next
}

# key 79: not shown after 41 ms: the window lost focus
# key 79: not shown after 50 ms (45 ms in launch): the window lost focus
$2 == "key" && $3 ~ /^[0-9]+:$/ && $4 == "not" && $5 == "shown" && $6 == "after" && whole($7) &&
    ($8 == "ms:" || ($8 == "ms" && $9 ~ /^\([0-9]+$/ && $10 == "ms" && $11 == "in" && $12 == "launch):")) {
    why = rest(($8 == "ms:") ? 9 : 13)
    if (why == "") why = "no reason given"
    nkn++
    knot[why]++
    next
}

# keys: 7 more presses not timed
$2 == "keys:" && whole($3) && $4 == "more" && $5 == "presses" && $6 == "not" && $7 == "timed" {
    nku++
    kuntimed += $3
    next
}

# start: sdl 352, gl 380, renderer 1023, assets 1403, apps 1453, first frame 1504 ms
# start: sdl 340, gl 366, renderer 700, assets 1000, apps 1040 ms; no first frame: <why>
# The numbers are running totals from main(); the summary shows the time between.
$2 == "start:" && $3 == "sdl" && $5 == "gl" && $7 == "renderer" && $9 == "assets" && $11 == "apps" &&
    stage($4) && stage($6) && stage($8) && stage($10) && stage($12) {
    p_sdl = $4 + 0; p_gl = $6 + 0; p_ren = $8 + 0; p_ast = $10 + 0; p_app = $12 + 0
    s = sprintf("sdl %d ms, gl %d ms, renderer %d ms, assets %d ms, apps %d ms", p_sdl, p_gl - p_sdl, p_ren - p_gl, p_ast - p_ren, p_app - p_ast)
    if ($12 ~ /,$/ && $13 == "first" && $14 == "frame" && whole($15) && $16 == "ms") {
        p_ff = $15 + 0
        s = s sprintf(", first frame %d ms (%d ms in all)", p_ff - p_app, p_ff)
    } else if ($12 !~ /,$/ && $13 == "ms;" && $14 == "no" && $15 == "first" && $16 == "frame:") {
        why = rest(17)
        if (why == "") why = "no reason given"
        s = s sprintf("; no first frame: %s (%d ms to the apps)", why, p_app)
    } else next
    nst++
    stline[nst] = s
    next
}

# foreground: first frame after 140 ms
$2 == "foreground:" && $3 == "first" && $4 == "frame" && $5 == "after" && whole($6) && $7 == "ms" {
    nfs++
    fshown[nfs] = $6 + 0
    next
}

# foreground: not shown after 5700 ms: the app went to the background
$2 == "foreground:" && $3 == "not" && $4 == "shown" && $5 == "after" && whole($6) && $7 == "ms:" {
    why = rest(8)
    if (why == "") why = "no reason given"
    nfn++
    fnot[why]++
    next
}

# launch com.webos.app.hdmi1: answered in 45 ms
# launch com.example.missing: failed after 12 ms: Permission denied
# A failed launch is counted but is never within the time that target 6 sets.
$2 == "launch" && $3 ~ /^[^:]+:$/ && $4 == "answered" && $5 == "in" && whole($6) && $7 == "ms" {
    nl++
    if ($6 + 0 <= 100) lwithin++
    lline[nl] = sprintf("%s answered in %d ms", $3, $6 + 0)
    next
}
$2 == "launch" && $3 ~ /^[^:]+:$/ && $4 == "failed" && $5 == "after" && whole($6) && $7 == "ms:" {
    nl++
    nlfail++
    why = rest(8)
    lline[nl] = sprintf("%s failed after %d ms%s", $3, $6 + 0, ((why == "") ? "" : ": " why))
    next
}

# window focus lost: no key yet, 3 other events since
# window focus lost: last key 12.3 s ago, 1 other events since
# With no key pressed: none yet, or the last one more than 5 s before.
$2 == "window" && $3 == "focus" && $4 == "lost:" && $5 == "no" && $6 == "key" && $7 == "yet," {
    nfl++
    nfl_nokey++
    flms[nfl] = msec()
    next
}
$2 == "window" && $3 == "focus" && $4 == "lost:" && $5 == "last" && $6 == "key" &&
    $7 ~ /^[0-9]+(\.[0-9]+)?$/ && $8 == "s" && $9 == "ago," {
    nfl++
    if ($7 + 0 > 5) nfl_nokey++
    flms[nfl] = msec()
    next
}

# The input lines below are matched on fixed fields too, so "key down scancode
# 484 ..." and the raw event lines are never taken for them. A line that has
# the fields but not the numbers is skipped.

# pointer shown  or  pointer hidden
$2 == "pointer" && ($3 == "shown" || $3 == "hidden") && NF == 3 {
    nin++
    if ($3 == "shown") npshown++
    else nphidden++
    next
}

# pointer: SDL_webOSCursorVisibility found in the SDL from LG; arrows hide the pointer
# pointer: SDL_webOSCursorVisibility not in the SDL from LG; the pointer stays as LG leaves it
# (The log says "LG" with an apostrophe s, which this program cannot hold: it is
# \047 below.) The last such line in the period gives the verdict.
$2 == "pointer:" && $3 == "SDL_webOSCursorVisibility" && ($4 == "found" || $4 == "not") &&
    $5 == "in" && $6 == "LG\047s" && $7 == "SDL;" {
    nin++
    pverdict = ($4 == "found") ? "arrows hide the pointer" : "pointer stays as LG leaves it"
    next
}

# mouse button 1 down at 960,540: taken as OK
# mouse button 3 down at 1210,302: ignored
$2 == "mouse" && $3 == "button" && whole($4) && $5 == "down" && $6 == "at" && $7 ~ /^[0-9]+,[0-9]+:$/ {
    s = rest(8)
    if (s == "taken as OK") ncok++
    else if (s == "ignored") { ncign++; cign_by[$4 + 0]++ }
    else next
    nin++
    next
}

# mouse button 1 up at 960,540
$2 == "mouse" && $3 == "button" && whole($4) && $5 == "up" && $6 == "at" && $7 ~ /^[0-9]+,[0-9]+$/ && NF == 7 {
    nin++
    ncup++
    next
}

# power Screen Saver: last input 118.8 s ago  (the state may hold spaces)
$2 == "power" && NF >= 8 && $(NF - 5) ~ /:$/ && $(NF - 4) == "last" && $(NF - 3) == "input" &&
    decimal($(NF - 2)) && $(NF - 1) == "s" && $NF == "ago" {
    s = fields(3, NF - 5)
    s = substr(s, 1, length(s) - 1)
    if (s == "") next
    nin++
    npw++
    pwline[npw] = sprintf("%s at %s (last input %s s ago)", s, stamp(), $(NF - 2))
    if (s == "Screen Saver") svms[++nsv] = msec()
    next
}

# front app com.webos.app.screensaver: last input 119.0 s ago
$2 == "front" && $3 == "app" && $4 ~ /^[^:]+:$/ && $5 == "last" && $6 == "input" && decimal($7) &&
    $8 == "s" && $9 == "ago" && NF == 9 {
    s = substr($4, 1, length($4) - 1)
    nin++
    nfa++
    faline[nfa] = sprintf("%s at %s (last input %s s ago)", s, stamp(), $7)
    if (s == "com.webos.app.screensaver") svms[++nsv] = msec()
    next
}

# screensaver request: allowed; last input 118.1 s ago
$2 == "screensaver" && $3 == "request:" && $4 == "allowed;" && $5 == "last" && $6 == "input" &&
    decimal($7) && $8 == "s" && $9 == "ago" && NF == 9 {
    nin++
    nsr++
    srgap[nsr] = $7 + 0
    next
}

# luna watch front app: <error text>  (or luna watch power: or luna watch screensaver:)
# The same subscription with the same text is listed once, with its count.
$2 == "luna" && $3 == "watch" && ($4 == "power:" || $4 == "screensaver:" || ($4 == "front" && $5 == "app:")) {
    i = ($4 == "front") ? 6 : 5
    s = fields(4, i - 1)
    if (i <= NF) s = s " " rest(i)
    else s = substr(s, 1, length(s) - 1)
    nin++
    if (!(s in lwcount)) lwkey[++nlw] = s
    lwcount[s]++
    next
}

# no input for 90 s; focused yes; pointer hidden; power Active  (or power unknown)
# A run of heartbeats is one stretch with no input: its count only grows, so a
# count no higher than the one before starts the next run.
$2 == "no" && $3 == "input" && $4 == "for" && whole($5) && $6 == "s;" && $7 == "focused" &&
    ($8 == "yes;" || $8 == "no;") && $9 == "pointer" && ($10 == "shown;" || $10 == "hidden;") &&
    $11 == "power" && NF >= 12 {
    hn = $5 + 0
    nin++
    nhb++
    if ($8 == "yes;") hbfocus++
    if (nhb == 1 || hn > hbmax) { hbmax = hn; hbmaxat = stamp() }
    if (nhb == 1 || hn <= hblast) { nrun++; runfrom[nrun] = stamp() }
    runto[nrun] = stamp()
    runcount[nrun]++
    runmax[nrun] = hn
    hblast = hn
    next
}

# The frame statistics, then the note on their percentiles when any printed.
function frame_report(   avg, shown) {
    if (nw > 0 && wtime > 0) {
        shown = 1
        avg = wtime / wframes
        printf "windows (fps lines): %d, %s%.0f frames, %.1f s\n", nw, estimated ? "about " : "", wframes, wtime / 1000
        printf "  average frame    %.2f ms (%.1f fps)\n", avg, 1000 / avg
        printf "  worst frame      %.2f ms\n", wworst
        spread("window worst", wmax, nw)
        spread("window average", wavg, nw)
        printf "  missed vsync     %d of %d windows had a frame over %g ms\n", wmissed, nw, missv
        quads(wq_lo, wq_hi)
    }
    if (nb > 0 && btime > 0) {
        shown = 1
        avg = btime / bgaps
        printf "bursts (drew ... idle): %d, %d frames, %.1f s\n", nb, bframes, btime / 1000
        printf "  average frame    %.2f ms (%.1f fps)\n", avg, 1000 / avg
        printf "  worst frame      %.2f ms\n", bworst
        spread("burst worst", bmax, nb)
        spread("burst average", bavg, nb)
        printf "  missed vsync     %d of %d bursts had a frame over %g ms\n", bmissed, nb, missv
        quads(bq_lo, bq_hi)
    }
    if (nrep > 0) {
        printf "key repeats (SDL repeat lines): %d lines from %d holds\n", nrep, nfirst
        if (nfirst > 0) {
            isort(first, nfirst, sf)
            printf "  first repeat     p50 %d ms after the press (%d to %d)\n", pct(sf, nfirst, 50), sf[1], sf[nfirst]
        }
        if (ngap > 0) {
            isort(gap, ngap, sg)
            printf "  gap after that   p50 %d ms (%d to %d)\n", pct(sg, ngap, 50), sg[1], sg[ngap]
        }
    }
    if (shown) print "note: the log keeps one average and one worst frame per window or burst, so percentiles are over those, not over single frames"
}

# The slow frames, the first frames and the timing sections follow the frame
# statistics. They are printed even when there are no frame statistics, which
# still ends in exit status 1.
END {
    frames_seen = (nw > 0 || nb > 0)
    if (frames_seen) frame_report()
    report_slow()
    report_first()
    report_keys()
    report_starts()
    report_foreground()
    report_launches()
    report_focus()
    report_input()
    if (!frames_seen) {
        # Standard output first, so the message follows the sections it is about
        # when both go to the same place.
        fflush()
        cmd = "cat 1>&2"
        print "frame-stats.sh: no frame statistics in the log for that period" | cmd
        close(cmd)
        exit 1
    }
}
'

if [ -n "$LOG" ]; then
    awk -v since="$SINCE" -v until="$UNTIL" -v miss="$MISS" "$PROG" "$LOG"
else
    awk -v since="$SINCE" -v until="$UNTIL" -v miss="$MISS" "$PROG"
fi
