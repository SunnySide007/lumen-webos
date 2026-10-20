#!/bin/sh
# Read-only survey of a rooted LG webOS TV. tools/recon.sh pipes this to the TV
# over SSH and saves what it prints.
#
# Nothing here writes to the TV: it reads files under /proc, /sys, /etc, /usr
# and /var, lists directories, and calls luna-send on get*/list* methods only.
# It runs under the TV's busybox sh, so it sticks to POSIX shell.

# Older busybox (webOS 4) spells it `timeout -t SECS`, newer `timeout SECS`.
if timeout 1 true >/dev/null 2>&1; then
    T="timeout 10"
elif timeout -t 1 true >/dev/null 2>&1; then
    T="timeout -t 10"
else
    T=""
fi

section() {
    printf '\n===== %s =====\n' "$1"
}

show() {
    printf '$ %s\n' "$*"
    "$@" 2>&1 || printf '(exit %s)\n' "$?"
}

showfile() {
    for f in "$@"; do
        printf -- '--- %s\n' "$f"
        if [ -r "$f" ]; then cat "$f"; else echo '(absent)'; fi
    done
}

luna() {
    printf '$ luna %s %s\n' "$1" "$2"
    # shellcheck disable=SC2086
    $T luna-send -n 1 -f "luna://$1" "$2" 2>&1 || echo '(no answer)'
}

section identity
show date -u
show uptime
show uname -a
showfile /etc/starfish-release /etc/os-release
for key in webos_name webos_release webos_build_id webos_imagename webos_release_codename core_os_kernel_version; do
    printf '%s: ' "$key"
    nyx-cmd OSInfo query "$key" 2>/dev/null || echo '(n/a)'
done
luna com.webos.service.tv.systemproperty/getSystemInfo \
    '{"keys":["modelName","firmwareVersion","sdkVersion","boardType","otaId","UHD"]}'

section hardware
showfile /proc/cpuinfo
show free
printf '$ head /proc/meminfo\n'
head -n 8 /proc/meminfo
printf '$ lsmod (graphics)\n'
lsmod 2>/dev/null | grep -i -E 'mali|galcore|gpu|kbase' || echo '(none matched)'
showfile /sys/class/misc/mali0/device/gpuinfo /proc/lg/pm/temperature

section storage
show df -h
for d in /media/developer /mnt/lg/appstore /mnt/lg/cmn_data /home /tmp /var; do
    show df -h "$d"
done

section graphics-and-media-libraries
for pattern in libSDL2 libGLESv2 libEGL libwayland-client libluna-service2 libplayerAPIs libAcbAPI libpf-1.0 libcbe; do
    ls -l /usr/lib/"$pattern"* 2>/dev/null || echo "$pattern: (absent)"
done

section qt-and-qml
show ls /usr/lib/qt5/qml
show ls /usr/lib/qt5/qml/KeyFilters
show ls -d /usr/lib/qt5/qml/QtMultimedia
show ls /usr/lib/qml

section system-apps
set -- /usr/palm/applications/*
printf 'system apps: %s\n' "$#"
show ls /usr/palm/applications
printf '\n-- home, launcher and screensaver apps (id: type)\n'
for d in /usr/palm/applications/*; do
    id=${d##*/}
    case $id in
        *home* | *launcher* | *screensaver*)
            printf '%s: ' "$id"
            grep -o '"type": *"[^"]*"' "$d/appinfo.json" 2>/dev/null | head -n 1 || echo '(no appinfo)'
            ;;
    esac
done
show ls -la /usr/palm/applications/com.webos.app.screensaver
show ls -la /usr/palm/applications/com.webos.app.screensaver/qml
showfile /usr/palm/applications/com.webos.app.screensaver/appinfo.json
showfile /usr/palm/applications/com.webos.app.home/appinfo.json

section installed-apps
printf '$ listApps (ids)\n'
# shellcheck disable=SC2086
$T luna-send -n 1 luna://com.webos.applicationManager/listApps '{}' 2>/dev/null \
    | grep -o '"id": *"[^"]*"' | sed 's/.*"id": *"//; s/"$//' | sort -u
show ls /media/developer/apps/usr/palm/applications
luna com.webos.applicationManager/getForegroundAppInfo '{}'

section compositor
luna com.webos.service.config/getConfigs '{"configNames":["com.webos.surfacemanager.keyFilters"]}'

section settings
for category in picture sound general option; do
    luna com.webos.settingsservice/getSystemSettings "{\"category\":\"$category\"}"
done

section content-store-host
grep -r -h -o -E '[A-Za-z]{2}\.(lgtvsdp|nextlgsdp)\.com' /var/luna/preferences /etc 2>/dev/null | sort -u \
    || echo '(none found)'

section homebrew
show ls -la /var/lib/webosbrew/init.d
printf '$ ls /var/luna/preferences (homebrew-related)\n'
found=""
for f in /var/luna/preferences/*; do
    case ${f##*/} in
        *webosbrew* | *devmode* | *tvweb* | *lumen*) echo "${f##*/}"; found=1 ;;
    esac
done
[ -n "$found" ] || echo '(none)'
if [ -d /var/lib/tvweb ]; then echo 'glasshouse: installed (/var/lib/tvweb)'; else echo 'glasshouse: not installed'; fi
printf '$ mounts\n'
cat /proc/mounts

section hosts
printf '$ wc -l /etc/hosts\n'
wc -l < /etc/hosts
printf '$ head -n 40 /etc/hosts\n'
head -n 40 /etc/hosts

section runtime
show node -v
printf 'init: '
if command -v systemctl >/dev/null 2>&1; then echo systemd; elif command -v initctl >/dev/null 2>&1; then echo upstart; else echo unknown; fi

section end
echo "recon complete"
