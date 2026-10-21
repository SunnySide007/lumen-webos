#!/bin/sh
# Summarises the frame times that hello-gles and Lumen's shell write to their
# log on the TV (/tmp/lumen-shell.log), for .claude/skills/lumen-tv-check.
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
# Exit status: 0 with a summary; 1 when the log holds no frame statistics in
# the period; 2 for a bad argument.
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

END {
    if (nw == 0 && nb == 0) {
        cmd = "cat 1>&2"
        print "frame-stats.sh: no frame statistics in the log for that period" | cmd
        close(cmd)
        exit 1
    }
    if (nw > 0 && wtime > 0) {
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
    print "note: the log keeps one average and one worst frame per window or burst, so percentiles are over those, not over single frames"
}
'

if [ -n "$LOG" ]; then
    awk -v since="$SINCE" -v until="$UNTIL" -v miss="$MISS" "$PROG" "$LOG"
else
    awk -v since="$SINCE" -v until="$UNTIL" -v miss="$MISS" "$PROG"
fi
