#!/bin/sh
# Surveys the TV, read-only, and saves a redacted report.
#
#   tools/recon.sh              the device survey (tools/recon/device.sh)
#   tools/recon.sh --privacy    the privacy and exposure survey (tools/recon/privacy.sh)
#
# Both scripts that run on the TV only read; review them before the first run.
# The raw output stays in tools/out/ (git ignores it), and the redacted copy
# goes to docs/device/ for review before anything is committed.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)

case "${1:-}" in
    '') survey=device; name=recon ;;
    --privacy) survey=privacy; name=privacy ;;
    *) echo "usage: tools/recon.sh [--privacy]" >&2; exit 2 ;;
esac

python=""
for candidate in python3 python; do
    if "$candidate" -c 'import sys; sys.exit(sys.version_info < (3, 8))' >/dev/null 2>&1; then
        python=$candidate
        break
    fi
done
# Without Python here (usual on Windows), redaction runs in the build container.
if [ -z "$python" ] && ! command -v docker >/dev/null 2>&1; then
    echo "recon.sh: redaction needs Python 3.8 or newer, or Docker for tools/dev.sh" >&2
    exit 1
fi

# redact <raw> <out>, both relative to the repository root.
redact() {
    if [ -n "$python" ]; then
        "$python" -I "$ROOT/tools/recon/redact.py" < "$ROOT/$1" > "$ROOT/$2"
    else
        # shellcheck disable=SC2016
        sh "$ROOT/tools/dev.sh" sh -c 'python3 -I tools/recon/redact.py < "$1" > "$2"' redact "$1" "$2"
    fi
}

stamp=$(date -u +%Y%m%d-%H%M%S)
mkdir -p "$ROOT/tools/out" "$ROOT/docs/device"
raw=tools/out/$name-$stamp.raw.txt
out=docs/device/$name-$stamp.txt

"$ROOT/tools/tv.sh" script "$ROOT/tools/recon/$survey.sh" > "$ROOT/$raw"
redact "$raw" "$out"

echo "raw (private):  $raw"
echo "redacted:       $out"
echo "Read the redacted copy before committing it."
