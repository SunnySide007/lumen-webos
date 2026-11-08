#!/bin/sh
# One build, one tools/bench.sh run per "look" value, each in a lab window.
# A finished run leaves tools/out/bench/done-<name>-<look>, so a second call
# carries on where the first stopped (a yield, a closed window). "full"
# means no "look" parameter at all.
#
#   sh tools/bench/looks.sh IPK NAME LOOK [LOOK ...]
#   sh tools/bench/looks.sh tools/out/home-design/d7de95e.ipk home-d7de95e plain plain,hero full
set -u
cd "$(dirname "$0")/../.." || exit 1
IPK=${1:?usage: looks.sh IPK NAME LOOK...}
NAME=${2:?usage: looks.sh IPK NAME LOOK...}
shift 2
[ -f "$IPK" ] || { echo "looks: no such IPK: $IPK" >&2; exit 1; }
for look in "$@"; do
    case $look in *[!A-Za-z0-9,_-]*) echo "looks: bad look: $look" >&2; exit 1 ;; esac
    tag=$(echo "$look" | tr ',' '+')
    marker=tools/out/bench/done-$NAME-$tag
    [ -f "$marker" ] && continue
    if [ "$look" = full ]; then
        sh tools/bench.sh run --ipk "$IPK" --name "$NAME-$tag" || exit $?
    else
        sh tools/bench.sh run --ipk "$IPK" --name "$NAME-$tag" --params "{\"look\":\"$look\"}" || exit $?
    fi
    : >"$marker"
done
