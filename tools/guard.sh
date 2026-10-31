#!/bin/sh
# Installs, checks or removes Lumen Guard on the TV, stage G1: the DNS
# forwarder that logs every lookup (docs/specs/privacy.md, "Lumen Guard").
# Build the binary first: tools/dev.sh make guard
#
#   tools/guard.sh status    whether the hook and binary are installed and
#                            current, Guard's pid, the nat rules, its status
#                            file and what the hook logged
#   tools/guard.sh push      copy the binary, the rules and the hook (inert
#                            there) to /var/lib/lumen/guard/, nothing else: the
#                            by-hand path is then
#                            tools/tv.sh script profiles/privacy/guard-apply.sh
#   tools/guard.sh install   push, then the hook to /var/lib/webosbrew/init.d/,
#                            and run it once. Take a backup first
#                            (tools/backup.sh): this persists
#   tools/guard.sh remove    delete the hook, then remove the nat rules and stop
#                            Guard (profiles/privacy/guard-undo.sh). The log
#                            stays in /var/lib/lumen/guard/ for `log`
#   tools/guard.sh log       copy lookups.log* to tools/out/guard/ and print the
#                            names by count and verdict as a Markdown table
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
BIN=${LUMEN_GUARD_BIN:-$ROOT/dist/guard/lumen-guard}
OUT=${LUMEN_GUARD_OUT:-$ROOT/tools/out/guard}
HOOK=$ROOT/profiles/privacy/lumen-guard
UNDO=$ROOT/profiles/privacy/guard-undo.sh
PROFILE=$ROOT/profiles/glasshouse
RULES="allowlist.txt required-blocks.txt b9-aggressive.hosts"
DIR=/var/lib/lumen/guard
DEST=/var/lib/webosbrew/init.d/lumen-guard
STAGE=/tmp/lumen-guard.new

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "guard.sh: $*" >&2
    exit 1
}

sha() {
    sha256sum <"$1" | cut -d' ' -f1
}

# The binary, the three rule files and the hook's inert copy, each checked,
# then moved into place whole so Guard never reads half a file.
cmd_push() {
    [ -f "$BIN" ] || die "no $BIN: build it with tools/dev.sh make guard"
    "$TV" push "$BIN" "$STAGE/lumen-guard"
    want=$(sha "$BIN")
    for f in $RULES; do
        "$TV" push "$PROFILE/$f" "$STAGE/$f"
        want="$want $(sha "$PROFILE/$f")"
    done
    "$TV" push "$HOOK" "$STAGE/boot-hook"
    want="$want $(sha "$HOOK")"
    got=$("$TV" run "set -e; mkdir -p $DIR/rules
        sh -n $STAGE/boot-hook
        cp $STAGE/lumen-guard $DIR/lumen-guard.tmp && chmod 755 $DIR/lumen-guard.tmp && mv $DIR/lumen-guard.tmp $DIR/lumen-guard
        for f in $RULES; do cp $STAGE/\$f $DIR/rules/\$f.tmp && mv $DIR/rules/\$f.tmp $DIR/rules/\$f; done
        cp $STAGE/boot-hook $DIR/boot-hook.tmp && mv $DIR/boot-hook.tmp $DIR/boot-hook
        rm -rf $STAGE
        cd $DIR && for f in lumen-guard rules/allowlist.txt rules/required-blocks.txt rules/b9-aggressive.hosts boot-hook; do sha256sum <\$f | cut -d' ' -f1; done" </dev/null | tr '\n' ' ')
    [ "$got" = "$want " ] || die "the files on the TV ($got) are not this checkout's ($want)"
    echo "pushed the binary, the rules and the hook to $DIR; SHA-256 checked"
    echo "(a running Guard keeps its old binary and rules until it restarts)"
}

cmd_install() {
    cmd_push
    "$TV" push "$HOOK" "$STAGE.hook"
    # Checked, then moved into place whole, so a boot never meets half a file.
    remote=$("$TV" run "sh -n $STAGE.hook && cp $STAGE.hook $DEST.tmp && chmod 755 $DEST.tmp && mv $DEST.tmp $DEST && rm -f $STAGE.hook && sha256sum < $DEST | cut -d' ' -f1" </dev/null)
    want=$(sha "$HOOK")
    [ "$remote" = "$want" ] || die "the installed hook's SHA-256 ($remote) is not this checkout's ($want)"
    echo "installed $DEST ($want)"
    "$TV" run "$DEST" </dev/null
    echo "ran it once; tools/guard.sh status shows how it started"
}

cmd_status() {
    echo "this checkout: hook $(sha "$HOOK")"
    if [ -f "$BIN" ]; then
        echo "this checkout: binary $(sha "$BIN")"
    else
        echo "this checkout: binary not built (tools/dev.sh make guard)"
    fi
    # Single quotes on purpose: this runs on the TV.
    # shellcheck disable=SC2016
    "$TV" run 'hook=/var/lib/webosbrew/init.d/lumen-guard dir=/var/lib/lumen/guard
        if [ -f $hook ]; then echo "on the TV:    hook $(sha256sum < $hook | cut -d" " -f1)"; else echo "on the TV:    hook not installed"; fi
        if [ -f $dir/lumen-guard ]; then echo "on the TV:    binary $(sha256sum < $dir/lumen-guard | cut -d" " -f1)"; else echo "on the TV:    no binary"; fi
        pid=$(cat /tmp/lumen-guard.pid 2>/dev/null)
        if [ -n "$pid" ] && grep -q lumen-guard /proc/$pid/cmdline 2>/dev/null; then echo "Guard: running, pid $pid"; else echo "Guard: not running"; fi
        iptables -t nat -L OUTPUT -n -v | grep "to:127.0.0.2:53" || echo "no nat rules to 127.0.0.2:53"
        cat $dir/status 2>/dev/null || echo "no status file"
        ls -l $dir/lookups.log* 2>/dev/null || echo "no lookup log"
        tail -n 20 /tmp/lumen-guard.log 2>/dev/null || echo "no hook log since the last boot"' </dev/null
}

cmd_remove() {
    "$TV" run "rm -f $DEST && echo 'removed $DEST'" </dev/null
    "$TV" script "$UNDO" </dev/null
}

# A Python 3 that runs: on Windows `python3` may be the Store's placeholder.
python_cmd() {
    for p in ${LUMEN_PYTHON:-} python3 python; do
        if "$p" -c 'import sys; sys.exit(sys.version_info < (3, 8))' >/dev/null 2>&1; then
            echo "$p"
            return 0
        fi
    done
    return 1
}

# Names come from the TV; only the two log names become local paths.
cmd_log() {
    list=$("$TV" run "cd $DIR 2>/dev/null && for f in lookups.log.1 lookups.log; do [ -f \$f ] && echo \$f; done; true" </dev/null)
    [ -n "$list" ] || die "no lookup log on the TV in $DIR"
    mkdir -p "$OUT"
    rm -f "$OUT/lookups.log" "$OUT/lookups.log.1"
    files=
    for name in $list; do
        case $name in
            lookups.log | lookups.log.1) ;;
            *) die "unexpected file name from the TV: $name" ;;
        esac
        "$TV" run "cat $DIR/$name" </dev/null >"$OUT/$name"
        files="$files $OUT/$name"
    done
    echo "fetched$files" >&2
    if py=$(python_cmd); then
        # shellcheck disable=SC2086 # one word per file
        "$py" -I "$ROOT/tools/guard/lookups.py" $files
    else
        echo "no Python 3 here; summarise with:" >&2
        echo "tools/dev.sh python3 -I tools/guard/lookups.py tools/out/guard/lookups.log*" >&2
    fi
}

case ${1:-} in
    status) cmd_status ;;
    push) cmd_push ;;
    install) cmd_install ;;
    remove) cmd_remove ;;
    log) cmd_log ;;
    -h | --help | help) usage ;;
    *)
        usage >&2
        exit 2
        ;;
esac
