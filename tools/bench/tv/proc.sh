# shellcheck shell=sh
# The shell's resident memory and its CPU over 30 s, settled. Read-only.
pid=$(pidof lumen-shell)
if [ -z "$pid" ]; then
    echo "error=lumen-shell is not running"
    exit 0
fi
pid=${pid%% *}
echo "rss_kb=$(sed -n 's/^VmRSS: *\([0-9]*\).*/\1/p' "/proc/$pid/status")"
t1=$(awk '{print $14 + $15}' "/proc/$pid/stat")
sleep 30
t2=$(awk '{print $14 + $15}' "/proc/$pid/stat")
# USER_HZ is 100 on the B9: ticks over 30 s, as a percent of one core.
echo "cpu_pct=$(awk -v a="$t1" -v b="$t2" 'BEGIN { printf "%.2f", (b - a) / 30 }')"
echo "mem=$(sed -n 's/^MemAvailable: *\([0-9]*\).*/\1/p' /proc/meminfo)"
