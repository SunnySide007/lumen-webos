#!/bin/sh
# Installs, checks or removes Lumen Guard on the TV, and is the owner's way
# to change a running Guard (docs/specs/privacy.md, "Lumen Guard"; decision
# 12: only the owner, from the PC, loosens the TV). Stage G2: Guard decides
# every name by profiles/guard's rules and the owner's own file; it blocks
# only once the owner switches blocking on.
# Build the binary first: tools/dev.sh make guard
#
#   tools/guard.sh status    whether the hook and binary are installed and
#                            current, Guard's pid, the resolver the TV uses and
#                            the mount over connman's resolv.conf, the nat
#                            rules, the mode file, Guard's status file and
#                            what the hook logged
#   tools/guard.sh push      copy the binary, profiles/guard's rules and the
#                            hook (inert there) to /var/lib/lumen/guard/,
#                            nothing else: the by-hand path is then
#                            tools/tv.sh script profiles/privacy/guard-apply.sh
#   tools/guard.sh install   push, then the hook to /var/lib/webosbrew/init.d/,
#                            and run it once. Take a backup first
#                            (tools/backup.sh): this persists
#   tools/guard.sh remove    delete the hook, then unmount Guard's resolv.conf,
#                            remove the nat rules and stop Guard
#                            (profiles/privacy/guard-undo.sh). The log stays in
#                            /var/lib/lumen/guard/ for `log`
#   tools/guard.sh log       copy lookups.log* to tools/out/guard/ and print the
#                            names by count, with what Guard logged and what
#                            this checkout's rules decide now
#
# The owner's controls; a running Guard reads each within a second:
#
#   tools/guard.sh block trial   block until the TV next boots (a reboot is
#                                the undo, as is `block off`)
#   tools/guard.sh block on      block from now on, across reboots: the
#                                family's yes (tier C)
#   tools/guard.sh block off     log only, as G1 did
#   tools/guard.sh learn [MIN]   open a learning window of MIN minutes (default
#                                15, at most 60): names no rule covers resolve,
#                                and are logged as learned. One at a time
#   tools/guard.sh learn stop    close it now
#   tools/guard.sh learned       the names the latest window learned
#   tools/guard.sh keep NAME...  add each to the owner's file (=NAME), then push
#                                it: it resolves for good
#   tools/guard.sh drop NAME...  take each out of the owner's file, then push it
#   tools/guard.sh owner         check the owner's file and push it to the TV
#   tools/guard.sh owner pull    copy the TV's owner file to this PC
#
# The owner's file is private/guard/owner.txt (LUMEN_GUARD_OWNER), outside
# what git tracks: it says what the household uses. Its syntax is the
# allowlist's, plus !name and !=name to block (profiles/guard/README.md).
# Blocking, and a push of the owner's file, are refused when the rules would
# block the store, DRM or the paid apps (tools/guard/rules.py --check).
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
BIN=${LUMEN_GUARD_BIN:-$ROOT/dist/guard/lumen-guard}
OUT=${LUMEN_GUARD_OUT:-$ROOT/tools/out/guard}
OWNER=${LUMEN_GUARD_OWNER:-$ROOT/private/guard/owner.txt}
HOOK=$ROOT/profiles/privacy/lumen-guard
UNDO=$ROOT/profiles/privacy/guard-undo.sh
PROFILE=$ROOT/profiles/guard
DIR=/var/lib/lumen/guard
DEST=/var/lib/webosbrew/init.d/lumen-guard
# Uploads land where only root can write: the app jails share /tmp.
STAGE_ROOT=/var/lib/lumen/stage
STAGE=$STAGE_ROOT/guard

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

# The rule files Guard reads, relative to profiles/guard and to rules/ on the
# TV: plain names only, since they go into commands that run there.
rule_files() {
    (
        cd "$PROFILE" || exit 1
        echo allowlist.txt required-blocks.txt carve-outs.txt
        for f in imported/*.txt; do
            [ -f "$f" ] && echo "$f"
        done
    ) | tr ' ' '\n' | while read -r f; do
        case $f in
            *[!A-Za-z0-9._/-]* | */*/* | .* | */.*) die "unexpected rule file name: $f" ;;
        esac
        echo "$f"
    done
}

make_stage() {
    "$TV" run "umask 077; mkdir -p $STAGE_ROOT && [ ! -L $STAGE_ROOT ] && rm -rf $STAGE $STAGE.hook $STAGE.owner" </dev/null ||
        die "could not make $STAGE_ROOT on the TV"
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

# Refuses rules (with the owner's file, if any) that would block the store,
# DRM or the paid apps, or let a firmware host through.
check_rules() {
    py=$(python_cmd) || die "no Python 3 here to check the rules (tools/guard/rules.py)"
    if [ -f "$OWNER" ]; then
        "$py" -I "$ROOT/tools/guard/rules.py" --check --rules "$PROFILE" --owner "$OWNER" ||
            die "these rules would block what must keep working; nothing changed on the TV"
    else
        "$py" -I "$ROOT/tools/guard/rules.py" --check --rules "$PROFILE" ||
            die "these rules would block what must keep working; nothing changed on the TV"
    fi
}

# The binary, the rule files and the hook's inert copy: checked against this
# checkout while staged, moved into place whole (the rules as one folder, so
# Guard never reads half a set), and checked again where they land. The
# owner's file is not touched.
cmd_push() {
    [ -f "$BIN" ] || die "no $BIN: build it with tools/dev.sh make guard"
    check_rules
    rules=$(rule_files) || die "could not list $PROFILE"
    make_stage
    "$TV" push "$BIN" "$STAGE/lumen-guard"
    want=$(sha "$BIN")
    for f in $rules; do
        "$TV" push "$PROFILE/$f" "$STAGE/rules/$f"
        want="$want $(sha "$PROFILE/$f")"
    done
    "$TV" push "$HOOK" "$STAGE/boot-hook"
    want="$want $(sha "$HOOK")"
    list=$(echo "$rules" | tr '\n' ' ')
    got=$("$TV" run "set -e
        staged=\$(cd $STAGE && for f in lumen-guard; do sha256sum <\$f | cut -d' ' -f1; done; for f in $list; do sha256sum <rules/\$f | cut -d' ' -f1; done; sha256sum <boot-hook | cut -d' ' -f1)
        staged=\$(echo \$staged)
        [ \"\$staged\" = '$want' ] || { rm -rf $STAGE; exit 1; }
        mkdir -p $DIR
        sh -n $STAGE/boot-hook
        cp $STAGE/lumen-guard $DIR/lumen-guard.tmp && chmod 755 $DIR/lumen-guard.tmp && mv $DIR/lumen-guard.tmp $DIR/lumen-guard
        rm -rf $DIR/rules.new $DIR/rules.old
        cp -R $STAGE/rules $DIR/rules.new
        if [ -d $DIR/rules ]; then mv $DIR/rules $DIR/rules.old; fi
        mv $DIR/rules.new $DIR/rules
        rm -rf $DIR/rules.old
        cp $STAGE/boot-hook $DIR/boot-hook.tmp && mv $DIR/boot-hook.tmp $DIR/boot-hook
        rm -rf $STAGE
        cd $DIR && { sha256sum <lumen-guard | cut -d' ' -f1; for f in $list; do sha256sum <rules/\$f | cut -d' ' -f1; done; sha256sum <boot-hook | cut -d' ' -f1; }" </dev/null | tr '\n' ' ')
    [ "$got" = "$want " ] || die "the files on the TV ($got) are not this checkout's ($want)"
    echo "pushed the binary, the rules and the hook to $DIR; SHA-256 checked"
    echo "(a running Guard keeps its old binary and rules until it restarts)"
}

cmd_install() {
    cmd_push
    want=$(sha "$HOOK")
    "$TV" push "$HOOK" "$STAGE.hook"
    # Checked against this checkout while staged, then moved into place whole,
    # so a boot never meets half a file or another one.
    remote=$("$TV" run "sh -n $STAGE.hook && [ \"\$(sha256sum < $STAGE.hook | cut -d' ' -f1)\" = $want ] && cp $STAGE.hook $DEST.tmp && chmod 755 $DEST.tmp && mv $DEST.tmp $DEST && sha256sum < $DEST | cut -d' ' -f1; rm -f $STAGE.hook" </dev/null)
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
    if [ -f "$OWNER" ]; then
        echo "this PC: owner file $(sha "$OWNER") ($OWNER)"
    else
        echo "this PC: no owner file ($OWNER)"
    fi
    # Single quotes on purpose: this runs on the TV.
    # shellcheck disable=SC2016
    "$TV" run 'hook=/var/lib/webosbrew/init.d/lumen-guard dir=/var/lib/lumen/guard log=/var/lib/lumen/log/lumen-guard.log
        if [ -f $hook ]; then echo "on the TV:    hook $(sha256sum < $hook | cut -d" " -f1)"; else echo "on the TV:    hook not installed"; fi
        if [ -f $dir/lumen-guard ]; then echo "on the TV:    binary $(sha256sum < $dir/lumen-guard | cut -d" " -f1)"; else echo "on the TV:    no binary"; fi
        if [ -f $dir/owner.txt ]; then echo "on the TV:    owner file $(sha256sum < $dir/owner.txt | cut -d" " -f1)"; else echo "on the TV:    no owner file"; fi
        if [ -f $dir/mode ]; then echo "mode file: $(cat $dir/mode)"; else echo "mode file: none (log only)"; fi
        pid=$(cat $dir/lumen-guard.pid 2>/dev/null)
        if [ -n "$pid" ] && tr "\0" " " 2>/dev/null </proc/$pid/cmdline | grep -q "lumen-guard --listen"; then echo "Guard: running, pid $pid"; else echo "Guard: not running"; fi
        grep -H "^nameserver" /etc/resolv.conf || echo "/etc/resolv.conf names no nameserver"
        mount | grep resolv || echo "no mount over resolv.conf"
        iptables -t nat -L OUTPUT -n -v | grep "to:127.0.0.2:53" || echo "no nat rules to 127.0.0.2:53"
        cat $dir/status 2>/dev/null || echo "no status file"
        ls -l $dir/lookups.log* 2>/dev/null || echo "no lookup log"
        if [ -f $log ]; then echo "$log, the latest run:"; tail -n 20 $log; else echo "no log: the hook has not run, or its log folder failed its check:"; ls -ld /var/lib/lumen /var/lib/lumen/log 2>&1; fi
        if [ -f $log.1 ]; then echo "$log.1, the run before:"; tail -n 5 $log.1; fi' </dev/null
}

cmd_remove() {
    "$TV" run "rm -f $DEST && echo 'removed $DEST'" </dev/null
    "$TV" script "$UNDO" </dev/null
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
        if [ -f "$OWNER" ]; then
            # shellcheck disable=SC2086 # one word per file
            "$py" -I "$ROOT/tools/guard/lookups.py" $files --owner "$OWNER"
        else
            # shellcheck disable=SC2086 # one word per file
            "$py" -I "$ROOT/tools/guard/lookups.py" $files
        fi
    else
        echo "no Python 3 here; summarise with:" >&2
        echo "tools/dev.sh python3 -I tools/guard/lookups.py tools/out/guard/lookups.log*" >&2
    fi
}

# Writes one of the owner's control files on the TV whole and root's alone,
# from a line built here of plain words.
write_control() {
    "$TV" run "umask 077; mkdir -p $DIR && printf '%s\n' \"$2\" >$DIR/$1.tmp && chmod 600 $DIR/$1.tmp && mv $DIR/$1.tmp $DIR/$1" </dev/null ||
        die "could not write $DIR/$1 on the TV"
}

cmd_block() {
    case ${1:-} in
        on)
            check_rules
            write_control mode block
            echo "blocking on, across reboots; tools/guard.sh block off undoes it"
            ;;
        trial)
            check_rules
            id=$("$TV" run "cat /proc/sys/kernel/random/boot_id" </dev/null | tr -d '\r')
            case $id in
                '' | *[!0-9a-f-]*) die "no boot id from the TV ('$id')" ;;
            esac
            write_control mode "block $id"
            echo "blocking on until the TV next boots; tools/guard.sh block off, or a reboot, undoes it"
            ;;
        off)
            "$TV" run "rm -f $DIR/mode" </dev/null || die "could not remove $DIR/mode on the TV"
            echo "blocking off: Guard logs only"
            ;;
        *) die "usage: tools/guard.sh block on|trial|off" ;;
    esac
    echo "a running Guard reads it within a second (tools/guard.sh status: mode, block_refused)"
}

cmd_learn() {
    case ${1:-15} in
        stop)
            "$TV" run "rm -f $DIR/learn" </dev/null || die "could not remove $DIR/learn on the TV"
            echo "learning window closed; tools/guard.sh learned lists what it learned"
            return
            ;;
        '' | *[!0-9]*) die "usage: tools/guard.sh learn [MINUTES, 1 to 60] | learn stop" ;;
    esac
    min=$1
    if [ "$min" -lt 1 ] || [ "$min" -gt 60 ]; then
        die "a learning window is 1 to 60 minutes, not $min"
    fi
    # One window at a time, by the TV's own clock.
    # shellcheck disable=SC2016 # $now and $end are the TV's
    out=$("$TV" run "now=\$(date +%s)
        if [ -f $DIR/learn ]; then set -- \$(cat $DIR/learn); if [ \"\${2:-0}\" -gt \"\$now\" ] 2>/dev/null; then echo \"open \$2\"; exit 0; fi; fi
        end=\$((now + $min * 60))
        umask 077; mkdir -p $DIR && echo \"\$now \$end\" >$DIR/learn.tmp && chmod 600 $DIR/learn.tmp && mv $DIR/learn.tmp $DIR/learn && echo \"new \$end\"" </dev/null) ||
        die "could not open a learning window on the TV"
    case $out in
        "open "*) die "a learning window is already open, until ${out#open } (unix time); tools/guard.sh learn stop closes it" ;;
        "new "*) echo "learning for $min minutes, until ${out#new } (unix time, the TV's clock): open the new app now" ;;
        *) die "unexpected answer from the TV: $out" ;;
    esac
    echo "then: tools/guard.sh learned, and keep or drop each name"
}

# The names logged as learned since the latest window opened.
cmd_learned() {
    # shellcheck disable=SC2016 # awk's own $ fields, on the TV
    names=$("$TV" run "cd $DIR 2>/dev/null || exit 0
        start=0; [ -f learn ] && set -- \$(cat learn) && start=\${1:-0}
        for f in lookups.log.1 lookups.log; do [ -f \$f ] && cat \$f; done |
            awk -v s=\"\$start\" '\$1 >= s && \$4 == \"learn\" { n[\$3]++ } END { for (k in n) print n[k], k }' | sort -rn" </dev/null)
    if [ -z "$names" ]; then
        echo "no names learned (open a window with tools/guard.sh learn, then use the app)"
        return
    fi
    echo "learned, by lookups (the names come from the TV's log):"
    echo "$names" | while read -r n name; do
        case $name in
            *[!a-z0-9._-]*) echo "  $n  (a name with odd characters, left out)" ;;
            *) echo "  $n  $name" ;;
        esac
    done
    echo "keep: tools/guard.sh keep NAME...   drop: tools/guard.sh drop NAME..."
}

# A host name as a rule: lower case letters, digits, '-', '_' and dots.
check_names() {
    [ $# -gt 0 ] || die "name at least one host name"
    for name in "$@"; do
        case $name in
            '' | *[!a-z0-9._-]* | .* | *. | *..*) die "not a plain lower-case host name: $name" ;;
        esac
    done
}

cmd_owner_push() {
    [ -f "$OWNER" ] || die "no owner file at $OWNER (tools/guard.sh keep NAME starts one; owner pull fetches the TV's)"
    check_rules
    want=$(sha "$OWNER")
    make_stage
    "$TV" push "$OWNER" "$STAGE.owner"
    got=$("$TV" run "[ \"\$(sha256sum < $STAGE.owner | cut -d' ' -f1)\" = $want ] && mkdir -p $DIR && cp $STAGE.owner $DIR/owner.txt.tmp && chmod 600 $DIR/owner.txt.tmp && mv $DIR/owner.txt.tmp $DIR/owner.txt && sha256sum < $DIR/owner.txt | cut -d' ' -f1; rm -f $STAGE.owner" </dev/null)
    [ "$got" = "$want" ] || die "the TV's owner file ($got) is not this PC's ($want)"
    echo "pushed $OWNER to $DIR/owner.txt; SHA-256 checked. A running Guard reads it within a second"
}

cmd_owner_pull() {
    mkdir -p "$(dirname "$OWNER")"
    tmp=$OWNER.pull
    "$TV" run "cat $DIR/owner.txt" </dev/null >"$tmp" || {
        rm -f "$tmp"
        die "no owner file on the TV"
    }
    if [ -f "$OWNER" ]; then
        cp "$OWNER" "$OWNER.bak"
        echo "kept this PC's earlier copy as $OWNER.bak"
    fi
    mv "$tmp" "$OWNER"
    echo "copied the TV's owner file to $OWNER"
}

new_owner_file() {
    mkdir -p "$(dirname "$OWNER")"
    [ -f "$OWNER" ] || cat >"$OWNER" <<'EOF'
# Lumen Guard: the owner's own rules (decision 12). Not in git: it says what
# the household uses. tools/guard.sh owner pushes it to the TV.
#   name     allows the name and every name under it
#   =name    allows that name only
#   !name    blocks the name and every name under it
#   !=name   blocks that name only
EOF
}

cmd_keep() {
    check_names "$@"
    new_owner_file
    for name in "$@"; do
        if awk -v n="$name" '{ r = $0; sub(/#.*/, "", r); gsub(/[ \t]/, "", r) }
            r == "=" n { found = 1 } END { exit !found }' "$OWNER"; then
            echo "already kept: $name"
        else
            printf '=%s  # kept %s\n' "$name" "$(date -u +%Y-%m-%d)" >>"$OWNER"
            echo "kept: $name"
        fi
    done
    cmd_owner_push
}

cmd_drop() {
    check_names "$@"
    [ -f "$OWNER" ] || die "no owner file at $OWNER: nothing to drop"
    cp "$OWNER" "$OWNER.new"
    for name in "$@"; do
        # The name's allow rules, exact or not, with or without a comment.
        awk -v n="$name" '{ r = $0; sub(/#.*/, "", r); gsub(/[ \t]/, "", r) }
            r == n || r == "=" n { next } { print }' "$OWNER.new" >"$OWNER.tmp"
        if cmp -s "$OWNER.new" "$OWNER.tmp"; then
            echo "not in the owner's file: $name (it stays blocked)"
        else
            echo "dropped: $name"
        fi
        mv "$OWNER.tmp" "$OWNER.new"
    done
    mv "$OWNER.new" "$OWNER"
    cmd_owner_push
}

case ${1:-} in
    status) cmd_status ;;
    push) cmd_push ;;
    install) cmd_install ;;
    remove) cmd_remove ;;
    log) cmd_log ;;
    block) cmd_block "${2:-}" ;;
    learn) cmd_learn "${2:-15}" ;;
    learned) cmd_learned ;;
    keep) shift; cmd_keep "$@" ;;
    drop) shift; cmd_drop "$@" ;;
    owner)
        case ${2:-push} in
            push) cmd_owner_push ;;
            pull) cmd_owner_pull ;;
            *) die "usage: tools/guard.sh owner [push|pull]" ;;
        esac
        ;;
    -h | --help | help) usage ;;
    *)
        usage >&2
        exit 2
        ;;
esac
