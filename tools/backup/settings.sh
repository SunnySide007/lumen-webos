#!/bin/sh
# Prints every LG setting category for tools/backup.sh. Read-only: luna()
# calls get* methods only. busybox sh.

if timeout 1 true >/dev/null 2>&1; then
    T="timeout 10"
elif timeout -t 1 true >/dev/null 2>&1; then
    T="timeout -t 10"
else
    T=""
fi

luna() {
    case "$1" in
        */get*|*/list*) ;;
        *) echo "luna $1: refused, not a get or list method"; return ;;
    esac
    printf '$ luna %s %s\n' "$1" "$2"
    # shellcheck disable=SC2086
    $T luna-send -n 1 -f "luna://$1" "$2" </dev/null 2>&1 || echo '(no answer)'
}

# A category this firmware doesn't know answers with an error, which is fine.
for category in picture sound general option network time lock caption \
    channel aspectRatio other screenSaver voice; do
    luna com.webos.settingsservice/getSystemSettings "{\"category\":\"$category\"}"
done

# Picture settings are kept per dynamic range; the plain call returns only
# the current one.
for range in sdr hdr hlg dolbyHdr technicolorHdr; do
    luna com.webos.settingsservice/getSystemSettings \
        "{\"category\":\"picture\",\"dimension\":{\"input\":\"default\",\"_3dStatus\":\"2d\",\"dynamicRange\":\"$range\"}}"
done
