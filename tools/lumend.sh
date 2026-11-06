#!/bin/sh
# Installs, tries, checks or removes lumend, Lumen's root daemon on the TV
# (decision 14; shell/crates/lumend). Build it first: tools/dev.sh make lumend
# Every command acts on the TV as root over tools/tv.sh: ask the owner first.
#
#   tools/lumend.sh push          copy lumend, the stand-in and the door
#                                 (inert there) to /var/lib/lumen/lumend/,
#                                 SHA-256 checked; refused while lumend runs
#                                 or a stand-in is mounted
#   tools/lumend.sh role install GROUP...
#                                 lumend's luna role, in the groups the owner
#                                 picked from `groups` (profiles/lumend/
#                                 role-install.sh); `role remove` takes it away
#   tools/lumend.sh groups        which permission groups hold each method
#                                 lumend calls (read only)
#   tools/lumend.sh check         what lumend would use (read only)
#   tools/lumend.sh conf          lumend.conf, as on the TV
#   tools/lumend.sh conf on|off PART...
#                                 switch parts on or off in lumend.conf: s1
#                                 senders lg-home icons modes shell screen-off
#                                 link, and sender:acr2
#   tools/lumend.sh try PART [SENDER...]   (or try shell preload: fact 10)
#                                 one part once, by hand; lumend must not run
#   tools/lumend.sh undo PART     a part's undo (`undo all`: every part's)
#   tools/lumend.sh start|stop    the daemon, by hand
#   tools/lumend.sh status        whether it runs, and its status file
#   tools/lumend.sh log           its log, the latest run and the one before
#   tools/lumend.sh door install  the door as a boot hook, retiring the hooks
#                                 lumend replaces (profiles/lumend/
#                                 door-install.sh); take a backup first
#                                 (tools/backup.sh): this persists
#   tools/lumend.sh door remove   the door away, the retired hooks back
#   tools/lumend.sh remove        everything above taken away
#                                 (profiles/lumend/remove.sh)
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
DIST=${LUMEN_LUMEND_DIST:-$ROOT/dist/lumend}
PROFILE=$ROOT/profiles/lumend
OUT=${LUMEN_LUMEND_OUT:-$ROOT/tools/out/lumend}
DIR=/var/lib/lumen/lumend
BIN=$DIR/lumend
FILES="lumend lumen-standin door"
PARTS="s1 senders lg-home icons modes shell screen-off link"
SENDERS="acr2"
# Uploads land where only root can write: the app jails share /tmp.
STAGE_ROOT=/var/lib/lumen/stage
STAGE=$STAGE_ROOT/lumend

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "lumend.sh: $*" >&2
    exit 1
}

sha() {
    sha256sum <"$1" | cut -d' ' -f1
}

# one_of WORD LIST: whether WORD is one of LIST's words.
one_of() {
    for w in $2; do
        [ "$1" = "$w" ] && return 0
    done
    return 1
}

part_ok() {
    one_of "$1" "$PARTS" || die "no part named $1 (parts: $PARTS)"
}

make_stage() {
    "$TV" run "umask 077; mkdir -p $STAGE_ROOT && [ ! -L $STAGE_ROOT ] && rm -rf $STAGE && mkdir $STAGE" </dev/null ||
        die "could not make $STAGE on the TV"
}

# The three files: checked against this checkout while staged, moved into
# place whole, and checked again where they land.
cmd_push() {
    for f in $FILES; do
        [ -f "$DIST/$f" ] || die "no $DIST/$f: build it with tools/dev.sh make lumend"
    done
    busy=$("$TV" run "if [ -x $BIN ] && $BIN status 2>/dev/null | grep -q '^running'; then echo running; elif grep -q ' /usr/sbin/acr2 ' /proc/mounts; then echo mounted; fi; true" </dev/null)
    case $busy in
        running) die "lumend runs: tools/lumend.sh stop first" ;;
        mounted) die "a stand-in is mounted: tools/lumend.sh undo senders first" ;;
    esac
    make_stage
    want=
    for f in $FILES; do
        "$TV" push "$DIST/$f" "$STAGE/$f"
        want="$want $(sha "$DIST/$f")"
    done
    got=$("$TV" run "set -e
        staged=\$(cd $STAGE && for f in $FILES; do sha256sum <\$f | cut -d' ' -f1; done | tr '\n' ' ')
        [ \"\$staged\" = '${want# } ' ] || { rm -rf $STAGE; exit 1; }
        sh -n $STAGE/door
        umask 077
        mkdir -p $DIR
        [ ! -L $DIR ] && chmod 700 $DIR
        for f in lumend lumen-standin; do cp $STAGE/\$f $DIR/\$f.tmp && chmod 755 $DIR/\$f.tmp && mv -f $DIR/\$f.tmp $DIR/\$f; done
        cp $STAGE/door $DIR/door.tmp && chmod 644 $DIR/door.tmp && mv -f $DIR/door.tmp $DIR/door
        rm -rf $STAGE
        cd $DIR && for f in $FILES; do sha256sum <\$f | cut -d' ' -f1; done" </dev/null | tr '\n' ' ')
    [ "$got" = "${want# } " ] || die "the files on the TV ($got) are not this checkout's (${want# })"
    echo "pushed lumend, the stand-in and the door (inert) to $DIR; SHA-256 checked"
}

# A profile script with its placeholder filled, run on the TV.
run_filled() {
    mkdir -p "$OUT"
    filled=$OUT/${1##*/}
    sed "s/@LUMEND_GROUPS@/$2/" "$1" >"$filled"
    "$TV" script "$filled" </dev/null
}

cmd_role() {
    case ${1:-} in
        install)
            shift
            [ $# -gt 0 ] || die "name the groups: tools/lumend.sh groups lists them"
            for g in "$@"; do
                case $g in
                    '' | *[!A-Za-z0-9._-]*) die "an odd group name: $g" ;;
                esac
            done
            run_filled "$PROFILE/role-install.sh" "$*"
            ;;
        remove) "$TV" script "$PROFILE/role-remove.sh" </dev/null ;;
        *) die "role install GROUP... or role remove" ;;
    esac
}

# conf on|off WORD...: the TV's lumend.conf, rewritten whole from what it
# says now and the switches asked for.
cmd_conf() {
    if [ $# -eq 0 ]; then
        now=$("$TV" run "cat $DIR/lumend.conf 2>/dev/null; true" </dev/null)
        printf '%s\n' "${now:-(no lumend.conf: every part is off)}"
        return 0
    fi
    case $1 in
        on | off) want=$1 ;;
        *) die "conf on|off PART..." ;;
    esac
    shift
    [ $# -gt 0 ] || die "conf $want PART..."
    for w in "$@"; do
        case $w in
            sender:*) one_of "${w#sender:}" "$SENDERS" || die "no sender named ${w#sender:} (senders: $SENDERS)" ;;
            *) part_ok "$w" ;;
        esac
    done
    now=$("$TV" run "cat $DIR/lumend.conf 2>/dev/null; true" </dev/null)
    text="# lumend's switches, written by tools/lumend.sh conf."
    for p in $PARTS $(for s in $SENDERS; do echo "sender:$s"; done); do
        case $p in
            sender:*) line="sender ${p#sender:}" ;;
            *) line=$p ;;
        esac
        state=off
        if printf '%s\n' "$now" | grep -qE "^[[:space:]]*${line}[[:space:]]+on[[:space:]]*\$"; then
            state=on
        fi
        one_of "$p" "$*" && state=$want
        text="$text
$line $state"
    done
    mkdir -p "$OUT"
    printf '%s\n' "$text" >"$OUT/lumend.conf"
    make_stage
    "$TV" push "$OUT/lumend.conf" "$STAGE/lumend.conf"
    want_sha=$(sha "$OUT/lumend.conf")
    got=$("$TV" run "[ \"\$(sha256sum <$STAGE/lumend.conf | cut -d' ' -f1)\" = $want_sha ] && [ -d $DIR ] && [ ! -L $DIR ] && cp $STAGE/lumend.conf $DIR/lumend.conf.tmp && chmod 600 $DIR/lumend.conf.tmp && mv -f $DIR/lumend.conf.tmp $DIR/lumend.conf && sha256sum <$DIR/lumend.conf | cut -d' ' -f1; rm -rf $STAGE" </dev/null)
    [ "$got" = "$want_sha" ] || die "lumend.conf on the TV is not the one written here"
    printf '%s\n' "$text"
    echo "(a running lumend reads it at its next start)"
}

cmd_door() {
    case ${1:-} in
        install)
            want=$(sha "$DIST/door")
            got=$("$TV" script "$PROFILE/door-install.sh" </dev/null | tee /dev/stderr | tail -n 1)
            [ "$got" = "$want" ] || die "the installed door's SHA-256 ($got) is not this checkout's ($want)"
            ;;
        remove) "$TV" script "$PROFILE/door-remove.sh" </dev/null ;;
        *) die "door install or door remove" ;;
    esac
}

cmd_log() {
    # Single quotes on purpose: this runs on the TV.
    # shellcheck disable=SC2016
    "$TV" run 'log=/var/lib/lumen/log/lumend.log
        if [ -f $log.1 ]; then echo "$log.1, the run before:"; tail -n 20 $log.1; fi
        if [ -f $log ]; then echo "$log, the latest run:"; tail -n 200 $log; else echo "no $log"; fi' </dev/null
}

# lumend's own commands, by name, with checked words only.
lumend_cmd() {
    "$TV" run "$BIN $*" </dev/null
}

case ${1:-} in
    push) cmd_push ;;
    role) shift; cmd_role "$@" ;;
    groups | check | status | start | stop) lumend_cmd "$1" ;;
    conf) shift; cmd_conf "$@" ;;
    try)
        [ $# -ge 2 ] || die "try PART [SENDER...]"
        part_ok "$2"
        if [ "$2 ${3:-}" = "shell preload" ]; then
            [ $# -eq 3 ] || die "try shell [preload]"
        else
            for s in $(shift 2; echo "$@"); do
                one_of "$s" "$SENDERS" || die "no sender named $s (senders: $SENDERS)"
            done
        fi
        shift
        lumend_cmd try "$@"
        ;;
    undo)
        [ $# -eq 2 ] || die "undo PART"
        [ "$2" = all ] || part_ok "$2"
        lumend_cmd undo "$2"
        ;;
    log) cmd_log ;;
    door) shift; cmd_door "$@" ;;
    remove) "$TV" script "$PROFILE/remove.sh" </dev/null ;;
    -h | --help | help) usage ;;
    *)
        usage >&2
        exit 2
        ;;
esac
