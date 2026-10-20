#!/bin/sh
# Uploads profiles/glasshouse/b9-aggressive.hosts to Glasshouse's custom block
# list, in "add" mode (with Glasshouse's own list, never instead of it). A dry
# run unless given --yes. Talks to the dashboard through the SSH tunnel, so
# open one first in another terminal: tools/tv.sh tunnel
#
#   tools/glasshouse-custom.sh                  show what would be uploaded
#   tools/glasshouse-custom.sh --yes            upload the active entries
#   tools/glasshouse-custom.sh --clear --yes    remove the custom list
#
# Options: --port N (the tunnel's local port, default 8080), --file FILE.
#
# Only uncommented 0.0.0.0 / :: lines are sent. The file is checked with
# tools/store-safe-check.sh first: nothing allowlisted may be in it.
# Glasshouse's API (v0.118.0, server/lib/routes.js): POST /api/adblock/custom
# with JSON {"mode": "add", "text": "<hosts lines>", "file": "<name>"}, or
# {"clear": true}; it needs "allowControl" on in its config.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
CURL=${LUMEN_CURL:-curl}
FILE=$ROOT/profiles/glasshouse/b9-aggressive.hosts
PORT=8080
yes=0 clear=0
MAX_NAMES=10000

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "glasshouse-custom.sh: $*" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case $1 in
        --yes) yes=1; shift ;;
        --clear) clear=1; shift ;;
        --port) [ $# -ge 2 ] || die "--port needs a number"; PORT=$2; shift 2 ;;
        --file) [ $# -ge 2 ] || die "--file needs a file"; FILE=$2; shift 2 ;;
        -h | --help | help) usage; exit 0 ;;
        *) usage >&2; die "unknown argument: $1" ;;
    esac
done
case $PORT in '' | *[!0-9]* | ??????*) die "not a port number: $PORT" ;; esac
URL=http://127.0.0.1:$PORT

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [ $clear -eq 1 ]; then
    printf '{"clear":true}\n' >"$tmp/body.json"
    summary="remove the custom list"
else
    [ -f "$FILE" ] || die "no such file: $FILE"
    # Active sinkhole lines only, one name per line, lower case.
    awk '{ sub(/#.*/, "") } NF >= 2 && ($1 == "0.0.0.0" || $1 == "::") {
            for (i = 2; i <= NF; i++) print tolower($i) }' "$FILE" | sort -u >"$tmp/names"
    n=$(wc -l <"$tmp/names" | tr -d ' ')
    [ "$n" -gt 0 ] || die "no active entries in $FILE: uncomment the ones the capture confirmed"
    [ "$n" -le $MAX_NAMES ] || die "$n names; Glasshouse takes at most $MAX_NAMES"
    if grep -v '^[a-z0-9_][a-z0-9_.-]*$' "$tmp/names" >/dev/null; then
        die "a name in $FILE has characters a host name cannot have"
    fi
    "$ROOT/tools/store-safe-check.sh" --hosts-file "$FILE" --stage 1 >"$tmp/check" 2>&1 || {
        cat "$tmp/check" >&2
        die "$FILE blocks something on the allowlist; nothing was uploaded"
    }
    # Names are plain ([a-z0-9_.-]), so they need no JSON escaping.
    {
        printf '{"mode":"add","file":"%s","text":"' "$(basename "$FILE" | tr -cd 'A-Za-z0-9._-')"
        awk '{ printf "0.0.0.0 %s\\n", $0 }' "$tmp/names"
        printf '"}\n'
    } >"$tmp/body.json"
    summary="add $n name(s) from $FILE"
fi

if [ $yes -eq 0 ]; then
    echo "Dry run: would $summary, through $URL/api/adblock/custom"
    [ $clear -eq 1 ] || sed 's/^/  0.0.0.0 /' "$tmp/names"
    echo "Run again with --yes to send it (with tools/tv.sh tunnel open)."
    exit 0
fi

"$CURL" -fsS --max-time 10 -o /dev/null "$URL/api/privacy" ||
    die "no dashboard at $URL: open the tunnel first (tools/tv.sh tunnel $PORT)"
"$CURL" -sS --max-time 60 -X POST -H 'Content-Type: application/json' \
    --data-binary "@$tmp/body.json" "$URL/api/adblock/custom" >"$tmp/reply" ||
    die "the upload failed"
cat "$tmp/reply"
echo
grep -q '"ok": *true' "$tmp/reply" || die "Glasshouse refused it (see its reply above)"
echo "done: $summary. Now run tools/store-safe-check.sh."
