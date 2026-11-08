#!/bin/sh
# Lumen's own certificate authority for Lumen Remote's phone page
# (docs/specs/remote.md, R4 and D4). It runs on the PC, never on the TV:
# the authority's key stays in ~/.lumen/remote/ca/, and only the TV's own
# certificate and key go to the TV (tools/remote.sh web install).
#
# The authority can vouch for the TV's names and nothing else: its
# certificate carries critical name constraints listing exactly the TV's
# address (and DNS name, if given), so a stolen authority key can't
# impersonate any other site to a phone that trusts it.
#
#   tools/remote/ca.sh init --ip ADDR [--dns NAME]
#                      make the authority (if none is there) and the TV's
#                      certificate. The names are fixed with the authority:
#                      a new address needs a new authority, installed on
#                      the phone again
#   tools/remote/ca.sh issue
#                      a new certificate for the TV from the same authority
#                      (it lasts 397 days); the phone needs nothing new
#   tools/remote/ca.sh show
#                      the names, the dates and the authority's fingerprint
#
# Options for tests: --dir DIR (default ~/.lumen/remote/ca), --ca-days N
# (3650), --leaf-days N (397).
#
# Files in DIR: lumen-ca.crt (install this on the phone: Settings, Security,
# Encryption and credentials, Install a certificate, CA certificate),
# lumen-ca.key (the authority's key: never leaves the PC), server.crt and
# server.key (the TV's), names (the names the authority allows).
set -eu

# Git Bash would rewrite "/CN=..." as a Windows path.
export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL='*'

DIR=${LUMEN_CA_DIR:-$HOME/.lumen/remote/ca}
CA_DAYS=3650
LEAF_DAYS=397
IP=
DNS=

die() {
    echo "ca.sh: $*" >&2
    exit 1
}

usage() {
    sed -n '2,/^set -eu$/{/^set -eu$/d;s/^# \{0,1\}//;p;}' "$0"
}

is_ipv4() {
    printf '%s\n' "$1" | grep -Eq '^(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9]?[0-9])(\.(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9]?[0-9])){3}$'
}

is_dns() {
    printf '%s\n' "$1" | grep -Eq '^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)*[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$' &&
        ! is_ipv4 "$1"
}

is_days() {
    case $1 in '' | *[!0-9]*) return 1 ;; esac
    [ "$1" -ge 1 ] && [ "$1" -le 36500 ]
}

# The authority's name constraints and the TV's subjectAltName, from the
# names file: one "IP:addr" or "DNS:name" per line.
constraints() {
    sed -e 's|^IP:\(.*\)$|permitted;IP:\1/255.255.255.255|' -e 's|^DNS:|permitted;DNS:|' "$DIR/names" | paste -sd, -
}

alt_names() {
    paste -sd, - <"$DIR/names"
}

first_name() {
    sed -n '1s/^[A-Z]*://p' "$DIR/names"
}

new_key() {
    (umask 077 && openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1" 2>/dev/null)
    chmod 600 "$1"
}

serial() {
    echo "0x$(openssl rand -hex 16)"
}

make_ca() {
    conf=$DIR/.ca.cnf
    cat >"$conf" <<EOF
[req]
distinguished_name = dn
prompt = no
[dn]
CN = Lumen Remote CA for $(first_name)
[ca]
basicConstraints = critical,CA:TRUE,pathlen:0
keyUsage = critical,keyCertSign,cRLSign
nameConstraints = critical,$(constraints)
subjectKeyIdentifier = hash
EOF
    new_key "$DIR/lumen-ca.key"
    openssl req -x509 -new -key "$DIR/lumen-ca.key" -sha256 -days "$CA_DAYS" \
        -set_serial "$(serial)" -config "$conf" -extensions ca -out "$DIR/lumen-ca.crt"
    rm -f "$conf"
}

make_leaf() {
    conf=$DIR/.leaf.cnf
    cat >"$conf" <<EOF
[req]
distinguished_name = dn
prompt = no
[dn]
CN = $(first_name)
[leaf]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = serverAuth
subjectAltName = $(alt_names)
authorityKeyIdentifier = keyid
subjectKeyIdentifier = hash
EOF
    new_key "$DIR/server.key.new"
    openssl req -new -key "$DIR/server.key.new" -config "$conf" -out "$DIR/server.csr"
    openssl x509 -req -in "$DIR/server.csr" -CA "$DIR/lumen-ca.crt" -CAkey "$DIR/lumen-ca.key" \
        -set_serial "$(serial)" -days "$LEAF_DAYS" -sha256 -extfile "$conf" -extensions leaf \
        -out "$DIR/server.crt.new" 2>/dev/null
    openssl verify -CAfile "$DIR/lumen-ca.crt" "$DIR/server.crt.new" >/dev/null ||
        die "the new certificate does not verify against the authority"
    mv "$DIR/server.key.new" "$DIR/server.key"
    mv "$DIR/server.crt.new" "$DIR/server.crt"
    rm -f "$conf" "$DIR/server.csr"
}

windows_hint() {
    case $(uname -s 2>/dev/null) in
        MINGW* | MSYS* | CYGWIN*)
            echo "on Windows, keep the keys yours alone, as the SSH key:"
            echo "  icacls \"%USERPROFILE%\\.lumen\\remote\\ca\" /inheritance:r /grant:r \"%USERNAME%:(OI)(CI)F\""
            ;;
    esac
}

cmd_show() {
    [ -f "$DIR/lumen-ca.crt" ] || die "no authority in $DIR: tools/remote/ca.sh init --ip ADDR"
    echo "names:     $(alt_names)"
    echo "authority: $(openssl x509 -in "$DIR/lumen-ca.crt" -noout -enddate | sed 's/notAfter=/until /')"
    echo "           SHA-256 $(openssl x509 -in "$DIR/lumen-ca.crt" -noout -fingerprint -sha256 | sed 's/.*=//')"
    if [ -f "$DIR/server.crt" ]; then
        echo "the TV:    $(openssl x509 -in "$DIR/server.crt" -noout -enddate | sed 's/notAfter=/until /')"
        echo "           SHA-256 $(openssl x509 -in "$DIR/server.crt" -noout -fingerprint -sha256 | sed 's/.*=//')"
    fi
}

cmd_init() {
    [ -n "$IP" ] || die "init needs --ip, the TV's address on the home network"
    want=$(mktemp)
    trap 'rm -f "$want"' EXIT
    echo "IP:$IP" >"$want"
    [ -z "$DNS" ] || echo "DNS:$DNS" >>"$want"
    (umask 077 && mkdir -p "$DIR")
    chmod 700 "$DIR"
    if [ -f "$DIR/lumen-ca.crt" ] || [ -f "$DIR/lumen-ca.key" ]; then
        cmp -s "$want" "$DIR/names" ||
            die "the authority in $DIR is for $(alt_names), not $(paste -sd, - <"$want"); move $DIR away for a new one"
        echo "the authority is there already, for $(alt_names)"
    else
        cp "$want" "$DIR/names"
        make_ca
        echo "made the authority for $(alt_names): $DIR/lumen-ca.crt"
    fi
    make_leaf
    echo "made the TV's certificate: $DIR/server.crt and server.key"
    windows_hint
    echo "next: install lumen-ca.crt on the phone as a CA certificate, then tools/remote.sh web install"
}

cmd_issue() {
    [ -f "$DIR/lumen-ca.key" ] && [ -f "$DIR/names" ] ||
        die "no authority in $DIR: tools/remote/ca.sh init --ip ADDR"
    make_leaf
    echo "made a new certificate for the TV: $DIR/server.crt; tools/remote.sh web install puts it there"
}

[ $# -gt 0 ] || {
    usage
    exit 2
}
cmd=$1
shift
while [ $# -gt 0 ]; do
    [ $# -ge 2 ] || die "$1 needs a value"
    case $1 in
        --ip) is_ipv4 "$2" || die "--ip wants an IPv4 address, not $2"; IP=$2 ;;
        --dns) is_dns "$2" || die "--dns wants a lower-case DNS name, not $2"; DNS=$2 ;;
        --dir) DIR=$2 ;;
        --ca-days) is_days "$2" || die "--ca-days wants 1 to 36500"; CA_DAYS=$2 ;;
        --leaf-days) is_days "$2" || die "--leaf-days wants 1 to 36500"; LEAF_DAYS=$2 ;;
        *) die "unknown argument: $1" ;;
    esac
    shift 2
done
command -v openssl >/dev/null 2>&1 || die "no openssl (Git for Windows ships one)"
case $cmd in
    init) cmd_init ;;
    issue) cmd_issue ;;
    show) cmd_show ;;
    -h | --help | help) usage ;;
    *) die "unknown command: $cmd" ;;
esac
