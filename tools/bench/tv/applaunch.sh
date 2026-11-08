# shellcheck shell=sh
# Launches $APP and times it to the app in front, by polling the app
# manager about every 50 ms on /proc/uptime's clock (10 ms steps). luna-send
# -i's piped output may be buffered, so its life events are never timed.
# With COLD=1 the app is closed first, if it runs. Prints key=value lines.
now_cs() {
    awk '{ printf "%d", $1 * 100 }' /proc/uptime
}
running() {
    luna luna://com.webos.applicationManager/running '{}' | grep -q "\"id\": *\"$APP\""
}
if [ "${COLD:-0}" = 1 ] && running; then
    luna luna://com.webos.applicationManager/closeByAppId "{\"id\":\"$APP\"}" >/dev/null
    i=0
    while running && [ $i -lt 50 ]; do
        sleep 0.2
        i=$((i + 1))
    done
    sleep 2
fi
if running; then echo "was_running=1"; else echo "was_running=0"; fi
t0=$(now_cs)
luna luna://com.webos.applicationManager/launch "{\"id\":\"$APP\"}" | grep -q '"returnValue": *true' ||
    { echo "error=launch refused"; exit 0; }
t1=$(now_cs)
echo "launch_call_ms=$(((t1 - t0) * 10))"
while :; do
    fg=$(luna luna://com.webos.applicationManager/getForegroundAppInfo '{}' | field appId)
    t=$(now_cs)
    if [ "$fg" = "$APP" ]; then
        echo "front_ms=$(((t - t0) * 10))"
        break
    fi
    if [ $((t - t0)) -gt 3000 ]; then
        echo "error=not in front after 30 s (front: ${fg:-nothing})"
        break
    fi
    sleep 0.05
done
echo "epoch=$(date +%s)"
