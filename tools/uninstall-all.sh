#!/bin/sh
# Returns the TV to how it was before Lumen (docs/safety.md, rule 2). A dry run
# unless given --yes: it prints each tools/tv.sh command it would run.
#
#   tools/uninstall-all.sh                         show what would be removed
#   tools/uninstall-all.sh --yes                   remove Lumen's parts
#   tools/uninstall-all.sh --glasshouse --yes      also undo Glasshouse
#   tools/uninstall-all.sh --plx-native --yes      also remove plx-native
#
# Lumen's parts: the shell app, /var/lib/lumen, Lumen's boot hooks
# (/var/lib/webosbrew/init.d/lumen-*), its kill switch, the capture directory
# and its files in /tmp. --glasshouse runs Glasshouse's documented undo
# (tvwebctl restores, stop, its files and boot hooks), then removes its app.
#
# It never touches Homebrew Channel, root, the SSH key or
# /var/luna/preferences/webosbrew_*. Every step runs even if an earlier one
# fails; the exit status says whether all of them succeeded. Cold boot the TV
# afterwards (unplug it) so no bind mount is left.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV_SH=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}

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

# step KIND ROLE ARG: KIND is `remove` (an app id) or `run` (a command on the
# TV). ROLE `restore` marks Glasshouse's undo; `purge` steps delete its files
# and run only if every restore succeeded, so a failed one can be retried.
step() {
    printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$steps"
}

step remove - "$LUMEN_APP"
step run - "rm -f $INITD/lumen-*"
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

# The last line of defence for rule 2's exceptions: nothing above may name
# Homebrew Channel, its settings, root's SSH keys or the whole init.d.
if grep -E 'webosbrew_|org\.webosbrew|authorized_keys|\.ssh|/home/root|rm -rf /var/lib/webosbrew|init\.d/\*|init\.d( |$)' "$steps" >/dev/null; then
    die "a step touches something it must never touch; nothing was run"
fi

tab=$(printf '\t')
if [ $yes -eq 0 ]; then
    echo "Dry run. With --yes, these run in order:"
    while IFS=$tab read -r kind role arg; do
        note=
        [ "$role" = purge ] && note="   (only if the restores above succeeded)"
        if [ "$kind" = remove ]; then
            echo "  tools/tv.sh remove $arg$note"
        else
            echo "  tools/tv.sh run '$arg'$note"
        fi
    done <"$steps"
    [ $glasshouse -eq 1 ] || echo "(Glasshouse is kept; add --glasshouse to undo it.)"
    [ $plx -eq 1 ] || echo "(plx-native is kept; add --plx-native to remove it.)"
    exit 0
fi

failed=0 restore_failed=0
while IFS=$tab read -r kind role arg; do
    if [ "$role" = purge ] && [ $restore_failed -eq 1 ]; then
        echo "> skipped (a Glasshouse restore failed): $kind $arg"
        failed=$((failed + 1))
        continue
    fi
    echo "> $kind $arg"
    if ! "$TV_SH" "$kind" "$arg" </dev/null; then
        echo "  (failed; going on)"
        failed=$((failed + 1))
        [ "$role" = restore ] && restore_failed=1
    fi
done <"$steps"

if [ $failed -eq 0 ]; then
    echo "done. Cold boot the TV (unplug it for 30 s) to clear any bind mounts."
else
    echo "$failed step(s) failed or were skipped; see above. An app that was not installed fails its remove step."
    [ $restore_failed -eq 0 ] || echo "Glasshouse's files were kept so its restores can be retried."
    exit 1
fi
