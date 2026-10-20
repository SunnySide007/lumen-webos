#!/bin/sh
# Cross-compiles tcpdump for the TV (armv7, webOS glibc 2.12), with libpcap
# linked in statically, and writes dist/capture/tcpdump. Runs in the build
# container:
#
#   tools/dev.sh make capture-tools
#
# The source tarballs are pinned by version and SHA-256. The hashes were taken
# on 2026-10-20 from tarballs whose PGP signatures checked out against The
# Tcpdump Group's signing key (RSA E089DEF1D9C15D0D, published next to them).
set -eu

LIBPCAP_VERSION=1.10.7
LIBPCAP_SHA256=68fa62cffb974f4275641ce14c2e2d75739251f30e00e6a0900903b247d76a03
TCPDUMP_VERSION=4.99.7
TCPDUMP_SHA256=39675dfa06fb0e99be8da5d4a1784acbaab58ae0870231d956ff7870cb99eec1
RELEASES=https://www.tcpdump.org/release

# The oldest glibc on a supported TV; nothing may need a newer symbol.
MAX_GLIBC=2.12
HOST=arm-webos-linux-gnueabi

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=$ROOT/dist/capture

die() {
    echo "capture/build.sh: $*" >&2
    exit 1
}

[ -n "${WEBOS_SDK:-}" ] || die "WEBOS_SDK is not set: run this through tools/dev.sh"
CC=$WEBOS_SDK/bin/$HOST-gcc
[ -x "$CC" ] || die "no compiler at $CC"
STRIP=$WEBOS_SDK/bin/$HOST-strip
READELF=$WEBOS_SDK/bin/$HOST-readelf

# Build in the container's own /tmp: faster than the bind-mounted checkout,
# and nothing is left behind.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

fetch() {
    name=$1 sum=$2
    echo "fetching $name" >&2
    curl -fsSL "$RELEASES/$name" -o "$work/$name"
    echo "$sum  $work/$name" | sha256sum -c - >/dev/null || die "SHA-256 mismatch for $name"
    tar -xJf "$work/$name" -C "$work"
}

fetch "libpcap-$LIBPCAP_VERSION.tar.xz" "$LIBPCAP_SHA256"
fetch "tcpdump-$TCPDUMP_VERSION.tar.xz" "$TCPDUMP_SHA256"

jobs=$(nproc 2>/dev/null || echo 2)

# Only the Linux packet socket: no USB, Bluetooth, D-Bus, RDMA, netmap or
# netlink library, none of which the TV build needs or the SDK provides.
echo "building libpcap $LIBPCAP_VERSION" >&2
(
    cd "$work/libpcap-$LIBPCAP_VERSION"
    CC=$CC ./configure --host=$HOST --with-pcap=linux \
        --disable-shared --enable-static \
        --disable-usb --disable-netmap --disable-bluetooth --disable-dbus \
        --disable-rdma --disable-remote --without-libnl >"$work/libpcap.log" 2>&1 ||
        { tail -n 40 "$work/libpcap.log" >&2; exit 1; }
    make -j"$jobs" libpcap.a >>"$work/libpcap.log" 2>&1 ||
        { tail -n 40 "$work/libpcap.log" >&2; exit 1; }
)

# tcpdump's configure finds the sibling libpcap-<version> directory and links
# its libpcap.a, so the binary needs nothing beyond the TV's glibc.
echo "building tcpdump $TCPDUMP_VERSION" >&2
(
    cd "$work/tcpdump-$TCPDUMP_VERSION"
    CC=$CC ./configure --host=$HOST \
        --without-crypto --without-smi --without-cap-ng >"$work/tcpdump.log" 2>&1 ||
        { tail -n 40 "$work/tcpdump.log" >&2; exit 1; }
    grep -q "libpcap-$LIBPCAP_VERSION/libpcap.a" Makefile ||
        die "tcpdump did not pick up the static libpcap"
    make -j"$jobs" tcpdump >>"$work/tcpdump.log" 2>&1 ||
        { tail -n 40 "$work/tcpdump.log" >&2; exit 1; }
)

mkdir -p "$OUT"
bin=$OUT/tcpdump
"$STRIP" -o "$bin" "$work/tcpdump-$TCPDUMP_VERSION/tcpdump"
chmod 0755 "$bin"

# Checks: a 32-bit ARM EABI5 executable, no libpcap.so, and no glibc symbol
# version newer than the TV's.
kind=$(file -b "$bin")
case $kind in
    *"ELF 32-bit LSB"*ARM*"EABI5"*) ;;
    *) die "unexpected binary type: $kind" ;;
esac
if "$READELF" -d "$bin" | grep -q 'libpcap'; then
    die "tcpdump links libpcap dynamically"
fi
newest=$("$READELF" -V "$bin" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n 1)
[ -n "$newest" ] || die "no GLIBC symbol versions found"
top=$(printf '%s\n%s\n' "$newest" "$MAX_GLIBC" | sort -V | tail -n 1)
[ "$top" = "$MAX_GLIBC" ] || die "needs GLIBC_$newest, newer than the TV's $MAX_GLIBC"

{
    echo "tcpdump $TCPDUMP_VERSION, libpcap $LIBPCAP_VERSION (static)"
    echo "type: $kind"
    echo "needs: $("$READELF" -d "$bin" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | tr '\n' ' ')"
    echo "newest glibc symbol: GLIBC_$newest"
    echo "sha256: $(sha256sum "$bin" | cut -d' ' -f1)"
} | tee "$OUT/BUILD-INFO.txt"
