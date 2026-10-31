#!/usr/bin/env bats
# tools/remote.sh and profiles/remote/*.sh against a fake tools/tv.sh that
# keeps the TV's files in this test's folder, a fake tools/dev.sh, and fake
# ssh-keygen and sha256sum. Nothing reaches a TV. The keys are made up.

KEY=AAAAC3NzaC1lZDI1NTE5AAAAIFakeKeyForLumenRemoteTestsOnly0123456789abc
OTHER=AAAAC3NzaC1lZDI1NTE5AAAAIAnotherFakeKeyNotThisPCs0123456789abcdefghi
OPTS='restrict,command="/var/lib/lumen/remote/lumen-ctl serve"'
OURS="$OPTS ssh-ed25519 $KEY lumen-remote"
OWNER='ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOwnersFakeKeyForTvShOnly0123456789abcdef owner@pc'

setup() {
    T="$BATS_TEST_TMPDIR"
    R="$T/tv"
    CALLS="$T/calls"
    REAL_SHA256SUM=$(command -v sha256sum)
    : >"$CALLS"
    mkdir -p "$T/bin" "$R/home/root/.ssh" "$R/var/lib/lumen"
    chmod 700 "$R/home/root/.ssh"
    printf '%s\n' "$OWNER" >"$R/home/root/.ssh/authorized_keys"
    chmod 600 "$R/home/root/.ssh/authorized_keys"

    # run and script run here, with /var/lib/ and /home/root/ moved under $R;
    # a path just after a double quote is the forced command's, and stays.
    cat >"$T/tv.sh" <<'EOF'
#!/bin/sh
printf '%s|%s\n' "$1" "$2" >>"$CALLS"
on_tv() {
    sed -e "s|\([^\"]\)/var/lib/|\1$R/var/lib/|g" -e "s|^/var/lib/|$R/var/lib/|" \
        -e "s|/home/root/|$R/home/root/|g"
}
export ON_TV=1
case $1 in
    run) sh -c "$(printf '%s\n' "$2" | on_tv)" ;;
    script) on_tv <"$2" >"$T/script.sh" && sh "$T/script.sh" </dev/null ;;
    *) exit 3 ;;
esac
EOF
    # The TV's sha256sum can be made to lie.
    cat >"$T/bin/sha256sum" <<'EOF'
#!/bin/sh
if [ -n "${ON_TV:-}" ] && [ -n "${FAKE_BAD_SHA:-}" ]; then
    cat >/dev/null
    echo "0000000000000000000000000000000000000000000000000000000000000000  -"
    exit 0
fi
exec "$REAL_SHA256SUM" "$@"
EOF
    cat >"$T/bin/ssh-keygen" <<'EOF'
#!/bin/sh
echo "ssh-keygen $*" >>"$CALLS"
while [ $# -gt 0 ]; do
    [ "$1" = -f ] && f=$2
    shift
done
echo "made-up private key" >"$f"
printf 'ssh-ed25519 %s lumen-remote\n' "$FAKE_KEY" >"$f.pub"
EOF
    # make ctl "builds" a lumen-ctl that says what it was asked.
    cat >"$T/dev.sh" <<'EOF'
#!/bin/sh
echo "dev|$*" >>"$CALLS"
printf '#!/bin/sh\necho "lumen-ctl $*"\n' >"$LUMEN_CTL_BIN"
EOF
    chmod +x "$T/tv.sh" "$T/dev.sh" "$T/bin/sha256sum" "$T/bin/ssh-keygen"
    export T R CALLS REAL_SHA256SUM FAKE_KEY="$KEY"
    export LUMEN_TV_SH="$T/tv.sh" LUMEN_DEV_SH="$T/dev.sh" LUMEN_CTL_BIN="$T/lumen-ctl.bin"
    export LUMEN_REMOTE_OUT="$T/out" LUMEN_REMOTE_KEYDIR="$T/pc/.lumen/remote"
    REMOTE="$BATS_TEST_DIRNAME/../remote.sh"
    AK="$R/home/root/.ssh/authorized_keys"
    D="$R/var/lib/lumen/remote"
}

remote() {
    PATH="$T/bin:$PATH" run "$REMOTE" "$@"
}

# The PC's key, as keygen leaves it.
pc_key() {
    mkdir -p "$LUMEN_REMOTE_KEYDIR"
    echo "made-up private key" >"$LUMEN_REMOTE_KEYDIR/id_ed25519"
    printf 'ssh-ed25519 %s lumen-remote\n' "${1:-$KEY}" >"$LUMEN_REMOTE_KEYDIR/id_ed25519.pub"
}

# Every file on the fake TV with its SHA-256, mode and size.
tv_files() {
    (cd "$R" && find . -type f | sort | while read -r f; do
        printf '%s %s %s\n' "$f" "$(stat -c %a "$f")" "$("$REAL_SHA256SUM" <"$f" | cut -d' ' -f1)"
    done)
}

installed() {
    pc_key
    remote install
    [ "$status" -eq 0 ]
    : >"$CALLS"
}

@test "the key is 68 characters, as an ed25519 key's base64 is" {
    [ ${#KEY} -eq 68 ] && [ ${#OTHER} -eq 68 ]
}

@test "an unknown command fails with usage and runs nothing" {
    remote frobnicate
    [ "$status" -eq 2 ]
    [[ "$output" == *"tools/remote.sh install"* ]]
    [ ! -s "$CALLS" ]
}

@test "keygen makes an ed25519 key with no passphrase, readable only by its owner" {
    remote keygen
    [ "$status" -eq 0 ]
    grep -qx "ssh-keygen -q -t ed25519 -N  -C lumen-remote -f $LUMEN_REMOTE_KEYDIR/id_ed25519" "$CALLS"
    [ "$(stat -c %a "$LUMEN_REMOTE_KEYDIR/id_ed25519")" = 600 ]
    [ "$(stat -c %a "$LUMEN_REMOTE_KEYDIR")" = 700 ]
    [ -f "$LUMEN_REMOTE_KEYDIR/id_ed25519.pub" ]
}

@test "keygen never replaces a key, private or public" {
    for f in id_ed25519 id_ed25519.pub; do
        rm -rf "$LUMEN_REMOTE_KEYDIR"
        mkdir -p "$LUMEN_REMOTE_KEYDIR"
        echo "the owner's" >"$LUMEN_REMOTE_KEYDIR/$f"
        remote keygen
        [ "$status" -eq 1 ]
        [[ "$output" == *"never replaces"* ]]
        [ "$(cat "$LUMEN_REMOTE_KEYDIR/$f")" = "the owner's" ]
    done
    [ ! -s "$CALLS" ]
}

@test "install builds, copies lumen-ctl checked, and adds one line after the owner's" {
    pc_key
    remote install
    [ "$status" -eq 0 ]
    grep -qx 'dev|make ctl' "$CALLS"
    cmp "$LUMEN_CTL_BIN" "$D/lumen-ctl"
    [ "$(stat -c %a "$D/lumen-ctl")" = 700 ]
    [ "$(stat -c %a "$D")" = 700 ]
    [ ! -e "$D/lumen-ctl.new" ]
    [[ "$output" == *"SHA-256 checked"* ]]
    # The owner's line first and unchanged, then ours; the mode kept.
    [ "$(cat "$AK")" = "$OWNER
$OURS" ]
    [ "$(stat -c %a "$AK")" = 600 ]
    [ ! -e "$AK.lumen-new" ]
    # Nothing goes through tools/tv.sh push, so nothing lands in the TV's
    # /tmp, which the apps' jails share.
    [ -z "$(grep '^push|' "$CALLS")" ]
    [ -z "$(grep '^run|' "$CALLS" | grep '/tmp/')" ]
}

@test "install keeps authorized_keys as it was, on the TV and here" {
    pc_key
    remote install
    [ "$status" -eq 0 ]
    [ "$(cat "$D/authorized_keys.before")" = "$OWNER" ]
    [ "$(cat "$T/out/authorized_keys.before")" = "$OWNER" ]
    [ "$(stat -c %a "$T/out/authorized_keys.before")" = 600 ]
}

@test "install prints the line with the key cut to 8 characters" {
    pc_key
    remote install
    [ "$status" -eq 0 ]
    [[ "$output" == *"the line: $OPTS ssh-ed25519 AAAAC3Nz... lumen-remote"* ]]
    [[ "$output" != *"$KEY"* ]]
    [[ "$output" != *"${KEY:8}"* ]]
}

@test "a second install adds no line and keeps the first copy of the file" {
    installed
    printf '%s\n' "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAddedLater owner@laptop" >>"$AK"
    remote install
    [ "$status" -eq 0 ]
    [[ "$output" == *"has this key's line already"* ]]
    [ "$(grep -c 'lumen-remote$' "$AK")" -eq 1 ]
    [ "$(cat "$D/authorized_keys.before")" = "$OWNER" ]
    [ "$(cat "$T/out/authorized_keys.before")" = "$OWNER" ]
}

@test "install refuses when a lumen-remote line for another key is there" {
    printf '%s ssh-ed25519 %s lumen-remote\n' "$OPTS" "$OTHER" >>"$AK"
    before=$(cat "$AK")
    pc_key
    remote install
    [ "$status" -ne 0 ]
    [[ "$output" == *"another key or with other options"* ]]
    [ "$(cat "$AK")" = "$before" ]
}

@test "install puts its line on a line of its own after one with no newline" {
    printf '%s' "$OWNER" >"$AK"
    pc_key
    remote install
    [ "$status" -eq 0 ]
    [ "$(sed -n 1p "$AK")" = "$OWNER" ]
    [ "$(sed -n 2p "$AK")" = "$OURS" ]
    [ "$(wc -l <"$AK")" -eq 2 ]
}

@test "install stops when the copy on the TV differs, and touches nothing else" {
    pc_key
    before=$(cat "$AK")
    FAKE_BAD_SHA=1 remote install
    [ "$status" -eq 1 ]
    [[ "$output" == *"authorized_keys was not touched"* ]]
    [ ! -e "$D/lumen-ctl" ] && [ ! -e "$D/lumen-ctl.new" ]
    [ "$(cat "$AK")" = "$before" ]
    [ -z "$(grep '^script|' "$CALLS")" ]
}

@test "install refuses a key that is missing, not ed25519 or not plain base64" {
    remote install
    [ "$status" -eq 1 ]
    [[ "$output" == *"tools/remote.sh keygen makes one"* ]]
    for bad in "ssh-rsa $KEY lumen-remote" \
        "ssh-ed25519 AAAAB3NzaC1yc2EAAAAI${KEY:25} lumen-remote" \
        "ssh-ed25519 ${KEY%?}\" lumen-remote" \
        "ssh-ed25519 ${KEY}A lumen-remote" \
        "ssh-ed25519 $KEY lumen-remote
ssh-ed25519 $KEY lumen-remote"; do
        pc_key
        printf '%s\n' "$bad" >"$LUMEN_REMOTE_KEYDIR/id_ed25519.pub"
        remote install
        [ "$status" -eq 1 ]
    done
    [ ! -s "$CALLS" ]
    [ "$(cat "$AK")" = "$OWNER" ]
}

@test "authorize.sh refuses to run with its placeholder unfilled" {
    mkdir -p "$D" && printf '#!/bin/sh\n' >"$D/lumen-ctl" && chmod 700 "$D/lumen-ctl"
    run "$T/tv.sh" script "$BATS_TEST_DIRNAME/../../profiles/remote/authorize.sh"
    [ "$status" -eq 1 ]
    [[ "$output" == *"placeholder"* ]]
    [ "$(cat "$AK")" = "$OWNER" ]
}

@test "remove takes out only Lumen Remote's line, and every other stays as it was" {
    installed
    # Look-alikes: the same comment without the options, the options without
    # restrict, a comment and a blank line.
    cat >"$AK" <<EOF
# keys for root
$OWNER
ssh-ed25519 $OTHER lumen-remote

$OURS
command="/var/lib/lumen/remote/lumen-ctl serve" ssh-ed25519 $OTHER lumen-remote
ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAddedLater owner@laptop
EOF
    want=$(grep -vxF "$OURS" "$AK")
    remote remove
    [ "$status" -eq 0 ]
    [ "$(cat "$AK")" = "$want" ]
    [ "$(grep -c '' "$AK")" -eq 6 ]
    [ "$(stat -c %a "$AK")" = 600 ]
    [ ! -e "$D" ]
    [ ! -e "$AK.lumen-new" ]
    [[ "$output" == *"removed Lumen Remote's line"* ]]
}

@test "remove refuses, changing nothing, when no other key would be left" {
    installed
    printf '# only a comment\n\n%s\n' "$OURS" >"$AK"
    before=$(tv_files)
    remote remove
    [ "$status" -eq 1 ]
    [[ "$output" == *"no way in"* ]]
    [ "$(tv_files)" = "$before" ]
}

@test "remove with nothing installed changes nothing and succeeds" {
    before=$(tv_files)
    remote remove
    [ "$status" -eq 0 ]
    [[ "$output" == *"no Lumen Remote line"* ]]
    [ "$(tv_files)" = "$before" ]
    remote remove
    [ "$status" -eq 0 ]
}

@test "remove after install leaves authorized_keys as it was before" {
    pc_key
    remote install
    [ "$status" -eq 0 ]
    remote remove
    [ "$status" -eq 0 ]
    [ "$(cat "$AK")" = "$OWNER" ]
    [ "$(cat "$T/out/authorized_keys.before")" = "$OWNER" ]
}

@test "status, probe, probe-keys and config with no change are read-only" {
    installed
    before=$(tv_files)
    remote status
    [ "$status" -eq 0 ]
    remote probe
    [ "$status" -eq 0 ]
    remote probe-keys command
    [ "$status" -eq 0 ]
    remote config
    [ "$status" -eq 0 ]
    [ "$(tv_files)" = "$before" ]
    [ -z "$(grep -v '^run|\|^script|' "$CALLS")" ]
}

@test "status names this PC's key and build, and never prints a key" {
    installed
    remote status
    [ "$status" -eq 0 ]
    [[ "$output" == *"on the TV: lumen-remote line: this PC's key"* ]]
    [[ "$output" == *"on the TV: binary: "*" (this build)"* ]]
    [[ "$output" == *"on the TV: sessions: 0"* ]]
    [[ "$output" == *"on the TV: authorized_keys.before: "*" bytes"* ]]
    [[ "$output" != *"$KEY"* ]]
    [[ "$output" != *"${OWNER#ssh-ed25519 }"* ]]
}

@test "status tells another key's line from this PC's" {
    printf '%s ssh-ed25519 %s lumen-remote\n' "$OPTS" "$OTHER" >>"$AK"
    pc_key
    remote status
    [ "$status" -eq 0 ]
    [[ "$output" == *"on the TV: lumen-remote line: key "* ]]
    [[ "$output" != *"this PC's key"* ]]
    [[ "$output" == *"on the TV: binary: none"* ]]
}

@test "probe runs lumen-ctl's probe and saves what it printed" {
    installed
    remote probe library
    [ "$status" -eq 0 ]
    [[ "$output" == *"lumen-ctl probe --bus library"* ]]
    grep -qx 'lumen-ctl probe --bus library' "$T"/out/probe-*.raw.txt
    remote probe-keys
    [ "$status" -eq 0 ]
    grep -qx 'lumen-ctl probe-keys' "$T"/out/probe-keys-*.raw.txt
}

@test "probe takes only library or command" {
    for arg in fake "library; rm -rf /" "--bus library"; do
        remote probe "$arg"
        [ "$status" -eq 1 ]
    done
    remote probe library command
    [ "$status" -eq 1 ]
    [ ! -s "$CALLS" ]
}

@test "config writes ctl.conf, keeping the value it isn't given" {
    installed
    remote config bus=library
    [ "$status" -eq 0 ]
    grep -qx 'bus library' "$D/ctl.conf"
    grep -qx 'keys none' "$D/ctl.conf"
    remote config keys=keymanager
    [ "$status" -eq 0 ]
    grep -qx 'bus library' "$D/ctl.conf"
    grep -qx 'keys keymanager' "$D/ctl.conf"
    [ "$(stat -c %a "$D/ctl.conf")" = 600 ]
    remote config
    [[ "$output" == *"bus library"* ]]
}

@test "config refuses other values before it runs anything" {
    installed
    for arg in bus=fake "keys=uinput" "bus=library;id" log=/tmp/x; do
        remote config "$arg"
        [ "$status" -eq 1 ]
    done
    [ ! -s "$CALLS" ]
}

@test "config needs lumen-ctl's folder" {
    remote config bus=library
    [ "$status" -ne 0 ]
    [ ! -e "$D" ]
}

@test "keys copies a template, checked, and --remove deletes it" {
    installed
    echo '{"service":"keymanager","press":[{"code":"$code"}],"codes":{"up":38}}' >"$T/keys.json"
    remote keys "$T/keys.json"
    [ "$status" -eq 0 ]
    cmp "$T/keys.json" "$D/keys.json"
    [ "$(stat -c %a "$D/keys.json")" = 600 ]
    remote keys --remove
    [ "$status" -eq 0 ]
    [ ! -e "$D/keys.json" ]
}

@test "keys refuses a file over 16 KiB, or none" {
    installed
    head -c 16385 /dev/zero >"$T/big.json"
    remote keys "$T/big.json"
    [ "$status" -eq 1 ]
    remote keys
    [ "$status" -eq 1 ]
    remote keys "$T/no-such-file"
    [ "$status" -eq 1 ]
    [ ! -s "$CALLS" ]
}
