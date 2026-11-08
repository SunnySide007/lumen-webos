#!/bin/sh
# The bench: measures a Lumen build on the TV the same way every time and
# writes its numbers (docs/device/bench.md). Tier A of docs/safety.md,
# "Working without asking": it installs, launches and closes Lumen builds,
# plays the robot remote, reads logs and screenshots, and always puts back
# the last good build. Nothing else on the TV changes.
#
#   tools/bench.sh window [--night]    is a lab window open now? (exit 0 yes,
#                                      1 no, with the reason); --night: the
#                                      window for work that lights nothing
#                                      (00:00 to 06:00, nothing else in front)
#   tools/bench.sh wait-window [MIN] [--night]
#                                      look every 2 min until one opens, for
#                                      up to MIN minutes (default 720)
#   tools/bench.sh last-good [COMMIT]  make tools/out/bench/last-good/ from
#                                      the build installed on the TV now
#   tools/bench.sh run [options]       measure a build
#       --ipk FILE      install this build first (default: what is installed)
#       --params JSON   launch parameters, such as '{"look":"plain"}'
#       --runs N        how many runs (default 3)
#       --name NAME     the report folder's suffix
#       --pattern FILE  the moves to play (default tools/bench/pattern-45s.txt)
#       --settle        the last run also stays settled for 5 min, then reads
#                       the shell's RSS and idle CPU
#       --force         run outside a lab window: only when the owner says
#                       the TV is free
#   tools/bench.sh apps [--apps "ID ..."] [--force]
#                                      app launch times and target 5: each app
#                                      (default netflix, youtube.leanback.v4,
#                                      com.beb.plxnative) launched cold, then a
#                                      robot Home press back to Lumen, then
#                                      warm and Home again; apps not running
#                                      before are closed after
#   tools/bench.sh lean-survey [--force]
#                                      the lean track's read-only survey:
#                                      memory, upstart jobs and their memory,
#                                      keep-alive apps, the largest processes
#   tools/bench.sh lean-plan [SURVEY]  dry run: what lean-try would stop
#   tools/bench.sh lean-try JOB [--force]
#                                      tier B: stop one candidate job until
#                                      the next boot, after a backup of the
#                                      last 24 h, noted for lean-undo
#   tools/bench.sh lean-undo [JOB]     start again what lean-try stopped
#   tools/bench.sh restore             put back the last good build
#   tools/bench.sh gate DIR [BASE]     the speed gate: DIR/summary.json against
#                                      BASE (default tools/out/bench/baseline.json)
#   tools/bench.sh with-lock CMD ...   run CMD holding the TV lock (for TV jobs
#                                      outside bench.sh, such as a Guard trial)
#
# One TV job at a time: run, apps, the lean commands, restore and with-lock
# take tools/out/bench/tv.lock (LUMEN_BENCH_LOCK), waiting up to an hour
# for a job that holds it. A lock whose process has gone is taken over.
#
# A yield's time goes to tools/out/bench/person-at (LUMEN_BENCH_PERSON):
# the TV's own idle record leaves out the bench's time ranges, so without
# it a person who stopped a run would not hold the window shut.
#
# Each run closes the shell, launches it with the parameters plus the
# bench's own ("saver":"hold" so LG's screensaver stays off, "shots" for
# two screenshots, "exitAfter" as a safety net), waits for its first frame
# (none in 10 s: the last good build goes back, exit 4), plays
# tools/bench/pattern-45s.txt through `lumen-ctl play`, and takes the
# frame window from the log's own first and last key. A real remote press
# stops it (exit 3); the last good build goes back and the shell is
# relaunched plainly, so the screensaver is LG's again.
#
# Reports go to tools/out/bench/<UTC stamp>-<name>/: the logs, frame-stats
# output, the robot's lines, the screenshots, run<N>.json, summary.json and
# gate.md. Needs dist/ctl/lumen-ctl (`tools/dev.sh make ctl`).
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
BENCH=$ROOT/tools/bench
OUTROOT=$ROOT/tools/out/bench
LEAN=${LUMEN_LEAN_PROFILE:-$ROOT/profiles/lean}
LAST_GOOD=$OUTROOT/last-good
APP=io.github.sunnyside007.lumen.shell
SAVER=com.webos.app.screensaver
STAGE=/var/lib/lumen/stage
CTL=$STAGE/bench-lumen-ctl
PATTERN=$STAGE/bench-pattern.txt
IDLE_S=1200
FIRST_FRAME_S=10
PY=${PYTHON:-python}
LOCK=${LUMEN_BENCH_LOCK:-$OUTROOT/tv.lock}
LOCK_WAIT_S=3600
PERSON=${LUMEN_BENCH_PERSON:-$OUTROOT/person-at}
command -v "$PY" >/dev/null 2>&1 || PY=python3

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "bench.sh: $*" >&2
    exit 1
}

say() {
    echo "bench: $*"
    [ -z "${OUT:-}" ] || echo "$(date -u +%H:%M:%SZ) $*" >>"$OUT/bench.log"
}

# tv.sh, with OpenSSH's post-quantum warning kept out of the output.
tv() {
    sh "$TV" "$@" 2>>"${OUT:-/dev/null}${OUT:+/tv-stderr.txt}"
}

# Runs tools/bench/tv/<name>.sh on the TV after lib.sh, with the given
# assignments first; its output as it came (tv_step_raw) or as text.
tv_step_raw() {
    name=$1
    shift
    tmp=$(mktemp)
    for a in "$@"; do echo "$a"; done >"$tmp"
    cat "$BENCH/tv/lib.sh" "$BENCH/tv/$name.sh" >>"$tmp"
    rc=0
    tv script "$tmp" || rc=$?
    rm -f "$tmp"
    return $rc
}

tv_step() {
    tv_step_raw "$@" | tr -d '\r'
}

value() {
    printf '%s\n' "$2" | sed -n "s/^$1=//p" | head -n 1
}

# Prints why no lab window is open, or nothing when one is. With "night",
# the window for work that lights nothing on the panel (Guard, hooks,
# surveys): 00:00 to 06:00 only, any power state, with Lumen, LG's
# screensaver or nothing in front, and 20 minutes since a person's last sign.
window_closed() {
    night=${1:-}
    info=$(tv_step window 2>/dev/null) || {
        echo "the TV does not answer"
        return
    }
    now=$(value now "$info")
    hour=$(value hour "$info")
    fg=$(value fg "$info")
    power=$(value power "$info")
    last=$(value last "$info")
    under=$(value under "$info")
    [ -n "$now" ] || {
        echo "the TV does not answer"
        return
    }
    person=$(cat "$PERSON" 2>/dev/null || true)
    person=${person%%.*}
    case $person in '' | *[!0-9]*) person='' ;; esac
    if [ -n "$person" ] && [ "$now" -ge "$person" ] && [ $((now - person)) -lt "$IDLE_S" ]; then
        echo "a person stopped a run $((now - person)) s ago (idle needs ${IDLE_S})"
        return
    fi
    if [ "$night" = night ]; then
        case $hour in
            00 | 01 | 02 | 03 | 04 | 05) ;;
            *)
                echo "not night ($hour h TV time): dark work runs 00:00 to 06:00"
                return
                ;;
        esac
        case $fg in
            "$APP" | "$SAVER" | "") ;;
            *)
                echo "$fg is in front"
                return
                ;;
        esac
        if [ -n "$last" ] && [ $((now - last)) -lt "$IDLE_S" ]; then
            echo "idle $((now - last)) s of ${IDLE_S}"
        fi
        return
    fi
    case $hour in
        00 | 01 | 02 | 03 | 04 | 05)
            echo "night ($hour h TV time): the screen stays dark until 06:00"
            return
            ;;
    esac
    case $power in
        Active | "Screen Saver") ;;
        *)
            echo "power state ${power:-unknown}"
            return
            ;;
    esac
    case $fg in
        "$APP") ;;
        "$SAVER")
            [ "$under" = "$APP" ] || {
                echo "LG's screensaver is over ${under:-an unknown app}, not Lumen"
                return
            }
            ;;
        *)
            echo "${fg:-nothing} is in front"
            return
            ;;
    esac
    [ -n "$last" ] || {
        echo "no record of the last input"
        return
    }
    idle=$((now - last))
    [ "$idle" -ge "$IDLE_S" ] || echo "idle ${idle} s of ${IDLE_S}"
}

cmd_window() {
    why=$(window_closed "$(night_arg "$@")")
    if [ -z "$why" ]; then
        echo "open"
        return 0
    fi
    echo "closed: $why"
    return 1
}

# "night" if the arguments hold --night.
night_arg() {
    for a in "$@"; do
        [ "$a" != --night ] || {
            echo night
            return
        }
    done
}

cmd_wait_window() {
    mode=$(night_arg "$@")
    limit=720
    for a in "$@"; do
        case $a in --night) ;; *[!0-9]* | '') die "wait-window [MIN] [--night]" ;; *) limit=$a ;; esac
    done
    end=$(($(date +%s) + limit * 60))
    while :; do
        why=$(window_closed "$mode")
        if [ -z "$why" ]; then
            echo "open at $(date -u +%Y-%m-%dT%H:%M:%SZ)"
            return 0
        fi
        echo "$(date -u +%H:%M:%SZ) closed: $why"
        [ "$(date +%s)" -lt "$end" ] || return 1
        sleep 120
    done
}

# The installed app's binary's SHA-256 on the TV.
tv_binary_sha() {
    tv run "sha256sum /media/developer/apps/usr/palm/applications/$APP/lumen-shell 2>/dev/null | cut -d' ' -f1" </dev/null | tr -d '\r'
}

last_good_ipk() {
    for f in "$LAST_GOOD"/*.ipk; do
        [ -f "$f" ] && echo "$f" && return
    done
}

cmd_last_good() {
    commit=${1:-unknown}
    tmp=$OUTROOT/last-good.new
    rm -rf "$tmp"
    mkdir -p "$tmp"
    tv run "tar -cf - -C /media/developer/apps/usr/palm/applications $APP" </dev/null >"$tmp/previous.tar"
    tar -tf "$tmp/previous.tar" | grep -q "^$APP/lumen-shell$" || die "the TV's copy has no lumen-shell; nothing made"
    rel=${tmp#"$ROOT"/}
    sh "$ROOT/tools/dev.sh" sh -c "rm -rf /tmp/prev && mkdir /tmp/prev && tar -xf $rel/previous.tar -C /tmp/prev && ares-package --no-minify --outdir $rel /tmp/prev/$APP && python3 -I tools/ipk/fix_modes.py $rel/*.ipk" >"$tmp/package.txt" 2>&1 ||
        die "packaging failed: see $tmp/package.txt"
    tar -xOf "$tmp/previous.tar" "$APP/lumen-shell" | sha256sum | cut -d' ' -f1 >"$tmp/binary.sha256"
    [ "$(cat "$tmp/binary.sha256")" = "$(tv_binary_sha)" ] || die "the copy does not match the TV's binary"
    printf 'commit %s\nmade %s from the build installed on the TV\nbinary %s\n' \
        "$commit" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$(cat "$tmp/binary.sha256")" >"$tmp/README.txt"
    rm -rf "$LAST_GOOD"
    mv "$tmp" "$LAST_GOOD"
    echo "last good: $(last_good_ipk), binary $(cat "$LAST_GOOD/binary.sha256")"
}

# Puts back the last good build, unless it is already installed, and checks
# its binary.
restore() {
    ipk=$(last_good_ipk)
    [ -n "$ipk" ] || {
        say "no last good build to put back"
        return 1
    }
    want=$(cat "$LAST_GOOD/binary.sha256")
    if [ "$(tv_binary_sha)" != "$want" ]; then
        say "putting back the last good build"
        tv close "$APP" </dev/null >/dev/null || true
        tv install "$ipk" </dev/null >/dev/null || say "install of the last good build failed"
    fi
    got=$(tv_binary_sha)
    if [ "$got" = "$want" ]; then
        say "last good build in place (binary ${want%"${want#????????????}"}…)"
    else
        say "RESTORE NOT VERIFIED: binary ${got:-missing}, want $want"
        return 1
    fi
}

# The shell relaunched with no parameters, as the Home it is: LG's
# screensaver works again.
relaunch_plain() {
    tv close "$APP" </dev/null >/dev/null || true
    sleep 2
    tv launch "$APP" </dev/null >/dev/null || say "plain relaunch failed"
}

cmd_restore() {
    restore
    relaunch_plain
}

# Waits for a first frame after $1; 0 when it came.
first_frame_after() {
    # Its own counter: cmd_run's loop counts runs in $i.
    waited=0
    while [ $waited -lt "$FIRST_FRAME_S" ]; do
        sleep 1
        if tv_step logsince "SINCE=$1" 2>/dev/null | grep -q '\] start: .*first frame'; then
            return 0
        fi
        waited=$((waited + 1))
    done
    return 1
}

cleanup() {
    rc=$?
    trap - EXIT INT TERM
    if [ "${CLEAN_DONE:-0}" = 0 ] && [ -n "${OUT:-}" ]; then
        CLEAN_DONE=1
        if [ "${KEEP:-0}" = 0 ]; then
            restore || true
        fi
        relaunch_plain
        # The window check leaves this run's own keys and launches out.
        [ -z "${BENCH_T0:-}" ] ||
            tv run "echo $BENCH_T0 \$((\$(date +%s) + 15)) >>$STAGE/bench-ranges; tail -n 200 $STAGE/bench-ranges >$STAGE/bench-ranges.new && mv $STAGE/bench-ranges.new $STAGE/bench-ranges" </dev/null || true
        say "done (exit $rc)"
    fi
    exit $rc
}

cmd_run() {
    IPK='' PARAMS='{}' RUNS=3 NAME=run SETTLE=0 FORCE=0 KEEP=0
    MOVES=$BENCH/pattern-45s.txt
    while [ $# -gt 0 ]; do
        case $1 in
            --ipk) IPK=$2; shift 2 ;;
            --params) PARAMS=$2; shift 2 ;;
            --runs) RUNS=$2; shift 2 ;;
            --name) NAME=$2; shift 2 ;;
            --settle) SETTLE=1; shift ;;
            --pattern) MOVES=$2; shift 2 ;;
            --force) FORCE=1; shift ;;
            *) die "unknown option for run: $1" ;;
        esac
    done
    case $RUNS in [1-9]) ;; *) die "--runs must be 1 to 9" ;; esac
    case $NAME in *[!A-Za-z0-9._-]* | '') die "--name: letters, digits, . _ - only" ;; esac
    [ -z "$IPK" ] || [ -f "$IPK" ] || die "no such IPK: $IPK"
    [ -f "$MOVES" ] || die "no such pattern: $MOVES"
    [ -n "$(last_good_ipk)" ] || die "no last good build: run tools/bench.sh last-good first"
    [ -f "$ROOT/dist/ctl/lumen-ctl" ] || die "no dist/ctl/lumen-ctl: tools/dev.sh make ctl"
    if [ "$FORCE" = 0 ]; then
        why=$(window_closed)
        [ -z "$why" ] || {
            echo "bench: no lab window: $why"
            exit 5
        }
    fi
    exit_after=600
    [ "$SETTLE" = 0 ] || exit_after=900
    launch=$("$PY" -I "$BENCH/gate.py" params "$PARAMS" "{\"saver\":\"hold\",\"shots\":[4,58],\"exitAfter\":$exit_after}") ||
        die "--params must be one JSON object"

    OUT=$OUTROOT/$(date -u +%Y%m%dT%H%M%SZ)-$NAME
    mkdir -p "$OUT"
    BENCH_T0=$(tv run 'date +%s' </dev/null | tr -d '\r')
    trap cleanup EXIT
    trap 'exit 130' INT TERM
    {
        echo "name $NAME"
        echo "commit $(git -C "$ROOT" rev-parse --short HEAD)$(git -C "$ROOT" diff --quiet HEAD -- shell || echo ' with uncommitted changes in shell/')"
        echo "ipk ${IPK:-the installed build}"
        [ -z "$IPK" ] || echo "ipk sha256 $(sha256sum <"$IPK" | cut -d' ' -f1)"
        echo "launch $launch"
        echo "runs $RUNS"
        echo "pattern ${MOVES#"$ROOT"/} $(sha256sum <"$MOVES" | cut -c1-12)"
        echo "last good $(cat "$LAST_GOOD/binary.sha256")"
        tv check </dev/null | grep -E '"(modelName|firmwareVersion)"' | tr -d ' ,'
    } >"$OUT/run.txt"
    say "report in ${OUT#"$ROOT"/}"

    tv push "$ROOT/dist/ctl/lumen-ctl" "$CTL" >/dev/null
    tv push "$MOVES" "$PATTERN" >/dev/null
    tv run "chmod 0700 $CTL" </dev/null

    if [ -n "$IPK" ]; then
        say "installing $(basename "$IPK")"
        tv close "$APP" </dev/null >/dev/null || true
        tv install "$IPK" </dev/null >"$OUT/install.txt" || die "install failed: $(cat "$OUT/install.txt")"
    fi
    sha=$(tv_binary_sha)
    echo "binary $sha" >>"$OUT/run.txt"

    i=1
    while [ $i -le "$RUNS" ]; do
        say "run $i of $RUNS"
        tv close "$APP" </dev/null >/dev/null || true
        sleep 2
        l0=$(tv run 'date +%s' </dev/null | tr -d '\r')
        tv launch "$APP" "$launch" </dev/null >"$OUT/run$i-launch.txt"
        if ! first_frame_after "$l0"; then
            say "AUTOMATIC UNDO: no first frame within ${FIRST_FRAME_S} s of the launch"
            tv_step logsince "SINCE=$l0" >"$OUT/run$i.log" 2>/dev/null || true
            exit 4
        fi
        set +e
        tv run "$CTL play $PATTERN" </dev/null >"$OUT/run$i-play.jsonl"
        played=$?
        set -e
        if [ "$played" = 3 ]; then
            say "YIELD: a person used the remote ($(grep yield "$OUT/run$i-play.jsonl" | head -n 1))"
            note_person "$OUT/run$i-play.jsonl"
            exit 3
        fi
        [ "$played" = 0 ] || {
            say "the robot remote failed (exit $played): $(tail -n 1 "$OUT/run$i-play.jsonl")"
            exit 1
        }
        if [ "$SETTLE" = 1 ] && [ $i -eq "$RUNS" ]; then
            say "settling for 5 min"
            sleep 270
            tv_step proc >"$OUT/run$i-proc.txt"
        else
            # The second screenshot is due 58 s after the start.
            now=$(tv run 'date +%s' </dev/null | tr -d '\r')
            wait_s=$((l0 + 62 - now))
            [ "$wait_s" -le 0 ] || sleep "$wait_s"
        fi
        tv_step logsince "SINCE=$l0" >"$OUT/run$i.log"
        first=$(grep '\] key down' "$OUT/run$i.log" | head -n 1 | sed 's/^\[\([0-9]*\)\..*/\1/')
        last=$(grep '\] key down' "$OUT/run$i.log" | tail -n 1 | sed 's/^\[\([0-9]*\)\..*/\1/')
        if [ -z "$first" ]; then
            say "run $i: no key reached the shell; stopping (the robot remote's keys do not arrive)"
            echo "no key down in the log" >"$OUT/run$i-stats.txt"
            exit 6
        else
            sh "$ROOT/tools/frame-stats.sh" --since "$first" --until $((last + 1)) "$OUT/run$i.log" >"$OUT/run$i-stats.txt" 2>&1 || true
        fi
        rss=$(value rss_kb "$(cat "$OUT/run$i-proc.txt" 2>/dev/null)")
        cpu=$(value cpu_pct "$(cat "$OUT/run$i-proc.txt" 2>/dev/null)")
        "$PY" -I "$BENCH/gate.py" run --stats "$OUT/run$i-stats.txt" --log "$OUT/run$i.log" \
            --play "$OUT/run$i-play.jsonl" ${rss:+--rss "$rss"} ${cpu:+--cpu "$cpu"} >"$OUT/run$i.json"
        tv_step_raw shots >"$OUT/run$i-shots.tar" 2>/dev/null || true
        if [ -s "$OUT/run$i-shots.tar" ]; then
            mkdir -p "$OUT/run$i-shots"
            tar -xf "$OUT/run$i-shots.tar" -C "$OUT/run$i-shots" 2>/dev/null || true
        fi
        rm -f "$OUT/run$i-shots.tar"
        say "run $i: $(grep -E '"(avg_frame_ms|skipped_per_10s|startup_ms|key_downs)"' "$OUT/run$i.json" | tr -d ' \n')"
        i=$((i + 1))
    done
    set -- "$OUT"/run[0-9].json
    "$PY" -I "$BENCH/gate.py" combine "$@" >"$OUT/summary.json"
    if [ -f "$OUTROOT/baseline.json" ]; then
        set +e
        "$PY" -I "$BENCH/gate.py" check "$OUT/summary.json" "$OUTROOT/baseline.json" >"$OUT/gate.md"
        gated=$?
        set -e
        say "gate: $([ "$gated" = 0 ] && echo pass || echo FAIL)"
        cat "$OUT/gate.md"
    fi
}

# One robot Home press with $1 in front: target 5 into $OUT/apps.tsv.
home_from() {
    app=$1 mode=$2
    set +e
    tv run "$CTL key home --hold-ms 180" </dev/null >"$OUT/home-$app-$mode.jsonl"
    rc=$?
    set -e
    if [ "$rc" = 3 ]; then
        say "YIELD: a person used the remote"
        note_person "$OUT/home-$app-$mode.jsonl"
        exit 3
    fi
    press=$(sed -n 's/.*"first":\([0-9.]*\).*/\1/p' "$OUT/home-$app-$mode.jsonl" | head -n 1)
    [ -n "$press" ] || {
        say "$app $mode: the Home press failed: $(tail -n 1 "$OUT/home-$app-$mode.jsonl")"
        return 0
    }
    sleep 3
    tv_step logsince "SINCE=${press%.*}" >"$OUT/home-$app-$mode.log"
    fg=$(value fg "$(tv_step window)")
    "$PY" -I "$BENCH/gate.py" home "$press" "$OUT/home-$app-$mode.log" >"$OUT/home-$app-$mode.json"
    t5=$(sed -n 's/.*"target5_ms": \([0-9]*\).*/\1/p' "$OUT/home-$app-$mode.json")
    say "$app $mode: Home press to Lumen ${t5:-not measured} ms; in front after: ${fg:-?}"
    printf '%s\t%s\thome\t%s\t%s\n' "$app" "$mode" "${t5:-}" "$fg" >>"$OUT/apps.tsv"
    # Lumen must be back in front: the automatic undo's second trigger.
    if [ "$fg" != "$APP" ]; then
        say "the Home press did not bring Lumen back (front: ${fg:-?}); bringing it back by launch"
        tv launch "$APP" </dev/null >/dev/null || true
        sleep 2
        [ "$(value fg "$(tv_step window)")" = "$APP" ] || {
            say "AUTOMATIC UNDO: Lumen does not come back in front"
            exit 4
        }
    fi
}

cmd_apps() {
    FORCE=0 APPS="netflix youtube.leanback.v4 com.beb.plxnative"
    while [ $# -gt 0 ]; do
        case $1 in
            --apps) APPS=$2; shift 2 ;;
            --force) FORCE=1; shift ;;
            *) die "unknown option for apps: $1" ;;
        esac
    done
    for app in $APPS; do
        case $app in *[!A-Za-z0-9._-]* | "$APP") die "--apps: plain app ids other than Lumen's" ;; esac
    done
    [ -f "$ROOT/dist/ctl/lumen-ctl" ] || die "no dist/ctl/lumen-ctl: tools/dev.sh make ctl"
    if [ "$FORCE" = 0 ]; then
        why=$(window_closed)
        [ -z "$why" ] || {
            echo "bench: no lab window: $why"
            exit 5
        }
    fi
    OUT=$OUTROOT/$(date -u +%Y%m%dT%H%M%SZ)-apps
    KEEP=1
    mkdir -p "$OUT"
    BENCH_T0=$(tv run 'date +%s' </dev/null | tr -d '\r')
    trap cleanup EXIT
    trap 'exit 130' INT TERM
    {
        echo "apps $APPS"
        echo "commit $(git -C "$ROOT" rev-parse --short HEAD)"
        echo "binary $(tv_binary_sha)"
        tv check </dev/null | grep -E '"(modelName|firmwareVersion)"' | tr -d ' ,'
    } >"$OUT/run.txt"
    say "report in ${OUT#"$ROOT"/}"
    tv push "$ROOT/dist/ctl/lumen-ctl" "$CTL" >/dev/null
    tv run "chmod 0700 $CTL" </dev/null
    tv_step running >"$OUT/running-before.txt"
    printf 'app\tmode\twhat\tms\tnote\n' >"$OUT/apps.tsv"
    for app in $APPS; do
        for mode in cold warm; do
            cold=0
            [ "$mode" = warm ] || cold=1
            r=$(tv_step applaunch "APP=$app" "COLD=$cold")
            echo "$r" >"$OUT/launch-$app-$mode.txt"
            ms=$(value front_ms "$r")
            err=$(value error "$r")
            was=$(value was_running "$r")
            say "$app $mode (running before: $was): in front after ${ms:-?} ms ${err:+($err)}"
            printf '%s\t%s\tlaunch\t%s\t%s\n' "$app" "$mode" "${ms:-}" "${err:-was_running=$was}" >>"$OUT/apps.tsv"
            # Let it settle in front, then Home.
            sleep 5
            home_from "$app" "$mode"
            sleep 3
        done
    done
    # Put back: close the apps that were not running before.
    tv_step running >"$OUT/running-after.txt"
    for app in $APPS; do
        grep -qx "$app" "$OUT/running-before.txt" ||
            tv close "$app" </dev/null >/dev/null || true
    done
    cat "$OUT/apps.tsv"
}

# The lean track (docs/device/bench.md): a job may be stopped only if
# profiles/lean/candidates.txt lists it and never-touch.txt matches none of
# its name. Prints why not, or nothing.
lean_refused() {
    job=$1
    case $job in *[!A-Za-z0-9._-]* | '') echo "not a plain job name: $job"; return ;; esac
    lower=$(printf '%s' "$job" | tr '[:upper:]' '[:lower:]')
    while read -r word _; do
        case $word in '' | '#'*) continue ;; esac
        case $lower in *"$word"*) echo "$job matches never-touch \"$word\""; return ;; esac
    done <"$LEAN/never-touch.txt"
    grep -qE "^$job([[:space:]]|$)" "$LEAN/candidates.txt" ||
        echo "$job is not in profiles/lean/candidates.txt"
}

# Exits unless a window is open: the night window (lights nothing), or the
# day's, or --force.
lean_window() {
    [ "${1:-}" = --force ] && return 0
    why=$(window_closed night)
    [ -z "$why" ] && return 0
    why2=$(window_closed)
    [ -z "$why2" ] && return 0
    echo "bench: no lab window: $why; $why2"
    exit 5
}

cmd_lean_survey() {
    lean_window "${1:-}"
    mkdir -p "$OUTROOT/../lean"
    out=$OUTROOT/../lean/survey-$(date -u +%Y%m%dT%H%M%SZ).txt
    {
        tv check </dev/null | grep -E '"(modelName|firmwareVersion)"' | tr -d ' ,'
        echo "front $(value fg "$(tv_step window)")"
        tv_step lean-survey
    } >"$out"
    echo "survey: ${out#"$ROOT"/}"
    sed -n '/^===== memory/,/^===== uptime/p' "$out" | sed '$d'
}

cmd_lean_plan() {
    survey=${1:-}
    if [ -z "$survey" ]; then
        for f in "$OUTROOT"/../lean/survey-*.txt; do [ -f "$f" ] && survey=$f; done
    fi
    echo "dry run: what lean-try would stop, from ${survey:-no survey}"
    n=0
    while read -r job why; do
        case $job in '' | '#'*) continue ;; esac
        n=$((n + 1))
        line=''
        [ -z "$survey" ] || line=$(sed -n '/^===== jobs/,/^=====  *processes/p' "$survey" | grep -E "^$job " | head -n 1)
        why_not=$(lean_refused "$job")
        if [ -n "$why_not" ]; then
            echo "  REFUSED $job: $why_not"
        else
            echo "  would stop $job (${line:-not in the survey}) ${why}"
            echo "    tools/bench.sh lean-try $job; undo: tools/bench.sh lean-undo $job, or a reboot"
        fi
    done <"$LEAN/candidates.txt"
    [ "$n" -gt 0 ] || echo "  no candidates yet: profiles/lean/candidates.txt is empty"
}

# The newest backup's age in seconds, or nothing.
backup_age() {
    dir=${LUMEN_BACKUP_DIR:-$HOME/Documents/Lumen-backups}
    newest=''
    for d in "$dir"/[0-9]*T[0-9]*Z; do [ -d "$d" ] && newest=${d##*/}; done
    [ -n "$newest" ] || return 0
    t=$(date -u -d "$(echo "$newest" | sed 's/^\(....\)\(..\)\(..\)T\(..\)\(..\)\(..\)Z$/\1-\2-\3 \4:\5:\6/')" +%s 2>/dev/null) || return 0
    echo $(($(date +%s) - t))
}

cmd_lean_try() {
    job=${1:-}
    [ -n "$job" ] || die "lean-try JOB [--force]"
    why=$(lean_refused "$job")
    [ -z "$why" ] || die "refused: $why"
    age=$(backup_age)
    if [ -z "$age" ] || [ "$age" -ge 86400 ]; then
        die "no backup in the last 24 h: tools/backup.sh first"
    fi
    lean_window "${2:-}"
    mkdir -p "$OUTROOT/../lean"
    r=$(tv_step lean-try "JOB=$job")
    echo "$r"
    err=$(value error "$r")
    [ -z "$err" ] || die "$err"
    printf '%s\t%s\t%s\t%s\ttools/bench.sh lean-undo %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$job" \
        "$(value avail_before_kb "$r")" "$(value avail_after_kb "$r")" "$job" >>"$OUTROOT/../lean/changes.tsv"
    echo "tier B: $job stopped until the next boot; undo: tools/bench.sh lean-undo $job"
}

cmd_lean_undo() {
    if [ -n "${1:-}" ]; then
        tv_step lean-undo "JOB=$1"
    else
        tv_step lean-undo
    fi
}

cmd_gate() {
    [ -f "${1:-}/summary.json" ] || die "no summary.json in ${1:-?}"
    "$PY" -I "$BENCH/gate.py" check "$1/summary.json" "${2:-$OUTROOT/baseline.json}"
}

# The TV lock: a directory holding the owner's pid, never removed on exit
# (each command has its own EXIT trap); a dead pid frees it.
take_lock() {
    mkdir -p "$(dirname "$LOCK")"
    lock_waited=0
    while ! mkdir "$LOCK" 2>/dev/null; do
        holder=$(cat "$LOCK/pid" 2>/dev/null || true)
        if [ -z "$holder" ]; then
            # Taken this instant and its pid not yet written, or left empty.
            sleep 2
            holder=$(cat "$LOCK/pid" 2>/dev/null || true)
            [ -n "$holder" ] || {
                rm -rf "$LOCK"
                continue
            }
        fi
        if [ "$holder" != $$ ] && kill -0 "$holder" 2>/dev/null; then
            [ "$lock_waited" -lt "$LOCK_WAIT_S" ] ||
                die "another TV job (pid $holder) has held the TV for an hour"
            [ "$lock_waited" -gt 0 ] || say "waiting for another TV job (pid $holder)"
            sleep 30
            lock_waited=$((lock_waited + 30))
        else
            rm -rf "$LOCK"
        fi
    done
    echo $$ >"$LOCK/pid"
}

# The yield's time on the TV's clock (the robot's own stamp), else now.
note_person() {
    at=$(sed -n 's/.*"yield":{"at":\([0-9]*\).*/\1/p' "$1" 2>/dev/null | head -n 1)
    [ -n "$at" ] || at=$(tv run 'date +%s' </dev/null | tr -d '\r')
    [ -z "$at" ] || echo "$at" >"$PERSON"
}

cmd_with_lock() {
    [ $# -gt 0 ] || die "with-lock CMD [ARG ...]"
    set +e
    "$@"
    rc=$?
    set -e
    exit "$rc"
}

cmd=${1:-}
[ $# -eq 0 ] || shift
case $cmd in
    window) cmd_window "$@" ;;
    wait-window) cmd_wait_window "$@" ;;
    last-good) cmd_last_good "$@" ;;
    run) take_lock; cmd_run "$@" ;;
    apps) take_lock; cmd_apps "$@" ;;
    lean-survey) take_lock; cmd_lean_survey "$@" ;;
    lean-plan) cmd_lean_plan "$@" ;;
    lean-try) take_lock; cmd_lean_try "$@" ;;
    lean-undo) take_lock; cmd_lean_undo "$@" ;;
    restore) take_lock; cmd_restore ;;
    with-lock) take_lock; cmd_with_lock "$@" ;;
    gate) cmd_gate "$@" ;;
    -h | --help | help | '') usage ;;
    *) die "unknown command: $cmd (tools/bench.sh --help)" ;;
esac
