#!/usr/bin/env bats
# Checks tools/tv.sh's argument and settings handling. None of these reach a TV:
# every case fails, or prints help, before SSH would run.

setup() {
    TV="$BATS_TEST_DIRNAME/../tv.sh"
    export LUMEN_ENV_FILE=/dev/null
    unset TV_HOST TV_KEY
}

@test "help lists the commands" {
    run "$TV" help
    [ "$status" -eq 0 ]
    [[ "$output" == *"install <file.ipk>"* ]]
    [[ "$output" != *"set -eu"* ]]
}

@test "an unknown command fails and shows help" {
    run "$TV" frobnicate
    [ "$status" -ne 0 ]
    [[ "$output" == *"unknown command: frobnicate"* ]]
}

@test "a missing TV_HOST is reported before anything connects" {
    run "$TV" check
    [ "$status" -ne 0 ]
    [[ "$output" == *"TV_HOST is not set"* ]]
}

@test "a missing key file is reported" {
    TV_HOST=192.0.2.1 TV_KEY=/nonexistent/key run "$TV" check
    [ "$status" -ne 0 ]
    [[ "$output" == *"no SSH key at /nonexistent/key"* ]]
}

@test "app ids with shell or JSON characters are refused" {
    key="$BATS_TEST_TMPDIR/key"
    touch "$key"
    TV_HOST=192.0.2.1 TV_KEY="$key" run "$TV" launch 'x"; reboot; "'
    [ "$status" -ne 0 ]
    [[ "$output" == *"not an app id"* ]]
}

@test "install refuses a file that does not exist" {
    key="$BATS_TEST_TMPDIR/key"
    touch "$key"
    TV_HOST=192.0.2.1 TV_KEY="$key" run "$TV" install /nonexistent.ipk
    [ "$status" -ne 0 ]
    [[ "$output" == *"no such file"* ]]
}

# push and tunnel: argument checks happen before any connection.
@test "push refuses remote paths outside scratch space" {
    key="$BATS_TEST_TMPDIR/key"
    touch "$key"
    src="$BATS_TEST_TMPDIR/file"
    echo hi >"$src"
    for bad in /etc/hosts /var/lib/x /media/developer/temp/ /tmp/ tmp/x \
        /tmp/../etc/passwd /media/developer/temp/a/../../../etc/x \
        '/tmp/a b' '/tmp/$(reboot)' "/tmp/a'b" /tmp//x; do
        TV_HOST=192.0.2.1 TV_KEY="$key" run "$TV" push "$src" "$bad"
        [ "$status" -ne 0 ]
        [[ "$output" == *"remote path"* ]]
    done
}

@test "push refuses a missing local file or missing arguments" {
    run "$TV" push /nonexistent /tmp/x
    [ "$status" -ne 0 ]
    [[ "$output" == *"no such file"* ]]
    run "$TV" push
    [ "$status" -ne 0 ]
    [[ "$output" == *"missing argument"* ]]
}

@test "tunnel refuses a bad port" {
    for bad in abc 0 65536 -1 8080x 99999999999999999999; do
        run "$TV" tunnel "$bad"
        [ "$status" -ne 0 ]
        [[ "$output" == *"not a port number"* ]]
    done
}

# A fake ssh on PATH stands in for the TV: it runs the remote command locally,
# with /tmp/ mapped to a test directory.
fake_ssh() {
    mkdir -p "$BATS_TEST_TMPDIR/bin" "$BATS_TEST_TMPDIR/tvroot"
    cat >"$BATS_TEST_TMPDIR/bin/ssh" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>"$BATS_TEST_TMPDIR/ssh-args"
for last; do :; done
case "\$*" in *" -N "*) exit 0 ;; esac
exec sh -c "\$(printf '%s' "\$last" | sed "s#/tmp/#$BATS_TEST_TMPDIR/tvroot/#g")"
EOF
    chmod +x "$BATS_TEST_TMPDIR/bin/ssh"
    PATH="$BATS_TEST_TMPDIR/bin:$PATH"
    key="$BATS_TEST_TMPDIR/key"
    touch "$key"
    export TV_HOST=192.0.2.1 TV_KEY="$key"
}

@test "push uploads the file and checks its size" {
    fake_ssh
    src="$BATS_TEST_TMPDIR/file"
    printf 'some bytes\n' >"$src"
    run "$TV" push "$src" /tmp/lumen-test/sub/file
    [ "$status" -eq 0 ]
    [[ "$output" == *"(11 bytes)"* ]]
    cmp "$src" "$BATS_TEST_TMPDIR/tvroot/lumen-test/sub/file"
}

@test "tunnel forwards loopback only, to the dashboard port" {
    fake_ssh
    run "$TV" tunnel 9090
    [ "$status" -eq 0 ]
    grep -q -- "-N -o ExitOnForwardFailure=yes -L 127.0.0.1:9090:127.0.0.1:8080" "$BATS_TEST_TMPDIR/ssh-args"
}
