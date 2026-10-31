#!/bin/sh
# Installs, checks or removes the privacy boot hook on the TV
# (profiles/privacy/lumen-privacy; docs/specs/privacy.md, stage S1).
#
#   tools/privacy-hook.sh status    whether it is installed and current, what
#                                   it logged, and the jobs and rule it keeps
#   tools/privacy-hook.sh install   copy it to /var/lib/webosbrew/init.d/ and
#                                   run it once now. Take a backup first
#                                   (tools/backup.sh): this persists
#   tools/privacy-hook.sh remove    delete it. What it did lasts until the next
#                                   boot; profiles/privacy/s1-undo.sh undoes
#                                   it now
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
HOOK=$ROOT/profiles/privacy/lumen-privacy
DEST=/var/lib/webosbrew/init.d/lumen-privacy
# Uploads land where only root can write: the app jails share /tmp.
STAGE=/var/lib/lumen/stage
NEW=$STAGE/lumen-privacy

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "privacy-hook.sh: $*" >&2
    exit 1
}

local_sha=$(sha256sum < "$HOOK" | cut -d' ' -f1)

case ${1:-} in
    status)
        echo "this checkout: $local_sha"
        "$TV" run "if [ -f $DEST ]; then echo \"on the TV:    \$(sha256sum < $DEST | cut -d' ' -f1)\"; else echo 'on the TV:    not installed'; fi
            for job in crashreportd rdxd uploadd remotelogger; do initctl status \$job; done
            iptables -L INPUT -n -v | grep 'dpt:11111' || echo 'no tcp 11111 rule'
            tail -n 20 /tmp/lumen-privacy.log 2>/dev/null || echo 'no log since the last boot'"
        ;;
    install)
        "$TV" run "umask 077; mkdir -p $STAGE && [ ! -L $STAGE ]" || die "could not make $STAGE on the TV"
        "$TV" push "$HOOK" "$NEW"
        # Checked against this checkout while staged, then moved into place
        # whole, so a boot never meets half a file or another one.
        remote_sha=$("$TV" run "sh -n $NEW && [ \"\$(sha256sum < $NEW | cut -d' ' -f1)\" = $local_sha ] && cp $NEW $DEST.tmp && chmod 755 $DEST.tmp && mv $DEST.tmp $DEST && sha256sum < $DEST | cut -d' ' -f1; rm -f $NEW")
        [ "$remote_sha" = "$local_sha" ] || die "the installed hook's SHA-256 ($remote_sha) is not this checkout's ($local_sha)"
        echo "installed $DEST ($local_sha)"
        "$TV" run "$DEST"
        echo "ran it once; tools/privacy-hook.sh status shows its log"
        ;;
    remove)
        "$TV" run "rm -f $DEST && echo 'removed $DEST'"
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac
