#!/bin/sh
# Runs on the TV (busybox sh). Records the TV's DNS traffic, TCP connection
# attempts and TLS ClientHellos (for server names) to a size-capped ring of
# pcap files. tools/capture.sh pushes it next to tcpdump and calls it; nothing
# here survives a reboot or adds a boot hook.
#
#   lumen-capture.sh start [HOURS] [FILE_MB] [FILES]   defaults: 24 50 10
#   lumen-capture.sh status
#   lumen-capture.sh stop
#   lumen-capture.sh list                              pcap files, one per line
#   lumen-capture.sh clean                             stop, then delete DIR
#
# Everything lives in DIR (default /media/developer/temp/lumen-capture).
set -eu

DIR=${LUMEN_CAPTURE_DIR:-/media/developer/temp/lumen-capture}
TCPDUMP=${LUMEN_CAPTURE_TCPDUMP:-$DIR/tcpdump}
PIDFILE=$DIR/tcpdump.pid
LOG=$DIR/tcpdump.log
PCAPS=$DIR/pcap

# DNS on port 53, over UDP and TCP (both the apps' queries to the connman
# proxy on 127.0.0.1 and its queries upstream). TCP SYNs without ACK: every
# new connection, including ones to addresses no lookup returned. The first
# segment of a TLS handshake record holding a ClientHello (content type 0x16,
# handshake type 0x01), for its server name. QUIC long-header packets to
# UDP 443. libpcap's tcp[] and udp[] only match IPv4, so IPv6 is spelled out
# with ip6[] offsets (no extension headers).
FILTER='port 53
 or (tcp[tcpflags] & (tcp-syn|tcp-ack) = tcp-syn)
 or (tcp[((tcp[12] & 0xf0) >> 2)] = 0x16 and tcp[((tcp[12] & 0xf0) >> 2) + 5] = 0x01)
 or (udp dst port 443 and (udp[8] & 0xc0) = 0xc0)
 or (ip6[6] = 6 and (ip6[53] & 0x12) = 0x02)
 or (ip6[6] = 6 and ip6[40 + ((ip6[52] & 0xf0) >> 2)] = 0x16
     and ip6[40 + ((ip6[52] & 0xf0) >> 2) + 5] = 0x01)
 or (ip6[6] = 17 and ip6[42:2] = 443 and (ip6[48] & 0xc0) = 0xc0)'

die() {
    echo "lumen-capture: $*" >&2
    exit 1
}

# rm -rf and the pcap writes must stay inside scratch space.
case $DIR in
    *..* | *[!A-Za-z0-9._/-]*) die "refusing to work in $DIR" ;;
    /media/developer/temp/lumen-capture | /tmp/?*) ;;
    *) die "refusing to work in $DIR" ;;
esac

is_number() {
    case $1 in '' | *[!0-9]*) return 1 ;; esac
    return 0
}

# in_range VALUE MIN MAX
in_range() {
    is_number "$1" || return 1
    [ "$1" -ge "$2" ] && [ "$1" -le "$3" ]
}

# The pid file's process, if it is still our tcpdump.
running_pid() {
    [ -f "$PIDFILE" ] || return 1
    pid=$(cat "$PIDFILE")
    is_number "$pid" || return 1
    kill -0 "$pid" 2>/dev/null || return 1
    grep -q tcpdump "/proc/$pid/cmdline" 2>/dev/null || return 1
    echo "$pid"
}

# Free kB on DIR's filesystem; 0 when unknown, which makes start refuse.
free_kb() {
    { df -Pk "$1" 2>/dev/null || df -k "$1"; } |
        awk 'NR > 1 { a = (NF >= 6) ? $4 : $3 } END { print a + 0 }'
}

# busybox before 1.30 wants `timeout -t SECS`; coreutils wants `timeout SECS`.
timeout_args() {
    if timeout -t 1 true 2>/dev/null; then
        echo "-t $1"
    else
        echo "$1"
    fi
}

cmd_start() {
    hours=${1:-24} mb=${2:-50} files=${3:-10}
    in_range "$hours" 1 72 || die "HOURS must be 1 to 72, not $hours"
    in_range "$mb" 1 200 || die "FILE_MB must be 1 to 200, not $mb"
    in_range "$files" 2 50 || die "FILES must be 2 to 50, not $files"
    if pid=$(running_pid); then
        die "a capture is already running (pid $pid); stop it first"
    fi
    [ -x "$TCPDUMP" ] || die "no tcpdump at $TCPDUMP: run tools/capture.sh push"

    # Keep the ring's worst case plus 200 MB free for the apps partition.
    need=$(( (mb * files + 200) * 1024 ))
    have=$(free_kb "$DIR")
    [ "$have" -ge "$need" ] ||
        die "only $((have / 1024)) MB free in $DIR; the ring needs $((need / 1024)) MB"

    mkdir -p "$PCAPS"
    stamp=$(date +%Y%m%d-%H%M%S)
    secs=$((hours * 3600))
    # shellcheck disable=SC2046 # timeout_args is one or two words on purpose
    (
        # No hangup when the SSH session that started it closes.
        trap '' HUP
        exec timeout $(timeout_args "$secs") "$TCPDUMP" -i any -s 2048 -B 1024 -U \
            -w "$PCAPS/lumen-$stamp.pcap" -C "$mb" -W "$files" "$FILTER"
    ) </dev/null >"$LOG" 2>&1 &
    echo $! >"$PIDFILE"
    sleep 2
    if ! pid=$(running_pid); then
        rm -f "$PIDFILE"
        cat "$LOG" >&2
        die "tcpdump exited at once"
    fi
    echo "capturing: pid $pid, until $hours h from now, at most $files files of $mb MB"
    echo "files: $PCAPS/lumen-$stamp.pcap*"
}

cmd_status() {
    if pid=$(running_pid); then
        echo "running: pid $pid"
    else
        echo "not running"
    fi
    if [ -d "$PCAPS" ]; then
        n=$(find "$PCAPS" -type f -name 'lumen-*.pcap*' | wc -l)
        kb=$(du -sk "$PCAPS" | awk '{ print $1 }')
        echo "pcap files: $n, $kb kB"
        ls -l "$PCAPS"
    else
        echo "pcap files: none"
    fi
    echo "free in $DIR: $(( $(free_kb "$DIR") / 1024 )) MB"
    if [ -f "$LOG" ]; then
        echo "tcpdump log:"
        tail -n 5 "$LOG"
    fi
}

cmd_stop() {
    if ! pid=$(running_pid); then
        rm -f "$PIDFILE"
        echo "not running"
        return 0
    fi
    # SIGTERM makes tcpdump flush its file and print its packet counts.
    # (Not SIGINT: a shell's background jobs start with SIGINT ignored.)
    kill -TERM "$pid"
    i=0
    while kill -0 "$pid" 2>/dev/null && [ $i -lt 10 ]; do
        sleep 1
        i=$((i + 1))
    done
    if kill -0 "$pid" 2>/dev/null; then
        kill -KILL "$pid" 2>/dev/null || true
    fi
    rm -f "$PIDFILE"
    echo "stopped pid $pid"
    [ -f "$LOG" ] && tail -n 3 "$LOG"
    return 0
}

cmd_list() {
    [ -d "$PCAPS" ] || return 0
    find "$PCAPS" -type f -name 'lumen-*.pcap*' | sort
}

cmd_clean() {
    cmd_stop >/dev/null
    rm -rf "$DIR"
    echo "removed $DIR"
}

command=${1:-}
[ $# -gt 0 ] && shift
case $command in
    start) cmd_start "$@" ;;
    status) cmd_status ;;
    stop) cmd_stop ;;
    list) cmd_list ;;
    clean) cmd_clean ;;
    *) die "usage: lumen-capture.sh start [HOURS] [FILE_MB] [FILES] | status | stop | list | clean" ;;
esac
