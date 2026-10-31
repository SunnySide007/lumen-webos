#!/bin/sh
# Installs, checks or removes Lumen Remote's door on the TV, stage R2
# (docs/specs/remote.md): lumen-ctl in /var/lib/lumen/remote/, and the PC's
# own key in root's authorized_keys with `lumen-ctl serve` as its forced
# command. Nothing starts at boot; lumen-ctl runs only while an SSH session
# with that key runs it. tools/remote/ctl.py drives it from the PC.
#
#   tools/remote.sh keygen     make the PC's key, ed25519 with no passphrase,
#                              in ~/.lumen/remote/ (%USERPROFILE% on Windows);
#                              never replaces one that is there
#   tools/remote.sh install    build lumen-ctl (tools/dev.sh make ctl), copy it
#                              to the TV and check its SHA-256, then add the
#                              key's line to authorized_keys if no lumen-remote
#                              line is there. The file as it was is kept once,
#                              in /var/lib/lumen/remote/authorized_keys.before
#                              and in tools/out/remote/
#   tools/remote.sh status     the key, the binary here and on the TV, the
#                              settings, the sessions open and the
#                              lumen-remote lines (keys shown hashed)
#   tools/remote.sh remove     take the key's line out of authorized_keys and
#                              delete /var/lib/lumen/remote/; every other line
#                              stays, and it refuses if no key would be left
#   tools/remote.sh config [bus=library|command] [keys=none|keymanager]
#                              show or change /var/lib/lumen/remote/ctl.conf,
#                              which the next session reads
#   tools/remote.sh keys FILE | --remove
#                              copy a key template to keys.json for
#                              keys=keymanager, or delete it
#   tools/remote.sh probe [library|command]
#   tools/remote.sh probe-keys [library|command]
#                              run lumen-ctl's probe on the TV and save what it
#                              prints in tools/out/remote/
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TV=${LUMEN_TV_SH:-$ROOT/tools/tv.sh}
DEV=${LUMEN_DEV_SH:-$ROOT/tools/dev.sh}
BIN=${LUMEN_CTL_BIN:-$ROOT/dist/ctl/lumen-ctl}
OUT=${LUMEN_REMOTE_OUT:-$ROOT/tools/out/remote}
KEYDIR=${LUMEN_REMOTE_KEYDIR:-$HOME/.lumen/remote}
KEY=$KEYDIR/id_ed25519
PROFILE=$ROOT/profiles/remote
DIR=/var/lib/lumen/remote
OPTS='restrict,command="/var/lib/lumen/remote/lumen-ctl serve"'

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

die() {
    echo "remote.sh: $*" >&2
    exit 1
}

no_args() {
    [ $# -eq 0 ] || die "unexpected argument: $1"
}

sha() {
    sha256sum <"$1" | cut -d' ' -f1
}

stamp() {
    date -u +%Y%m%d-%H%M%S
}

# The TV's side of put(), single-quoted since it runs there: the file comes on
# stdin into /var/lib/lumen/stage/ (root's alone; never /tmp, which every
# app's jail shares), is checked against the SHA-256 the PC sent in this
# command, and only then goes into place. A mismatch deletes it and changes
# nothing. Prints the SHA-256 of the file in place.
# shellcheck disable=SC2016
PUT='set -e
name=$1 want=$2 dest=$3 mode=$4 s=/var/lib/lumen/stage
if [ ! -d "${dest%/*}" ]; then echo "no ${dest%/*}: tools/remote.sh install first" >&2; exit 1; fi
mkdir -p /var/lib/lumen
umask 077
mkdir -p $s
if [ -L $s ] || [ ! -d $s ]; then echo "$s is not a plain folder; nothing changed" >&2; exit 1; fi
set -- $(ls -ldn $s)
if [ "$3" != 0 ]; then echo "$s is not root'"'"'s; nothing changed" >&2; exit 1; fi
chmod 700 $s
rm -f "$s/$name"
cat >"$s/$name"
got=$(sha256sum <"$s/$name" | cut -d" " -f1)
if [ "$got" != "$want" ]; then
    rm -f "$s/$name"
    echo "the staged copy is $got, not $want; deleted, nothing changed" >&2
    exit 1
fi
chmod "$mode" "$s/$name"
cp -p "$s/$name" "$dest.new"
rm -f "$s/$name"
mv "$dest.new" "$dest"
sha256sum <"$dest" | cut -d" " -f1'

# put FILE DEST MODE: FILE here to DEST on the TV through the stage, checked.
# DEST's folder must exist; DEST and MODE are this script's own.
put() {
    want=$(sha "$1")
    got=$("$TV" run "set -- ${2##*/} $want $2 $3; $PUT" <"$1") || return 1
    [ "$got" = "$want" ] || {
        echo "remote.sh: $2 on the TV is $got, not $want" >&2
        return 1
    }
}

# The public key's base64, checked: it goes into a file on the TV.
pub_key() {
    [ -f "$KEY.pub" ] || die "no key at $KEY.pub: tools/remote.sh keygen makes one"
    [ "$(grep -c '' "$KEY.pub")" = 1 ] || die "$KEY.pub is not one line"
    read -r type b64 _ <"$KEY.pub" || true
    [ "$type" = ssh-ed25519 ] || die "$KEY.pub is not an ed25519 key"
    case $b64 in
        *[!A-Za-z0-9+/]*) die "$KEY.pub's key is not base64" ;;
        AAAAC3NzaC1lZDI1NTE5AAAAI*) ;;
        *) die "$KEY.pub's key is not an ed25519 key" ;;
    esac
    [ ${#b64} -eq 68 ] || die "$KEY.pub's key is not 68 characters long"
    echo "$b64"
}

# The authorized_keys line with the key cut to its first 8 characters.
redacted() {
    echo "$OPTS ssh-ed25519 $(printf '%s' "$1" | cut -c1-8)... lumen-remote"
}

cmd_keygen() {
    no_args "$@"
    if [ -e "$KEY" ] || [ -e "$KEY.pub" ]; then
        die "a key is at $KEY already, and keygen never replaces one"
    fi
    (umask 077 && mkdir -p "$KEYDIR")
    chmod 700 "$KEYDIR"
    ssh-keygen -q -t ed25519 -N '' -C lumen-remote -f "$KEY" </dev/null
    chmod 600 "$KEY"
    echo "made $KEY and $KEY.pub; tools/remote.sh install puts it on the TV"
}

cmd_install() {
    no_args "$@"
    b64=$(pub_key)
    "$DEV" make ctl
    [ -f "$BIN" ] || die "no $BIN after tools/dev.sh make ctl"
    want=$(sha "$BIN")
    "$TV" run "mkdir -p /var/lib/lumen && (umask 077 && mkdir -p $DIR) && chmod 700 $DIR" </dev/null
    put "$BIN" "$DIR/lumen-ctl" 700 ||
        die "copying lumen-ctl failed (this build is $want); authorized_keys was not touched"
    echo "copied lumen-ctl to $DIR/lumen-ctl; SHA-256 checked ($want)"

    # authorize.sh checks lumen-ctl against this build again before it
    # touches authorized_keys.
    script=$(mktemp)
    trap 'rm -f "$script"' EXIT
    sed -e "s|@KEY@|$b64|" -e "s|@SHA@|$want|" "$PROFILE/authorize.sh" >"$script"
    "$TV" script "$script"

    # A copy here, beside any earlier one that differs.
    mkdir -p "$OUT"
    new=$OUT/authorized_keys.before.new
    (umask 077 && "$TV" run "cat $DIR/authorized_keys.before" </dev/null >"$new")
    saved=$OUT/authorized_keys.before
    if [ -f "$saved" ] && ! cmp -s "$saved" "$new"; then
        saved=$OUT/authorized_keys.before-$(stamp)
    fi
    mv "$new" "$saved"
    echo "authorized_keys as it was before Lumen Remote: $DIR/authorized_keys.before, and ${saved#"$ROOT"/} here"
    echo "the line: $(redacted "$b64")"
    echo "next: tools/remote.sh probe, then tools/remote/ctl.py hello"
}

cmd_status() {
    no_args "$@"
    mine=
    if [ -f "$KEY.pub" ] && b64=$(pub_key 2>/dev/null); then
        mine=$(printf '%s' "$b64" | sha256sum | cut -d' ' -f1)
        echo "this PC:   key $KEY"
        echo "this PC:   its line would be $(redacted "$b64")"
    else
        echo "this PC:   no key at $KEY (tools/remote.sh keygen)"
    fi
    built=
    if [ -f "$BIN" ]; then
        built=$(sha "$BIN")
        echo "this PC:   lumen-ctl $built"
    else
        echo "this PC:   lumen-ctl not built (tools/dev.sh make ctl)"
    fi
    report=$("$TV" script "$PROFILE/status.sh")
    printf '%s\n' "$report" | while IFS= read -r line; do
        if [ -n "$mine" ] && [ "$line" = "lumen-remote line: key $mine" ]; then
            line="lumen-remote line: this PC's key"
        elif [ -n "$built" ] && [ "$line" = "binary: $built" ]; then
            line="binary: $built (this build)"
        fi
        echo "on the TV: $line"
    done
}

cmd_remove() {
    no_args "$@"
    "$TV" script "$PROFILE/remove.sh"
    [ ! -e "$KEY" ] || echo "the PC's key stays in $KEYDIR; delete it by hand once you are done with it"
}

# Values from the TV are checked as the flags are, and fall back to lumen-ctl's
# defaults.
conf_value() {
    printf '%s\n' "$1" | sed -n "s/^$2[[:space:]]\{1,\}\([a-z]*\)[[:space:]]*\$/\1/p" | tail -n 1
}

cmd_config() {
    bus='' keys=''
    for arg in "$@"; do
        case $arg in
            bus=library | bus=command) bus=${arg#bus=} ;;
            keys=none | keys=keymanager) keys=${arg#keys=} ;;
            *) die "config takes bus=library|command and keys=none|keymanager, not $arg" ;;
        esac
    done
    if [ -z "$bus$keys" ]; then
        "$TV" run "cat $DIR/ctl.conf 2>/dev/null || echo 'no ctl.conf: bus command, keys none'" </dev/null
        return
    fi
    current=$("$TV" run "cat $DIR/ctl.conf 2>/dev/null; true" </dev/null)
    [ -n "$bus" ] || bus=$(conf_value "$current" bus)
    [ -n "$keys" ] || keys=$(conf_value "$current" keys)
    case $bus in library | command) ;; *) bus='command' ;; esac
    case $keys in none | keymanager) ;; *) keys=none ;; esac
    conf=$(mktemp)
    trap 'rm -f "$conf"' EXIT
    printf '# Written by tools/remote.sh config.\nbus %s\nkeys %s\n' "$bus" "$keys" >"$conf"
    put "$conf" "$DIR/ctl.conf" 600 || die "ctl.conf was not changed"
    grep -v '^#' "$conf"
    echo "the next session reads it; one that is open keeps what it started with"
}

cmd_keys() {
    [ $# -eq 1 ] || die "keys takes a template file, or --remove"
    if [ "$1" = --remove ]; then
        "$TV" run "rm -f $DIR/keys.json && echo 'removed $DIR/keys.json'" </dev/null
        return
    fi
    [ -f "$1" ] || die "no such file: $1"
    [ "$(wc -c <"$1" | tr -d ' ')" -le 16384 ] || die "$1 is over 16 KiB, which lumen-ctl refuses"
    put "$1" "$DIR/keys.json" 600 || die "keys.json was not changed"
    echo "copied $1 to $DIR/keys.json ($(sha "$1"))"
    echo "lumen-ctl reads it with keys=keymanager (tools/remote.sh config keys=keymanager), and logs why if it can't"
}

cmd_probe() {
    which=$1
    shift
    [ $# -le 1 ] || die "unexpected argument: $2"
    bus=${1:-}
    case $bus in
        '' | library | command) ;;
        *) die "$which takes library or command, not $bus" ;;
    esac
    mkdir -p "$OUT"
    file=$OUT/$which-$(stamp).raw.txt
    status=0
    "$TV" run "$DIR/lumen-ctl $which${bus:+ --bus $bus}" </dev/null >"$file" 2>&1 || status=$?
    cat "$file"
    echo "saved in ${file#"$ROOT"/}" >&2
    return $status
}

command=${1:-}
[ $# -gt 0 ] && shift
case $command in
    keygen) cmd_keygen "$@" ;;
    install) cmd_install "$@" ;;
    status) cmd_status "$@" ;;
    remove) cmd_remove "$@" ;;
    config) cmd_config "$@" ;;
    keys) cmd_keys "$@" ;;
    probe | probe-keys) cmd_probe "$command" "$@" ;;
    -h | --help | help) usage ;;
    *)
        usage >&2
        exit 2
        ;;
esac
