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
# Everything on the TV lives in /var/lib/lumen/capture/, a folder only root
# may use: root runs tcpdump and the helper from it, and tcpdump writes the
# pcaps there, so no app can swap either or point a write elsewhere. Both are
# uploaded to root's stage and checked against this checkout before they are
# moved there. The capture is time-boxed, size-capped (100 MB by default, with
# 100 MB of /var kept free for webOS), and ends at a reboot; no boot hook.
# Summarise a fetched capture with:
# tools/dev.sh python3 -I tools/capture/summarize.py tools/out/capture/*.pcap*
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV_SH=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
BIN=${LUMEN_CAPTURE_BIN:-$ROOT/dist/capture/tcpdump}
HELPER=$ROOT/tools/capture/lumen-capture.sh
OUT=${LUMEN_CAPTURE_OUT:-$ROOT/tools/out/capture}
REMOTE=/var/lib/lumen/capture
STAGE=/var/lib/lumen/stage/capture

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

# Both files checked against this checkout while staged, moved whole into a
# capture folder that is real, root's and writable by no one else, and
# checked again where they land.
cmd_push() {
    [ -f "$BIN" ] || die "no $BIN: build it with tools/dev.sh make capture-tools"
    tv run "rm -rf $STAGE" </dev/null
    tv push "$BIN" "$STAGE/tcpdump" </dev/null
    tv push "$HELPER" "$STAGE/lumen-capture.sh" </dev/null
    want="$(local_sha256 "$BIN") $(local_sha256 "$HELPER")"
    got=$(tv run "set -e
        staged=\$(cd $STAGE && for f in tcpdump lumen-capture.sh; do sha256sum <\$f | cut -d' ' -f1; done | tr '\n' ' ')
        [ \"\$staged\" = '$want ' ] || { rm -rf $STAGE; exit 1; }
        umask 077
        mkdir -p $REMOTE
        if [ -L $REMOTE ] || [ ! -d $REMOTE ]; then rm -rf $STAGE; exit 1; fi
        set -- \$(ls -ldn $REMOTE)
        case \$1 in d????[!w]??[!w]*) ;; *) rm -rf $STAGE; exit 1 ;; esac
        [ \"\$3\" = 0 ] || { rm -rf $STAGE; exit 1; }
        for f in tcpdump lumen-capture.sh; do cp $STAGE/\$f $REMOTE/\$f.tmp && chmod 755 $REMOTE/\$f.tmp && mv $REMOTE/\$f.tmp $REMOTE/\$f; done
        rm -rf $STAGE
        cd $REMOTE && for f in tcpdump lumen-capture.sh; do sha256sum <\$f | cut -d' ' -f1; done" </dev/null | tr '\n' ' ')
    [ "$got" = "$want " ] ||
        die "tcpdump and the helper on the TV (${got:-none}) are not this checkout's ($want), or $REMOTE is not root's alone"
    tv run "$REMOTE/tcpdump --version" </dev/null
    echo "tcpdump and the helper on the TV match this checkout ($want)"
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
