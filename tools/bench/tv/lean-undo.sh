# shellcheck shell=sh
# Starts again the jobs Lumen's lean track stopped (all, or only $JOB), as
# its record lists them, and nothing else; the rest stay in the record.
rec=/var/lib/lumen/lean/stopped
[ -f "$rec" ] || { echo "nothing stopped"; exit 0; }
: >"$rec.new"
while read -r job when; do
    if [ -n "${JOB:-}" ] && [ "$job" != "$JOB" ]; then
        echo "$job $when" >>"$rec.new"
        continue
    fi
    initctl start "$job" >/dev/null 2>&1
    echo "started $job: $(initctl status "$job" 2>/dev/null)"
done <"$rec"
mv "$rec.new" "$rec"
[ -s "$rec" ] || rm -f "$rec"
