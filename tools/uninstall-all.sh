#!/bin/sh
# Returns the TV to how it was before Lumen (docs/safety.md, rule 2). A dry run
# unless given --yes: it prints each tools/tv.sh command it would run.
#
#   tools/uninstall-all.sh                         show what would be removed
#   tools/uninstall-all.sh --yes                   remove Lumen's parts
#   tools/uninstall-all.sh --glasshouse --yes      also undo Glasshouse
#   tools/uninstall-all.sh --plx-native --yes      also remove plx-native
#
# Lumen's parts: route D's key filter (takeover/filter-undo.sh) and its
# default-home setting (takeover/undo.sh), both undone first, while Lumen's
# app and saved values are still there; the shell app, kept if the default
# home could not be set back, so the TV never has a Home that isn't
# installed; Lumen's boot hooks (/var/lib/webosbrew/init.d/lumen-*), Lumen
# Guard's resolv.conf mount, nat rules and process
# (profiles/privacy/guard-undo.sh), Lumen Remote's line in root's
# authorized_keys (tools/remote.sh remove), /var/lib/lumen, its kill switch,
# the capture directory and its files in /tmp. --glasshouse runs
# Glasshouse's documented undo (tvwebctl restores, stop, its files and boot
# hooks), then removes its app.
#
# It never touches Homebrew Channel, root, the owner's SSH key or
# /var/luna/preferences/webosbrew_*. Every step runs even if an earlier one
# fails; the exit status says whether all of them succeeded. Cold boot the TV
# afterwards (unplug it) so no bind mount is left.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV_SH=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
REMOTE_SH=${LUMEN_REMOTE_SH:-$ROOT/tools/remote.sh}

LUMEN_APP=io.github.sunnyside007.lumen.shell
GLASSHOUSE_APP=io.github.rorygallagher2024.lg-webos-dashboard
PLX_APP=com.beb.plxnative
TVWEBCTL=/var/lib/tvweb/tvwebctl
INITD=/var/lib/webosbrew/init.d

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "uninstall-all.sh: $*" >&2
    exit 2
}

yes=0 glasshouse=0 plx=0
for arg in "$@"; do
    case $arg in
        --yes) yes=1 ;;
        --glasshouse) glasshouse=1 ;;
        --plx-native) plx=1 ;;
        -h | --help | help) usage; exit 0 ;;
        *) usage >&2; die "unknown argument: $arg" ;;
    esac
done

steps=$(mktemp)
trap 'rm -f "$steps"' EXIT
# Ctrl-C stops here, before the next step, rather than carrying on.
trap 'echo "interrupted; later steps not run" >&2; exit 130' INT
trap 'exit 143' TERM

# step KIND ROLE ARG: KIND is `remove` (an app id), `run` (a command on the
# TV), `script` (a file here, run on the TV) or `remote` (tools/remote.sh
# with ARG as its command). ROLE `restore` marks
# Glasshouse's undo; `purge` steps delete its files and run only if every
# restore succeeded, so a failed one can be retried. ROLE `home` marks the
# default home's undo, and the `app` step, Lumen's removal, runs only if it
# succeeded.
step() {
    printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$steps"
}

# Route D first: the key filter's saved list lives in /var/lib/lumen, and the
# default home must be LG's again before Lumen's app goes.
step script - "$ROOT/takeover/filter-undo.sh"
step script home "$ROOT/takeover/undo.sh"
step remove app "$LUMEN_APP"
step run - "rm -f $INITD/lumen-*"
# Guard's mount and rules first, then Guard, while its files are still there.
step script - "$ROOT/profiles/privacy/guard-undo.sh"
# Lumen Remote's key line, while lumen-ctl is still there for it to name.
step remote - remove
step run - "rm -rf /var/lib/lumen"
step run - "rm -f /var/luna/preferences/lumen_disable"
step run - "if [ -f /media/developer/temp/lumen-capture/lumen-capture.sh ]; then sh /media/developer/temp/lumen-capture/lumen-capture.sh clean; else rm -rf /media/developer/temp/lumen-capture; fi"
step run - "rm -f /tmp/lumen-*"

if [ $glasshouse -eq 1 ]; then
    # Glasshouse's documented undo (docs/TV-CHANGES.md upstream), in order.
    for action in restore-home restore-keys restore-glasshouse-home stop; do
        step run restore "if [ -x $TVWEBCTL ]; then $TVWEBCTL $action; else echo no tvwebctl, skipped $action; fi"
    done
    step run purge "rm -rf /var/lib/tvweb /media/developer/temp/glasshouse-install"
    step run purge "rm -f $INITD/50-tvweb* $INITD/20-tvweb-services $INITD/20-services.sh /var/lib/webosbrew/tvweb-boot.log*"
    step remove purge "$GLASSHOUSE_APP"
fi

if [ $plx -eq 1 ]; then
    step remove - "$PLX_APP"
fi

tab=$(printf '\t')

# The last line of defence for rule 2's exceptions: nothing above, nor any
# script it runs, may name Homebrew Channel, its settings, root's SSH keys or
# the whole init.d. The one exception is the `remote` step: tools/remote.sh
# remove (profiles/remote/remove.sh) edits authorized_keys to take out
# Lumen Remote's own line, keeps every other, and refuses if no key would be
# left; tools/tests/remote.bats checks that.
if {
    cat "$steps"
    while IFS=$tab read -r kind _ arg; do
        if [ "$kind" = script ]; then cat "$arg"; fi
    done <"$steps"
} | grep -E 'webosbrew_|org\.webosbrew|authorized_keys|\.ssh|/home/root|rm -rf /var/lib/webosbrew|init\.d/\*|init\.d( |$)' >/dev/null; then
    die "a step touches something it must never touch; nothing was run"
fi
if [ $yes -eq 0 ]; then
    echo "Dry run. With --yes, these run in order:"
    while IFS=$tab read -r kind role arg; do
        note=
        [ "$role" = purge ] && note="   (only if the restores above succeeded)"
        [ "$role" = app ] && note="   (only if the default home was set back)"
        if [ "$kind" = remove ]; then
            echo "  tools/tv.sh remove $arg$note"
        elif [ "$kind" = script ]; then
            echo "  tools/tv.sh script ${arg#"$ROOT"/}$note"
        elif [ "$kind" = remote ]; then
            echo "  tools/remote.sh $arg$note"
        else
            echo "  tools/tv.sh run '$arg'$note"
        fi
    done <"$steps"
    [ $glasshouse -eq 1 ] || echo "(Glasshouse is kept; add --glasshouse to undo it.)"
    [ $plx -eq 1 ] || echo "(plx-native is kept; add --plx-native to remove it.)"
    exit 0
fi

failed=0 restore_failed=0 home_failed=0
while IFS=$tab read -r kind role arg; do
    if [ "$role" = purge ] && [ $restore_failed -eq 1 ]; then
        echo "> skipped (a Glasshouse restore failed): $kind $arg"
        failed=$((failed + 1))
        continue
    fi
    if [ "$role" = app ] && [ $home_failed -eq 1 ]; then
        echo "> skipped (the default home was not set back): $kind $arg"
        failed=$((failed + 1))
        continue
    fi
    echo "> $kind $arg"
    if [ "$kind" = remote ]; then
        set -- "$REMOTE_SH" "$arg"
    else
        set -- "$TV_SH" "$kind" "$arg"
    fi
    if ! "$@" </dev/null; then
        echo "  (failed; going on)"
        failed=$((failed + 1))
        [ "$role" = restore ] && restore_failed=1
        [ "$role" = home ] && home_failed=1
    fi
done <"$steps"

if [ $failed -eq 0 ]; then
    echo "done. Cold boot the TV (unplug it for 30 s) to clear any bind mounts."
else
    echo "$failed step(s) failed or were skipped; see above. An app that was not installed fails its remove step."
    [ $restore_failed -eq 0 ] || echo "Glasshouse's files were kept so its restores can be retried."
    [ $home_failed -eq 0 ] || echo "Lumen's app was kept, since the TV's Home still names it: tools/takeover.sh undo, then run this again."
    exit 1
fi
