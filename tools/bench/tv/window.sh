# shellcheck shell=sh
# What decides a lab window (docs/safety.md), as key=value lines. Read-only.
echo "now=$(date +%s)"
echo "hour=$(date +%H)"
# 1 (Monday) to 7, for the owner's weekday slot.
echo "dow=$(date +%u)"
echo "fg=$(luna luna://com.webos.applicationManager/getForegroundAppInfo '{}' | field appId)"
echo "power=$(luna luna://com.webos.service.tvpower/power/getPowerState '{}' | field state)"
echo "pid=$(pidof lumen-shell)"
d=$(lumen_dir)
last='' under=''
if [ -n "$d" ]; then
    # A person's last sign: a key, the pointer, a click, or any change of
    # the app in front or the power state, except LG's screensaver coming.
    # The bench's own runs are not a person: lines inside the time ranges
    # tools/bench.sh recorded ("start end" a line) are left out.
    ranges=/var/lib/lumen/stage/bench-ranges
    [ -f "$ranges" ] || : >"$ranges"
    tail -n 20000 "$d/lumen-shell.log" |
        awk -F'[][]' -v rf="$ranges" '
            BEGIN { n = 0; while ((getline l < rf) > 0) { split(l, p, " "); a[n] = p[1]; b[n] = p[2]; n++ } }
            { t = $2 + 0; for (i = 0; i < n; i++) if (t >= a[i] && t <= b[i]) next; print }
        ' >/var/lib/lumen/stage/bench-window.log
    last=$(grep -E '\] (key down|pointer shown|mouse button|front app |power )' /var/lib/lumen/stage/bench-window.log |
        grep -vE 'front app com\.webos\.app\.screensaver|power Screen Saver' |
        tail -n 1 | sed 's/^\[\([0-9]*\).*/\1/')
    # The app LG's screensaver came over.
    under=$(grep -E '\] front app ' /var/lib/lumen/stage/bench-window.log |
        grep -v 'front app com\.webos\.app\.screensaver' |
        tail -n 1 | sed 's/.*\] front app \([^:]*\):.*/\1/')
    rm -f /var/lib/lumen/stage/bench-window.log
fi
echo "last=$last"
echo "under=$under"
