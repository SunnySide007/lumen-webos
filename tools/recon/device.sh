#!/bin/sh
# Read-only survey of a rooted LG webOS TV. tools/recon.sh pipes this to the TV
# over SSH and saves what it prints.
#
# Nothing here writes to the TV: it reads files under /proc, /sys, /etc, /usr
# and /var, lists directories and processes, and calls luna-send on get*/list*
# methods only.
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
for f in cpuinfo_min_freq cpuinfo_max_freq scaling_cur_freq scaling_governor scaling_available_frequencies; do
    showfile /sys/devices/system/cpu/cpu0/cpufreq/"$f"
done
for z in /sys/class/thermal/thermal_zone*; do
    [ -d "$z" ] || continue
    printf '%s: %s %s\n' "${z##*/}" "$(cat "$z/type" 2>/dev/null)" "$(cat "$z/temp" 2>/dev/null)"
done

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

section input
# Uniq can hold a Bluetooth address in a form redact.py doesn't match.
printf -- '--- /proc/bus/input/devices\n'
sed 's/^U: Uniq=..*/U: Uniq=<redacted>/' /proc/bus/input/devices 2>/dev/null || echo '(absent)'
showfile /proc/bus/input/handlers
show ls -l /dev/input
for node in /dev/uinput /dev/input/uinput /dev/uhid; do
    if [ -e "$node" ]; then show ls -l "$node"; else echo "$node: (absent)"; fi
done
set -- /dev/hidraw*
if [ -e "$1" ]; then show ls -l "$@"; else echo 'hidraw: (none)'; fi
show ls /sys/bus/hid/drivers
show ls /sys/bus/usb/drivers
printf '$ lsmod (all)\n'
lsmod 2>/dev/null || echo '(lsmod failed)'
printf '$ modules on disk (input, hid, bluetooth)\n'
find /lib/modules -name '*.ko' 2>/dev/null | grep -i -E 'hid|xpad|uinput|joy|evdev|bt|blue' || echo '(none matched)'
if [ -r /proc/config.gz ]; then
    printf '$ kernel config (input, hid, bluetooth)\n'
    zcat /proc/config.gz | grep -E '^CONFIG_(INPUT_(UINPUT|JOYDEV|EVDEV|JOYSTICK)|JOYSTICK_XPAD|HID|UHID|USB_HID|BT)' \
        || echo '(none matched)'
else
    echo '/proc/config.gz: (absent)'
fi

section bluetooth
show ls /sys/class/bluetooth
for tool in bluetoothd bluetoothctl hciconfig hcitool btmgmt bsa_server; do
    printf '%s: %s\n' "$tool" "$(command -v "$tool" || echo '(absent)')"
done
printf '$ bluetooth processes\n'
found=""
for p in /proc/[0-9]*; do
    comm=$(cat "$p/comm" 2>/dev/null) || continue
    case $comm in
        *blue* | *Blue* | *bsa* | *hci* | *btd* | bt*) printf '%s %s\n' "${p##*/}" "$comm"; found=1 ;;
    esac
done
[ -n "$found" ] || echo '(none)'
printf '$ luna services (bluetooth, gamepad, input)\n'
found=""
for f in /usr/share/luna-service2/services.d/*; do
    case ${f##*/} in
        *blue* | *gamepad* | *joystick* | *hid* | *input*) echo "${f##*/}"; found=1 ;;
    esac
done
[ -n "$found" ] || echo '(none matched)'
show ls /etc/bluetooth
# The adapter's and paired devices' names can be personal, so they're masked
# here; redact.py masks their addresses.
for call in adapter/getStatus device/getStatus; do
    printf '$ luna com.webos.service.bluetooth2/%s {}\n' "$call"
    # shellcheck disable=SC2086
    $T luna-send -n 1 -f "luna://com.webos.service.bluetooth2/$call" '{}' 2>&1 \
        | sed 's/"name": *"[^"]*"/"name": "<redacted>"/g'
done

section usb
for dev in /sys/bus/usb/devices/*; do
    [ -r "$dev/idVendor" ] || continue
    printf '%s: %s:%s %s\n' "${dev##*/}" "$(cat "$dev/idVendor")" "$(cat "$dev/idProduct")" \
        "$(cat "$dev/product" 2>/dev/null)"
done
show ls /tmp/usb
printf '$ USB storage mounts\n'
grep ' /tmp/usb' /proc/mounts || echo '(none)'

section network
for nic in /sys/class/net/*; do
    printf '%s: state %s, speed %s\n' "${nic##*/}" "$(cat "$nic/operstate" 2>/dev/null)" \
        "$(cat "$nic/speed" 2>/dev/null)"
done
showfile /proc/net/wireless
# The network's name also appears as displayName, which redact.py doesn't mask.
printf '$ luna com.webos.service.wifi/getstatus {}\n'
# shellcheck disable=SC2086
$T luna-send -n 1 -f luna://com.webos.service.wifi/getstatus '{}' 2>&1 \
    | sed 's/"displayName": *"[^"]*"/"displayName": "<redacted>"/g'

section audio
showfile /proc/asound/cards

section end
echo "recon complete"
