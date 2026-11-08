# shellcheck shell=sh
# The shell's timed screenshots as a tar on stdout, then deleted: they are
# the app's own files in its own /tmp.
d=$(lumen_dir)
[ -n "$d" ] || exit 0
cd "$d" || exit 0
set -- lumen-shot-*.png
[ -f "$1" ] || exit 0
tar -cf - "$@"
rm -f "$@"
