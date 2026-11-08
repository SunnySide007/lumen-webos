#!/usr/bin/env bats
# packaging/shell/lumen-privacy-read.sh against a fake file system
# (LUMEN_PRIVACY_READ_PREFIX) with a fake iptables and initctl. Nothing reaches a TV.

setup() {
    SCRIPT="$BATS_TEST_DIRNAME/../../packaging/shell/lumen-privacy-read.sh"
    ROOT="$BATS_TEST_TMPDIR/root"
    mkdir -p "$ROOT/var/lib/lumen/guard" "$ROOT/usr/sbin" "$ROOT/sbin"
    export LUMEN_PRIVACY_READ_PREFIX="$ROOT"
}

fakes() {
    cat >"$ROOT/usr/sbin/iptables" <<'EOF'
#!/bin/sh
[ "$*" = "-L INPUT -n" ] || exit 2
echo "Chain INPUT (policy ACCEPT)"
echo "target     prot opt source               destination"
echo "REJECT     tcp  --  0.0.0.0/0            0.0.0.0/0            tcp dpt:11111 reject-with icmp-port-unreachable"
EOF
    cat >"$ROOT/sbin/initctl" <<'EOF'
#!/bin/sh
[ "$1" = status ] || exit 2
case $2 in
    rdxd) echo "initctl: Unknown job: rdxd" >&2; exit 1 ;;
    *) echo "$2 stop/waiting" ;;
esac
EOF
    chmod +x "$ROOT/usr/sbin/iptables" "$ROOT/sbin/initctl"
}

@test "it prints the status file, the rules and each job, in that order" {
    fakes
    printf 'mode block\nlookups_today 6789\n' >"$ROOT/var/lib/lumen/guard/status"
    run "$SCRIPT"
    [ "$status" -eq 0 ]
    expected='mode block
lookups_today 6789
--- rules
Chain INPUT (policy ACCEPT)
target     prot opt source               destination
REJECT     tcp  --  0.0.0.0/0            0.0.0.0/0            tcp dpt:11111 reject-with icmp-port-unreachable
--- jobs
crashreportd stop/waiting
uploadd stop/waiting
remotelogger stop/waiting'
    [ "$output" = "$expected" ]
}

@test "what it cannot read it leaves empty, and it still exits 0" {
    run "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = $'--- rules\n--- jobs' ]
}

@test "it writes nothing" {
    fakes
    printf 'mode log\n' >"$ROOT/var/lib/lumen/guard/status"
    before=$(find "$ROOT" -exec ls -ld {} + | sort)
    run "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$(find "$ROOT" -exec ls -ld {} + | sort)" = "$before" ]
}
