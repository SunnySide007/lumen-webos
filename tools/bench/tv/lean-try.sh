# shellcheck shell=sh
# The lean track's one change (tier B, docs/safety.md): stops one upstart
# job, $JOB, until the next boot, after noting it in Lumen's own record so
# that lean-undo starts only what Lumen stopped. Prints key=value lines.
rec=/var/lib/lumen/lean/stopped
mkdir -p /var/lib/lumen/lean
chmod 0700 /var/lib/lumen/lean
avail() { sed -n 's/^MemAvailable: *\([0-9]*\).*/\1/p' /proc/meminfo; }
state=$(initctl status "$JOB" 2>/dev/null)
case $state in
    *start/running*) ;;
    '') echo "error=no job $JOB"; exit 0 ;;
    *) echo "error=$JOB is not running ($state)"; exit 0 ;;
esac
before=$(avail)
echo "$JOB $(date +%s)" >>"$rec"
initctl stop "$JOB" >/dev/null 2>&1
sleep 10
echo "job=$JOB"
echo "state=$(initctl status "$JOB" 2>/dev/null)"
echo "avail_before_kb=$before"
echo "avail_after_kb=$(avail)"
