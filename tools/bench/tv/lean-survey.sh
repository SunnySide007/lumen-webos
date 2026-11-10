# shellcheck shell=sh
# The lean track's survey (docs/device/bench.md, "The lean track"): memory,
# the upstart jobs and their resident memory, SAM's keep-alive apps, the
# running apps, and the 40 largest processes. Read-only: it calls only the
# app manager and configd, which always run, and reads /proc.
echo "===== memory"
grep -E '^(MemTotal|MemAvailable|SwapTotal|SwapFree|Cached):' /proc/meminfo
echo "===== uptime"
cat /proc/uptime
echo "===== keepAliveApps"
luna luna://com.webos.service.config/getConfigs '{"configNames":["com.webos.applicationManager.keepAliveApps"]}' |
    tr -d '\n' | sed 's/.*keepAliveApps": *\[\([^]]*\)\].*/\1/' | tr -d ' "' | tr ',' '\n'
echo
echo "===== running apps"
# The reply is one line: one id a line first.
luna luna://com.webos.applicationManager/running '{}' |
    tr ',{' '\n\n' | sed -n 's/.*"id": *"\([^"]*\)".*/\1/p'
# kB resident for a pid, or nothing. /proc writes a tab after the colon.
rss() {
    sed -n 's/^VmRSS:[[:space:]]*\([0-9]*\).*/\1/p' "/proc/$1/status" 2>/dev/null | head -n 1
}
echo "===== jobs (name state pid rss_kB)"
initctl list 2>/dev/null | while read -r job state rest; do
    pid=$(echo "$rest" | sed -n 's/.*process \([0-9]*\).*/\1/p')
    r=0
    [ -n "$pid" ] && r=$(rss "$pid")
    echo "$job ${state%,} ${pid:--} ${r:-0}"
done
echo "===== processes (rss_kB pid name), the 40 largest"
for d in /proc/[0-9]*; do
    p=${d#/proc/}
    r=$(rss "$p")
    if [ -z "$r" ] || [ "$r" -eq 0 ]; then continue; fi
    n=$(sed -n 's/^Name:[[:space:]]*//p' "$d/status" 2>/dev/null)
    echo "$r $p $n"
done | sort -rn | head -n 40
echo "===== end"
