#!/bin/sh
# Route D on the TV (takeover/README.md): Lumen as the B9's Home through LG's
# own setting, the boot hook that closes LG's Home after boot, and the
# long-Home key filter. The TV runs takeover/*.sh through tools/tv.sh script,
# the same files tools/uninstall-all.sh runs. The hook and the filter go to
# the TV inside the script that installs them, with the PC's SHA-256, and are
# staged in root's /var/lib/lumen/stage, never in /tmp, which every app's
# jail shares.
#
#   tools/takeover.sh status          the default home, Lumen's app, the saved
#                                     value, LG's Home, the hook and the key
#                                     filter, with their SHA-256 sums compared
#                                     to this checkout's
#   tools/takeover.sh apply           Lumen's id into settingsservice's
#                                     defaultApps.home, the value it replaces
#                                     saved once. Take a backup first
#                                     (tools/backup.sh): this persists
#   tools/takeover.sh undo            the saved value back, or LG's Home
#   tools/takeover.sh hook status     the boot hook: installed or not, its
#                                     by-hand copy, its log
#   tools/takeover.sh hook push       the hook's inert copy to
#                                     /var/lib/lumen/takeover/boot-hook, nothing
#                                     else: then tools/tv.sh script
#                                     takeover/home-apply.sh runs it by hand
#   tools/takeover.sh hook install    push, then the hook to
#                                     /var/lib/webosbrew/init.d/lumen-home, from
#                                     the next boot on. Only after the by-hand
#                                     run and the owner's yes: this persists
#   tools/takeover.sh hook remove     delete the installed hook
#   tools/takeover.sh filter check    the compositor's key-filter list, Lumen's
#                                     entry in it and its file
#   tools/takeover.sh filter apply [--noop]
#                                     the owner's decision, step by step as the
#                                     README's trial says: Lumen's long-Home
#                                     filter into the list, or with --noop the
#                                     build that only logs. This persists at
#                                     once, and a reboot doesn't undo it
#   tools/takeover.sh filter undo     Lumen's entry out of the list
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
HOOK=$ROOT/takeover/lumen-home
FILTER=$ROOT/takeover/filters/lumen-home.js
DIR=/var/lib/lumen/takeover
DEST=/var/lib/webosbrew/init.d/lumen-home
LOG=/var/lib/lumen/log/lumen-home.log
# Ends the here-document a generated script carries its file in.
END_MARK=LUMEN_STAGED_FILE_END

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "takeover.sh: $*" >&2
    exit 1
}

sha() {
    sha256sum <"$1" | cut -d' ' -f1
}

# build_filter OUT NOOP: the filter as the TV gets it; NOOP 1 is the build
# that acts on nothing.
build_filter() {
    if [ "$2" = 1 ]; then
        sed 's/^var lumenHomeActs = true;$/var lumenHomeActs = false;/' "$FILTER" >"$1"
        grep -qx 'var lumenHomeActs = false;' "$1" || die "could not make the no-op build of $FILTER"
    else
        cp "$FILTER" "$1"
    fi
}

# The TV's engines parse ES5 only. A grep catches the newer syntax, node
# --check (where node is here) any syntax error, and the TV's node 0.12
# compiles the file again before it is used.
check_filter() {
    if sed 's|//.*$||' "$1" | grep -nE '=>|`|\b(let|const|class)\b|\.\.\.'; then
        die "$FILTER uses syntax newer than ES5 (above)"
    fi
    grep -q '^function lumenHomeKeys(key, pressed, autoRepeat) {$' "$1" ||
        die "$FILTER has no lumenHomeKeys(key, pressed, autoRepeat)"
    if command -v node >/dev/null 2>&1; then
        node --check "$1" || die "$FILTER does not parse"
    else
        echo "no node here; the TV's node checks the file"
    fi
}

cmd_status() {
    hook_sha=$(sha "$HOOK")
    tmp=$(mktemp)
    build_filter "$tmp" 0
    real_sha=$(sha "$tmp")
    build_filter "$tmp" 1
    noop_sha=$(sha "$tmp")
    rm -f "$tmp"
    echo "this checkout: hook $hook_sha"
    echo "this checkout: filter $real_sha, no-op build $noop_sha"
    out=$("$TV" script "$ROOT/takeover/status.sh" </dev/null)
    printf '%s\n' "$out"
    echo "== against this checkout"
    case $(printf '%s\n' "$out" | sed -n 's/^hook in init.d: //p') in
        none) echo "hook: not installed" ;;
        "$hook_sha") echo "hook: installed, this checkout's" ;;
        '') echo "hook: unknown" ;;
        *) echo "hook: installed, but NOT this checkout's" ;;
    esac
    case $(printf '%s\n' "$out" | sed -n 's/^filter file: //p') in
        none) echo "filter file: none" ;;
        "$real_sha") echo "filter file: this checkout's filter" ;;
        "$noop_sha") echo "filter file: this checkout's no-op build" ;;
        '') echo "filter file: unknown" ;;
        *) echo "filter file: NOT this checkout's" ;;
    esac
}

cmd_apply() {
    echo "tools/backup.sh comes first: this changes the TV's Home until tools/takeover.sh undo."
    "$TV" script "$ROOT/takeover/apply.sh" </dev/null
}

cmd_undo() {
    "$TV" script "$ROOT/takeover/undo.sh" </dev/null
}

# generate TEMPLATE PAYLOAD [NAME=VALUE...]: the TV script TEMPLATE, after
# WANT, the PC's SHA-256 of PAYLOAD, each NAME=VALUE, and staged_copy, which
# prints PAYLOAD from a quoted here-document (the shell expands nothing in
# it). The TV writes its root-only stage from staged_copy and checks it
# against WANT before it uses it, so the file and the sum the TV trusts both
# come in the script, never from a file a jail could reach, and a copy that
# differs by a byte installs nothing.
generate() {
    template=$1 payload=$2
    shift 2
    [ -z "$(tail -c 1 "$payload")" ] || die "$payload does not end in a newline"
    if grep -qx "$END_MARK" "$payload"; then
        die "$payload holds the line $END_MARK"
    fi
    printf 'WANT=%s\n' "$(sha "$payload")"
    for assignment in "$@"; do
        printf '%s\n' "$assignment"
    done
    printf "staged_copy() {\n    cat <<'%s'\n" "$END_MARK"
    cat "$payload"
    printf '%s\n}\n' "$END_MARK"
    cat "$template"
}

# hook_put INITD: the hook to its by-hand copy, and with INITD 1 into init.d,
# through takeover/hook-install.sh; each copy's SHA-256 is compared again.
hook_put() {
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    generate "$ROOT/takeover/hook-install.sh" "$HOOK" "INITD=$1" >"$work/hook-install.sh"
    out=$("$TV" script "$work/hook-install.sh" </dev/null) || {
        printf '%s\n' "$out"
        die "the hook was not put on the TV"
    }
    printf '%s\n' "$out"
    want=$(sha "$HOOK")
    got=$(printf '%s\n' "$out" | sed -n 's/^boot-hook: //p')
    [ "$got" = "$want" ] || die "the hook's copy on the TV ($got) is not this checkout's ($want)"
    if [ "$1" = 1 ]; then
        got=$(printf '%s\n' "$out" | sed -n 's/^init\.d: //p')
        [ "$got" = "$want" ] || die "the installed hook's SHA-256 ($got) is not this checkout's ($want)"
    fi
}

cmd_hook_push() {
    hook_put 0
    echo "pushed the hook to $DIR/boot-hook, inert there:"
    echo "tools/tv.sh script takeover/home-apply.sh runs it by hand"
}

cmd_hook_install() {
    hook_put 1
    echo "installed $DEST; it runs from the next boot"
    echo "(tools/tv.sh script takeover/home-apply.sh runs it now)"
}

cmd_hook_status() {
    echo "this checkout: hook $(sha "$HOOK")"
    "$TV" run "for f in $DEST $DIR/boot-hook; do if [ -f \$f ]; then echo \"\$f: \$(sha256sum < \$f | cut -d' ' -f1)\"; else echo \"\$f: none\"; fi; done
        if [ -f $LOG ]; then echo '$LOG, the latest run:'; tail -n 30 $LOG; else echo 'no log: the hook has not run, or its log folder failed its check:'; ls -ld /var/lib/lumen /var/lib/lumen/log 2>&1; fi
        if [ -f $LOG.1 ]; then echo '$LOG.1, the run before:'; tail -n 5 $LOG.1; fi" </dev/null
}

cmd_hook_remove() {
    "$TV" run "rm -f $DEST && echo 'removed $DEST'" </dev/null
}

cmd_filter_apply() {
    noop=0
    case ${1:-} in
        --noop) noop=1 ;;
        '') ;;
        *) usage >&2; exit 2 ;;
    esac
    # A .js name, which node --check wants.
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    tmp=$work/lumen-home.js
    build_filter "$tmp" "$noop"
    check_filter "$tmp"
    if [ "$noop" = 1 ]; then
        echo "the no-op build ($(sha "$tmp")): it times Home presses and logs, and acts on nothing"
    else
        echo "the filter ($(sha "$tmp")): a long Home press opens LG's Home"
    fi
    echo "tools/backup.sh comes first. configd keeps the list across reboots: tools/takeover.sh filter undo takes it out."
    generate "$ROOT/takeover/filter-apply.sh" "$tmp" >"$work/filter-apply.sh"
    "$TV" script "$work/filter-apply.sh" </dev/null
}

case ${1:-} in
    status) cmd_status ;;
    apply) cmd_apply ;;
    undo) cmd_undo ;;
    hook)
        case ${2:-} in
            status) cmd_hook_status ;;
            push) cmd_hook_push ;;
            install) cmd_hook_install ;;
            remove) cmd_hook_remove ;;
            *) usage >&2; exit 2 ;;
        esac
        ;;
    filter)
        case ${2:-} in
            check) "$TV" script "$ROOT/takeover/filter-check.sh" </dev/null ;;
            apply) cmd_filter_apply "${3:-}" ;;
            undo) "$TV" script "$ROOT/takeover/filter-undo.sh" </dev/null ;;
            *) usage >&2; exit 2 ;;
        esac
        ;;
    -h | --help | help) usage ;;
    *)
        usage >&2
        exit 2
        ;;
esac
