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
# Exit status: 0 with a summary; 1 when the log holds no frame statistics in
# the period (any timing sections are still printed first); 2 for a bad
# argument.
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
# Digits, and for a number inside a start line a comma after them.
function whole(s) { return s ~ /^[0-9]+$/ }
function stage(s) { return s ~ /^[0-9]+,?$/ }
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
    next
}
$2 == "window" && $3 == "focus" && $4 == "lost:" && $5 == "last" && $6 == "key" &&
    $7 ~ /^[0-9]+(\.[0-9]+)?$/ && $8 == "s" && $9 == "ago," {
    nfl++
    if ($7 + 0 > 5) nfl_nokey++
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

# The timing sections follow the frame statistics. They are printed even when
# there are no frame statistics, which still ends in exit status 1.
END {
    frames_seen = (nw > 0 || nb > 0)
    if (frames_seen) frame_report()
    report_keys()
    report_starts()
    report_foreground()
    report_launches()
    report_focus()
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
