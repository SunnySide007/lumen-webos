# Lumen build environment: cross-compiles the native shell for LG webOS TVs
# (armv7, soft-float), packages it as an .ipk with LG's own packager, and runs
# the host-side tests and lints. Use it through tools/dev.sh:
#
#   tools/dev.sh make ipk
#   tools/dev.sh make check
#
# Every download is pinned to a version and, where the source publishes one, a
# checksum, so two builds of the same commit use the same tools.

FROM ubuntu:26.04@sha256:c098726dd9b9733f3e885f8de0ef375206a311b0a317eadc40d347b1115fb944

# webosbrew's buildroot SDK: GCC 12 targeting arm-webos-linux-gnueabi, the
# same toolchain family plx-native and moonlight-tv build with.
ARG WEBOS_SDK_URL=https://github.com/openlgtv/buildroot-nc4/releases/download/2026.08-webos/arm-webos-linux-gnueabi_sdk-buildroot-x86_64.tar.gz
ARG WEBOS_SDK_SHA256=2230a6351e5cce6fcb9d4fdcb4697335f223995401ef969c81744aaeb14c5344
# Keep in step with shell/rust-toolchain.toml.
ARG RUST_VERSION=1.99.0
# LG's packager needs Node 20 or newer; Ubuntu 24.04 ships 18.
ARG NODE_VERSION=22.23.3
ARG NODE_SHA256=df450af89261115ef9f9e3830c3eeb2cc9213b63c720b1af623cb5dcbe2e02de
ARG ARES_CLI_VERSION=3.2.6

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    WEBOS_SDK=/opt/arm-webos-linux-gnueabi_sdk-buildroot

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      bats bzip2 ca-certificates curl file gcc git libc6-dev make \
      openssh-client python3 shellcheck xz-utils \
 && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL "$WEBOS_SDK_URL" -o /tmp/webos-sdk.tar.gz \
 && echo "$WEBOS_SDK_SHA256  /tmp/webos-sdk.tar.gz" | sha256sum -c - \
 && tar -xzf /tmp/webos-sdk.tar.gz -C /opt \
 && rm /tmp/webos-sdk.tar.gz \
 && "$WEBOS_SDK/relocate-sdk.sh" \
 && "$WEBOS_SDK/bin/arm-webos-linux-gnueabi-gcc" --version | head -1

RUN curl -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh \
 && sh /tmp/rustup-init.sh -y --no-modify-path --profile minimal \
      --default-toolchain "$RUST_VERSION" \
      --component clippy,rustfmt \
      --target armv7-unknown-linux-gnueabi \
 && rm /tmp/rustup-init.sh \
 && chmod -R a+w "$RUSTUP_HOME" "$CARGO_HOME"

RUN curl -fsSL "https://nodejs.org/dist/v$NODE_VERSION/node-v$NODE_VERSION-linux-x64.tar.xz" -o /tmp/node.tar.xz \
 && echo "$NODE_SHA256  /tmp/node.tar.xz" | sha256sum -c - \
 && tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 \
      --exclude=CHANGELOG.md --exclude=LICENSE --exclude=README.md \
 && rm /tmp/node.tar.xz \
 && node --version

RUN npm install -g "@webos-tools/cli@$ARES_CLI_VERSION" \
 && ares-package --version

ENV PATH=$CARGO_HOME/bin:$WEBOS_SDK/bin:$PATH

WORKDIR /work
