#!/usr/bin/env bats
# packaging/shell/lumen-icons.sh against a fake file system (LUMEN_ICONS_PREFIX)
# and a fake luna-send that answers from a file. Nothing reaches a TV.

setup() {
    SCRIPT="$BATS_TEST_DIRNAME/../../packaging/shell/lumen-icons.sh"
    FIXTURE="$BATS_TEST_DIRNAME/../../shell/crates/lumen-shell/fixtures/luna/b9-launch-points.json"
    ROOT="$BATS_TEST_TMPDIR/root"
    APP="$ROOT/media/developer/apps/usr/palm/applications/io.github.sunnyside007.lumen.shell"
    OUT="$APP/lg-icons"
    SMALL="$ROOT/usr/palm/plugins/inputapps/assets/small"
    POINTS="$BATS_TEST_TMPDIR/points.json"
    LUNA_ARGS="$BATS_TEST_TMPDIR/luna-args"
    mkdir -p "$APP" "$ROOT/usr/bin" "$SMALL"
    chmod 0755 "$APP"
    cat >"$ROOT/usr/bin/luna-send" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" >"$LUNA_ARGS"
[ -z "${FAKE_LUNA_FAIL:-}" ] || exit 1
# As on the B9: with any stdin but /dev/null it answers nothing, and exits 0.
[ "$(readlink /proc/self/fd/0)" = /dev/null ] || exit 0
cat "$POINTS"
EOF
    chmod +x "$ROOT/usr/bin/luna-send"
    export LUMEN_ICONS_PREFIX="$ROOT" POINTS LUNA_ARGS
}

# png PATH [BODY]: a file that starts with the PNG signature.
png() {
    mkdir -p "$(dirname "$1")"
    printf '\211PNG\r\n\032\n%s' "${2:-body}" >"$1"
}

# points PATH...: a one-line listLaunchPoints reply naming each PATH as an icon.
points() {
    {
        printf '{"returnValue":true,"launchPoints":['
        sep=
        for p in "$@"; do
            printf '%s{"id":"x","icon":"%s","largeIcon":"","params":{}}' "$sep" "$p"
            sep=,
        done
        printf ']}\n'
    } >"$POINTS"
}

# The files in the copy, relative to it, sorted.
copied_files() {
    (cd "$OUT" && find . -type f | sed 's|^\./||' | sort)
}

@test "copies the icons the B9's launch points name, mirrored by path" {
    cp "$FIXTURE" "$POINTS"
    for p in $(grep -oE '"(icon|largeIcon)": "/[^"]*"' "$FIXTURE" | sed 's/^[^:]*: "//; s/"$//'); do
        png "$ROOT$p" "$p"
    done
    png "$SMALL/hometheater.png"
    png "$SMALL/pc.png"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 10 copied, 0 refused" ]
    [ "$(cat "$OUT/status")" = "$output" ]
    expected=".sum
media/cryptofs/apps/usr/palm/applications/cdp-30/Plex_130x130.png
media/cryptofs/apps/usr/palm/applications/cdp-30/Plex_80x80.png
media/cryptofs/apps/usr/palm/applications/netflix/LARGE_APP_ICON.png
media/cryptofs/apps/usr/palm/applications/netflix/SMALL_APP_ICON.png
mnt/otncabi/usr/palm/applications/com.webos.app.discovery/asset/hd1080/lgstore.png
mnt/otncabi/usr/palm/applications/com.webos.app.discovery/asset/hd1080/lgstore_130x130.png
status
usr/palm/applications/com.webos.app.photovideo/icon.png
usr/palm/applications/com.webos.app.photovideo/icon_photovideo.png
usr/palm/plugins/inputapps/assets/small/hometheater.png
usr/palm/plugins/inputapps/assets/small/pc.png"
    [ "$(copied_files)" = "$expected" ]
    cmp "$OUT/media/cryptofs/apps/usr/palm/applications/netflix/LARGE_APP_ICON.png" \
        "$ROOT/media/cryptofs/apps/usr/palm/applications/netflix/LARGE_APP_ICON.png"
    [ "$(stat -c %a "$OUT/usr/palm/applications/com.webos.app.photovideo/icon.png")" = 644 ]
    [ "$(stat -c %a "$OUT/usr/palm/applications/com.webos.app.photovideo")" = 755 ]
    [ "$(stat -c %a "$OUT")" = 755 ]
    # Developer-mode apps are read where they are, so they are not copied.
    [ ! -e "$OUT/media/developer" ]
    [ "$(cat "$LUNA_ARGS")" = "-n
1
-w
4000
luna://com.webos.applicationManager/listLaunchPoints
{}" ]
    # Nothing is left beside the copy (the folder also holds Lumen's icons).
    [ "$(ls -A "$APP" | grep '^lg-icons')" = "lg-icons" ]
}

@test "refuses paths outside the roots, odd names, links out and anything not a small PNG" {
    A="$ROOT/usr/palm/applications/a"
    png "$A/good.png"
    png "$A/sub/i.png"
    ln -s sub/i.png "$A/inner.png"
    png "$ROOT/etc/secret.png" "a secret"
    ln -s ../../../../etc/secret.png "$A/link.png"
    png "$ROOT/usr/palm/applications/b/i.png"
    ln -s ../b/i.png "$A/other.png"
    printf 'a secret' >"$A/text.png"
    png "$A/big.png" "$(head -c 1048577 /dev/zero | tr '\0' x)"
    mkdir -p "$A/dir.png"
    mkfifo "$A/fifo.png"
    png "$A/i.jpg"
    png "$ROOT/usr/palm/applications/.hidden/i.png"
    png "$ROOT/usr/palm/applications/a b/i.png"
    points /usr/palm/applications/a/good.png /usr/palm/applications/a/inner.png \
        /etc/secret.png /usr/palm/applications/a/link.png /usr/palm/applications/a/other.png \
        /usr/palm/applications/a/text.png /usr/palm/applications/a/big.png \
        /usr/palm/applications/a/dir.png /usr/palm/applications/a/fifo.png \
        /usr/palm/applications/a/missing.png /usr/palm/applications/a/i.jpg \
        /usr/palm/applications/a /usr/palm/applications/a/../b/i.png \
        /usr/palm/applications/a/./good.png /usr/palm/applications/a//good.png \
        /usr/palm/applications/.hidden/i.png "/usr/palm/applications/a b/i.png" \
        usr/palm/applications/a/good.png /usr/palm/applicationsX/a/good.png \
        '/usr/palm/applications/a/q\"x.png'
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 2 copied, 18 refused" ]
    [ "$(copied_files)" = ".sum
status
usr/palm/applications/a/good.png
usr/palm/applications/a/inner.png" ]
    [ -z "$(find "$OUT" -type l)" ]
    [ -f "$OUT/usr/palm/applications/a/inner.png" ]
}

@test "the input icons are LG's own PNG files, nothing else" {
    png "$SMALL/HDMI_4.png"
    printf 'not a png' >"$SMALL/broken.png"
    printf 'notes' >"$SMALL/readme.txt"
    # Six folders up from small/ is the fake root: a real PNG outside it.
    png "$ROOT/etc/secret.png" "a secret"
    ln -s ../../../../../../etc/secret.png "$SMALL/away.png"
    [ -f "$SMALL/away.png" ]
    points
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 1 copied, 2 refused" ]
    [ "$(copied_files)" = ".sum
status
usr/palm/plugins/inputapps/assets/small/HDMI_4.png" ]
    ! grep -rq "a secret" "$OUT"
}

@test "LG Home's icon is copied though no launch point names it, where listApps puts it" {
    LG_ICON=/usr/palm/applications/com.webos.app.home/assets/icon.png
    # The shell finds the copy by the path the B9's listApps gives.
    apps="$BATS_TEST_DIRNAME/../../shell/crates/lumen-shell/fixtures/luna/b9-apps.json"
    run python3 -I -c 'import json, sys
home = [a for a in json.load(open(sys.argv[1]))["apps"] if a["id"] == "com.webos.app.home"][0]
for key in ("largeIcon", "icon"):
    print(home["folderPath"] + "/" + home[key])' "$apps"
    [ "$status" -eq 0 ]
    [ "$output" = "$LG_ICON
$LG_ICON" ]
    png "$ROOT$LG_ICON" "LG Home"
    points
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 1 copied, 0 refused" ]
    [ "$(copied_files)" = ".sum
status
usr/palm/applications/com.webos.app.home/assets/icon.png" ]
    cmp "$OUT$LG_ICON" "$ROOT$LG_ICON"
}

@test "LG Home's icon is checked like any other, and copied once" {
    LG_ICON=/usr/palm/applications/com.webos.app.home/assets/icon.png
    png "$ROOT$LG_ICON"
    points "$LG_ICON"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 1 copied, 0 refused" ]
    # A link out of its folder is refused, as a launch point's would be.
    rm "$ROOT$LG_ICON"
    png "$ROOT/etc/secret.png" "a secret"
    ln -s ../../../../../etc/secret.png "$ROOT$LG_ICON"
    [ -f "$ROOT$LG_ICON" ]
    points
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 0 copied, 1 refused" ]
    [ ! -e "$OUT$LG_ICON" ]
    run grep -rq "a secret" "$OUT"
    [ "$status" -eq 1 ]
    # Nothing at its path is nothing to plan: the same plan, so no change.
    rm "$ROOT$LG_ICON"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: unchanged" ]
}

@test "an icon replaced where it was is copied again" {
    png "$ROOT/usr/palm/applications/a/i.png" old
    points /usr/palm/applications/a/i.png
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    png "$ROOT/usr/palm/applications/a/i.png" "new and longer"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 1 copied, 0 refused" ]
    cmp "$ROOT/usr/palm/applications/a/i.png" "$OUT/usr/palm/applications/a/i.png"
}

@test "a run cut off mid-swap is put right, and never loses the last copy" {
    png "$ROOT/usr/palm/applications/a/i.png"
    points /usr/palm/applications/a/i.png
    run sh "$SCRIPT"
    # Cut off between the two renames: only lg-icons.old is left.
    mv "$OUT" "$APP/lg-icons.old"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: unchanged" ]
    [ "$(ls -A "$APP")" = "lg-icons" ]
    # A run that fails while only lg-icons.old is left keeps it.
    mv "$OUT" "$APP/lg-icons.old"
    FAKE_LUNA_FAIL=1 run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [ -f "$OUT/usr/palm/applications/a/i.png" ]
    [ "$(ls -A "$APP")" = "lg-icons" ]
}

@test "LG's two 0777 developer folders are accepted" {
    png "$ROOT/usr/palm/applications/a/i.png"
    points /usr/palm/applications/a/i.png
    chmod 0777 "$ROOT/media/developer" "$ROOT/media/developer/apps"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 1 copied, 0 refused" ]
}

@test "each folder our installs set is refused if others can write it" {
    points
    D="$ROOT/media/developer/apps"
    for dir in "$D/usr" "$D/usr/palm" "$D/usr/palm/applications" "$APP"; do
        chmod 0777 "$dir"
        run sh "$SCRIPT"
        chmod 0755 "$dir"
        [ "$status" -eq 1 ]
        [[ "$output" == "lumen-icons: $dir is writable by others (drwxrwxrwx"* ]]
        [ "$(ls -A "$APP")" = "" ]
    done
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
}

@test "a link in the path, or a folder that is not where it should be, is refused" {
    points
    D="$ROOT/media/developer"
    # LG's folders are not checked, but a link there moves Lumen's folder.
    mv "$D/apps" "$D/real"
    ln -s real "$D/apps"
    run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [[ "$output" == *"is not where it should be"* ]]
    [ ! -e "$D/real/usr/palm/applications/io.github.sunnyside007.lumen.shell/lg-icons" ]
    rm "$D/apps"
    mv "$D/real" "$D/apps"
    # One of the checked folders a link.
    mv "$D/apps/usr/palm" "$D/apps/usr/real"
    ln -s real "$D/apps/usr/palm"
    run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [[ "$output" == *"palm is not a folder"* ]]
    [ ! -e "$D/apps/usr/real/applications/io.github.sunnyside007.lumen.shell/lg-icons" ]
}

@test "the sum is sha256sum's, and a run without sha256sum says so" {
    png "$ROOT/usr/palm/applications/a/i.png"
    points /usr/palm/applications/a/i.png
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    grep -qxE '[0-9a-f]{64}  -' "$OUT/.sum"
    # A PATH with only what the script needs before it looks.
    mkdir -p "$BATS_TEST_TMPDIR/bin"
    ln -s "$(command -v id)" "$BATS_TEST_TMPDIR/bin/id"
    PATH="$BATS_TEST_TMPDIR/bin" run /bin/sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [ "$output" = "lumen-icons: no sha256sum" ]
}

@test "a second run with nothing changed leaves the copy alone" {
    png "$ROOT/usr/palm/applications/a/i.png"
    points /usr/palm/applications/a/i.png
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    touch "$OUT/marker"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: unchanged" ]
    [ -e "$OUT/marker" ]
    [ "$(ls -A "$APP")" = "lg-icons" ]
}

@test "a change redoes the copy, and an app that left takes its icons with it" {
    png "$ROOT/usr/palm/applications/a/i.png"
    png "$ROOT/usr/palm/applications/b/i.png"
    points /usr/palm/applications/a/i.png /usr/palm/applications/b/i.png
    run sh "$SCRIPT"
    [ -f "$OUT/usr/palm/applications/b/i.png" ]
    touch "$OUT/marker"
    points /usr/palm/applications/a/i.png
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 1 copied, 0 refused" ]
    [ -f "$OUT/usr/palm/applications/a/i.png" ]
    [ ! -e "$OUT/usr/palm/applications/b" ]
    [ ! -e "$OUT/marker" ]
    [ "$(ls -A "$APP")" = "lg-icons" ]
}

@test "a failed or refused luna call keeps the copy there was" {
    png "$ROOT/usr/palm/applications/a/i.png"
    points /usr/palm/applications/a/i.png
    run sh "$SCRIPT"
    touch "$OUT/marker"
    FAKE_LUNA_FAIL=1 run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [[ "$output" == *"luna-send failed"* ]]
    printf '{"returnValue":false,"errorText":"denied"}\n' >"$POINTS"
    run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [[ "$output" == *"listLaunchPoints refused"* ]]
    [ -e "$OUT/marker" ]
    [ "$(ls -A "$APP")" = "lg-icons" ]
}

@test "luna-send gets /dev/null, whatever the script's stdin is" {
    png "$ROOT/usr/palm/applications/a/i.png"
    points /usr/palm/applications/a/i.png
    # Over SSH the script's stdin is a pipe.
    run sh -c 'printf "" | sh "$1"' _ "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 1 copied, 0 refused" ]
}

@test "an empty answer from luna says so, and keeps the copy there was" {
    : >"$POINTS"
    run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [ "$output" = "lumen-icons: listLaunchPoints answered nothing" ]
    [ "$(ls -A "$APP")" = "" ]
    png "$ROOT/usr/palm/applications/a/i.png"
    points /usr/palm/applications/a/i.png
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    : >"$POINTS"
    run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [ "$output" = "lumen-icons: listLaunchPoints answered nothing" ]
    [ -f "$OUT/usr/palm/applications/a/i.png" ]
    [ "$(ls -A "$APP")" = "lg-icons" ]
}

@test "a live lock makes a run wait its turn; a stale one is taken over" {
    png "$ROOT/usr/palm/applications/a/i.png"
    points /usr/palm/applications/a/i.png
    # A running copy of the script holds the lock.
    (exec -a lumen-icons.sh sleep 30) &
    holder=$!
    ln -s "$holder" "$APP/lg-icons.lock"
    run sh "$SCRIPT"
    kill "$holder"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: busy" ]
    [ ! -e "$OUT" ]
    [ "$(readlink "$APP/lg-icons.lock")" = "$holder" ]
    # A folder where the lock goes is not a lock.
    rm -f "$APP/lg-icons.lock"
    mkdir "$APP/lg-icons.lock"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 1 copied, 0 refused" ]
    [ "$(ls -A "$APP")" = "lg-icons" ]
    # A leftover lg-icons.old beside the copy is cleared, not filled.
    mkdir "$APP/lg-icons.old"
    rm -f "$OUT/.sum"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 1 copied, 0 refused" ]
    [ "$(ls -A "$APP")" = "lg-icons" ]
    [ ! -e "$OUT/lg-icons" ]
    # Another process with the PID after a reboot, or none at all.
    for stale in $$ "$(sh -c 'echo $$')" junk; do
        rm -f "$APP/lg-icons.lock" "$OUT/.sum"
        ln -s "$stale" "$APP/lg-icons.lock"
        run sh "$SCRIPT"
        [ "$status" -eq 0 ]
        [ "$output" = "lumen-icons: changed, 1 copied, 0 refused" ]
        [ "$(ls -A "$APP")" = "lg-icons" ]
    done
}

@test "at most 128 app icons and 8 MB in all" {
    set --
    for i in $(seq 1 130); do
        png "$ROOT/usr/palm/applications/a/$i.png"
        set -- "$@" "/usr/palm/applications/a/$i.png"
    done
    points "$@"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 128 copied, 2 refused" ]

    rm -rf "$ROOT/usr/palm/applications/a"
    set --
    for i in $(seq 1 9); do
        # 1 MB exactly, the most one file may be.
        png "$ROOT/usr/palm/applications/a/$i.png" "$(head -c 1048568 /dev/zero | tr '\0' x)"
        set -- "$@" "/usr/palm/applications/a/$i.png"
    done
    [ "$(wc -c <"$ROOT/usr/palm/applications/a/1.png")" -eq 1048576 ]
    points "$@"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]
    [ "$output" = "lumen-icons: changed, 8 copied, 1 refused" ]
}

@test "it runs only as root, in a folder no one else can write" {
    mkdir -p "$BATS_TEST_TMPDIR/bin"
    printf '#!/bin/sh\necho 1000\n' >"$BATS_TEST_TMPDIR/bin/id"
    chmod +x "$BATS_TEST_TMPDIR/bin/id"
    PATH="$BATS_TEST_TMPDIR/bin:$PATH" LUMEN_ICONS_PREFIX= run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [ "$output" = "lumen-icons: not root" ]

    points
    chmod 0775 "$APP"
    run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [[ "$output" == *"writable by others"* ]]
    chmod 0757 "$APP"
    run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [[ "$output" == *"writable by others"* ]]
    chmod 0755 "$APP"
    run sh "$SCRIPT"
    [ "$status" -eq 0 ]

    rm -rf "$APP"
    run sh "$SCRIPT"
    [ "$status" -eq 1 ]
    [[ "$output" == *"no app folder"* ]]
}
