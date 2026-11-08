#!/usr/bin/env bats
# tools/remote.sh web and profiles/remote/web-*.sh (R4) against a fake
# tools/tv.sh that keeps the TV's files in this test's folder, a fake
# tools/dev.sh, and fake iptables, netstat and setsid. Nothing reaches a TV.

setup() {
    T="$BATS_TEST_TMPDIR"
    R="$T/tv"
    CALLS="$T/calls"
    : >"$CALLS"
    mkdir -p "$T/bin" "$R/var/lib/lumen" "$R/etc" "$T/ca" "$T/dist"
    echo "root:x:0:0:root:/home/root:/bin/sh" >"$R/etc/passwd"
    echo "root:x:0:" >"$R/etc/group"

    # As in remote.bats: /var/lib/ and /etc/ moved under $R, root's uid the
    # test's own.
    cat >"$T/tv.sh" <<'EOF'
#!/bin/sh
printf '%s|%s\n' "$1" "$2" >>"$CALLS"
on_tv() {
    sed -e "s|\([^\"]\)/var/lib/|\1$R/var/lib/|g" -e "s|^/var/lib/|$R/var/lib/|" \
        -e "s|/etc/passwd|$R/etc/passwd|g" -e "s|/etc/group|$R/etc/group|g" \
        -e 's/"\$3" != 0/"$3" != '"$(id -u)"'/' -e "s/chown -R 0:0/chown -R $(id -u):$(id -g)/"
}
case $1 in
    run) sh -c "$(printf '%s\n' "$2" | on_tv)" ;;
    script) on_tv <"$2" >"$T/script.sh" && sh "$T/script.sh" </dev/null ;;
    *) exit 3 ;;
esac
EOF
    # make ctl web "builds" both binaries.
    cat >"$T/dev.sh" <<'EOF'
#!/bin/sh
echo "dev|$*" >>"$CALLS"
printf 'lumen-ctl build\n' >"$LUMEN_CTL_BIN"
printf 'lumen-remote build\n' >"$LUMEN_WEB_BIN"
EOF
    # The INPUT chain as lines in a file, listed as iptables -L -n lists it.
    cat >"$T/bin/iptables" <<'EOF'
#!/bin/sh
echo "iptables $*" >>"$CALLS"
f=$T/rules
touch "$f"
[ -z "${FAKE_NO_RULES:-}" ] || { [ "$1" != -I ] || exit 1; }
case $1 in
    -L)
        n=0
        while IFS= read -r l; do
            n=$((n + 1))
            case " $* " in
                *" --line-numbers "*) echo "$n    $l" ;;
                *) echo "$l" ;;
            esac
        done <"$f"
        ;;
    -I)
        port='' net=''
        while [ $# -gt 0 ]; do
            case $1 in --dport) port=$2 ;; -s) net=$2 ;; esac
            shift
        done
        { echo "REJECT     tcp  --  !$net  0.0.0.0/0  tcp dpt:$port reject-with icmp-port-unreachable"; cat "$f"; } >"$f.new"
        mv "$f.new" "$f"
        ;;
    -D)
        if [ $# -eq 3 ]; then
            sed -i "${3}d" "$f"
        else
            # By its spec: the first rule on that port from outside that net.
            port='' net=''
            while [ $# -gt 0 ]; do
                case $1 in --dport) port=$2 ;; -s) net=$2 ;; esac
                shift
            done
            n=$(grep -n -- "!$net .*dpt:$port " "$f" | head -n 1 | cut -d: -f1)
            [ -n "$n" ] || exit 1
            sed -i "${n}d" "$f"
        fi
        ;;
    *) exit 2 ;;
esac
EOF
    cat >"$T/bin/netstat" <<'EOF'
#!/bin/sh
[ -z "${FAKE_LISTEN:-}" ] || echo "tcp 0 0 0.0.0.0:8443 0.0.0.0:* LISTEN"
EOF
    # lumen-remote, played by sleep under its name, or failing to start.
    cat >"$T/bin/setsid" <<'EOF'
#!/bin/bash
echo "setsid $*" >>"$CALLS"
if [ -n "${FAKE_START_FAILS:-}" ]; then
    echo "lumen-remote: 0.0.0.0:8443: Address in use"
    exit 1
fi
echo "lumen-remote: listening on $3"
exec -a "$1" sleep 300
EOF
    chmod +x "$T/tv.sh" "$T/dev.sh" "$T/bin/"*
    printf 'IP:192.168.86.20\nDNS:lumen-tv.lan\n' >"$T/ca/names"
    echo "the TV's certificate" >"$T/ca/server.crt"
    echo "the TV's key" >"$T/ca/server.key"
    mkdir -p "$T/ui/assets"
    echo "<!doctype html>" >"$T/ui/index.html"
    echo "app" >"$T/ui/assets/app-1a2b.js"
    export T R CALLS
    export LUMEN_TV_SH="$T/tv.sh" LUMEN_DEV_SH="$T/dev.sh" LUMEN_CTL_BIN="$T/dist/lumen-ctl"
    export LUMEN_WEB_BIN="$T/dist/lumen-remote" LUMEN_UI_DIST="$T/ui" LUMEN_CA_DIR="$T/ca"
    export LUMEN_REMOTE_OUT="$T/out" LUMEN_REMOTE_KEYDIR="$T/pc/.lumen/remote"
    REMOTE="$BATS_TEST_DIRNAME/../remote.sh"
    D="$R/var/lib/lumen/remote"
}

teardown() {
    pkill -f "^$D/lumen-remote " 2>/dev/null || true
}

remote() {
    PATH="$T/bin:$PATH" run "$REMOTE" "$@"
}

# What tools/remote.sh install leaves, enough for web install.
installed() {
    mkdir -p "$D"
    chmod 700 "$D"
}

web_installed() {
    installed
    remote web install --allow 192.168.86.0/24
    [ "$status" -eq 0 ]
    : >"$CALLS"
}

@test "web install refuses a network that isn't NET/BITS before it runs anything" {
    for bad in "" 192.168.86.0 192.168.86.0/8 192.168.86.0/33 300.1.1.1/24 "1.2.3.4/24;reboot"; do
        remote web install --allow "$bad"
        [ "$status" -eq 1 ]
    done
    remote web install
    [ "$status" -eq 1 ]
    [ ! -s "$CALLS" ]
}

@test "web install needs the authority's certificate and the built page" {
    installed
    rm "$T/ca/server.key"
    remote web install --allow 192.168.86.0/24
    [ "$status" -eq 1 ]
    [[ "$output" == *"ca.sh init"* ]]
    echo "the TV's key" >"$T/ca/server.key"
    rm "$T/ui/index.html"
    remote web install --allow 192.168.86.0/24
    [ "$status" -eq 1 ]
    [[ "$output" == *"npm run build"* ]]
    [ ! -s "$CALLS" ]
}

@test "web install needs tools/remote.sh install first" {
    remote web install --allow 192.168.86.0/24
    [ "$status" -eq 1 ]
    [[ "$output" == *"tools/remote.sh install first"* ]]
    [ ! -e "$D" ]
}

@test "web install copies the binaries, the key and the page, checked, and writes web.conf" {
    installed
    remote web install --allow 192.168.86.0/24
    [ "$status" -eq 0 ]
    grep -qx 'dev|make ctl web' "$CALLS"
    cmp "$LUMEN_CTL_BIN" "$D/lumen-ctl"
    cmp "$LUMEN_WEB_BIN" "$D/lumen-remote"
    cmp "$T/ca/server.crt" "$D/tls/server.crt"
    cmp "$T/ca/server.key" "$D/tls/server.key"
    [ "$(stat -c %a "$D/lumen-remote")" = 700 ]
    [ "$(stat -c %a "$D/tls")" = 700 ]
    [ "$(stat -c %a "$D/tls/server.key")" = 600 ]
    [ "$(stat -c %a "$D/tls/server.crt")" = 644 ]
    cmp "$T/ui/assets/app-1a2b.js" "$D/web/assets/app-1a2b.js"
    [ "$(stat -c %a "$D/web/index.html")" = 644 ]
    [ ! -e "$D/web.tar" ] && [ ! -e "$D/web.new" ] && [ ! -e "$D/web.old" ]
    [ "$(grep -v '^#' "$D/web.conf")" = "listen 192.168.86.20:8443
allow 192.168.86.0/24
name 192.168.86.20
name lumen-tv.lan" ]
    # The authority's key never goes.
    [ -z "$(grep -r 'lumen-ca' "$D" 2>/dev/null)" ]
    [[ "$output" == *"files"* ]]
}

@test "a second web install replaces the page whole" {
    web_installed
    rm "$T/ui/assets/app-1a2b.js"
    echo "app 2" >"$T/ui/assets/app-3c4d.js"
    remote web install --allow 192.168.86.0/24
    [ "$status" -eq 0 ]
    [ ! -e "$D/web/assets/app-1a2b.js" ]
    [ -f "$D/web/assets/app-3c4d.js" ]
}

@test "web start refuses when uid or gid 61101 is someone's, and starts nothing" {
    web_installed
    echo "app:x:61101:61101::/home/app:/bin/false" >>"$R/etc/passwd"
    remote web start
    [ "$status" -eq 1 ]
    [[ "$output" == *"uid 61101"* ]]
    sed -i '$d' "$R/etc/passwd"
    echo "app:x:61101:" >>"$R/etc/group"
    remote web start
    [ "$status" -eq 1 ]
    [[ "$output" == *"gid 61101"* ]]
    [ -z "$(grep '^setsid\|^iptables -I' "$CALLS")" ]
}

@test "web start refuses when something listens on 8443" {
    web_installed
    FAKE_LISTEN=1 remote web start
    [ "$status" -eq 1 ]
    [[ "$output" == *"listens on tcp 8443"* ]]
    [ -z "$(grep '^setsid\|^iptables -I' "$CALLS")" ]
}

@test "web start needs web install" {
    installed
    remote web start
    [ "$status" -eq 1 ]
    [[ "$output" == *"web install first"* ]]
}

@test "web start adds the rule and starts lumen-remote; stop undoes both; both are safe twice" {
    web_installed
    remote web start
    [ "$status" -eq 0 ]
    [[ "$output" == *"lumen-remote runs"* ]]
    [[ "$output" == *"https://192.168.86.20:8443"* ]]
    grep -qx 'iptables -I INPUT -p tcp --dport 8443 ! -s 192.168.86.0/24 -j REJECT' "$CALLS"
    grep -q "^setsid $D/lumen-remote --listen 192.168.86.20:8443 --allow 192.168.86.0/24 --name 192.168.86.20 --name lumen-tv.lan$" "$CALLS"
    pgrep -f "^$D/lumen-remote " >/dev/null
    remote web start
    [ "$status" -eq 0 ]
    [[ "$output" == *"running already"* ]]
    [ "$(grep -c '^setsid' "$CALLS")" = 1 ]
    remote web status
    [[ "$output" == *"lumen-remote: running, pid"* ]]
    [[ "$output" == *"8443 rule: in place"* ]]
    remote web stop
    [ "$status" -eq 0 ]
    [[ "$output" == *"lumen-remote stopped"* ]]
    [ ! -s "$T/rules" ]
    ! pgrep -f "^$D/lumen-remote " >/dev/null
    remote web stop
    [ "$status" -eq 0 ]
    [[ "$output" == *"was not running"* ]]
}

@test "a start that fails takes its rule back out" {
    web_installed
    FAKE_START_FAILS=1 remote web start
    [ "$status" -eq 1 ]
    [[ "$output" == *"did not start"* ]]
    [[ "$output" == *"Address in use"* ]]
    [ ! -s "$T/rules" ]
}

@test "web start starts nothing when the rule can't go in" {
    web_installed
    FAKE_NO_RULES=1 remote web start
    [ "$status" -eq 1 ]
    [ -z "$(grep '^setsid' "$CALLS")" ]
}

@test "web stop takes out a rule for any network on 8443, and no other" {
    installed
    printf '%s\n' "REJECT     tcp  --  0.0.0.0/0  0.0.0.0/0  tcp dpt:11111 reject-with icmp-port-unreachable" \
        "REJECT     tcp  --  !10.0.0.0/8  0.0.0.0/0  tcp dpt:8443 reject-with icmp-port-unreachable" >"$T/rules"
    remote web stop
    [ "$status" -eq 0 ]
    [ "$(cat "$T/rules")" = "REJECT     tcp  --  0.0.0.0/0  0.0.0.0/0  tcp dpt:11111 reject-with icmp-port-unreachable" ]
}

@test "another rule on 8443 isn't taken for ours, and stop and remove leave it" {
    web_installed
    accept="ACCEPT     tcp  --  192.168.86.5  0.0.0.0/0  tcp dpt:8443"
    printf '%s\n' "$accept" "REJECT     tcp  --  !10.0.0.0/8  0.0.0.0/0  tcp dpt:8443 reject-with icmp-port-unreachable" >"$T/rules"
    remote web start
    [ "$status" -eq 0 ]
    [[ "$output" == *"rejecting tcp 8443 from outside 192.168.86.0/24"* ]]
    head -n 1 "$T/rules" | grep -q '^REJECT .*!192.168.86.0/24 .*dpt:8443 '
    remote web start
    [[ "$output" == *"running already"* ]]
    remote web stop
    [ "$status" -eq 0 ]
    [ "$(cat "$T/rules")" = "$accept" ]
    remote web status
    [[ "$output" == *"8443 rule: none"* ]]
    remote remove
    [ "$status" -eq 0 ]
    [ "$(cat "$T/rules")" = "$accept" ]
}

@test "remove stops lumen-remote, takes out its rule and deletes what web install copied" {
    web_installed
    remote web start
    [ "$status" -eq 0 ]
    remote remove
    [ "$status" -eq 0 ]
    [[ "$output" == *"stopped lumen-remote"* ]]
    [ ! -s "$T/rules" ]
    [ ! -e "$D" ]
    sleep 1
    ! pgrep -f "^$D/lumen-remote " >/dev/null
}

@test "web takes only its own subcommands" {
    remote web frobnicate
    [ "$status" -eq 1 ]
    remote web start now
    [ "$status" -eq 1 ]
    [ ! -s "$CALLS" ]
}
