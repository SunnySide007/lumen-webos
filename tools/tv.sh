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
#   tools/tv.sh log                 print the Lumen shell's log from its last run
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

cmd_check() {
    # Single quotes on purpose: this expands on the TV, not here.
    # shellcheck disable=SC2016
    tv_ssh 'echo "connected as $(id -un) to $(uname -n)";
        luna-send -n 1 -f luna://com.webos.service.tv.systemproperty/getSystemInfo \
            "{\"keys\":[\"modelName\",\"firmwareVersion\",\"sdkVersion\"]}" 2>&1'
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
    tv_ssh "luna-send -n 1 -f luna://com.webos.applicationManager/$method '{\"id\":\"$2\"}'"
}

cmd_remove() {
    check_app_id "${1:-}"
    tv_ssh "luna-send-pub -n 1 -f luna://com.webos.appInstallService/dev/remove '{\"id\":\"$1\"}'"
}

# A native app may run inside webOS's jail, which gives it a private /tmp.
cmd_log() {
    # shellcheck disable=SC2016
    tv_ssh 'f=/tmp/lumen-shell.log
        [ -f "$f" ] || f=$(find /var/palm/jail -name lumen-shell.log 2>/dev/null | head -n 1)
        if [ -n "$f" ] && [ -f "$f" ]; then echo "# $f"; tail -n 200 "$f"; else echo "no lumen-shell.log yet"; fi'
}

command=${1:-help}
[ $# -gt 0 ] && shift

case $command in
    help | -h | --help) usage; exit 0 ;;
    check | shell | run | script | install | launch | close | remove | log) ;;
    *) usage >&2; die "unknown command: $command" ;;
esac

load_env
case $command in
    check) cmd_check ;;
    shell) tv_ssh -t ;;
    run) need_arg "${1:-}"; tv_ssh "$1" ;;
    script) need_arg "${1:-}"; [ -f "$1" ] || die "no such file: $1"; tv_ssh 'sh -s' < "$1" ;;
    install) cmd_install "${1:-}" ;;
    launch) cmd_luna_app launch "${1:-}" ;;
    close) cmd_luna_app closeByAppId "${1:-}" ;;
    remove) cmd_remove "${1:-}" ;;
    log) cmd_log ;;
esac
