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
    [[ "$output" == *"launch <app id> [json]"* ]]
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

@test "launch refuses parameters that are not one plain JSON object, before connecting" {
    for bad in look '"look"' '{"look":"plain"' 'x{"look":"plain"}' \
        '{"a":"$(reboot)"}' "{\"a\":\"it's\"}" '{"a":"b\"}' '{"a":"`reboot`"}' \
        '{"a":1};{}' '{"a":1} | reboot {}' '{"a":[1]}'; do
        run "$TV" launch netflix "$bad"
        [ "$status" -ne 0 ]
        [[ "$output" == *"launch parameters"* ]]
    done
}

@test "install refuses a file that does not exist" {
    key="$BATS_TEST_TMPDIR/key"
    touch "$key"
    TV_HOST=192.0.2.1 TV_KEY="$key" run "$TV" install /nonexistent.ipk
    [ "$status" -ne 0 ]
    [[ "$output" == *"no such file"* ]]
}

# push and tunnel: argument checks happen before any connection.
@test "push refuses remote paths outside root's stage, /tmp and developer/temp too" {
    key="$BATS_TEST_TMPDIR/key"
    touch "$key"
    src="$BATS_TEST_TMPDIR/file"
    echo hi >"$src"
    for bad in /tmp/x /tmp/lumen-install.ipk /media/developer/temp/x \
        /media/developer/temp/lumen-capture/tcpdump \
        /etc/hosts /var/lib/x /media/developer/temp/ /tmp/ tmp/x \
        /var/lib/lumen/stage/ /var/lib/lumen/stagex /var/lib/lumen/stage/../guard/x \
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
# and a script sent to `sh -s` too, with /tmp/, the apps' jails
# (/var/palm/jail) and /var/lib/lumen moved into tvroot, and root's uid
# changed to ours, or to fake_ssh's argument. The /tmp/ line comes first: the
# paths the others write are under /tmp themselves. FAKE_TAMPER changes the
# IPK once it is uploaded.
fake_ssh() {
    R="$BATS_TEST_TMPDIR/tvroot"
    mkdir -p "$BATS_TEST_TMPDIR/bin" "$R"
    cat >"$BATS_TEST_TMPDIR/tvmap.sed" <<EOF
s#/tmp/#$R/#g
s#/var/palm/jail#$R/jail#g
s#/var/lib/lumen#$R/var/lib/lumen#g
s#^ROOT_UID=0\$#ROOT_UID=${1:-$(id -u)}#
EOF
    cat >"$BATS_TEST_TMPDIR/bin/ssh" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>"$BATS_TEST_TMPDIR/ssh-args"
for last; do :; done
case "\$*" in *" -N "*) exit 0 ;; esac
cmd=\$(printf '%s' "\$last" | sed -f "$BATS_TEST_TMPDIR/tvmap.sed")
if [ "\$cmd" = "sh -s" ]; then
    sed -f "$BATS_TEST_TMPDIR/tvmap.sed" | sh -s
    exit
fi
sh -c "\$cmd" || exit
case "\$cmd" in
    *"cat > "*install.ipk) [ -z "\${FAKE_TAMPER:-}" ] || printf x >>"$R/var/lib/lumen/stage/install.ipk" ;;
esac
EOF
    chmod +x "$BATS_TEST_TMPDIR/bin/ssh"
    PATH="$BATS_TEST_TMPDIR/bin:$PATH"
    key="$BATS_TEST_TMPDIR/key"
    touch "$key"
    export TV_HOST=192.0.2.1 TV_KEY="$key"
}

# A fake luna-send-pub: logs its call and answers that the app is installed.
fake_installer() {
    cat >"$BATS_TEST_TMPDIR/bin/luna-send-pub" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>"$BATS_TEST_TMPDIR/luna-calls"
echo '{"returnValue": true, "details": {"state": "installed"}}'
EOF
    chmod +x "$BATS_TEST_TMPDIR/bin/luna-send-pub"
}

# A fake luna-send: logs its call and answers.
fake_luna() {
    cat >"$BATS_TEST_TMPDIR/bin/luna-send" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>"$BATS_TEST_TMPDIR/luna-calls"
echo '{"returnValue": true}'
EOF
    chmod +x "$BATS_TEST_TMPDIR/bin/luna-send"
}

@test "launch sends the app id alone, or with its launch parameters as params" {
    fake_ssh
    fake_luna
    run "$TV" launch netflix
    [ "$status" -eq 0 ]
    run "$TV" launch io.github.sunnyside007.lumen.shell '{"look":"plain +hero, -dither"}'
    [ "$status" -eq 0 ]
    run "$TV" close io.github.sunnyside007.lumen.shell
    [ "$status" -eq 0 ]
    m=luna://com.webos.applicationManager
    [ "$(cat "$BATS_TEST_TMPDIR/luna-calls")" = "-n 1 -f $m/launch {\"id\":\"netflix\"}
-n 1 -f $m/launch {\"id\":\"io.github.sunnyside007.lumen.shell\",\"params\":{\"look\":\"plain +hero, -dither\"}}
-n 1 -f $m/closeByAppId {\"id\":\"io.github.sunnyside007.lumen.shell\"}" ]
}

@test "push uploads into root's stage, made root's alone, and checks the size" {
    fake_ssh
    src="$BATS_TEST_TMPDIR/file"
    printf 'some bytes\n' >"$src"
    run "$TV" push "$src" /var/lib/lumen/stage/lumen-test/sub/file
    [ "$status" -eq 0 ]
    [[ "$output" == *"(11 bytes)"* ]]
    cmp "$src" "$R/var/lib/lumen/stage/lumen-test/sub/file"
    [[ "$(ls -ldn "$R/var/lib/lumen/stage")" == drwx------* ]]
    [[ "$(ls -ldn "$R/var/lib/lumen/stage/lumen-test/sub")" == drwx------* ]]
    [[ "$(ls -ln "$R/var/lib/lumen/stage/lumen-test/sub/file")" == -rw-------* ]]
}

@test "push uploads nothing to a stage that is a link, others may write, or isn't root's" {
    fake_ssh
    src="$BATS_TEST_TMPDIR/file"
    echo hi >"$src"
    mkdir -p "$R/var/lib/lumen" "$R/elsewhere"
    ln -s "$R/elsewhere" "$R/var/lib/lumen/stage"
    run "$TV" push "$src" /var/lib/lumen/stage/x
    [ "$status" -ne 0 ]
    [[ "$output" == *"stage is not a real folder"* ]]
    [[ "$output" == *"nothing uploaded"* ]]
    [ ! -e "$R/elsewhere/x" ]
    rm "$R/var/lib/lumen/stage"
    mkdir "$R/var/lib/lumen/stage"
    chmod 777 "$R/var/lib/lumen/stage"
    run "$TV" push "$src" /var/lib/lumen/stage/x
    [ "$status" -ne 0 ]
    [[ "$output" == *"writable by others (drwxrwxrwx)"* ]]
    [ ! -e "$R/var/lib/lumen/stage/x" ]
    chmod 700 "$R/var/lib/lumen/stage"
    fake_ssh 99999
    run "$TV" push "$src" /var/lib/lumen/stage/x
    [ "$status" -ne 0 ]
    [[ "$output" == *"is not root's"* ]]
    [ ! -e "$R/var/lib/lumen/stage/x" ]
}

@test "install stages the IPK in root's stage, checks it, installs it from there and tidies up" {
    fake_ssh
    fake_installer
    ipk="$BATS_TEST_TMPDIR/app.ipk"
    printf 'an ipk\n' >"$ipk"
    run "$TV" install "$ipk"
    [ "$status" -eq 0 ]
    [[ "$output" == *"installed"* ]]
    grep -qF "\"ipkUrl\":\"$R/var/lib/lumen/stage/install.ipk\"" "$BATS_TEST_TMPDIR/luna-calls"
    [ ! -e "$R/var/lib/lumen/stage/install.ipk" ]
    [ ! -e "$R/var/lib/lumen/stage/install.log" ]
    # Nothing went through /tmp.
    [ "$(ls -A "$R")" = var ]
}

@test "install installs nothing when the staged IPK isn't the PC's, or the stage isn't safe" {
    fake_ssh
    fake_installer
    ipk="$BATS_TEST_TMPDIR/app.ipk"
    printf 'an ipk\n' >"$ipk"
    FAKE_TAMPER=1 run "$TV" install "$ipk"
    [ "$status" -ne 0 ]
    [[ "$output" == *"the staged IPK is not this one"* ]]
    [ ! -e "$BATS_TEST_TMPDIR/luna-calls" ]
    [ ! -e "$R/var/lib/lumen/stage/install.ipk" ]
    chmod 777 "$R/var/lib/lumen/stage"
    run "$TV" install "$ipk"
    [ "$status" -ne 0 ]
    [[ "$output" == *"nothing uploaded"* ]]
    [ ! -e "$BATS_TEST_TMPDIR/luna-calls" ]
    [ ! -e "$R/var/lib/lumen/stage/install.ipk" ]
}

@test "tunnel forwards loopback only, to the dashboard port" {
    fake_ssh
    run "$TV" tunnel 9090
    [ "$status" -eq 0 ]
    grep -q -- "-N -o ExitOnForwardFailure=yes -L 127.0.0.1:9090:127.0.0.1:8080" "$BATS_TEST_TMPDIR/ssh-args"
}

# log: the shell writes /tmp/lumen-shell.log, and lumen-shell.fallback.log next
# to it when the first cannot be opened. A jailed app's /tmp is under
# /var/palm/jail.
@test "log prints every lumen-shell log in /tmp, each under its path" {
    fake_ssh
    tvroot="$BATS_TEST_TMPDIR/tvroot"
    printf 'main one\nmain two\n' >"$tvroot/lumen-shell.log"
    printf 'fallback one\n' >"$tvroot/lumen-shell.fallback.log"
    printf 'install text\n' >"$tvroot/lumen-install.log"
    mkdir -p "$tvroot/jail/app/tmp"
    printf 'jailed text\n' >"$tvroot/jail/app/tmp/lumen-shell.log"
    run "$TV" log
    [ "$status" -eq 0 ]
    [[ "$output" == *"# $tvroot/lumen-shell.log"$'\n'"main one"$'\n'"main two"* ]]
    [[ "$output" == *"# $tvroot/lumen-shell.fallback.log"$'\n'"fallback one"* ]]
    # Other logs and the jails are left alone while /tmp has the shell's.
    [[ "$output" != *"install text"* ]]
    [[ "$output" != *"jailed text"* ]]
}

@test "log prints the last 200 lines of each log" {
    fake_ssh
    tvroot="$BATS_TEST_TMPDIR/tvroot"
    for f in lumen-shell.log lumen-shell.fallback.log; do
        i=1
        while [ "$i" -le 250 ]; do
            printf 'row-%03d\n' "$i"
            i=$((i + 1))
        done >"$tvroot/$f"
    done
    run "$TV" log
    [ "$status" -eq 0 ]
    [[ "$output" == *"row-250"* ]]
    [[ "$output" == *"row-051"* ]]
    [[ "$output" != *"row-050"* ]]
    # Two headers and 200 lines under each.
    [ "${#lines[@]}" -eq 402 ]
}

@test "log reads only the fallback log when that is all there is" {
    fake_ssh
    printf 'fallback only\n' >"$BATS_TEST_TMPDIR/tvroot/lumen-shell.fallback.log"
    run "$TV" log
    [ "$status" -eq 0 ]
    [[ "$output" == "# $BATS_TEST_TMPDIR/tvroot/lumen-shell.fallback.log"$'\n'"fallback only" ]]
}

@test "log falls back to the app's jail when /tmp has none" {
    fake_ssh
    jail="$BATS_TEST_TMPDIR/tvroot/jail/app/tmp"
    mkdir -p "$jail" "$BATS_TEST_TMPDIR/tvroot/jail/other/tmp"
    printf 'jailed main\n' >"$jail/lumen-shell.log"
    printf 'jailed fallback\n' >"$jail/lumen-shell.fallback.log"
    run "$TV" log
    [ "$status" -eq 0 ]
    [[ "$output" == *"# $jail/lumen-shell.log"$'\n'"jailed main"* ]]
    [[ "$output" == *"# $jail/lumen-shell.fallback.log"$'\n'"jailed fallback"* ]]
}

@test "log finds a jail that holds only the fallback log" {
    fake_ssh
    jail="$BATS_TEST_TMPDIR/tvroot/jail/app/tmp"
    mkdir -p "$jail"
    printf 'jailed fallback\n' >"$jail/lumen-shell.fallback.log"
    run "$TV" log
    [ "$status" -eq 0 ]
    [[ "$output" == "# $jail/lumen-shell.fallback.log"$'\n'"jailed fallback" ]]
}

@test "log says so when there is no log anywhere" {
    fake_ssh
    mkdir -p "$BATS_TEST_TMPDIR/tvroot/jail/app/tmp"
    run "$TV" log
    [ "$status" -eq 0 ]
    [ "$output" = "no lumen-shell.log yet" ]
}
