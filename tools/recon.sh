#!/bin/sh
# Surveys the TV, read-only, and saves a redacted report.
#
#   tools/recon.sh
#
# What runs on the TV is tools/recon/remote.sh, which only reads; review it
# before the first run. The raw output stays in tools/out/ (git ignores it), and
# the redacted copy goes to docs/device/ for review before anything is committed.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)

python=""
for candidate in python3 python; do
    if "$candidate" -c 'import sys; sys.exit(sys.version_info < (3, 8))' >/dev/null 2>&1; then
        python=$candidate
        break
    fi
done
[ -n "$python" ] || { echo "recon.sh: needs Python 3.8 or newer for redaction" >&2; exit 1; }

stamp=$(date -u +%Y%m%d-%H%M%S)
mkdir -p "$ROOT/tools/out" "$ROOT/docs/device"
raw="$ROOT/tools/out/recon-$stamp.raw.txt"
out="$ROOT/docs/device/recon-$stamp.txt"

"$ROOT/tools/tv.sh" script "$ROOT/tools/recon/remote.sh" > "$raw"
"$python" -I "$ROOT/tools/recon/redact.py" < "$raw" > "$out"

echo "raw (private):  $raw"
echo "redacted:       $out"
echo "Read the redacted copy before committing it."
