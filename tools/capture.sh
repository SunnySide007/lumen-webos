#!/bin/sh
# Records what the TV looks up and connects to, on the TV itself, for building
# the Glasshouse block list (profiles/glasshouse). Runs on the host and talks
# to the TV through tools/tv.sh. Build the TV's tcpdump first:
# tools/dev.sh make capture-tools
#
#   tools/capture.sh push                  upload tcpdump and the helper
#   tools/capture.sh start [HOURS]         capture in the background (default 24 h)
#   tools/capture.sh status                running?, file sizes, free space
#   tools/capture.sh stop                  end the capture early
#   tools/capture.sh fetch                 copy the pcaps to tools/out/capture/
#   tools/capture.sh clean                 remove everything this put on the TV
#
# Everything on the TV lives in /media/developer/temp/lumen-capture/. The
# capture is time-boxed, size-capped, and ends at a reboot; no boot hook.
# Summarise a fetched capture with:
# tools/dev.sh python3 -I tools/capture/summarize.py tools/out/capture/*.pcap*
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV_SH=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
BIN=${LUMEN_CAPTURE_BIN:-$ROOT/dist/capture/tcpdump}
HELPER=$ROOT/tools/capture/lumen-capture.sh
OUT=${LUMEN_CAPTURE_OUT:-$ROOT/tools/out/capture}
REMOTE=/media/developer/temp/lumen-capture

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "capture.sh: $*" >&2
    exit 1
}

tv() {
    "$TV_SH" "$@"
}

helper() {
    tv run "sh $REMOTE/lumen-capture.sh $*" </dev/null
}

local_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

cmd_push() {
    [ -f "$BIN" ] || die "no $BIN: build it with tools/dev.sh make capture-tools"
    tv push "$BIN" "$REMOTE/tcpdump" </dev/null
    tv push "$HELPER" "$REMOTE/lumen-capture.sh" </dev/null
    want=$(local_sha256 "$BIN")
    got=$(tv run "chmod 0755 $REMOTE/tcpdump $REMOTE/lumen-capture.sh &&
        if command -v sha256sum >/dev/null 2>&1; then sha256sum $REMOTE/tcpdump;
        else echo no-sha256sum; fi" </dev/null | cut -d' ' -f1)
    tv run "$REMOTE/tcpdump --version" </dev/null
    if [ "$got" = no-sha256sum ]; then
        echo "no sha256sum on the TV: only the size was checked (tv.sh push)"
        return 0
    fi
    [ "$got" = "$want" ] || die "tcpdump on the TV has SHA-256 $got, expected $want"
    echo "tcpdump on the TV matches dist/capture/tcpdump ($want)"
}

cmd_start() {
    hours=${1:-24}
    case $hours in '' | *[!0-9]*) die "HOURS must be a whole number, not $hours" ;; esac
    if [ "$hours" -lt 1 ] || [ "$hours" -gt 72 ]; then
        die "HOURS must be 1 to 72, not $hours"
    fi
    helper start "$hours"
}

# Names come from the TV; only plain pcap names become local paths.
cmd_fetch() {
    set -f
    list=$(helper list)
    [ -n "$list" ] || die "no pcap files on the TV"
    mkdir -p "$OUT"
    n=0
    for remote in $list; do
        name=$(basename "$remote")
        case $name in
            *[!A-Za-z0-9._-]*) die "unexpected file name from the TV: $name" ;;
            lumen-*.pcap*) ;;
            *) die "unexpected file name from the TV: $name" ;;
        esac
        tv run "cat $REMOTE/pcap/$name" </dev/null >"$OUT/$name"
        n=$((n + 1))
    done
    echo "fetched $n file(s) to $OUT"
    ls -l "$OUT"
}

command=${1:-help}
[ $# -gt 0 ] && shift

case $command in
    help | -h | --help) usage ;;
    push) cmd_push ;;
    start) cmd_start "${1:-}" ;;
    status) helper status ;;
    stop) helper stop ;;
    fetch) cmd_fetch ;;
    clean) helper clean ;;
    *) usage >&2; die "unknown command: $command" ;;
esac
