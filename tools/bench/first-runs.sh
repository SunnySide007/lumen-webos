#!/bin/sh
# Stage 1's first runs, in the handoff's order (5.7), one tools/bench.sh run
# each, every one starting only in a lab window. A finished step leaves
# tools/out/bench/done-<name>, so a second call carries on where the first
# stopped (a yield, a closed window).
#
#   sh tools/bench/first-runs.sh FIX.ipk
#
# BENCH_FLAGS=--force runs them outside a lab window: only when the owner
# says the TV is free.
set -u
cd "$(dirname "$0")/../.." || exit 1
FIX=${1:?usage: first-runs.sh FIX.ipk}
for f in tools/out/bench/last-good/*.ipk; do BASE=$f; done
[ -f "${BASE:-}" ] || { echo "first-runs: no last good build" >&2; exit 1; }

step() {
    name=$1
    shift
    [ -f "tools/out/bench/done-$name" ] && return 0
    # shellcheck disable=SC2086 # flags, split on purpose
    sh tools/bench.sh run --name "$name" ${BENCH_FLAGS:-} "$@"
    rc=$?
    [ "$rc" = 0 ] && : >"tools/out/bench/done-$name"
    return "$rc"
}

baseline() {
    [ -f tools/out/bench/baseline.json ] && return 0
    for d in tools/out/bench/2*-base-3a89955; do [ -d "$d" ] && dir=$d; done
    cp "$dir/summary.json" tools/out/bench/baseline.json &&
        echo "first-runs: baseline from $dir"
}

step base-3a89955 --ipk "$BASE" --settle &&
    baseline &&
    step fix-plain --ipk "$FIX" --params '{"look":"plain"}' --settle &&
    step fix-nodither --ipk "$FIX" --params '{"look":"-dither"}' &&
    step fix-full --ipk "$FIX" &&
    if [ ! -f tools/out/bench/done-apps ]; then
        # shellcheck disable=SC2086 # flags, split on purpose
        sh tools/bench.sh apps ${BENCH_FLAGS:-} && : >tools/out/bench/done-apps
    fi
