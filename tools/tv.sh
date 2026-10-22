#!/bin/sh
# Talks to a rooted LG TV over SSH, with key authentication only.
# Settings come from .env at the repository root (copy .env.example).
#
#   tools/tv.sh check               confirm the key works; print model and firmware
#   tools/tv.sh shell               interactive root shell
#   tools/tv.sh run '<command>'     run one command
#   tools/tv.sh script <file>       run a local shell script on the TV
#   tools/tv.sh install <file.ipk>  install or update an app
#   tools/tv.sh launch <app id>     start an app
#   tools/tv.sh close <app id>      stop an app
#   tools/tv.sh remove <app id>     uninstall an app
#   tools/tv.sh log                 print the Lumen shell's logs from its last run
#                                   (lumen-shell.log, and the .fallback.log if any)
#   tools/tv.sh push <local> <remote>
#                                   upload a file; <remote> must be under
#                                   /media/developer/temp/ or /tmp/
#   tools/tv.sh tunnel [port]       forward 127.0.0.1:<port> (default 8080) here
#                                   to the TV's Glasshouse dashboard; Ctrl-C ends it
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "tv.sh: $*" >&2
    exit 1
}

load_env() {
    env_file=${LUMEN_ENV_FILE:-$ROOT/.env}
    if [ -f "$env_file" ]; then
        # shellcheck disable=SC1090
        . "$env_file"
    fi
    [ -n "${TV_HOST:-}" ] || die "TV_HOST is not set: copy .env.example to .env and fill it in"
    TV_USER=${TV_USER:-root}
    TV_PORT=${TV_PORT:-22}
    TV_KEY=${TV_KEY:-$HOME/.ssh/lumen_tv}
    # .env holds a literal "~/" path; expand it here, since sourcing does not.
    # shellcheck disable=SC2088
    case $TV_KEY in "~/"*) TV_KEY=$HOME/${TV_KEY#"~/"} ;; esac
    [ -f "$TV_KEY" ] || die "no SSH key at $TV_KEY (docs/setup.md explains how to make one)"
}

# Passwords are never offered: BatchMode and the auth options make a missing or
# unauthorised key fail at once instead of prompting.
tv_ssh() {
    ssh -i "$TV_KEY" -p "$TV_PORT" \
        -o BatchMode=yes -o IdentitiesOnly=yes \
        -o PasswordAuthentication=no -o KbdInteractiveAuthentication=no \
        -o ConnectTimeout=8 -o StrictHostKeyChecking=accept-new \
        "$TV_USER@$TV_HOST" "$@"
}

need_arg() {
    [ -n "${1:-}" ] || die "missing argument; see tools/tv.sh help"
}

# App ids go into JSON and a shell command line on the TV; keep them to the
# characters webOS allows in an id.
check_app_id() {
    need_arg "${1:-}"
    case $1 in
        *[!A-Za-z0-9._-]*) die "not an app id: $1" ;;
    esac
}

# Over SSH, luna-send -f prints nothing unless its stdin is /dev/null, so every
# luna-send below gets /dev/null.
cmd_check() {
    # Single quotes on purpose: this expands on the TV, not here.
    # shellcheck disable=SC2016
    tv_ssh 'echo "connected as $(id -un) to $(uname -n)";
        luna-send -n 1 -f luna://com.webos.service.tv.systemproperty/getSystemInfo \
            "{\"keys\":[\"modelName\",\"firmwareVersion\",\"sdkVersion\"]}" </dev/null 2>&1'
}

# `sh -s` reads the script from stdin as it runs, so a command in the script
# that reads stdin would swallow the rest of it. Inside braces, sh parses the
# whole script before running anything, and the commands get /dev/null.
cmd_script() {
    { echo '{'; cat "$1"; printf '\n} </dev/null\n'; } | tv_ssh 'sh -s'
}

cmd_install() {
    need_arg "${1:-}"
    [ -f "$1" ] || die "no such file: $1"
    remote=/tmp/lumen-install.ipk
    tv_ssh "cat > $remote" < "$1"
    # appInstallService streams progress; watch it for up to 60 s, the way
    # webos-galaxy-launcher's installer does.
    tv_ssh 'sh -s' <<EOF
log=/tmp/lumen-install.log
luna-send-pub -i luna://com.webos.appInstallService/dev/install \
    '{"id":"com.ares.defaultName","ipkUrl":"$remote","subscribe":true}' </dev/null >\$log 2>&1 &
pid=\$!
status=timeout
i=0
while [ \$i -lt 60 ]; do
    sleep 1
    if grep -q '"state": *"installed"' \$log; then status=ok; break; fi
    if grep -qiE 'failed|"returnValue": *false' \$log; then status=failed; break; fi
    i=\$((i + 1))
done
kill \$pid 2>/dev/null
rm -f $remote
if [ "\$status" != ok ]; then
    echo "install \$status:"
    cat \$log
    rm -f \$log
    exit 1
fi
rm -f \$log
echo installed
EOF
}

cmd_luna_app() {
    method=$1
    check_app_id "${2:-}"
    tv_ssh "luna-send -n 1 -f luna://com.webos.applicationManager/$method '{\"id\":\"$2\"}' </dev/null"
}

cmd_remove() {
    check_app_id "${1:-}"
    tv_ssh "luna-send-pub -n 1 -f luna://com.webos.appInstallService/dev/remove '{\"id\":\"$1\"}' </dev/null"
}

# A native app may run inside webOS's jail, which gives it a private /tmp. The
# shell writes lumen-shell.log there, and lumen-shell.fallback.log beside it when
# the first cannot be opened. Both are printed, each under its path; the jail is
# searched only when /tmp has none.
cmd_log() {
    # Single quotes on purpose: this runs on the TV, under busybox sh.
    # shellcheck disable=SC2016
    tv_ssh 'n=0
        for f in /tmp/lumen-shell*.log; do
            [ -f "$f" ] || continue
            echo "# $f"; tail -n 200 "$f"; n=1
        done
        if [ "$n" = 0 ]; then
            d=$(find /var/palm/jail -name "lumen-shell*.log" 2>/dev/null | head -n 1)
            d=${d%/*}
            if [ -n "$d" ]; then
                for f in "$d"/lumen-shell*.log; do
                    [ -f "$f" ] || continue
                    echo "# $f"; tail -n 200 "$f"; n=1
                done
            fi
        fi
        [ "$n" = 1 ] || echo "no lumen-shell.log yet"'
}

# Uploads go only to scratch space: never over system or app files. The path
# also lands in a remote command line, so it is kept to plain characters.
check_push_args() {
    need_arg "${1:-}"
    need_arg "${2:-}"
    [ -f "$1" ] || die "no such file: $1"
    case $2 in
        *[!A-Za-z0-9._/-]*) die "remote path has unsupported characters: $2" ;;
        */../* | */.. | */./* | */. | *//*) die "remote path must be plain: $2" ;;
        /media/developer/temp/?* | /tmp/?*) ;;
        *) die "remote path must be under /media/developer/temp/ or /tmp/: $2" ;;
    esac
    case $2 in */) die "remote path must name a file: $2" ;; esac
}

cmd_push() {
    remote_dir=$(dirname "$2")
    tv_ssh "mkdir -p '$remote_dir' && cat > '$2'" < "$1"
    want=$(wc -c < "$1" | tr -d ' ')
    got=$(tv_ssh "wc -c < '$2'" </dev/null | tr -d ' \r')
    [ "$got" = "$want" ] || die "upload of $1 is $got bytes on the TV, expected $want"
    echo "pushed $1 to $2 ($want bytes)"
}

check_port() {
    case $1 in
        '' | *[!0-9]* | ??????*) die "not a port number: $1" ;;
    esac
    if [ "$1" -lt 1 ] || [ "$1" -gt 65535 ]; then
        die "not a port number: $1"
    fi
}

# The dashboard has no login, so the local end listens on loopback only.
cmd_tunnel() {
    echo "Glasshouse dashboard: http://127.0.0.1:$1/ (Ctrl-C to close the tunnel)"
    tv_ssh -N -o ExitOnForwardFailure=yes -L "127.0.0.1:$1:127.0.0.1:8080"
}

command=${1:-help}
[ $# -gt 0 ] && shift

case $command in
    help | -h | --help) usage; exit 0 ;;
    check | shell | run | script | install | launch | close | remove | log) ;;
    push) check_push_args "${1:-}" "${2:-}" ;;
    tunnel) port=${1:-8080}; check_port "$port" ;;
    *) usage >&2; die "unknown command: $command" ;;
esac

load_env
case $command in
    check) cmd_check ;;
    shell) tv_ssh -t ;;
    run) need_arg "${1:-}"; tv_ssh "$1" ;;
    script) need_arg "${1:-}"; [ -f "$1" ] || die "no such file: $1"; cmd_script "$1" ;;
    install) cmd_install "${1:-}" ;;
    launch) cmd_luna_app launch "${1:-}" ;;
    close) cmd_luna_app closeByAppId "${1:-}" ;;
    remove) cmd_remove "${1:-}" ;;
    log) cmd_log ;;
    push) cmd_push "$1" "$2" ;;
    tunnel) cmd_tunnel "$port" ;;
esac
