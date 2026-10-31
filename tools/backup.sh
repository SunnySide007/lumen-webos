#!/bin/sh
# Backs up the TV's own data to this PC. Nothing on the TV is written: it
# reads settings over luna, lists running state, and streams tar archives
# back over SSH.
#
#   tools/backup.sh
#
# Each run makes a folder under $LUMEN_BACKUP_DIR (default
# ~/Documents/Lumen-backups), named for the time, holding:
#
#   settings.txt       every LG setting category, as the TV printed it
#   state.txt          installed apps, jobs, firewall rules, hosts, mounts
#   cmn_data.tar.gz    the common-data partition (mmcblk0p52): /var, /home
#                      and LG's caches
#   db8.tar.gz         LG's database partition (mmcblk0p51)
#   developer.tar.gz   /media/developer: Homebrew Channel and the
#                      developer-mode apps
#   *.log              what tar said on the TV, and its exit status
#   SHA256SUMS, README.txt
#
# The archives hold the Wi-Fi password and the apps' sign-ins, so the folder
# must be outside this repository; the script refuses one inside it.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
TV=$ROOT/tools/tv.sh

die() {
    echo "backup.sh: $*" >&2
    exit 1
}

if [ $# -ne 0 ]; then
    echo "usage: tools/backup.sh   (LUMEN_BACKUP_DIR chooses the folder)" >&2
    exit 2
fi

# The physical path of $1, which need not exist yet.
resolve() {
    dir=$1 rest=
    while [ ! -d "$dir" ]; do
        rest=/${dir##*/}$rest
        dir=$(dirname "$dir")
    done
    printf '%s%s\n' "$(cd "$dir" && pwd -P)" "$rest"
}

base=${LUMEN_BACKUP_DIR:-$HOME/Documents/Lumen-backups}
case $base in
    /*|[A-Za-z]:*) ;;
    *) base=$PWD/$base ;;
esac
base=$(resolve "$base")
case $base/ in
    "$ROOT"/*) die "refusing to save inside the repository ($base): the backup holds secrets" ;;
esac

out=$base/$(date -u +%Y%m%dT%H%M%SZ)
mkdir -p "$out"
chmod 700 "$base" "$out" 2>/dev/null || true
echo "backing up the TV to $out"

"$TV" script "$ROOT/tools/backup/settings.sh" > "$out/settings.txt"
echo "settings   $(grep -c '^\$ luna' "$out/settings.txt") categories asked"
"$TV" script "$ROOT/tools/backup/state.sh" > "$out/state.txt"
echo "state      $(wc -l < "$out/state.txt") lines"

warnings=0

# archive <name> <directory on the TV>. --one-file-system keeps tar on that
# partition, so the app jails' bind mounts aren't followed. tar's exit status
# goes to the log, since the pipe's status is gzip's.
archive() {
    "$TV" run "{ tar -C '$2' --one-file-system --numeric-owner --ignore-failed-read -cf - . ; echo \"tar exit \$?\" >&2; } | gzip -1" \
        > "$out/$1.tar.gz" 2> "$out/$1.log"
    gzip -t "$out/$1.tar.gz" || die "$1.tar.gz is damaged; see $out/$1.log"
    entries=$(tar -tzf "$out/$1.tar.gz" | wc -l)
    status=$(sed -n 's/^tar exit //p' "$out/$1.log")
    printf '%-10s %s KB, %s entries, tar exit %s\n' "$1" \
        "$(du -k "$out/$1.tar.gz" | cut -f1)" "$entries" "${status:-unknown}"
    # 1 means a file changed while it was read: expected on a running TV.
    case ${status:-unknown} in
        0|1) ;;
        *) warnings=$((warnings + 1)); echo "  WARNING: see $out/$1.log" ;;
    esac
}

archive cmn_data /mnt/lg/cmn_data
archive db8 /mnt/lg/db8
archive developer /media/developer

(cd "$out" && sha256sum settings.txt state.txt ./*.tar.gz > SHA256SUMS)

cat > "$out/README.txt" <<'EOF'
Lumen backup of an LG webOS TV, made by tools/backup.sh. Private: it holds
the Wi-Fi password and the apps' sign-ins. Never commit or share it.

Check it:          sha256sum -c SHA256SUMS
See inside:        tar -tzf cmn_data.tar.gz | less

Put back one LG setting: find it in settings.txt, then on the TV, e.g.
  luna-send -n 1 luna://com.webos.settingsservice/setSystemSettings \
    '{"category":"option","settings":{"quickStartMode":"off"}}' </dev/null

Put back one file: extract it here, e.g.
  tar -xzf cmn_data.tar.gz ./var/luna/preferences/webosbrew_block_updates
then copy it to the same place under /mnt/lg/cmn_data on the TV (or under
/mnt/lg/db8, /media/developer for the other archives) with tools/tv.sh.

The archives were read while the TV ran, so LG's databases in them may be
mid-write; settings.txt is the dependable copy of the settings. Restoring a
whole archive over a running TV is not supported.
EOF

if [ "$warnings" -eq 0 ]; then
    echo "done: $out"
else
    echo "done with $warnings warning(s): $out"
fi
