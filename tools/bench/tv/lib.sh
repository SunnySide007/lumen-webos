# shellcheck shell=sh
# Shared by the bench's TV-side steps (tools/bench.sh puts this first in
# each script it sends with `tv.sh script`). POSIX sh for the B9's busybox.
# Read-only on the TV except where a step says so.

# Lumen's log folder: /tmp, or the shell's own /tmp inside its jail.
lumen_dir() {
    if [ -f /tmp/lumen-shell.log ]; then
        echo /tmp
        return
    fi
    f=$(find /var/palm/jail -name lumen-shell.log 2>/dev/null | head -n 1)
    [ -n "$f" ] && echo "${f%/*}"
}

# One luna call. luna-send prints nothing with a pipe on stdin.
luna() {
    luna-send -n 1 -f "$1" "$2" </dev/null 2>/dev/null
}

# The first "NAME": "value" in luna's pretty-printed reply.
field() {
    sed -n "s/.*\"$1\": *\"\([^\"]*\)\".*/\1/p" | head -n 1
}

# The log's lines stamped at or after $1 (whole seconds).
log_since() {
    d=$(lumen_dir)
    [ -n "$d" ] || return 0
    tail -n 20000 "$d/lumen-shell.log" | awk -F'[][]' -v t="$1" '$2 + 0 >= t + 0'
}
