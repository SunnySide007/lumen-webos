# Lumen studio: the offline HDR content pipeline. Renders procedural scenes to
# linear EXR with numpy, grades them to BT.2020 / PQ and encodes HEVC Main10
# with x265. Use it through tools/studio.sh:
#
#   tools/studio.sh python3 studio/render.py --help
#
# The encoder's version decides the bits of every video, so every download is
# pinned: the base image by digest, Ubuntu's packages to a dated archive
# snapshot, and the Python wheels by version and checksum. The build copies
# nothing from the repository: tools/studio.sh mounts it at run time.

FROM ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac

# snapshot.ubuntu.com serves the archive as it was at this moment.
ARG UBUNTU_SNAPSHOT=20261001T000000Z
# Wheels from PyPI for CPython 3.12 on x86_64, with the SHA-256s PyPI publishes.
ARG NUMPY_VERSION=2.5.3
ARG NUMPY_SHA256=b7e18c623bb5c95acb3b3328861272816ba199fb531921c5d6d0b675f1fde9e3
ARG OPENEXR_VERSION=3.4.16
ARG OPENEXR_SHA256=0ed6a8828726b104ffa0bd659e429d8a2dfaeab19a529653436ee68abcba0770

ENV DEBIAN_FRONTEND=noninteractive \
    PATH=/opt/studio/bin:$PATH

# The snapshot is served over HTTPS only, so the CA bundle comes first, from
# the live archive (apt still checks every package against Ubuntu's key).
# Ubuntu's ffmpeg links libx265 (built for 8, 10 and 12 bits) and zimg, which
# the SDR previews use to tone-map.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && echo "APT::Snapshot \"$UBUNTU_SNAPSHOT\";" >/etc/apt/apt.conf.d/50snapshot \
 && apt-get update \
 && apt-get install -y --no-install-recommends ffmpeg python3 python3-venv \
 && rm -rf /var/lib/apt/lists/* \
 && ffmpeg -hide_banner -version | head -n 1

# Fail the build unless x265 really writes Main10 and the preview filters exist.
RUN ffmpeg -hide_banner -loglevel error -f lavfi -i testsrc2=size=256x144:duration=0.2 \
      -pix_fmt yuv420p10le -c:v libx265 -x265-params log-level=error -f hevc /tmp/main10.hevc \
 && test "$(ffprobe -v error -select_streams v:0 -show_entries stream=profile,pix_fmt \
      -of csv=p=0 /tmp/main10.hevc)" = "Main 10,yuv420p10le" \
 && rm /tmp/main10.hevc \
 && ffmpeg -hide_banner -filters | grep -q ' zscale ' \
 && ffmpeg -hide_banner -filters | grep -q ' tonemap '

RUN python3 -m venv /opt/studio \
 && printf '%s\n' \
      "numpy==$NUMPY_VERSION --hash=sha256:$NUMPY_SHA256" \
      "openexr==$OPENEXR_VERSION --hash=sha256:$OPENEXR_SHA256" \
      >/tmp/requirements.txt \
 && pip install --no-cache-dir --only-binary=:all: --require-hashes -r /tmp/requirements.txt \
 && rm /tmp/requirements.txt \
 && python3 -c 'import numpy, OpenEXR; print("numpy", numpy.__version__, "OpenEXR", OpenEXR.__version__)'

WORKDIR /work
